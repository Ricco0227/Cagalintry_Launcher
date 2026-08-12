//! NeoForge installation.
//!
//! Unlike Fabric and Quilt, NeoForge cannot be installed by fetching a version
//! profile. It ships an installer jar containing an `install_profile.json` that
//! declares a chain of *processors* — small Java tools that must be executed
//! locally, in order, to derive a patched client jar from the vanilla one. The
//! patch itself is a binary diff (`data/client.lzma`) that cannot legally or
//! practically be distributed pre-applied, which is why every launcher has to
//! run this chain rather than download a finished artifact.
//!
//! The chain for a recent build looks like: read NeoForm mappings, download
//! Mojang's official mappings, merge them, split the client jar into code and
//! resources, remap the code to intermediate names, then apply the binary
//! patch. Ten processors, of which the client needs six.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use cagalintry_net::{DownloadSpec, Downloader, ProgressSender};
use serde::Deserialize;

use crate::loader::LoaderError;
use crate::maven::MavenCoord;
use crate::meta::{Library, VersionDetail};
use crate::paths::DataDirs;

const NEOFORGE_MAVEN_BASE: &str = "https://maven.neoforged.net/releases/net/neoforged/neoforge";

/// The subset of `install_profile.json` needed to run an install.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InstallProfile {
    /// Format version. Only spec 1 exists, and refusing anything else is
    /// better than misinterpreting a future format.
    #[serde(default)]
    spec: u32,
    #[serde(default)]
    minecraft: String,
    #[serde(default)]
    data: HashMap<String, SidedValue>,
    #[serde(default)]
    processors: Vec<Processor>,
    #[serde(default)]
    libraries: Vec<Library>,
}

#[derive(Debug, Deserialize)]
struct SidedValue {
    #[serde(default)]
    client: String,
}

#[derive(Debug, Deserialize)]
struct Processor {
    /// Absent means "run on every side".
    #[serde(default)]
    sides: Option<Vec<String>>,
    jar: String,
    #[serde(default)]
    classpath: Vec<String>,
    #[serde(default)]
    args: Vec<String>,
    /// Expected `path -> sha1` pairs. Often absent.
    #[serde(default)]
    outputs: HashMap<String, String>,
}

impl Processor {
    fn runs_on_client(&self) -> bool {
        match &self.sides {
            None => true,
            Some(sides) => sides.iter().any(|side| side == "client"),
        }
    }
}

pub struct NeoForgeInstaller {
    downloader: Downloader,
    dirs: DataDirs,
}

impl NeoForgeInstaller {
    pub fn new(downloader: Downloader, dirs: DataDirs) -> Self {
        Self { downloader, dirs }
    }

    /// Install NeoForge, returning the version id to launch.
    ///
    /// `java` must be a JVM that can run the processor tools, and
    /// `vanilla_client_jar` the already-installed client for `mc_version` —
    /// the chain patches it rather than downloading its own.
    pub async fn install(
        &self,
        loader_version: &str,
        java: &Path,
        vanilla_client_jar: &Path,
        progress: Option<&ProgressSender>,
    ) -> Result<String, LoaderError> {
        let version_id = format!("neoforge-{loader_version}");

        let installer = self.download_installer(loader_version).await?;
        let contents = read_installer(&installer).await?;

        if contents.profile.spec != 1 {
            return Err(LoaderError::UnsupportedInstaller {
                version: loader_version.to_string(),
                spec: contents.profile.spec,
            });
        }

        // The version profile is written first so a launch can resolve it, and
        // is cheap to rewrite if the chain later fails.
        self.write_version_profile(&version_id, &contents.version_json).await?;

        let data = self.build_data_map(
            &contents.profile,
            &installer,
            &contents.data_files,
            vanilla_client_jar,
        );

        // The chain's whole purpose is producing this file. If it is already
        // there the install is done, and re-running would cost minutes.
        if let Some(patched) = data.get("PATCHED")
            && tokio::fs::metadata(patched).await.is_ok()
        {
            tracing::debug!(version_id, "NeoForge already installed");
            return Ok(version_id);
        }

        self.download_processor_libraries(&contents.profile, progress).await?;

        // Processors write into these paths; none of them create directories.
        for path in data.values() {
            if let Some(parent) = Path::new(path).parent()
                && parent.is_absolute()
            {
                let _ = tokio::fs::create_dir_all(parent).await;
            }
        }

        let client_processors: Vec<&Processor> = contents
            .profile
            .processors
            .iter()
            .filter(|processor| processor.runs_on_client())
            .collect();

        for (index, processor) in client_processors.iter().enumerate() {
            tracing::info!(
                step = index + 1,
                of = client_processors.len(),
                jar = %processor.jar,
                "running NeoForge processor"
            );
            self.run_processor(processor, &data, java, index + 1, client_processors.len())
                .await?;
        }

        // Only meaningful once the chain has run; a missing patched jar here
        // means a processor silently did nothing.
        if let Some(patched) = data.get("PATCHED")
            && tokio::fs::metadata(patched).await.is_err()
        {
            return Err(LoaderError::ProcessorProducedNothing {
                expected: patched.clone(),
            });
        }

        Ok(version_id)
    }

    async fn download_installer(&self, loader_version: &str) -> Result<PathBuf, LoaderError> {
        let url = format!(
            "{NEOFORGE_MAVEN_BASE}/{loader_version}/neoforge-{loader_version}-installer.jar"
        );
        let dest = self
            .dirs
            .cache()
            .join("neoforge")
            .join(format!("neoforge-{loader_version}-installer.jar"));

        // NeoForge publishes no hash alongside the installer, so presence is
        // the only cache signal available.
        if tokio::fs::metadata(&dest).await.is_err() {
            self.downloader.download(&DownloadSpec::new(url, &dest), None).await?;
        }
        Ok(dest)
    }

    async fn write_version_profile(
        &self,
        version_id: &str,
        version_json: &[u8],
    ) -> Result<(), LoaderError> {
        // Parse before writing: a stored document that doesn't deserialise
        // would be treated as a valid cache entry forever after.
        serde_json::from_slice::<VersionDetail>(version_json)
            .map_err(|source| LoaderError::Profile { loader: "NeoForge", source })?;

        let path = self.dirs.version_json(version_id);
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|source| LoaderError::Io { path: parent.display().to_string(), source })?;
        }
        tokio::fs::write(&path, version_json)
            .await
            .map_err(|source| LoaderError::Io { path: path.display().to_string(), source })
    }

    async fn download_processor_libraries(
        &self,
        profile: &InstallProfile,
        progress: Option<&ProgressSender>,
    ) -> Result<(), LoaderError> {
        let specs: Vec<DownloadSpec> = profile
            .libraries
            .iter()
            .filter_map(|library| {
                let artifact = library.downloads.as_ref()?.artifact.as_ref()?;
                let relative = artifact
                    .path
                    .clone()
                    .or_else(|| MavenCoord::parse(&library.name).map(|c| c.path()))?;
                // Some entries carry no URL; those are produced by the chain
                // itself rather than downloaded.
                if artifact.url.is_empty() {
                    return None;
                }
                Some(
                    DownloadSpec::new(&artifact.url, self.dirs.library(&relative))
                        .with_sha1(&artifact.sha1)
                        .with_size(artifact.size),
                )
            })
            .collect();

        self.downloader.download_all(&specs, progress).await?;
        Ok(())
    }

    /// Resolve every `data` entry, plus the built-in tokens, to a concrete
    /// string — usually an absolute path.
    fn build_data_map(
        &self,
        profile: &InstallProfile,
        installer: &Path,
        data_files: &HashMap<String, PathBuf>,
        vanilla_client_jar: &Path,
    ) -> HashMap<String, String> {
        let mut data = HashMap::new();

        for (key, value) in &profile.data {
            data.insert(key.clone(), self.resolve_value(&value.client, data_files));
        }

        // Supplied by the launcher rather than the profile.
        data.insert("SIDE".to_string(), "client".to_string());
        data.insert("MINECRAFT_JAR".to_string(), vanilla_client_jar.display().to_string());
        data.insert("ROOT".to_string(), self.dirs.root().display().to_string());
        data.insert("INSTALLER".to_string(), installer.display().to_string());
        data.insert("LIBRARY_DIR".to_string(), self.dirs.libraries().display().to_string());
        data.insert("MINECRAFT_VERSION".to_string(), profile.minecraft.clone());

        data
    }

    /// A data value is one of three things: a Maven coordinate in brackets, a
    /// path inside the installer jar, or a quoted literal.
    fn resolve_value(&self, value: &str, data_files: &HashMap<String, PathBuf>) -> String {
        if let Some(coord) = value.strip_prefix('[').and_then(|v| v.strip_suffix(']')) {
            return match MavenCoord::parse(coord) {
                Some(coord) => self.dirs.library(&coord.path()).display().to_string(),
                None => value.to_string(),
            };
        }
        if value.starts_with('/') {
            return data_files
                .get(value.trim_start_matches('/'))
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| value.to_string());
        }
        // `'1.21.4-20241203.161809'` — quoted to mark it as not a path.
        value.trim_matches('\'').to_string()
    }

    /// Substitute tokens in a processor argument.
    fn resolve_arg(&self, arg: &str, data: &HashMap<String, String>) -> String {
        if let Some(token) = arg.strip_prefix('{').and_then(|a| a.strip_suffix('}')) {
            return data.get(token).cloned().unwrap_or_else(|| arg.to_string());
        }
        if let Some(coord) = arg.strip_prefix('[').and_then(|a| a.strip_suffix(']')) {
            return match MavenCoord::parse(coord) {
                Some(coord) => self.dirs.library(&coord.path()).display().to_string(),
                None => arg.to_string(),
            };
        }
        arg.to_string()
    }

    async fn run_processor(
        &self,
        processor: &Processor,
        data: &HashMap<String, String>,
        java: &Path,
        step: usize,
        total: usize,
    ) -> Result<(), LoaderError> {
        let jar_coord = MavenCoord::parse(&processor.jar).ok_or_else(|| {
            LoaderError::BadProcessor {
                jar: processor.jar.clone(),
                reason: "not a Maven coordinate".to_string(),
            }
        })?;
        let jar_path = self.dirs.library(&jar_coord.path());

        let main_class = main_class_of(&jar_path).await?;

        // The processor's own jar always leads the classpath.
        let mut classpath = vec![jar_path];
        for entry in &processor.classpath {
            if let Some(coord) = MavenCoord::parse(entry) {
                let path = self.dirs.library(&coord.path());
                if !classpath.contains(&path) {
                    classpath.push(path);
                }
            }
        }

        let separator = if cfg!(windows) { ";" } else { ":" };
        let classpath = classpath
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(separator);

        let args: Vec<String> = processor
            .args
            .iter()
            .map(|arg| self.resolve_arg(arg, data))
            .collect();

        let mut command = tokio::process::Command::new(java);
        command.arg("-cp").arg(&classpath).arg(&main_class).args(&args);

        #[cfg(windows)]
        {
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }

        let output = command.output().await.map_err(|source| LoaderError::Io {
            path: java.display().to_string(),
            source,
        })?;

        if !output.status.success() {
            // The tail is where the actual complaint is; the head is banners.
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let detail: String = format!("{stdout}\n{stderr}")
                .lines()
                .filter(|line| !line.trim().is_empty())
                .rev()
                .take(12)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
                .join("\n");

            return Err(LoaderError::ProcessorFailed {
                step,
                total,
                jar: processor.jar.clone(),
                code: output.status.code(),
                detail,
            });
        }

        self.verify_outputs(processor, data).await
    }

    /// Check any declared `path -> sha1` outputs. Not every profile declares
    /// them, but where one does, a silently wrong artifact is worth catching
    /// here rather than as a crash on first launch.
    async fn verify_outputs(
        &self,
        processor: &Processor,
        data: &HashMap<String, String>,
    ) -> Result<(), LoaderError> {
        for (path_token, sha_token) in &processor.outputs {
            let path = self.resolve_arg(path_token, data);
            let expected = self.resolve_arg(sha_token, data);
            let expected = expected.trim_matches('\'');

            if expected.is_empty() {
                continue;
            }

            let actual = cagalintry_net::hash::sha1_file(Path::new(&path))
                .await
                .map_err(|_| LoaderError::ProcessorProducedNothing { expected: path.clone() })?;

            if !cagalintry_net::hash::hashes_match(&actual, expected) {
                return Err(LoaderError::OutputMismatch {
                    path,
                    expected: expected.to_string(),
                    actual,
                });
            }
        }
        Ok(())
    }
}

/// What was read out of the installer jar.
struct InstallerContents {
    profile: InstallProfile,
    version_json: Vec<u8>,
    /// `data/...` entries extracted to disk, keyed by their name in the jar.
    data_files: HashMap<String, PathBuf>,
}

/// Read the installer jar: its two JSON documents, plus the `data/` payload
/// the processors consume (notably the binary patch).
async fn read_installer(installer: &Path) -> Result<InstallerContents, LoaderError> {
    let installer = installer.to_path_buf();

    tokio::task::spawn_blocking(move || {
        let io = |source| LoaderError::Io { path: installer.display().to_string(), source };
        let file = std::fs::File::open(&installer).map_err(io)?;
        let mut archive = zip::ZipArchive::new(file).map_err(|source| LoaderError::Archive {
            path: installer.display().to_string(),
            source,
        })?;

        let read = |archive: &mut zip::ZipArchive<std::fs::File>, name: &str| -> Result<Vec<u8>, LoaderError> {
            let mut entry = archive.by_name(name).map_err(|source| LoaderError::Archive {
                path: format!("{} ({name})", installer.display()),
                source,
            })?;
            let mut bytes = Vec::new();
            std::io::copy(&mut entry, &mut bytes).map_err(io)?;
            Ok(bytes)
        };

        let profile_bytes = read(&mut archive, "install_profile.json")?;
        let version_json = read(&mut archive, "version.json")?;

        let profile: InstallProfile =
            serde_json::from_slice(&profile_bytes).map_err(|source| LoaderError::Profile {
                loader: "NeoForge",
                source,
            })?;

        // Extract alongside the installer so paths stay stable across runs.
        let data_dir = installer
            .parent()
            .unwrap_or(Path::new("."))
            .join(format!("{}-data", installer.file_stem().unwrap_or_default().to_string_lossy()));
        std::fs::create_dir_all(&data_dir).map_err(io)?;

        let mut data_files = HashMap::new();
        let names: Vec<String> = (0..archive.len())
            .filter_map(|i| archive.by_index(i).ok().map(|e| e.name().to_string()))
            .filter(|name| name.starts_with("data/") && !name.ends_with('/'))
            .collect();

        for name in names {
            let Some(file_name) = Path::new(&name).file_name() else { continue };
            let dest = data_dir.join(file_name);

            let mut entry = archive.by_name(&name).map_err(|source| LoaderError::Archive {
                path: format!("{} ({name})", installer.display()),
                source,
            })?;
            let mut out = std::fs::File::create(&dest).map_err(io)?;
            std::io::copy(&mut entry, &mut out).map_err(io)?;

            data_files.insert(name, dest);
        }

        Ok(InstallerContents { profile, version_json, data_files })
    })
    .await
    .map_err(|source| LoaderError::Io {
        path: "neoforge installer".to_string(),
        source: std::io::Error::other(source),
    })?
}

/// Read `Main-Class` from a jar's manifest.
///
/// The profile names the jar to run but not its entry point, so this is the
/// only way to know what to hand `java -cp`.
async fn main_class_of(jar: &Path) -> Result<String, LoaderError> {
    let jar = jar.to_path_buf();

    tokio::task::spawn_blocking(move || {
        let io = |source| LoaderError::Io { path: jar.display().to_string(), source };
        let file = std::fs::File::open(&jar).map_err(io)?;
        let mut archive = zip::ZipArchive::new(file).map_err(|source| LoaderError::Archive {
            path: jar.display().to_string(),
            source,
        })?;

        let mut manifest = String::new();
        {
            let mut entry =
                archive
                    .by_name("META-INF/MANIFEST.MF")
                    .map_err(|source| LoaderError::Archive {
                        path: jar.display().to_string(),
                        source,
                    })?;
            std::io::Read::read_to_string(&mut entry, &mut manifest).map_err(io)?;
        }

        parse_main_class(&manifest).ok_or_else(|| LoaderError::BadProcessor {
            jar: jar.display().to_string(),
            reason: "its manifest declares no Main-Class".to_string(),
        })
    })
    .await
    .map_err(|source| LoaderError::Io {
        path: "processor jar".to_string(),
        source: std::io::Error::other(source),
    })?
}

/// Extract `Main-Class` from manifest text.
///
/// Manifest lines wrap at 72 bytes with a leading space on continuations, and
/// a long class name genuinely does wrap — ignoring that yields a truncated
/// class name and a confusing `ClassNotFoundException`.
fn parse_main_class(manifest: &str) -> Option<String> {
    let mut value: Option<String> = None;

    for line in manifest.lines() {
        let line = line.strip_suffix('\r').unwrap_or(line);

        if let Some(rest) = line.strip_prefix("Main-Class:") {
            value = Some(rest.trim().to_string());
        } else if let Some(continuation) = line.strip_prefix(' ') {
            if let Some(existing) = value.as_mut() {
                existing.push_str(continuation.trim_end());
            }
        } else if value.is_some() {
            // A new header ends the value we were collecting.
            break;
        }
    }

    value.filter(|v| !v.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn installer() -> NeoForgeInstaller {
        NeoForgeInstaller::new(
            Downloader::new().unwrap(),
            DataDirs::with_root(if cfg!(windows) { r"C:\data" } else { "/data" }),
        )
    }

    #[test]
    fn parses_a_plain_manifest() {
        let manifest = "Manifest-Version: 1.0\r\nMain-Class: net.neoforged.Tool\r\n\r\n";
        assert_eq!(parse_main_class(manifest).as_deref(), Some("net.neoforged.Tool"));
    }

    #[test]
    fn joins_a_wrapped_main_class() {
        // Manifests wrap at 72 bytes; a long class name really does split, and
        // taking only the first line gives a class that does not exist.
        let manifest = concat!(
            "Manifest-Version: 1.0\r\n",
            "Main-Class: net.neoforged.installertools.binarypatcher.ConsoleTo\r\n",
            " ol\r\n",
            "\r\n"
        );
        assert_eq!(
            parse_main_class(manifest).as_deref(),
            Some("net.neoforged.installertools.binarypatcher.ConsoleTool")
        );
    }

    #[test]
    fn a_manifest_without_a_main_class_yields_nothing() {
        assert_eq!(parse_main_class("Manifest-Version: 1.0\r\n\r\n"), None);
        assert_eq!(parse_main_class(""), None);
    }

    #[test]
    fn a_following_header_ends_the_value() {
        let manifest = "Main-Class: a.B\r\nImplementation-Title: something\r\n";
        assert_eq!(parse_main_class(manifest).as_deref(), Some("a.B"));
    }

    #[test]
    fn processors_without_a_sides_list_run_everywhere() {
        let all = Processor {
            sides: None,
            jar: "a:b:1".into(),
            classpath: vec![],
            args: vec![],
            outputs: HashMap::new(),
        };
        assert!(all.runs_on_client());

        let server_only = Processor { sides: Some(vec!["server".into()]), ..all };
        assert!(!server_only.runs_on_client());
    }

    #[test]
    fn maven_data_values_resolve_to_library_paths() {
        let installer = installer();
        let resolved = installer.resolve_value(
            "[net.neoforged:neoforge:21.4.157:client]",
            &HashMap::new(),
        );
        assert!(resolved.ends_with("neoforge-21.4.157-client.jar"));
        assert!(resolved.contains("libraries"));
    }

    #[test]
    fn quoted_data_values_are_literals() {
        // MCP_VERSION is a version string, not a path.
        let resolved = installer().resolve_value("'1.21.4-20241203.161809'", &HashMap::new());
        assert_eq!(resolved, "1.21.4-20241203.161809");
    }

    #[test]
    fn slash_prefixed_data_values_come_from_the_installer_jar() {
        let mut files = HashMap::new();
        files.insert("data/client.lzma".to_string(), PathBuf::from("/tmp/x/client.lzma"));

        let resolved = installer().resolve_value("/data/client.lzma", &files);
        assert_eq!(resolved, PathBuf::from("/tmp/x/client.lzma").display().to_string());
    }

    #[test]
    fn braced_arguments_are_substituted_from_the_data_map() {
        let mut data = HashMap::new();
        data.insert("SIDE".to_string(), "client".to_string());
        data.insert("PATCHED".to_string(), "/data/libraries/patched.jar".to_string());

        let installer = installer();
        assert_eq!(installer.resolve_arg("{SIDE}", &data), "client");
        assert_eq!(installer.resolve_arg("{PATCHED}", &data), "/data/libraries/patched.jar");
        // Literals pass through untouched.
        assert_eq!(installer.resolve_arg("--task", &data), "--task");
        // An unknown token stays visible rather than becoming empty, so a
        // failure names the thing that was missing.
        assert_eq!(installer.resolve_arg("{NOPE}", &data), "{NOPE}");
    }

    #[test]
    fn bracketed_arguments_resolve_to_library_paths() {
        // Processor 3 passes the NeoForm archive this way.
        let resolved = installer().resolve_arg(
            "[net.neoforged:neoform:1.21.4-20241203.161809@zip]",
            &HashMap::new(),
        );
        assert!(resolved.ends_with("neoform-1.21.4-20241203.161809.zip"));
    }

    #[test]
    fn the_install_profile_shape_parses() {
        // Trimmed from a real NeoForge 21.4.157 installer.
        let json = r#"{
          "spec": 1,
          "profile": "NeoForge",
          "version": "neoforge-21.4.157",
          "minecraft": "1.21.4",
          "data": {
            "BINPATCH": {"client": "/data/client.lzma", "server": "/data/server.lzma"},
            "PATCHED": {"client": "[net.neoforged:neoforge:21.4.157:client]", "server": "x"},
            "MCP_VERSION": {"client": "'1.21.4-20241203.161809'", "server": "'x'"}
          },
          "processors": [
            {"sides": ["server"], "jar": "a:b:1", "classpath": [], "args": ["--x"]},
            {"jar": "net.neoforged.installertools:binarypatcher:2.1.2:fatjar",
             "classpath": ["net.neoforged.installertools:binarypatcher:2.1.2:fatjar"],
             "args": ["--clean", "{MC_SRG}", "--output", "{PATCHED}", "--apply", "{BINPATCH}"]}
          ],
          "libraries": [
            {"name": "net.neoforged.fancymodloader:loader:6.0.18",
             "downloads": {"artifact": {"sha1": "87957873971a693e716ff29f8b94f833f23c741f",
                                        "size": 514106,
                                        "url": "https://maven.neoforged.net/releases/x.jar",
                                        "path": "net/neoforged/fancymodloader/loader/6.0.18/loader-6.0.18.jar"}}}
          ]
        }"#;

        let profile: InstallProfile = serde_json::from_str(json).unwrap();
        assert_eq!(profile.spec, 1);
        assert_eq!(profile.minecraft, "1.21.4");
        assert_eq!(profile.processors.len(), 2);
        assert_eq!(profile.libraries.len(), 1);

        // Only the second processor is the client's business.
        let client: Vec<&Processor> =
            profile.processors.iter().filter(|p| p.runs_on_client()).collect();
        assert_eq!(client.len(), 1);
        assert!(client[0].jar.contains("binarypatcher"));
    }

    /// The only way to know the chain actually works.
    ///
    /// Runs the real installer for a real build: downloads the vanilla client
    /// jar, provisions a JVM, then executes every client-side processor and
    /// checks the patched client exists at the end. Ignored by default because
    /// it needs the network, a few minutes, and roughly 150 MB.
    ///
    /// Run with:
    ///   cargo test -p cagalintry-mc --  --ignored --nocapture neoforge_installs
    #[tokio::test]
    #[ignore = "network, ~150 MB and several minutes"]
    async fn neoforge_installs_end_to_end() {
        const MC: &str = "1.21.4";
        const NEOFORGE: &str = "21.4.157";

        // The shared data directory, so an already-provisioned JVM and any
        // cached libraries are reused rather than fetched again.
        let dirs = DataDirs::discover().unwrap();
        dirs.ensure().await.unwrap();
        let downloader = Downloader::new().unwrap();

        let vanilla = crate::Installer::new(downloader.clone(), dirs.clone());
        let manifest = vanilla.version_manifest().await.unwrap();
        let detail = vanilla.version_detail(&manifest, MC).await.unwrap();
        let resolved = vanilla.resolve(&detail).unwrap();

        // Only the client jar is needed; the chain patches it and never looks
        // at assets, so several hundred megabytes are skipped.
        let client = detail.downloads.as_ref().unwrap().client.as_ref().unwrap();
        downloader
            .download(
                &DownloadSpec::new(&client.url, &resolved.client_jar)
                    .with_sha1(&client.sha1)
                    .with_size(client.size),
                None,
            )
            .await
            .unwrap();

        let java = crate::JavaProvisioner::new(downloader.clone(), dirs.clone())
            .provide(detail.java_component(), detail.java_major_version(), None, None)
            .await
            .unwrap();
        println!("using java: {}", java.executable.display());

        let version_id = NeoForgeInstaller::new(downloader, dirs.clone())
            .install(NEOFORGE, &java.executable, &resolved.client_jar, None)
            .await
            .expect("NeoForge install failed");

        assert_eq!(version_id, format!("neoforge-{NEOFORGE}"));

        // The whole point of the chain.
        let patched = dirs.library(&format!(
            "net/neoforged/neoforge/{NEOFORGE}/neoforge-{NEOFORGE}-client.jar"
        ));
        let metadata = tokio::fs::metadata(&patched)
            .await
            .unwrap_or_else(|_| panic!("no patched client at {}", patched.display()));
        assert!(metadata.len() > 0, "patched client is empty");
        println!("patched client: {} ({} bytes)", patched.display(), metadata.len());

        // And the profile the launcher will resolve and launch.
        let profile = dirs.version_json(&version_id);
        assert!(tokio::fs::metadata(&profile).await.is_ok());
    }

    #[test]
    fn built_in_tokens_are_available_to_processors() {
        let profile: InstallProfile =
            serde_json::from_str(r#"{"spec":1,"minecraft":"1.21.4","data":{},"processors":[],"libraries":[]}"#)
                .unwrap();

        let data = installer().build_data_map(
            &profile,
            Path::new("/cache/installer.jar"),
            &HashMap::new(),
            Path::new("/meta/1.21.4.jar"),
        );

        assert_eq!(data["SIDE"], "client");
        assert_eq!(data["MINECRAFT_VERSION"], "1.21.4");
        assert!(data["MINECRAFT_JAR"].contains("1.21.4.jar"));
        assert!(data.contains_key("ROOT"));
        assert!(data.contains_key("INSTALLER"));
        assert!(data.contains_key("LIBRARY_DIR"));
    }
}
