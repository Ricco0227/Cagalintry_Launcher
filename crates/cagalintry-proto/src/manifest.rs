//! The pack manifest — the authoritative description of one revision of a modpack.
//!
//! Shaped deliberately close to Modrinth's `.mrpack` index so import/export stays
//! a thin mapping, but carries the fields sync actually needs: a monotonic
//! `revision`, a content `kind`, per-entry `enabled`, and the personal-path list
//! that protects a player's own settings from being overwritten.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::validate::{self, ValidationError};

/// Bumped only on a breaking manifest change. Clients refuse manifests they
/// don't understand rather than guessing at unknown semantics.
pub const MANIFEST_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackManifest {
    pub schema: u32,
    pub pack_id: Uuid,
    /// Monotonic, server-assigned. The launcher compares this against the
    /// installed revision to decide whether Play becomes Update.
    pub revision: u64,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    pub mc_version: String,
    pub loader: LoaderSpec,
    #[serde(default)]
    pub entries: Vec<PackEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overrides: Option<OverridesRef>,
    /// Glob patterns, relative to the instance root, that sync must never
    /// overwrite. Defaults cover the files that are inherently per-player.
    #[serde(default = "default_personal_paths")]
    pub personal_paths: Vec<String>,
}

impl PackManifest {
    /// A brand-new, empty pack at revision 0.
    pub fn new(pack_id: Uuid, name: impl Into<String>, mc_version: impl Into<String>, loader: LoaderSpec) -> Self {
        Self {
            schema: MANIFEST_SCHEMA_VERSION,
            pack_id,
            revision: 0,
            name: name.into(),
            summary: None,
            mc_version: mc_version.into(),
            loader,
            entries: Vec::new(),
            overrides: None,
            personal_paths: default_personal_paths(),
        }
    }

    /// Full structural validation. Run this on **both** sides: the server before
    /// storing a pushed revision, and the client before acting on a fetched one.
    /// A manifest is untrusted input even when it comes from your own server.
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.schema != MANIFEST_SCHEMA_VERSION {
            return Err(ValidationError::UnsupportedSchema {
                found: self.schema,
                supported: MANIFEST_SCHEMA_VERSION,
            });
        }
        if self.name.trim().is_empty() {
            return Err(ValidationError::EmptyField("name"));
        }
        if self.mc_version.trim().is_empty() {
            return Err(ValidationError::EmptyField("mcVersion"));
        }

        let mut seen_paths = std::collections::HashSet::new();
        let mut seen_identities = std::collections::HashSet::new();
        for entry in &self.entries {
            entry.validate()?;
            // Two entries writing the same file would make apply order decide the
            // outcome, which is a great way to get a pack that installs differently
            // depending on hashmap iteration order.
            if !seen_paths.insert(entry.path.to_ascii_lowercase()) {
                return Err(ValidationError::DuplicatePath(entry.path.clone()));
            }
            // Two versions of one mod in the same pack is a crash on launch, and
            // it would also make the diff ambiguous about which one changed.
            if !seen_identities.insert(entry.identity()) {
                return Err(ValidationError::DuplicateProject(entry.display_name().to_string()));
            }
        }

        for pattern in &self.personal_paths {
            validate::validate_relative_path(pattern)?;
        }

        Ok(())
    }

    pub fn entries_of(&self, kind: EntryKind) -> impl Iterator<Item = &PackEntry> {
        self.entries.iter().filter(move |e| e.kind == kind)
    }
}

/// Files that belong to the player, not the pack. Overwriting these is how a
/// launcher earns a reputation for eating people's keybinds.
fn default_personal_paths() -> Vec<String> {
    vec![
        "options.txt".to_string(),
        "optionsof.txt".to_string(),
        "optionsshaders.txt".to_string(),
        "servers.dat".to_string(),
        "servers.dat_old".to_string(),
        "usercache.json".to_string(),
        "realms_persistence.json".to_string(),
        "config/**/*keybind*".to_string(),
    ]
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackEntry {
    pub kind: EntryKind,
    pub source: ContentSource,
    /// Destination relative to the instance root, forward slashes only.
    pub path: String,
    pub hashes: Hashes,
    pub size: u64,
    /// Mirrors of the same file. Restricted to [`ALLOWED_DOWNLOAD_HOSTS`].
    ///
    /// [`ALLOWED_DOWNLOAD_HOSTS`]: crate::validate::ALLOWED_DOWNLOAD_HOSTS
    pub downloads: Vec<String>,
    #[serde(default)]
    pub side: Side,
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Display name, carried so the UI and changelogs can say "Sodium" rather
    /// than a project ID, without a round-trip to Modrinth.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version_number: Option<String>,
}

impl PackEntry {
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate::validate_relative_path(&self.path)?;

        // A "resourcepack" that installs into mods/ would run as code. Pin each
        // kind to its own directory so a mislabelled entry can't cross over.
        let expected = self.kind.directory();
        if !self.path.starts_with(&format!("{expected}/")) {
            return Err(ValidationError::PathOutsideKindDirectory {
                path: self.path.clone(),
                expected,
            });
        }

        self.hashes.validate()?;

        if self.downloads.is_empty() {
            return Err(ValidationError::NoDownloads(self.path.clone()));
        }
        for url in &self.downloads {
            validate::validate_download_url(url)?;
        }

        Ok(())
    }

    /// Stable identity across revisions. A version bump of the same Modrinth
    /// project must read as "updated", not "removed then added" — otherwise the
    /// diff would delete and redownload every mod on every change.
    ///
    /// Kind is part of the key, so the rare case of a project moving between
    /// content types correctly reads as a removal plus an addition: the file has
    /// to leave one directory and appear in another.
    pub fn identity(&self) -> String {
        let kind = self.kind.directory();
        match &self.source {
            ContentSource::Modrinth { project_id, .. } => format!("{kind}:modrinth:{project_id}"),
        }
    }

    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or(&self.path)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EntryKind {
    Mod,
    ResourcePack,
    ShaderPack,
}

impl EntryKind {
    pub const ALL: [EntryKind; 3] = [Self::Mod, Self::ResourcePack, Self::ShaderPack];

    /// The instance-relative directory this kind installs into.
    pub const fn directory(self) -> &'static str {
        match self {
            Self::Mod => "mods",
            Self::ResourcePack => "resourcepacks",
            Self::ShaderPack => "shaderpacks",
        }
    }

    /// Modrinth's `project_type` facet value for this kind.
    pub const fn modrinth_project_type(self) -> &'static str {
        match self {
            Self::Mod => "mod",
            Self::ResourcePack => "resourcepack",
            Self::ShaderPack => "shader",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase", rename_all_fields = "camelCase")]
pub enum ContentSource {
    Modrinth { project_id: String, version_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hashes {
    pub sha1: String,
    pub sha512: String,
}

impl Hashes {
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate::validate_hex(&self.sha1, 40, "sha1")?;
        validate::validate_hex(&self.sha512, 128, "sha512")?;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    /// Client-only — a shader or a minimap. Never expected on the server.
    Client,
    #[default]
    Both,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoaderSpec {
    pub kind: LoaderKind,
    /// `None` for vanilla, otherwise the exact loader version.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

impl LoaderSpec {
    pub fn vanilla() -> Self {
        Self { kind: LoaderKind::Vanilla, version: None }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LoaderKind {
    Vanilla,
    Fabric,
    Quilt,
    NeoForge,
}

impl LoaderKind {
    /// Facet value Modrinth uses to filter versions by loader.
    pub const fn modrinth_facet(self) -> &'static str {
        match self {
            Self::Vanilla => "minecraft",
            Self::Fabric => "fabric",
            Self::Quilt => "quilt",
            Self::NeoForge => "neoforge",
        }
    }

    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Vanilla => "Vanilla",
            Self::Fabric => "Fabric",
            Self::Quilt => "Quilt",
            Self::NeoForge => "NeoForge",
        }
    }
}

/// Pointer to the overrides bundle — a zip of config files, stored as a blob on
/// the sync server and addressed by content hash.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OverridesRef {
    pub blob_id: String,
    pub sha256: String,
    pub size: u64,
}

const fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(kind: EntryKind, path: &str, project: &str, version: &str) -> PackEntry {
        PackEntry {
            kind,
            source: ContentSource::Modrinth {
                project_id: project.to_string(),
                version_id: version.to_string(),
            },
            path: path.to_string(),
            hashes: Hashes { sha1: "a".repeat(40), sha512: "b".repeat(128) },
            size: 1,
            downloads: vec!["https://cdn.modrinth.com/data/x/y.jar".to_string()],
            side: Side::Both,
            enabled: true,
            name: None,
            version_number: None,
        }
    }

    fn manifest(entries: Vec<PackEntry>) -> PackManifest {
        PackManifest {
            entries,
            ..PackManifest::new(Uuid::nil(), "Test", "1.21.4", LoaderSpec::vanilla())
        }
    }

    #[test]
    fn round_trips_through_json() {
        let m = manifest(vec![entry(EntryKind::Mod, "mods/sodium.jar", "AANobbMI", "v1")]);
        let json = serde_json::to_string(&m).unwrap();
        assert_eq!(serde_json::from_str::<PackManifest>(&json).unwrap(), m);
    }

    #[test]
    fn serializes_with_camel_case_keys() {
        let json = serde_json::to_value(manifest(vec![])).unwrap();
        assert!(json.get("packId").is_some());
        assert!(json.get("mcVersion").is_some());
        assert!(json.get("personalPaths").is_some());
    }

    #[test]
    fn accepts_a_well_formed_manifest() {
        manifest(vec![
            entry(EntryKind::Mod, "mods/sodium.jar", "AANobbMI", "v1"),
            entry(EntryKind::ShaderPack, "shaderpacks/bsl.zip", "Q1vvjJYV", "v2"),
        ])
        .validate()
        .unwrap();
    }

    #[test]
    fn rejects_two_entries_writing_the_same_file() {
        let m = manifest(vec![
            entry(EntryKind::Mod, "mods/dup.jar", "aaa", "v1"),
            entry(EntryKind::Mod, "mods/dup.jar", "bbb", "v1"),
        ]);
        assert!(matches!(m.validate(), Err(ValidationError::DuplicatePath(_))));
    }

    #[test]
    fn rejects_content_installing_outside_its_own_directory() {
        // A shaderpack pointed at mods/ would be loaded as executable code.
        let m = manifest(vec![entry(EntryKind::ShaderPack, "mods/evil.jar", "x", "v1")]);
        assert!(matches!(
            m.validate(),
            Err(ValidationError::PathOutsideKindDirectory { .. })
        ));
    }

    #[test]
    fn rejects_an_unknown_schema_version() {
        let mut m = manifest(vec![]);
        m.schema = 99;
        assert!(matches!(m.validate(), Err(ValidationError::UnsupportedSchema { .. })));
    }

    #[test]
    fn identity_is_stable_across_a_version_bump() {
        // This is what makes a mod update read as "updated" rather than
        // "removed and added" in the diff.
        let old = entry(EntryKind::Mod, "mods/sodium-0.6.0.jar", "AANobbMI", "v1");
        let new = entry(EntryKind::Mod, "mods/sodium-0.6.1.jar", "AANobbMI", "v2");
        assert_eq!(old.identity(), new.identity());
    }

    #[test]
    fn default_personal_paths_cover_the_obvious_per_player_files() {
        let m = manifest(vec![]);
        assert!(m.personal_paths.iter().any(|p| p == "options.txt"));
        assert!(m.personal_paths.iter().any(|p| p == "servers.dat"));
    }
}
