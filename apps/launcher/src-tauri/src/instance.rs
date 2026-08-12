//! Instances: what the player actually sees in the Library.
//!
//! Each instance is a directory with an `instance.json` beside the game files.
//! Keeping the record in the directory it describes means an instance can be
//! copied, backed up or hand-inspected without a database, and a corrupt entry
//! can never take the whole library down with it.

use std::path::PathBuf;

use cagalintry_mc::DataDirs;
use cagalintry_proto::{LoaderKind, LoaderSpec};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum InstanceError {
    #[error("reading {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("{path} is not a valid instance: {source}")]
    Parse {
        path: String,
        #[source]
        source: serde_json::Error,
    },

    #[error("no instance with id {0}")]
    NotFound(Uuid),

    #[error("an instance needs a name")]
    EmptyName,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Instance {
    pub id: Uuid,
    pub name: String,
    pub mc_version: String,
    pub loader: LoaderSpec,

    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub last_played: Option<OffsetDateTime>,

    /// Maximum heap in mebibytes.
    #[serde(default = "default_memory")]
    pub max_memory_mb: u32,

    /// Overrides Java selection for this instance only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub java_path: Option<PathBuf>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra_jvm_args: Vec<String>,

    /// Set once this instance is bound to a synced pack.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pack: Option<PackLink>,
}

/// What ties an instance to a pack on the sync server, and the revision
/// currently on disk. The gap between this and the server's head is what turns
/// Play into Update.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackLink {
    pub pack_id: Uuid,
    pub installed_revision: u64,
}

fn default_memory() -> u32 {
    4096
}

impl Instance {
    pub fn new(name: impl Into<String>, mc_version: impl Into<String>, loader: LoaderSpec) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            mc_version: mc_version.into(),
            loader,
            created_at: OffsetDateTime::now_utc(),
            last_played: None,
            max_memory_mb: default_memory(),
            java_path: None,
            extra_jvm_args: Vec::new(),
            pack: None,
        }
    }

    /// The version id to resolve and launch.
    ///
    /// Vanilla instances launch the Minecraft version directly; a modded one
    /// launches the loader's profile, which inherits from it.
    pub fn version_id(&self) -> String {
        match (self.loader.kind, &self.loader.version) {
            (LoaderKind::Vanilla, _) | (_, None) => self.mc_version.clone(),
            (LoaderKind::Fabric, Some(v)) => format!("fabric-loader-{v}-{}", self.mc_version),
            (LoaderKind::Quilt, Some(v)) => format!("quilt-loader-{v}-{}", self.mc_version),
            (LoaderKind::NeoForge, Some(v)) => format!("neoforge-{v}"),
        }
    }
}

/// Reads and writes instances on disk.
pub struct InstanceStore {
    dirs: DataDirs,
}

impl InstanceStore {
    pub fn new(dirs: DataDirs) -> Self {
        Self { dirs }
    }

    /// Every instance, newest first.
    ///
    /// A directory that fails to parse is logged and skipped rather than
    /// failing the whole listing — one bad `instance.json` should not hide
    /// everything else the player owns.
    pub async fn list(&self) -> Result<Vec<Instance>, InstanceError> {
        let root = self.dirs.instances();
        let mut entries = match tokio::fs::read_dir(&root).await {
            Ok(entries) => entries,
            // Nothing created yet.
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(source) => {
                return Err(InstanceError::Io { path: root.display().to_string(), source });
            }
        };

        let mut instances = Vec::new();
        while let Ok(Some(entry)) = entries.next_entry().await {
            let config = entry.path().join("instance.json");
            let Ok(bytes) = tokio::fs::read(&config).await else {
                continue;
            };
            match serde_json::from_slice::<Instance>(&bytes) {
                Ok(instance) => instances.push(instance),
                Err(err) => {
                    tracing::warn!(path = %config.display(), error = %err, "skipping unreadable instance");
                }
            }
        }

        // Newest first: the instance you just made should be the one you see.
        instances.sort_by_key(|instance| std::cmp::Reverse(instance.created_at));
        Ok(instances)
    }

    pub async fn get(&self, id: Uuid) -> Result<Instance, InstanceError> {
        let path = self.dirs.instance(&id.to_string()).config_file();
        let bytes = tokio::fs::read(&path)
            .await
            .map_err(|_| InstanceError::NotFound(id))?;
        serde_json::from_slice(&bytes).map_err(|source| InstanceError::Parse {
            path: path.display().to_string(),
            source,
        })
    }

    pub async fn save(&self, instance: &Instance) -> Result<(), InstanceError> {
        if instance.name.trim().is_empty() {
            return Err(InstanceError::EmptyName);
        }

        let dirs = self.dirs.instance(&instance.id.to_string());
        dirs.ensure().await.map_err(|source| InstanceError::Io {
            path: dirs.root().display().to_string(),
            source,
        })?;

        let path = dirs.config_file();
        let json = serde_json::to_vec_pretty(instance).map_err(|source| InstanceError::Parse {
            path: path.display().to_string(),
            source,
        })?;

        tokio::fs::write(&path, json)
            .await
            .map_err(|source| InstanceError::Io { path: path.display().to_string(), source })
    }

    pub async fn delete(&self, id: Uuid) -> Result<(), InstanceError> {
        let dirs = self.dirs.instance(&id.to_string());
        tokio::fs::remove_dir_all(dirs.root())
            .await
            .map_err(|source| InstanceError::Io {
                path: dirs.root().display().to_string(),
                source,
            })
    }

    pub fn game_dir(&self, id: Uuid) -> PathBuf {
        self.dirs.instance(&id.to_string()).game_dir()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store(name: &str) -> (InstanceStore, PathBuf) {
        let root = std::env::temp_dir().join("cagalintry-instance-tests").join(name);
        std::fs::create_dir_all(&root).unwrap();
        (InstanceStore::new(DataDirs::with_root(&root)), root)
    }

    #[test]
    fn a_vanilla_instance_launches_the_minecraft_version_directly() {
        let instance = Instance::new("Vanilla", "1.21.4", LoaderSpec::vanilla());
        assert_eq!(instance.version_id(), "1.21.4");
    }

    #[test]
    fn a_modded_instance_launches_its_loader_profile() {
        let fabric = Instance::new(
            "Modded",
            "1.21.4",
            LoaderSpec { kind: LoaderKind::Fabric, version: Some("0.16.10".into()) },
        );
        assert_eq!(fabric.version_id(), "fabric-loader-0.16.10-1.21.4");

        let quilt = Instance::new(
            "Q",
            "1.21.4",
            LoaderSpec { kind: LoaderKind::Quilt, version: Some("0.27.0".into()) },
        );
        assert_eq!(quilt.version_id(), "quilt-loader-0.27.0-1.21.4");
    }

    #[test]
    fn a_loader_without_a_pinned_version_falls_back_to_vanilla() {
        let instance = Instance::new(
            "Unpinned",
            "1.21.4",
            LoaderSpec { kind: LoaderKind::Fabric, version: None },
        );
        assert_eq!(instance.version_id(), "1.21.4");
    }

    #[tokio::test]
    async fn instances_round_trip_through_disk() {
        let (store, root) = store("round-trip");
        let instance = Instance::new("My Instance", "1.21.4", LoaderSpec::vanilla());

        store.save(&instance).await.unwrap();
        let loaded = store.get(instance.id).await.unwrap();

        assert_eq!(loaded.id, instance.id);
        assert_eq!(loaded.name, "My Instance");
        assert_eq!(loaded.mc_version, "1.21.4");
        assert_eq!(loaded.max_memory_mb, 4096);
        assert!(loaded.pack.is_none());

        tokio::fs::remove_dir_all(&root).await.ok();
    }

    #[tokio::test]
    async fn listing_an_empty_library_is_not_an_error() {
        let (store, root) = store("empty");
        assert!(store.list().await.unwrap().is_empty());
        tokio::fs::remove_dir_all(&root).await.ok();
    }

    #[tokio::test]
    async fn one_corrupt_instance_does_not_hide_the_others() {
        let (store, root) = store("corrupt");
        let good = Instance::new("Good", "1.21.4", LoaderSpec::vanilla());
        store.save(&good).await.unwrap();

        let broken = root.join("instances").join("not-a-uuid");
        tokio::fs::create_dir_all(&broken).await.unwrap();
        tokio::fs::write(broken.join("instance.json"), b"{ not json").await.unwrap();

        let listed = store.list().await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "Good");

        tokio::fs::remove_dir_all(&root).await.ok();
    }

    #[tokio::test]
    async fn an_instance_must_have_a_name() {
        let (store, root) = store("unnamed");
        let mut instance = Instance::new("   ", "1.21.4", LoaderSpec::vanilla());
        assert!(matches!(store.save(&instance).await, Err(InstanceError::EmptyName)));

        instance.name = "Named".into();
        store.save(&instance).await.unwrap();

        tokio::fs::remove_dir_all(&root).await.ok();
    }

    #[tokio::test]
    async fn deleting_removes_the_whole_instance_directory() {
        let (store, root) = store("delete");
        let instance = Instance::new("Doomed", "1.21.4", LoaderSpec::vanilla());
        store.save(&instance).await.unwrap();

        store.delete(instance.id).await.unwrap();
        assert!(matches!(
            store.get(instance.id).await,
            Err(InstanceError::NotFound(_))
        ));

        tokio::fs::remove_dir_all(&root).await.ok();
    }
}
