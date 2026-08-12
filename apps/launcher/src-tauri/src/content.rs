//! Mods, resource packs and shader packs installed in an instance.
//!
//! The record is a list of [`PackEntry`] — deliberately the same type a synced
//! pack is made of. An instance's installed content *is* a manifest, so
//! publishing it later means uploading what is already there rather than
//! deriving it, and a pack update reconciles against the same structure it
//! would have produced itself.

use std::path::{Path, PathBuf};

use cagalintry_mc::DataDirs;
use cagalintry_net::{Checksum, DownloadSpec, Downloader};
use cagalintry_proto::PackEntry;
use uuid::Uuid;

/// Suffix Minecraft ignores, which is how a mod is turned off without losing it.
const DISABLED_SUFFIX: &str = ".disabled";

#[derive(Debug, thiserror::Error)]
pub enum ContentError {
    #[error(transparent)]
    Download(#[from] cagalintry_net::DownloadError),

    #[error("{path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("reading the content list: {0}")]
    Parse(#[from] serde_json::Error),

    #[error("this content is not valid: {0}")]
    Invalid(#[from] cagalintry_proto::ValidationError),

    #[error("nothing installed at {0}")]
    NotFound(String),
}

pub struct ContentStore {
    dirs: DataDirs,
}

impl ContentStore {
    pub fn new(dirs: DataDirs) -> Self {
        Self { dirs }
    }

    fn record_path(&self, instance_id: Uuid) -> PathBuf {
        self.dirs
            .instance(&instance_id.to_string())
            .root()
            .join(".cagalintry")
            .join("content.json")
    }

    fn game_dir(&self, instance_id: Uuid) -> PathBuf {
        self.dirs.instance(&instance_id.to_string()).game_dir()
    }

    pub async fn list(&self, instance_id: Uuid) -> Result<Vec<PackEntry>, ContentError> {
        let path = self.record_path(instance_id);
        match tokio::fs::read(&path).await {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
            // Nothing installed yet.
            Err(_) => Ok(Vec::new()),
        }
    }

    async fn save(&self, instance_id: Uuid, entries: &[PackEntry]) -> Result<(), ContentError> {
        let path = self.record_path(instance_id);
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|source| ContentError::Io { path: parent.display().to_string(), source })?;
        }

        let json = serde_json::to_vec_pretty(entries)?;
        tokio::fs::write(&path, json)
            .await
            .map_err(|source| ContentError::Io { path: path.display().to_string(), source })
    }

    /// Download an entry into the instance and record it.
    ///
    /// Replacing an entry for the same project removes the old file first, so
    /// updating a mod cannot leave two versions of it in `mods/` — which
    /// crashes the game on launch.
    pub async fn install(
        &self,
        instance_id: Uuid,
        entry: PackEntry,
        downloader: &Downloader,
    ) -> Result<(), ContentError> {
        entry.validate()?;

        let game_dir = self.game_dir(instance_id);
        let destination = game_dir.join(&entry.path);

        let mut spec = DownloadSpec::new(
            entry.downloads.first().cloned().unwrap_or_default(),
            &destination,
        )
        .with_size(entry.size);
        spec.checksum = Some(Checksum::Sha512(entry.hashes.sha512.clone()));

        downloader.download(&spec, None).await?;

        let mut entries = self.list(instance_id).await?;
        let identity = entry.identity();

        // Remove any previous file for this project before recording the new
        // one, so an update replaces rather than accumulates.
        for existing in entries.iter().filter(|e| e.identity() == identity) {
            if existing.path != entry.path {
                remove_file_variants(&game_dir, &existing.path).await;
            }
        }
        entries.retain(|e| e.identity() != identity);
        entries.push(entry);

        self.save(instance_id, &entries).await
    }

    pub async fn remove(&self, instance_id: Uuid, path: &str) -> Result<(), ContentError> {
        let mut entries = self.list(instance_id).await?;
        let before = entries.len();
        entries.retain(|entry| entry.path != path);

        if entries.len() == before {
            return Err(ContentError::NotFound(path.to_string()));
        }

        // Validated on the way in, but this builds a filesystem path from
        // stored data, so it is checked again rather than trusted.
        cagalintry_proto::validate::validate_relative_path(path)?;
        remove_file_variants(&self.game_dir(instance_id), path).await;

        self.save(instance_id, &entries).await
    }

    /// Turn content on or off without deleting it.
    ///
    /// Minecraft loads by file extension, so a disabled mod is simply renamed
    /// out of the way and renamed back when re-enabled.
    pub async fn set_enabled(
        &self,
        instance_id: Uuid,
        path: &str,
        enabled: bool,
    ) -> Result<(), ContentError> {
        cagalintry_proto::validate::validate_relative_path(path)?;

        let mut entries = self.list(instance_id).await?;
        let entry = entries
            .iter_mut()
            .find(|entry| entry.path == path)
            .ok_or_else(|| ContentError::NotFound(path.to_string()))?;

        let game_dir = self.game_dir(instance_id);
        let active = game_dir.join(path);
        let disabled = with_disabled_suffix(&active);

        let (from, to) = if enabled { (&disabled, &active) } else { (&active, &disabled) };

        if tokio::fs::metadata(from).await.is_ok() {
            tokio::fs::rename(from, to)
                .await
                .map_err(|source| ContentError::Io { path: from.display().to_string(), source })?;
        }

        entry.enabled = enabled;
        self.save(instance_id, &entries).await
    }

}

/// Delete a content file whether or not it is currently disabled.
async fn remove_file_variants(game_dir: &Path, relative: &str) {
    let active = game_dir.join(relative);
    let _ = tokio::fs::remove_file(&active).await;
    let _ = tokio::fs::remove_file(with_disabled_suffix(&active)).await;
}

fn with_disabled_suffix(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(DISABLED_SUFFIX);
    path.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cagalintry_proto::{ContentSource, EntryKind, Hashes, Side};

    fn store(name: &str) -> (ContentStore, DataDirs, PathBuf) {
        let root = std::env::temp_dir().join("cagalintry-content-tests").join(name);
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let dirs = DataDirs::with_root(&root);
        (ContentStore::new(dirs.clone()), dirs, root)
    }

    fn entry(project: &str, filename: &str, version: &str) -> PackEntry {
        PackEntry {
            kind: EntryKind::Mod,
            source: ContentSource::Modrinth {
                project_id: project.to_string(),
                version_id: version.to_string(),
            },
            path: format!("mods/{filename}"),
            hashes: Hashes { sha1: "a".repeat(40), sha512: "b".repeat(128) },
            size: 10,
            downloads: vec!["https://cdn.modrinth.com/data/x/y.jar".to_string()],
            side: Side::Both,
            enabled: true,
            name: Some(project.to_string()),
            version_number: Some(version.to_string()),
        }
    }

    /// Put a file on disk as if it had been downloaded.
    async fn place(dirs: &DataDirs, instance: Uuid, relative: &str) {
        let path = dirs.instance(&instance.to_string()).game_dir().join(relative);
        tokio::fs::create_dir_all(path.parent().unwrap()).await.unwrap();
        tokio::fs::write(&path, b"jar").await.unwrap();
    }

    #[tokio::test]
    async fn an_empty_instance_has_no_content() {
        let (store, _, root) = store("empty");
        assert!(store.list(Uuid::new_v4()).await.unwrap().is_empty());
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn content_is_recorded_and_can_be_removed() {
        let (store, dirs, root) = store("remove");
        let instance = Uuid::new_v4();

        store
            .save(instance, &[entry("sodium", "sodium-0.6.0.jar", "v1")])
            .await
            .unwrap();
        place(&dirs, instance, "mods/sodium-0.6.0.jar").await;

        assert_eq!(store.list(instance).await.unwrap().len(), 1);

        store.remove(instance, "mods/sodium-0.6.0.jar").await.unwrap();
        assert!(store.list(instance).await.unwrap().is_empty());

        let path = dirs.instance(&instance.to_string()).game_dir().join("mods/sodium-0.6.0.jar");
        assert!(tokio::fs::metadata(&path).await.is_err(), "file should be gone");

        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn removing_something_not_installed_is_an_error() {
        let (store, _, root) = store("missing");
        let result = store.remove(Uuid::new_v4(), "mods/nope.jar").await;
        assert!(matches!(result, Err(ContentError::NotFound(_))));
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn disabling_renames_the_file_rather_than_deleting_it() {
        let (store, dirs, root) = store("disable");
        let instance = Uuid::new_v4();
        let game_dir = dirs.instance(&instance.to_string()).game_dir();

        store.save(instance, &[entry("sodium", "sodium.jar", "v1")]).await.unwrap();
        place(&dirs, instance, "mods/sodium.jar").await;

        store.set_enabled(instance, "mods/sodium.jar", false).await.unwrap();

        assert!(tokio::fs::metadata(game_dir.join("mods/sodium.jar")).await.is_err());
        assert!(tokio::fs::metadata(game_dir.join("mods/sodium.jar.disabled")).await.is_ok());
        assert!(!store.list(instance).await.unwrap()[0].enabled);

        // And back again, with the file intact.
        store.set_enabled(instance, "mods/sodium.jar", true).await.unwrap();
        assert!(tokio::fs::metadata(game_dir.join("mods/sodium.jar")).await.is_ok());
        assert!(store.list(instance).await.unwrap()[0].enabled);

        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn removing_a_disabled_mod_deletes_the_renamed_file_too() {
        let (store, dirs, root) = store("remove-disabled");
        let instance = Uuid::new_v4();
        let game_dir = dirs.instance(&instance.to_string()).game_dir();

        store.save(instance, &[entry("sodium", "sodium.jar", "v1")]).await.unwrap();
        place(&dirs, instance, "mods/sodium.jar").await;
        store.set_enabled(instance, "mods/sodium.jar", false).await.unwrap();

        store.remove(instance, "mods/sodium.jar").await.unwrap();
        assert!(tokio::fs::metadata(game_dir.join("mods/sodium.jar.disabled")).await.is_err());

        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn reinstalling_a_project_replaces_its_previous_file() {
        // Two versions of one mod in `mods/` crashes the game on launch, so an
        // update must remove the old jar rather than sit alongside it.
        let (store, dirs, root) = store("replace");
        let instance = Uuid::new_v4();
        let game_dir = dirs.instance(&instance.to_string()).game_dir();

        store.save(instance, &[entry("sodium", "sodium-0.6.0.jar", "v1")]).await.unwrap();
        place(&dirs, instance, "mods/sodium-0.6.0.jar").await;

        // Stand in for install() without the download: same bookkeeping.
        let mut entries = store.list(instance).await.unwrap();
        let updated = entry("sodium", "sodium-0.6.1.jar", "v2");
        for existing in entries.iter().filter(|e| e.identity() == updated.identity()) {
            if existing.path != updated.path {
                super::remove_file_variants(&game_dir, &existing.path).await;
            }
        }
        entries.retain(|e| e.identity() != updated.identity());
        entries.push(updated);
        store.save(instance, &entries).await.unwrap();

        assert_eq!(store.list(instance).await.unwrap().len(), 1);
        assert!(tokio::fs::metadata(game_dir.join("mods/sodium-0.6.0.jar")).await.is_err());

        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[test]
    fn the_disabled_suffix_is_appended_to_the_whole_filename() {
        // `sodium.jar.disabled`, not `sodium.disabled` — the latter would be
        // a different file and re-enabling would restore the wrong name.
        let path = Path::new("/game/mods/sodium.jar");
        assert!(with_disabled_suffix(path).ends_with("sodium.jar.disabled"));
    }
}
