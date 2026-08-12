//! Modpacks: the one thing the player creates, and what the Library lists.
//!
//! A modpack is a name, a Minecraft version and a loader — created locally, and
//! playable immediately. There is no separate "instance" concept: the pack *is*
//! the installed game directory, and binding one to the sync server later only
//! adds a revision to track.
//!
//! Each pack is a directory with a `pack.json` beside the game files. Keeping
//! the record in the directory it describes means a pack can be copied, backed
//! up or hand-inspected without a database, and a corrupt entry can never take
//! the whole library down with it.

use std::path::PathBuf;

use cagalintry_mc::DataDirs;
use cagalintry_proto::LoaderSpec;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum PackError {
    #[error("reading {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("{path} is not a valid pack: {source}")]
    Parse {
        path: String,
        #[source]
        source: serde_json::Error,
    },

    #[error("no pack with id {0}")]
    NotFound(Uuid),

    #[error("a modpack needs a name")]
    EmptyName,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pack {
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

    /// Overrides Java selection for this pack only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub java_path: Option<PathBuf>,

    /// Always serialised, even when empty. Skipping it would send no key at
    /// all, and the frontend treats this as a list it can always read — an
    /// absent one blanks the Settings tab rather than showing an empty field.
    #[serde(default)]
    pub extra_jvm_args: Vec<String>,

    /// The revision on disk, once this pack is bound to the sync server.
    ///
    /// `None` means the pack has never synced — it is local, and Play is always
    /// the right button. When it is set, the gap between this and the server's
    /// head is what turns Play into Update.
    ///
    /// There is no separate link record because [`id`] doubles as the pack's
    /// identity on the server: joining somebody else's pack creates it locally
    /// under the same id.
    ///
    /// [`id`]: Self::id
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installed_revision: Option<u64>,
}

fn default_memory() -> u32 {
    8192
}

impl Pack {
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
            installed_revision: None,
        }
    }
}

/// Reads and writes packs on disk.
pub struct PackStore {
    dirs: DataDirs,
}

impl PackStore {
    pub fn new(dirs: DataDirs) -> Self {
        Self { dirs }
    }

    /// Every pack, newest first.
    ///
    /// A directory that fails to parse is logged and skipped rather than
    /// failing the whole listing — one bad `pack.json` should not hide
    /// everything else the player owns.
    pub async fn list(&self) -> Result<Vec<Pack>, PackError> {
        let root = self.dirs.packs();
        let mut entries = match tokio::fs::read_dir(&root).await {
            Ok(entries) => entries,
            // Nothing created yet.
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(source) => {
                return Err(PackError::Io { path: root.display().to_string(), source });
            }
        };

        let mut packs = Vec::new();
        while let Ok(Some(entry)) = entries.next_entry().await {
            let config = entry.path().join("pack.json");
            let Ok(bytes) = tokio::fs::read(&config).await else {
                continue;
            };
            match serde_json::from_slice::<Pack>(&bytes) {
                Ok(pack) => packs.push(pack),
                Err(err) => {
                    tracing::warn!(path = %config.display(), error = %err, "skipping unreadable pack");
                }
            }
        }

        // Newest first: the pack you just made should be the one you see.
        packs.sort_by_key(|pack| std::cmp::Reverse(pack.created_at));
        Ok(packs)
    }

    pub async fn get(&self, id: Uuid) -> Result<Pack, PackError> {
        let path = self.dirs.pack(&id.to_string()).config_file();
        let bytes = tokio::fs::read(&path)
            .await
            .map_err(|_| PackError::NotFound(id))?;
        serde_json::from_slice(&bytes).map_err(|source| PackError::Parse {
            path: path.display().to_string(),
            source,
        })
    }

    pub async fn save(&self, pack: &Pack) -> Result<(), PackError> {
        if pack.name.trim().is_empty() {
            return Err(PackError::EmptyName);
        }

        let dirs = self.dirs.pack(&pack.id.to_string());
        dirs.ensure().await.map_err(|source| PackError::Io {
            path: dirs.root().display().to_string(),
            source,
        })?;

        let path = dirs.config_file();
        let json = serde_json::to_vec_pretty(pack).map_err(|source| PackError::Parse {
            path: path.display().to_string(),
            source,
        })?;

        tokio::fs::write(&path, json)
            .await
            .map_err(|source| PackError::Io { path: path.display().to_string(), source })
    }

    pub async fn delete(&self, id: Uuid) -> Result<(), PackError> {
        let dirs = self.dirs.pack(&id.to_string());
        tokio::fs::remove_dir_all(dirs.root())
            .await
            .map_err(|source| PackError::Io {
                path: dirs.root().display().to_string(),
                source,
            })
    }

    pub fn game_dir(&self, id: Uuid) -> PathBuf {
        self.dirs.pack(&id.to_string()).game_dir()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store(name: &str) -> (PackStore, PathBuf) {
        let root = std::env::temp_dir().join("cagalintry-pack-tests").join(name);
        std::fs::create_dir_all(&root).unwrap();
        (PackStore::new(DataDirs::with_root(&root)), root)
    }

    #[tokio::test]
    async fn packs_round_trip_through_disk() {
        let (store, root) = store("round-trip");
        let pack = Pack::new("My Pack", "1.21.4", LoaderSpec::vanilla());

        store.save(&pack).await.unwrap();
        let loaded = store.get(pack.id).await.unwrap();

        assert_eq!(loaded.id, pack.id);
        assert_eq!(loaded.name, "My Pack");
        assert_eq!(loaded.mc_version, "1.21.4");
        assert_eq!(loaded.max_memory_mb, 8192);
        // A freshly created pack is local-only until it is bound to the server.
        assert!(loaded.installed_revision.is_none());

        tokio::fs::remove_dir_all(&root).await.ok();
    }

    #[test]
    fn every_pack_serialises_the_fields_the_frontend_reads_unconditionally() {
        // The Settings tab does `extraJvmArgs.join(" ")` with no guard, so
        // omitting the key on a pack with no custom arguments — which is every
        // freshly created one — blanks the whole page.
        let json = serde_json::to_value(Pack::new("P", "1.21.4", LoaderSpec::vanilla())).unwrap();
        assert_eq!(json["extraJvmArgs"], serde_json::json!([]));
        assert!(json.get("maxMemoryMb").is_some());
    }

    #[tokio::test]
    async fn listing_an_empty_library_is_not_an_error() {
        let (store, root) = store("empty");
        assert!(store.list().await.unwrap().is_empty());
        tokio::fs::remove_dir_all(&root).await.ok();
    }

    #[tokio::test]
    async fn one_corrupt_pack_does_not_hide_the_others() {
        let (store, root) = store("corrupt");
        let good = Pack::new("Good", "1.21.4", LoaderSpec::vanilla());
        store.save(&good).await.unwrap();

        let broken = root.join("packs").join("not-a-uuid");
        tokio::fs::create_dir_all(&broken).await.unwrap();
        tokio::fs::write(broken.join("pack.json"), b"{ not json").await.unwrap();

        let listed = store.list().await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "Good");

        tokio::fs::remove_dir_all(&root).await.ok();
    }

    #[tokio::test]
    async fn a_pack_must_have_a_name() {
        let (store, root) = store("unnamed");
        let mut pack = Pack::new("   ", "1.21.4", LoaderSpec::vanilla());
        assert!(matches!(store.save(&pack).await, Err(PackError::EmptyName)));

        pack.name = "Named".into();
        store.save(&pack).await.unwrap();

        tokio::fs::remove_dir_all(&root).await.ok();
    }

    #[tokio::test]
    async fn deleting_removes_the_whole_pack_directory() {
        let (store, root) = store("delete");
        let pack = Pack::new("Doomed", "1.21.4", LoaderSpec::vanilla());
        store.save(&pack).await.unwrap();

        store.delete(pack.id).await.unwrap();
        assert!(matches!(
            store.get(pack.id).await,
            Err(PackError::NotFound(_))
        ));

        tokio::fs::remove_dir_all(&root).await.ok();
    }
}
