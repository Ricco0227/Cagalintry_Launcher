//! Comparing two pack revisions.
//!
//! This is the engine behind the Update button: it decides what the button says,
//! what the confirmation screen lists, and — in the client — exactly which files
//! get downloaded and which get deleted. Nothing here touches the disk or the
//! network, which is what makes the update path cheap to test.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::manifest::{EntryKind, LoaderSpec, PackEntry, PackManifest};

/// One entry that exists in both revisions but isn't identical.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryUpdate {
    pub from: PackEntry,
    pub to: PackEntry,
}

impl EntryUpdate {
    /// The file content itself changed, so it has to be redownloaded.
    pub fn content_changed(&self) -> bool {
        self.from.hashes != self.to.hashes
    }

    /// Same bytes, different location — a rename, satisfiable by moving the file
    /// instead of pulling it down again.
    pub fn is_pure_move(&self) -> bool {
        !self.content_changed() && self.from.path != self.to.path
    }

    /// Someone toggled the mod on or off without changing its version.
    pub fn is_pure_toggle(&self) -> bool {
        !self.content_changed()
            && self.from.path == self.to.path
            && self.from.enabled != self.to.enabled
    }
}

/// A change to a scalar field of the pack, kept as a before/after pair so the UI
/// can render "1.21.4 → 1.21.5".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Change<T> {
    pub from: T,
    pub to: T,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackDiff {
    pub added: Vec<PackEntry>,
    pub updated: Vec<EntryUpdate>,
    pub removed: Vec<PackEntry>,
    pub overrides_changed: bool,
    /// Set when the loader or its version moved. Requires reinstalling the
    /// loader before the instance can launch.
    pub loader_change: Option<Change<LoaderSpec>>,
    /// Set when the pack moved to a different Minecraft version. Requires a full
    /// version reinstall, and is worth warning the player about explicitly.
    pub mc_version_change: Option<Change<String>>,
}

impl PackDiff {
    /// Nothing to do — the installed revision already matches.
    pub fn is_empty(&self) -> bool {
        self.added.is_empty()
            && self.updated.is_empty()
            && self.removed.is_empty()
            && !self.overrides_changed
            && self.loader_change.is_none()
            && self.mc_version_change.is_none()
    }

    /// What the Update badge shows.
    pub fn change_count(&self) -> usize {
        self.added.len() + self.updated.len() + self.removed.len()
    }

    /// True when the game itself has to be reinstalled, not just its content.
    /// The UI warns about this before starting, because it is a slow update.
    pub fn requires_reinstall(&self) -> bool {
        self.mc_version_change.is_some() || self.loader_change.is_some()
    }

    /// Total bytes that need fetching. Pure moves and toggles cost nothing.
    pub fn download_size(&self) -> u64 {
        let added: u64 = self.added.iter().map(|e| e.size).sum();
        let updated: u64 = self
            .updated
            .iter()
            .filter(|u| u.content_changed())
            .map(|u| u.to.size)
            .sum();
        added + updated
    }

    /// Human-readable one-liner for changelogs and the update dialog.
    pub fn summary(&self) -> String {
        if self.is_empty() {
            return "Up to date".to_string();
        }
        let mut parts = Vec::new();
        if !self.added.is_empty() {
            parts.push(format!("{} added", self.added.len()));
        }
        if !self.updated.is_empty() {
            parts.push(format!("{} updated", self.updated.len()));
        }
        if !self.removed.is_empty() {
            parts.push(format!("{} removed", self.removed.len()));
        }
        if self.overrides_changed {
            parts.push("config changed".to_string());
        }
        parts.join(", ")
    }

    pub fn added_of(&self, kind: EntryKind) -> impl Iterator<Item = &PackEntry> {
        self.added.iter().filter(move |e| e.kind == kind)
    }
}

/// Compare the installed revision against the target revision.
///
/// Entries are matched on [`PackEntry::identity`] rather than path, so bumping a
/// mod's version reads as one update instead of a delete plus an install.
pub fn diff(from: &PackManifest, to: &PackManifest) -> PackDiff {
    let from_by_id: BTreeMap<String, &PackEntry> =
        from.entries.iter().map(|e| (e.identity(), e)).collect();
    let mut to_by_id: BTreeMap<String, &PackEntry> =
        to.entries.iter().map(|e| (e.identity(), e)).collect();

    let mut added = Vec::new();
    let mut updated = Vec::new();
    let mut removed = Vec::new();

    for (id, old) in &from_by_id {
        match to_by_id.remove(id) {
            Some(new) if *old != new => updated.push(EntryUpdate {
                from: (*old).clone(),
                to: new.clone(),
            }),
            Some(_) => {} // identical, nothing to do
            None => removed.push((*old).clone()),
        }
    }
    // Whatever is left in `to` was never in `from`.
    added.extend(to_by_id.into_values().cloned());

    // BTreeMap iteration is already deterministic, but sort by install path so
    // the UI lists changes in the order a person would scan them.
    added.sort_by(|a, b| a.path.cmp(&b.path));
    removed.sort_by(|a, b| a.path.cmp(&b.path));
    updated.sort_by(|a, b| a.to.path.cmp(&b.to.path));

    PackDiff {
        added,
        updated,
        removed,
        overrides_changed: overrides_differ(from, to),
        loader_change: (from.loader != to.loader).then(|| Change {
            from: from.loader.clone(),
            to: to.loader.clone(),
        }),
        mc_version_change: (from.mc_version != to.mc_version).then(|| Change {
            from: from.mc_version.clone(),
            to: to.mc_version.clone(),
        }),
    }
}

/// Overrides are content-addressed, so comparing hashes is enough — a rebuilt
/// bundle with identical contents is correctly treated as unchanged.
fn overrides_differ(from: &PackManifest, to: &PackManifest) -> bool {
    match (&from.overrides, &to.overrides) {
        (Some(a), Some(b)) => a.sha256 != b.sha256,
        (None, None) => false,
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{ContentSource, Hashes, LoaderKind, OverridesRef, Side};
    use uuid::Uuid;

    fn entry(path: &str, project: &str, version: &str, hash_seed: u8) -> PackEntry {
        PackEntry {
            kind: EntryKind::Mod,
            source: ContentSource::Modrinth {
                project_id: project.to_string(),
                version_id: version.to_string(),
            },
            path: path.to_string(),
            hashes: Hashes {
                sha1: format!("{hash_seed:02x}").repeat(20),
                sha512: format!("{hash_seed:02x}").repeat(64),
            },
            size: 1_000,
            downloads: vec!["https://cdn.modrinth.com/data/x/y.jar".to_string()],
            side: Side::Both,
            enabled: true,
            name: Some(project.to_string()),
            version_number: Some(version.to_string()),
        }
    }

    fn manifest(revision: u64, entries: Vec<PackEntry>) -> PackManifest {
        PackManifest {
            revision,
            entries,
            ..PackManifest::new(Uuid::nil(), "Test", "1.21.4", LoaderSpec::vanilla())
        }
    }

    #[test]
    fn identical_revisions_produce_no_work() {
        let a = manifest(1, vec![entry("mods/sodium.jar", "sodium", "v1", 1)]);
        let b = manifest(2, vec![entry("mods/sodium.jar", "sodium", "v1", 1)]);
        let d = diff(&a, &b);
        assert!(d.is_empty());
        assert_eq!(d.change_count(), 0);
        assert_eq!(d.summary(), "Up to date");
    }

    #[test]
    fn detects_an_added_mod() {
        let a = manifest(1, vec![]);
        let b = manifest(2, vec![entry("mods/sodium.jar", "sodium", "v1", 1)]);
        let d = diff(&a, &b);
        assert_eq!(d.added.len(), 1);
        assert!(d.updated.is_empty() && d.removed.is_empty());
        assert_eq!(d.download_size(), 1_000);
    }

    #[test]
    fn detects_a_removed_mod() {
        let a = manifest(1, vec![entry("mods/sodium.jar", "sodium", "v1", 1)]);
        let b = manifest(2, vec![]);
        let d = diff(&a, &b);
        assert_eq!(d.removed.len(), 1);
        assert_eq!(d.download_size(), 0);
    }

    #[test]
    fn a_version_bump_is_one_update_not_a_delete_and_add() {
        // The whole reason entries are matched on project identity: without it
        // every pack update would redownload the entire mod list.
        let a = manifest(1, vec![entry("mods/sodium-0.6.0.jar", "sodium", "v1", 1)]);
        let b = manifest(2, vec![entry("mods/sodium-0.6.1.jar", "sodium", "v2", 2)]);
        let d = diff(&a, &b);
        assert_eq!(d.updated.len(), 1);
        assert!(d.added.is_empty() && d.removed.is_empty());
        assert!(d.updated[0].content_changed());
        assert_eq!(d.change_count(), 1);
    }

    #[test]
    fn a_rename_with_identical_bytes_needs_no_download() {
        let a = manifest(1, vec![entry("mods/old-name.jar", "sodium", "v1", 1)]);
        let b = manifest(2, vec![entry("mods/new-name.jar", "sodium", "v1", 1)]);
        let d = diff(&a, &b);
        assert_eq!(d.updated.len(), 1);
        assert!(d.updated[0].is_pure_move());
        assert_eq!(d.download_size(), 0);
    }

    #[test]
    fn disabling_a_mod_is_an_update_that_downloads_nothing() {
        let a = manifest(1, vec![entry("mods/sodium.jar", "sodium", "v1", 1)]);
        let mut disabled = entry("mods/sodium.jar", "sodium", "v1", 1);
        disabled.enabled = false;
        let b = manifest(2, vec![disabled]);
        let d = diff(&a, &b);
        assert_eq!(d.updated.len(), 1);
        assert!(d.updated[0].is_pure_toggle());
        assert_eq!(d.download_size(), 0);
    }

    #[test]
    fn detects_a_changed_overrides_bundle() {
        let a = manifest(1, vec![]);
        let mut b = manifest(2, vec![]);
        b.overrides = Some(OverridesRef {
            blob_id: "blob".into(),
            sha256: "c".repeat(64),
            size: 42,
        });
        let d = diff(&a, &b);
        assert!(d.overrides_changed);
        assert!(!d.is_empty());
        // Config-only changes still count as work even though no entry moved.
        assert_eq!(d.change_count(), 0);
    }

    #[test]
    fn identical_overrides_hashes_are_not_a_change() {
        let bundle = OverridesRef { blob_id: "b1".into(), sha256: "c".repeat(64), size: 42 };
        let mut a = manifest(1, vec![]);
        a.overrides = Some(bundle.clone());
        let mut b = manifest(2, vec![]);
        // Different blob id, same content — a rebuild of the same config set.
        b.overrides = Some(OverridesRef { blob_id: "b2".into(), ..bundle });
        assert!(!diff(&a, &b).overrides_changed);
    }

    #[test]
    fn a_minecraft_or_loader_change_forces_a_reinstall() {
        let a = manifest(1, vec![]);
        let mut b = manifest(2, vec![]);
        b.mc_version = "1.21.5".into();
        b.loader = LoaderSpec { kind: LoaderKind::Fabric, version: Some("0.16.10".into()) };

        let d = diff(&a, &b);
        assert!(d.requires_reinstall());
        assert_eq!(d.mc_version_change.as_ref().unwrap().to, "1.21.5");
        assert_eq!(d.loader_change.as_ref().unwrap().to.kind, LoaderKind::Fabric);
    }

    #[test]
    fn mixed_changes_are_reported_together() {
        let a = manifest(
            1,
            vec![
                entry("mods/sodium.jar", "sodium", "v1", 1),
                entry("mods/lithium.jar", "lithium", "v1", 2),
            ],
        );
        let b = manifest(
            2,
            vec![
                entry("mods/sodium.jar", "sodium", "v2", 3), // updated
                entry("mods/iris.jar", "iris", "v1", 4),     // added
                                                             // lithium removed
            ],
        );
        let d = diff(&a, &b);
        assert_eq!(d.added.len(), 1);
        assert_eq!(d.updated.len(), 1);
        assert_eq!(d.removed.len(), 1);
        assert_eq!(d.change_count(), 3);
        assert_eq!(d.summary(), "1 added, 1 updated, 1 removed");
    }

    #[test]
    fn diffing_is_directional() {
        // Rolling back must delete what rolling forward installed.
        let a = manifest(1, vec![]);
        let b = manifest(2, vec![entry("mods/sodium.jar", "sodium", "v1", 1)]);
        assert_eq!(diff(&a, &b).added.len(), 1);
        assert_eq!(diff(&b, &a).removed.len(), 1);
    }
}
