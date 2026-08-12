//! Getting a JRE that can run the requested version.
//!
//! Preference order is deliberate: Mojang's own runtime first, because it is
//! the exact build the version was tested against and it needs no cooperation
//! from the player. A system JVM is the fallback, and on Windows on ARM it is
//! frequently the *only* option — Mojang does not publish an arm64 runtime for
//! every component, and pretending otherwise would produce a confusing 404
//! rather than a useful message.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use cagalintry_net::{DownloadSpec, Downloader, ProgressSender};
use serde::{Deserialize, Serialize};

use crate::paths::DataDirs;

/// Index of every Mojang-published runtime, keyed by platform then component.
pub const JAVA_RUNTIME_MANIFEST_URL: &str =
    "https://launchermeta.mojang.com/v1/products/java-runtime/2ec0cc96c44e5a76b9c8b7c39df7210883d12871/all.json";

#[derive(Debug, thiserror::Error)]
pub enum JavaError {
    #[error(transparent)]
    Download(#[from] cagalintry_net::DownloadError),

    #[error("writing {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error(
        "no Java {major} runtime is available: Mojang publishes no `{component}` build for {platform}, \
         and no suitable Java was found on this system. Install Java {major} and set it in Settings."
    )]
    Unavailable { component: String, platform: String, major: u32 },

    #[error("the Java runtime at {0} is missing its executable")]
    MissingExecutable(PathBuf),
}

/// Which JVM a launch should use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JavaRuntime {
    pub executable: PathBuf,
    pub major_version: u32,
    pub source: JavaSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum JavaSource {
    /// Downloaded from Mojang — the build the version was tested with.
    Mojang,
    /// Found on this machine, via settings, JAVA_HOME, or PATH.
    System,
}

// ---------------------------------------------------------------------------
// Mojang's runtime index
// ---------------------------------------------------------------------------

/// platform -> component -> builds
pub type AllRuntimes = HashMap<String, HashMap<String, Vec<RuntimeBuild>>>;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RuntimeBuild {
    pub manifest: RuntimeManifestRef,
    pub version: RuntimeVersion,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RuntimeManifestRef {
    pub sha1: String,
    pub size: u64,
    pub url: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RuntimeVersion {
    pub name: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RuntimeManifest {
    pub files: HashMap<String, RuntimeFile>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum RuntimeFile {
    Directory,
    File {
        downloads: RuntimeFileDownloads,
        #[serde(default)]
        executable: bool,
    },
    Link {
        target: String,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RuntimeFileDownloads {
    /// Only `raw` is used. The `lzma` variant would need a decompressor for a
    /// one-off saving on a download that happens once per Java major version.
    pub raw: RuntimeManifestRef,
}

/// Mojang's key for this machine in the runtime index.
pub fn runtime_platform() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => "windows-x64",
        ("windows", "x86") => "windows-x86",
        ("windows", "aarch64") => "windows-arm64",
        ("macos", "aarch64") => "mac-os-arm64",
        ("macos", _) => "mac-os",
        ("linux", "x86") => "linux-i386",
        ("linux", _) => "linux",
        _ => "linux",
    }
}

pub struct JavaProvisioner {
    downloader: Downloader,
    dirs: DataDirs,
}

impl JavaProvisioner {
    pub fn new(downloader: Downloader, dirs: DataDirs) -> Self {
        Self { downloader, dirs }
    }

    /// Ensure a usable JVM exists and return how to invoke it.
    ///
    /// `override_path` short-circuits everything — it is the value from
    /// Settings, and a player who has set it means it.
    pub async fn provide(
        &self,
        component: &str,
        major_version: u32,
        override_path: Option<&Path>,
        progress: Option<&ProgressSender>,
    ) -> Result<JavaRuntime, JavaError> {
        if let Some(path) = override_path {
            return Ok(JavaRuntime {
                executable: path.to_path_buf(),
                major_version,
                source: JavaSource::System,
            });
        }

        // Already provisioned on a previous run.
        let installed = self.dirs.java_component(component);
        if let Some(executable) = find_java_executable(&installed).await {
            return Ok(JavaRuntime { executable, major_version, source: JavaSource::Mojang });
        }

        match self.install_mojang_runtime(component, progress).await {
            Ok(executable) => {
                return Ok(JavaRuntime { executable, major_version, source: JavaSource::Mojang });
            }
            Err(err) => {
                tracing::warn!(
                    component,
                    platform = runtime_platform(),
                    error = %err,
                    "no Mojang runtime available, falling back to a system JVM"
                );
            }
        }

        if let Some(executable) = discover_system_java(major_version).await {
            return Ok(JavaRuntime { executable, major_version, source: JavaSource::System });
        }

        Err(JavaError::Unavailable {
            component: component.to_string(),
            platform: runtime_platform().to_string(),
            major: major_version,
        })
    }

    async fn install_mojang_runtime(
        &self,
        component: &str,
        progress: Option<&ProgressSender>,
    ) -> Result<PathBuf, JavaError> {
        let all: AllRuntimes = self.downloader.fetch_json(JAVA_RUNTIME_MANIFEST_URL).await?;

        let build = all
            .get(runtime_platform())
            .and_then(|components| components.get(component))
            .and_then(|builds| builds.first())
            .ok_or_else(|| JavaError::Unavailable {
                component: component.to_string(),
                platform: runtime_platform().to_string(),
                major: 0,
            })?;

        let manifest: RuntimeManifest = self.downloader.fetch_json(&build.manifest.url).await?;
        let root = self.dirs.java_component(component);

        let mut specs = Vec::new();
        let mut executables = Vec::new();

        for (relative, file) in &manifest.files {
            let path = join_checked(&root, relative);
            match file {
                RuntimeFile::Directory => {
                    tokio::fs::create_dir_all(&path).await.map_err(|source| JavaError::Io {
                        path: path.display().to_string(),
                        source,
                    })?;
                }
                RuntimeFile::File { downloads, executable } => {
                    specs.push(
                        DownloadSpec::new(&downloads.raw.url, &path)
                            .with_sha1(&downloads.raw.sha1)
                            .with_size(downloads.raw.size),
                    );
                    if *executable {
                        executables.push(path);
                    }
                }
                // Symlinks inside the runtime (macOS mostly). Recreating them
                // needs no download.
                RuntimeFile::Link { target } => {
                    create_link(&path, target).await?;
                }
            }
        }

        self.downloader.download_all(&specs, progress).await?;

        for path in executables {
            mark_executable(&path).await?;
        }

        find_java_executable(&root)
            .await
            .ok_or_else(|| JavaError::MissingExecutable(root))
    }
}

/// Locate the `java` binary inside a provisioned runtime.
///
/// `java` rather than `javaw` on Windows: the console variant is what lets the
/// launcher capture the game's stdout and stderr, which is the whole basis of
/// the log viewer and crash reporting.
async fn find_java_executable(root: &Path) -> Option<PathBuf> {
    let candidates = if cfg!(windows) {
        vec![root.join("bin").join("java.exe")]
    } else {
        vec![
            root.join("bin").join("java"),
            // macOS runtimes are wrapped in a bundle.
            root.join("jre.bundle")
                .join("Contents")
                .join("Home")
                .join("bin")
                .join("java"),
        ]
    };

    for candidate in candidates {
        if tokio::fs::metadata(&candidate).await.is_ok() {
            return Some(candidate);
        }
    }
    None
}

/// Look for a JVM already on this machine that is new enough to run the version.
///
/// The version check is the whole point. `JAVA_HOME` very often points at an
/// older JDK than what is on `PATH` — this machine has 17 in `JAVA_HOME` and 25
/// on `PATH` — and handing Minecraft a too-old JVM produces an
/// `UnsupportedClassVersionError` that reads like a launcher bug rather than
/// "install a newer Java".
async fn discover_system_java(required_major: u32) -> Option<PathBuf> {
    let exe = if cfg!(windows) { "java.exe" } else { "java" };
    let mut candidates = Vec::new();

    if let Some(home) = std::env::var_os("JAVA_HOME") {
        candidates.push(PathBuf::from(home).join("bin").join(exe));
    }
    if let Some(path) = std::env::var_os("PATH") {
        candidates.extend(std::env::split_paths(&path).map(|dir| dir.join(exe)));
    }

    let mut best: Option<(u32, PathBuf)> = None;
    for candidate in candidates {
        if tokio::fs::metadata(&candidate).await.is_err() {
            continue;
        }
        let Some(major) = probe_java_major(&candidate).await else {
            continue;
        };
        if major < required_major {
            tracing::debug!(path = %candidate.display(), major, required_major, "system Java is too old");
            continue;
        }
        // Prefer the closest match above the requirement: a JVM far newer than
        // the version was built against is likelier to trip on a removed flag.
        if best.as_ref().is_none_or(|(current, _)| major < *current) {
            best = Some((major, candidate));
        }
    }

    best.map(|(_, path)| path)
}

/// Ask a JVM its version. Returns `None` if it can't be run or understood.
async fn probe_java_major(executable: &Path) -> Option<u32> {
    let output = tokio::process::Command::new(executable)
        .arg("-version")
        .output()
        .await
        .ok()?;

    // `java -version` writes to stderr, which is a well-known wart.
    let text = String::from_utf8_lossy(&output.stderr);
    parse_java_major(&text).or_else(|| parse_java_major(&String::from_utf8_lossy(&output.stdout)))
}

/// Extract the major version from `java -version` output.
///
/// Handles both the modern form (`openjdk version "21.0.3"`) and the legacy
/// `1.x` form (`java version "1.8.0_401"` is Java 8, not Java 1).
fn parse_java_major(text: &str) -> Option<u32> {
    let quoted = text.split('"').nth(1)?;
    let mut parts = quoted.split(['.', '_', '-']);
    let first: u32 = parts.next()?.parse().ok()?;

    if first == 1 {
        parts.next()?.parse().ok()
    } else {
        Some(first)
    }
}

#[cfg(unix)]
async fn mark_executable(path: &Path) -> Result<(), JavaError> {
    use std::os::unix::fs::PermissionsExt as _;
    let mut permissions = tokio::fs::metadata(path)
        .await
        .map_err(|source| JavaError::Io { path: path.display().to_string(), source })?
        .permissions();
    permissions.set_mode(permissions.mode() | 0o755);
    tokio::fs::set_permissions(path, permissions)
        .await
        .map_err(|source| JavaError::Io { path: path.display().to_string(), source })
}

#[cfg(not(unix))]
async fn mark_executable(_path: &Path) -> Result<(), JavaError> {
    // Windows has no executable bit.
    Ok(())
}

#[cfg(unix)]
async fn create_link(path: &Path, target: &str) -> Result<(), JavaError> {
    let io = |source| JavaError::Io { path: path.display().to_string(), source };
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(io)?;
    }
    let _ = tokio::fs::remove_file(path).await;
    tokio::fs::symlink(target, path).await.map_err(io)
}

#[cfg(not(unix))]
async fn create_link(_path: &Path, _target: &str) -> Result<(), JavaError> {
    // Only the macOS runtimes contain links, so there is nothing to do here.
    Ok(())
}

/// Join a manifest-supplied relative path, dropping any component that would
/// escape the runtime directory.
fn join_checked(base: &Path, relative: &str) -> PathBuf {
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

    #[test]
    fn this_machine_maps_to_a_mojang_platform_key() {
        let platform = runtime_platform();
        assert!(
            [
                "windows-x64",
                "windows-x86",
                "windows-arm64",
                "mac-os",
                "mac-os-arm64",
                "linux",
                "linux-i386",
            ]
            .contains(&platform),
            "unexpected platform key: {platform}"
        );
    }

    #[test]
    fn runtime_file_variants_parse() {
        let json = r#"{
          "files": {
            "bin": {"type": "directory"},
            "bin/java": {
              "type": "file",
              "executable": true,
              "downloads": {"raw": {"sha1": "aa", "size": 12, "url": "https://example.test/java"}}
            },
            "lib/link": {"type": "link", "target": "../other"}
          }
        }"#;
        let manifest: RuntimeManifest = serde_json::from_str(json).unwrap();
        assert_eq!(manifest.files.len(), 3);
        assert!(matches!(manifest.files["bin"], RuntimeFile::Directory));
        assert!(matches!(
            manifest.files["bin/java"],
            RuntimeFile::File { executable: true, .. }
        ));
        assert!(matches!(manifest.files["lib/link"], RuntimeFile::Link { .. }));
    }

    #[test]
    fn the_all_runtimes_index_parses() {
        let json = r#"{
          "windows-arm64": {
            "java-runtime-delta": [
              {"manifest": {"sha1":"bb","size":1,"url":"https://example.test/m.json"},
               "version": {"name":"21.0.3","released":"2024-01-01"}}
            ],
            "jre-legacy": []
          }
        }"#;
        let all: AllRuntimes = serde_json::from_str(json).unwrap();
        assert_eq!(all["windows-arm64"]["java-runtime-delta"].len(), 1);
        // An empty list is how Mojang says "not published for this platform".
        assert!(all["windows-arm64"]["jre-legacy"].first().is_none());
    }

    #[test]
    fn runtime_paths_cannot_escape_the_component_directory() {
        let base = Path::new("/data/java/java-runtime-delta");
        let escaped = join_checked(base, "../../../etc/passwd");
        assert!(escaped.starts_with(base));
    }

    #[test]
    fn parses_modern_java_version_output() {
        let text = "openjdk version \"21.0.3\" 2024-04-16\nOpenJDK Runtime Environment";
        assert_eq!(parse_java_major(text), Some(21));
        assert_eq!(parse_java_major("openjdk version \"25.0.2\" 2025-10-21"), Some(25));
        assert_eq!(parse_java_major("openjdk version \"17\""), Some(17));
    }

    #[test]
    fn parses_the_legacy_one_dot_x_scheme() {
        // Java 8 reports itself as 1.8.0_401; reading that as major version 1
        // would reject every JVM that can actually run older Minecraft.
        assert_eq!(parse_java_major("java version \"1.8.0_401\""), Some(8));
        assert_eq!(parse_java_major("java version \"1.7.0_80\""), Some(7));
    }

    #[test]
    fn unparseable_version_output_is_rejected_rather_than_guessed() {
        assert_eq!(parse_java_major("command not found"), None);
        assert_eq!(parse_java_major(""), None);
        assert_eq!(parse_java_major("version \"not-a-number\""), None);
    }

    #[tokio::test]
    async fn system_java_discovery_respects_the_required_version() {
        // Real probe against whatever this machine has. A JVM is only accepted
        // when it actually satisfies the requirement, so asking for something
        // absurd must find nothing even though Java is installed.
        assert!(discover_system_java(999).await.is_none());
    }

    #[tokio::test]
    async fn a_runtime_without_an_executable_is_not_considered_installed() {
        let dir = std::env::temp_dir().join("cagalintry-java-empty");
        tokio::fs::create_dir_all(&dir).await.unwrap();
        assert!(find_java_executable(&dir).await.is_none());
        tokio::fs::remove_dir_all(&dir).await.ok();
    }

    #[tokio::test]
    async fn an_explicit_override_wins_over_everything() {
        let dirs = DataDirs::with_root(std::env::temp_dir().join("cagalintry-java-override"));
        let provisioner = JavaProvisioner::new(Downloader::new().unwrap(), dirs);
        let chosen = Path::new("/custom/jdk/bin/java");

        let runtime = provisioner
            .provide("java-runtime-delta", 21, Some(chosen), None)
            .await
            .unwrap();

        // No network access, no probing — the player's setting is taken as-is.
        assert_eq!(runtime.executable, chosen);
        assert_eq!(runtime.source, JavaSource::System);
    }
}
