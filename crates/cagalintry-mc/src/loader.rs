//! Mod loader support.
//!
//! Fabric and Quilt both publish a ready-made version profile — a document in
//! Mojang's own format with `inheritsFrom` pointing at the vanilla version. So
//! installing a loader is: ask its metadata service for the profile, write it
//! where the installer already looks for version JSON, and let the existing
//! inheritance merge do the rest. No special cases anywhere downstream.
//!
//! NeoForge is not like this. It ships an installer jar whose processors have
//! to be executed locally to produce a patched client, and it is handled
//! separately.

use cagalintry_proto::LoaderKind;
use serde::{Deserialize, Serialize};

use cagalintry_net::Downloader;

use crate::paths::DataDirs;

const FABRIC_META: &str = "https://meta.fabricmc.net/v2";
const QUILT_META: &str = "https://meta.quiltmc.org/v3";
const NEOFORGE_MAVEN: &str =
    "https://maven.neoforged.net/releases/net/neoforged/neoforge/maven-metadata.xml";

#[derive(Debug, thiserror::Error)]
pub enum LoaderError {
    #[error(transparent)]
    Download(#[from] cagalintry_net::DownloadError),

    #[error("writing {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("{loader} has no builds for Minecraft {mc_version}")]
    Unsupported { loader: &'static str, mc_version: String },

    #[error("could not read the version profile {loader} returned: {source}")]
    Profile {
        loader: &'static str,
        #[source]
        source: serde_json::Error,
    },

    #[error("reading {path}: {source}")]
    Archive {
        path: String,
        #[source]
        source: zip::result::ZipError,
    },

    #[error("the NeoForge {version} installer uses format spec {spec}, which this launcher does not understand")]
    UnsupportedInstaller { version: String, spec: u32 },

    #[error("processor {jar} cannot be run: {reason}")]
    BadProcessor { jar: String, reason: String },

    #[error(
        "NeoForge install step {step} of {total} ({jar}) failed{}\n{detail}",
        .code.map(|c| format!(" with exit code {c}")).unwrap_or_default()
    )]
    ProcessorFailed {
        step: usize,
        total: usize,
        jar: String,
        code: Option<i32>,
        detail: String,
    },

    #[error("the NeoForge install finished but produced no {expected}")]
    ProcessorProducedNothing { expected: String },

    #[error("{path} did not match its expected checksum: expected {expected}, got {actual}")]
    OutputMismatch {
        path: String,
        expected: String,
        actual: String,
    },
}

/// One installable build of a loader.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoaderVersion {
    pub version: String,
    /// Whether the loader's own metadata marks this as a stable release.
    /// Unstable builds are still offered, just not preselected.
    pub stable: bool,
}

// --- Fabric ---------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct FabricLoaderEntry {
    loader: FabricLoaderInfo,
}

#[derive(Debug, Deserialize)]
struct FabricLoaderInfo {
    version: String,
    #[serde(default)]
    stable: bool,
}

// --- Quilt ----------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct QuiltLoaderEntry {
    loader: QuiltLoaderInfo,
}

#[derive(Debug, Deserialize)]
struct QuiltLoaderInfo {
    version: String,
}

pub struct LoaderInstaller {
    downloader: Downloader,
    dirs: DataDirs,
}

impl LoaderInstaller {
    pub fn new(downloader: Downloader, dirs: DataDirs) -> Self {
        Self { downloader, dirs }
    }

    /// Builds available for a Minecraft version, newest first.
    pub async fn list_versions(
        &self,
        kind: LoaderKind,
        mc_version: &str,
    ) -> Result<Vec<LoaderVersion>, LoaderError> {
        let versions = match kind {
            LoaderKind::Vanilla => Vec::new(),

            LoaderKind::Fabric => {
                let url = format!("{FABRIC_META}/versions/loader/{mc_version}");
                let entries: Vec<FabricLoaderEntry> = self.downloader.fetch_json(&url).await?;
                entries
                    .into_iter()
                    .map(|entry| LoaderVersion {
                        version: entry.loader.version,
                        stable: entry.loader.stable,
                    })
                    .collect()
            }

            LoaderKind::Quilt => {
                let url = format!("{QUILT_META}/versions/loader/{mc_version}");
                let entries: Vec<QuiltLoaderEntry> = self.downloader.fetch_json(&url).await?;
                entries
                    .into_iter()
                    .map(|entry| LoaderVersion {
                        // Quilt has no stable flag; the suffix is the signal.
                        stable: !is_prerelease(&entry.loader.version),
                        version: entry.loader.version,
                    })
                    .collect()
            }

            LoaderKind::NeoForge => self.list_neoforge_versions(mc_version).await?,
        };

        if versions.is_empty() && kind != LoaderKind::Vanilla {
            return Err(LoaderError::Unsupported {
                loader: kind.display_name(),
                mc_version: mc_version.to_string(),
            });
        }

        Ok(versions)
    }

    /// NeoForge publishes only a Maven metadata document, and its versions
    /// encode the Minecraft version they target: 1.21.4 becomes the 21.4.x
    /// series. Filtering on that prefix is how a build is matched to a game
    /// version, since nothing else in the document says.
    async fn list_neoforge_versions(
        &self,
        mc_version: &str,
    ) -> Result<Vec<LoaderVersion>, LoaderError> {
        let xml = self.downloader.fetch_bytes(NEOFORGE_MAVEN).await?;
        let xml = String::from_utf8_lossy(&xml);

        let Some(prefix) = neoforge_prefix(mc_version) else {
            return Ok(Vec::new());
        };

        let mut versions: Vec<LoaderVersion> = extract_xml_values(&xml, "version")
            .into_iter()
            .filter(|version| version.starts_with(&prefix))
            .map(|version| LoaderVersion {
                stable: !is_prerelease(&version),
                version,
            })
            .collect();

        versions.reverse(); // Maven metadata is oldest first.
        Ok(versions)
    }

    /// Make a loader's version profile available under `meta/versions`, so the
    /// ordinary install path can resolve it. Returns the version id to launch.
    ///
    /// `java` and `vanilla_client_jar` are only used by NeoForge, whose install
    /// derives a patched client by running tools locally. They are passed
    /// unconditionally so callers have one entry point rather than a match on
    /// loader kind.
    pub async fn ensure_profile(
        &self,
        kind: LoaderKind,
        mc_version: &str,
        loader_version: &str,
        java: &std::path::Path,
        vanilla_client_jar: &std::path::Path,
        progress: Option<&cagalintry_net::ProgressSender>,
    ) -> Result<String, LoaderError> {
        let version_id = profile_version_id(kind, mc_version, loader_version);

        let (name, url) = match kind {
            LoaderKind::Vanilla => return Ok(version_id),

            LoaderKind::Fabric => (
                "Fabric",
                format!("{FABRIC_META}/versions/loader/{mc_version}/{loader_version}/profile/json"),
            ),

            LoaderKind::Quilt => (
                "Quilt",
                format!("{QUILT_META}/versions/loader/{mc_version}/{loader_version}/profile/json"),
            ),

            LoaderKind::NeoForge => {
                // No ready-made profile exists: the installer's processors have
                // to run locally to derive a patched client.
                return crate::neoforge::NeoForgeInstaller::new(
                    self.downloader.clone(),
                    self.dirs.clone(),
                )
                .install(loader_version, java, vanilla_client_jar, progress)
                .await;
            }
        };

        let path = self.dirs.version_json(&version_id);
        if tokio::fs::metadata(&path).await.is_ok() {
            return Ok(version_id);
        }

        let bytes = self.downloader.fetch_bytes(&url).await?;

        // Parse before writing: a stored file that doesn't deserialise would be
        // treated as a valid cache entry on every later run.
        serde_json::from_slice::<crate::meta::VersionDetail>(&bytes)
            .map_err(|source| LoaderError::Profile { loader: name, source })?;

        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|source| LoaderError::Io { path: parent.display().to_string(), source })?;
        }
        tokio::fs::write(&path, &bytes)
            .await
            .map_err(|source| LoaderError::Io { path: path.display().to_string(), source })?;

        Ok(version_id)
    }
}

/// The version id a loader profile is published under, and therefore the id the
/// launcher resolves and launches.
///
/// These names are set by each loader's metadata service — the `id` field
/// inside the profile it returns has to match, or the installer would cache the
/// document under a name nothing looks up. Defined once here so nothing else
/// has to know the convention.
pub fn profile_version_id(kind: LoaderKind, mc_version: &str, loader_version: &str) -> String {
    match kind {
        // Not a loader: the Minecraft version is launched directly.
        LoaderKind::Vanilla => mc_version.to_string(),
        _ if loader_version.is_empty() => mc_version.to_string(),
        LoaderKind::Fabric => format!("fabric-loader-{loader_version}-{mc_version}"),
        LoaderKind::Quilt => format!("quilt-loader-{loader_version}-{mc_version}"),
        // NeoForge names its profile after itself alone; the Minecraft version
        // is implied by the loader version's own numbering.
        LoaderKind::NeoForge => format!("neoforge-{loader_version}"),
    }
}

/// The version prefix NeoForge builds for a Minecraft version share.
///
/// Nothing in the Maven metadata says which game version a build targets, so
/// the numbering is the only link. Two schemes exist:
///
/// - Minecraft `1.x`: drop the leading `1.`, so `1.21.4` builds are `21.4.*`
///   and `1.21` builds are `21.0.*`.
/// - Minecraft `26.x` onwards: the game version verbatim plus a build number,
///   so `26.2` builds are `26.2.*` (in practice `26.2.0.<build>`).
///
/// Snapshots have no NeoForge builds and produce no prefix.
fn neoforge_prefix(mc_version: &str) -> Option<String> {
    if let Some(rest) = mc_version.strip_prefix("1.") {
        let mut parts = rest.split('.');
        let major = parts.next()?;
        if major.is_empty() || !major.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        let minor = parts.next().unwrap_or("0");
        return Some(format!("{major}.{minor}."));
    }

    let numeric = mc_version.contains('.')
        && mc_version
            .split('.')
            .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()));

    numeric.then(|| format!("{mc_version}."))
}

/// Loader builds mark themselves with a suffix rather than a flag.
fn is_prerelease(version: &str) -> bool {
    let lowered = version.to_ascii_lowercase();
    ["-beta", "-alpha", "-rc", "-pre", "-snapshot"]
        .iter()
        .any(|marker| lowered.contains(marker))
}

/// Pull the text of every `<tag>` from a document.
///
/// Maven metadata is the one XML this launcher reads, and it is a flat list of
/// versions. A full parser would be a dependency earning its keep on one file.
fn extract_xml_values(xml: &str, tag: &str) -> Vec<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let mut values = Vec::new();
    let mut rest = xml;

    while let Some(start) = rest.find(&open) {
        let after = &rest[start + open.len()..];
        let Some(end) = after.find(&close) else { break };
        let value = after[..end].trim();
        if !value.is_empty() {
            values.push(value.to_string());
        }
        rest = &after[end + close.len()..];
    }

    values
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_fabric_loader_list() {
        let json = r#"[
          {"loader":{"separator":".","build":10,"maven":"net.fabricmc:fabric-loader:0.16.10",
                     "version":"0.16.10","stable":true},
           "intermediary":{"maven":"net.fabricmc:intermediary:1.21.4","version":"1.21.4","stable":true}},
          {"loader":{"version":"0.16.9","stable":false}}
        ]"#;
        let entries: Vec<FabricLoaderEntry> = serde_json::from_str(json).unwrap();
        assert_eq!(entries[0].loader.version, "0.16.10");
        assert!(entries[0].loader.stable);
        assert!(!entries[1].loader.stable);
    }

    #[test]
    fn neoforge_maps_the_legacy_one_dot_x_scheme() {
        assert_eq!(neoforge_prefix("1.21.4").as_deref(), Some("21.4."));
        assert_eq!(neoforge_prefix("1.20.1").as_deref(), Some("20.1."));
        // A version with no patch component targets the .0 series.
        assert_eq!(neoforge_prefix("1.21").as_deref(), Some("21.0."));
    }

    #[test]
    fn neoforge_maps_the_modern_scheme() {
        // Verified against the live Maven metadata: Minecraft 26.2 builds are
        // published as 26.2.0.<build>, and 26.1.2 builds as 26.1.2.<build>.
        assert_eq!(neoforge_prefix("26.2").as_deref(), Some("26.2."));
        assert_eq!(neoforge_prefix("26.1.2").as_deref(), Some("26.1.2."));
    }

    #[test]
    fn snapshots_have_no_neoforge_builds() {
        assert_eq!(neoforge_prefix("25w02a"), None);
        assert_eq!(neoforge_prefix("1.21-pre1"), None);
        assert_eq!(neoforge_prefix("26"), None);
    }

    #[test]
    fn prerelease_builds_are_recognised_by_suffix() {
        assert!(is_prerelease("21.4.30-beta"));
        assert!(is_prerelease("0.27.0-beta.1"));
        assert!(is_prerelease("1.0.0-rc.2"));
        assert!(!is_prerelease("21.4.30"));
        assert!(!is_prerelease("0.16.10"));
    }

    #[test]
    fn extracts_versions_from_maven_metadata() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
        <metadata>
          <groupId>net.neoforged</groupId>
          <artifactId>neoforge</artifactId>
          <versioning>
            <versions>
              <version>20.1.5</version>
              <version>21.4.30-beta</version>
              <version>21.4.31</version>
            </versions>
          </versioning>
        </metadata>"#;

        let versions = extract_xml_values(xml, "version");
        assert_eq!(versions, ["20.1.5", "21.4.30-beta", "21.4.31"]);
    }

    #[test]
    fn maven_extraction_survives_a_truncated_document() {
        // A half-downloaded document must not panic or loop.
        assert_eq!(extract_xml_values("<version>1.0</version><version>2.0", "version"), ["1.0"]);
        assert!(extract_xml_values("", "version").is_empty());
        assert!(extract_xml_values("<other>x</other>", "version").is_empty());
    }

    #[test]
    fn profile_ids_match_what_each_loader_publishes() {
        // These have to be exact: the id inside the downloaded profile must
        // equal the name it is cached under, or resolution looks up a file
        // that was never written.
        assert_eq!(
            profile_version_id(LoaderKind::Fabric, "1.21.4", "0.16.10"),
            "fabric-loader-0.16.10-1.21.4"
        );
        assert_eq!(
            profile_version_id(LoaderKind::Quilt, "1.21.4", "0.27.0"),
            "quilt-loader-0.27.0-1.21.4"
        );
        assert_eq!(
            profile_version_id(LoaderKind::NeoForge, "1.21.4", "21.4.30"),
            "neoforge-21.4.30"
        );
        assert_eq!(profile_version_id(LoaderKind::Vanilla, "1.21.4", ""), "1.21.4");
    }

    #[test]
    fn a_loader_without_a_pinned_version_falls_back_to_vanilla() {
        // An instance can name a loader before a build has been chosen; that
        // must launch vanilla rather than a profile id with a hole in it.
        assert_eq!(profile_version_id(LoaderKind::Fabric, "1.21.4", ""), "1.21.4");
    }

    #[test]
    fn vanilla_needs_no_profile() {
        // Not a loader; the Minecraft version is launched directly, so this
        // must not touch the network even with nonsense paths.
        let dirs = DataDirs::with_root("/tmp/cagalintry-loader-test");
        let installer = LoaderInstaller::new(Downloader::new().unwrap(), dirs);

        let id = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(installer.ensure_profile(
                LoaderKind::Vanilla,
                "1.21.4",
                "",
                std::path::Path::new("java"),
                std::path::Path::new("client.jar"),
                None,
            ))
            .unwrap();
        assert_eq!(id, "1.21.4");
    }
}
