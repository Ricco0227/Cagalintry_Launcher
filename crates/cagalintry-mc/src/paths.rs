//! Where the launcher keeps things on disk.
//!
//! Libraries, assets and Java runtimes are shared across every modpack and
//! deduplicated by content, so a second modpack on the same version costs
//! almost nothing. Only what genuinely differs — mods, config, worlds, options
//! — lives inside the modpack itself.

use std::path::{Path, PathBuf};

/// Overrides the data directory. Pointing two launcher processes at two
/// directories is how sync is tested end-to-end on a single machine.
pub const DATA_DIR_ENV: &str = "CAGALINTRY_DATA_DIR";

#[derive(Debug, Clone)]
pub struct DataDirs {
    root: PathBuf,
}

impl DataDirs {
    /// `%APPDATA%\Cagalintry` on Windows, `~/.local/share/Cagalintry` on Linux,
    /// `~/Library/Application Support/Cagalintry` on macOS — unless overridden.
    pub fn discover() -> std::io::Result<Self> {
        let root = match std::env::var_os(DATA_DIR_ENV) {
            Some(path) => PathBuf::from(path),
            None => dirs::data_dir()
                .ok_or_else(|| {
                    std::io::Error::other("could not determine the platform data directory")
                })?
                .join("Cagalintry"),
        };
        Ok(Self { root })
    }

    pub fn with_root(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Version and loader metadata, keyed by version id.
    pub fn meta(&self) -> PathBuf {
        self.root.join("meta")
    }

    pub fn version_dir(&self, version_id: &str) -> PathBuf {
        self.meta().join("versions").join(version_id)
    }

    pub fn version_json(&self, version_id: &str) -> PathBuf {
        self.version_dir(version_id).join(format!("{version_id}.json"))
    }

    pub fn client_jar(&self, version_id: &str) -> PathBuf {
        self.version_dir(version_id).join(format!("{version_id}.jar"))
    }

    /// Maven-style library tree, shared by every modpack.
    pub fn libraries(&self) -> PathBuf {
        self.root.join("libraries")
    }

    pub fn library(&self, relative: &str) -> PathBuf {
        join_relative(&self.libraries(), relative)
    }

    pub fn assets(&self) -> PathBuf {
        self.root.join("assets")
    }

    pub fn asset_indexes(&self) -> PathBuf {
        self.assets().join("indexes")
    }

    pub fn asset_index(&self, id: &str) -> PathBuf {
        self.asset_indexes().join(format!("{id}.json"))
    }

    pub fn asset_objects(&self) -> PathBuf {
        self.assets().join("objects")
    }

    pub fn asset_object(&self, hash: &str) -> PathBuf {
        self.asset_objects().join(&hash[..2]).join(hash)
    }

    /// Pre-1.7 versions read assets by name from here instead of the object
    /// store, so those files are materialised per asset index.
    pub fn virtual_assets(&self, index_id: &str) -> PathBuf {
        self.assets().join("virtual").join(index_id)
    }

    /// Provisioned Java runtimes, one directory per Mojang component name.
    pub fn java(&self) -> PathBuf {
        self.root.join("java")
    }

    pub fn java_component(&self, component: &str) -> PathBuf {
        self.java().join(component)
    }

    /// Natives are extracted per version, not per modpack — the contents are
    /// a pure function of the version.
    pub fn natives(&self, version_id: &str) -> PathBuf {
        self.root.join("natives").join(version_id)
    }

    pub fn packs(&self) -> PathBuf {
        self.root.join("packs")
    }

    pub fn pack(&self, id: &str) -> PackDirs {
        PackDirs { root: self.packs().join(id) }
    }

    /// Partial downloads, staged updates, and anything else safe to delete.
    pub fn cache(&self) -> PathBuf {
        self.root.join("cache")
    }

    pub fn logs(&self) -> PathBuf {
        self.root.join("logs")
    }

    /// Create the directories that must exist before anything is written.
    pub async fn ensure(&self) -> std::io::Result<()> {
        for dir in [
            self.meta(),
            self.libraries(),
            self.asset_indexes(),
            self.asset_objects(),
            self.java(),
            self.packs(),
            self.cache(),
            self.logs(),
        ] {
            tokio::fs::create_dir_all(dir).await?;
        }
        Ok(())
    }
}

/// One modpack's own files.
#[derive(Debug, Clone)]
pub struct PackDirs {
    root: PathBuf,
}

impl PackDirs {
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The game's working directory — what vanilla calls `.minecraft`.
    ///
    /// Nested inside the modpack rather than being the modpack root so that
    /// launcher bookkeeping can sit alongside it without ever appearing in the
    /// game's own directory listing.
    pub fn game_dir(&self) -> PathBuf {
        self.root.join("minecraft")
    }

    pub fn config_file(&self) -> PathBuf {
        self.root.join("pack.json")
    }

    pub fn mods(&self) -> PathBuf {
        self.game_dir().join("mods")
    }

    pub fn resource_packs(&self) -> PathBuf {
        self.game_dir().join("resourcepacks")
    }

    pub fn shader_packs(&self) -> PathBuf {
        self.game_dir().join("shaderpacks")
    }

    pub fn config(&self) -> PathBuf {
        self.game_dir().join("config")
    }

    /// Never synced, never deleted by an update.
    pub fn saves(&self) -> PathBuf {
        self.game_dir().join("saves")
    }

    /// Where a pack update assembles files before they are moved into place, so
    /// an interrupted update leaves the modpack untouched rather than half
    /// written. Inside the modpack so the move is same-volume, and therefore
    /// atomic-ish rather than a copy.
    pub fn staging(&self) -> PathBuf {
        self.root.join(".cagalintry").join("staging")
    }

    pub fn logs(&self) -> PathBuf {
        self.root.join(".cagalintry").join("logs")
    }

    pub async fn ensure(&self) -> std::io::Result<()> {
        for dir in [self.game_dir(), self.mods(), self.resource_packs(), self.shader_packs()] {
            tokio::fs::create_dir_all(dir).await?;
        }
        Ok(())
    }
}

/// Join a `/`-separated relative path onto a base, refusing anything that would
/// escape it. Library paths come from version JSON we didn't author.
fn join_relative(base: &Path, relative: &str) -> PathBuf {
    let mut out = base.to_path_buf();
    for component in relative.split('/') {
        if component.is_empty() || component == "." || component == ".." {
            continue;
        }
        out.push(component);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dirs() -> DataDirs {
        DataDirs::with_root(if cfg!(windows) { r"C:\data" } else { "/data" })
    }

    #[test]
    fn shared_content_lives_outside_packs() {
        // The point of the layout: two modpacks on 1.21.4 share one copy of
        // every library, asset and Java runtime.
        let d = dirs();
        assert!(d.libraries().ends_with("libraries"));
        assert!(d.assets().ends_with("assets"));
        assert!(d.java_component("java-runtime-delta").ends_with("java-runtime-delta"));
        assert!(!d.libraries().starts_with(d.packs()));
    }

    #[test]
    fn assets_are_bucketed_by_hash_prefix() {
        let path = dirs().asset_object("abcdef0123456789");
        assert!(path.ends_with(Path::new("ab").join("abcdef0123456789")));
    }

    #[test]
    fn the_game_directory_is_nested_inside_the_pack() {
        // Keeps pack.json and staging out of the game's own directory.
        let pack = dirs().pack("abc");
        assert!(pack.game_dir().starts_with(pack.root()));
        assert!(pack.config_file().starts_with(pack.root()));
        assert!(!pack.config_file().starts_with(pack.game_dir()));
    }

    #[test]
    fn staging_is_on_the_same_volume_as_the_pack() {
        // Otherwise applying an update degrades from a rename into a copy.
        let pack = dirs().pack("abc");
        assert!(pack.staging().starts_with(pack.root()));
    }

    #[test]
    fn library_paths_cannot_escape_the_library_root() {
        let d = dirs();
        let escaped = d.library("../../../Windows/System32/evil.dll");
        assert!(escaped.starts_with(d.libraries()));
        assert!(escaped.ends_with(Path::new("Windows").join("System32").join("evil.dll")));
    }

    #[test]
    fn library_coordinates_become_nested_directories() {
        let path = dirs().library("org/lwjgl/lwjgl/3.3.3/lwjgl-3.3.3.jar");
        assert!(path.ends_with(Path::new("org").join("lwjgl").join("lwjgl").join("3.3.3").join("lwjgl-3.3.3.jar")));
    }

    #[test]
    fn the_data_directory_can_be_overridden_for_side_by_side_profiles() {
        let custom = DataDirs::with_root("/tmp/profile-b");
        assert_eq!(custom.root(), Path::new("/tmp/profile-b"));
    }
}
