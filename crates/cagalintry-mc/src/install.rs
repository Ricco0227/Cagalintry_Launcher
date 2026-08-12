//! Turning a version id into a complete, verified installation on disk.
//!
//! Resolution is separated from downloading: [`Installer::resolve`] works out
//! *what* a version needs (classpath, natives, assets, Java) purely from
//! metadata, and [`Installer::install`] then makes it true. Keeping the two
//! apart means the plan can be inspected, counted and shown to the player
//! before a single byte is fetched.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use cagalintry_net::{DownloadSpec, Downloader, ProgressSender};

use crate::maven::MavenCoord;
use crate::meta::{
    Argument, AssetIndex, AssetIndexRef, Library, VERSION_MANIFEST_URL, VersionDetail,
    VersionManifest,
};
use crate::paths::DataDirs;
use crate::rules::{Platform, rules_allow};

/// Loader profiles inherit from vanilla, which never inherits further. A cycle
/// would otherwise hang resolution.
const MAX_INHERITANCE_DEPTH: usize = 8;

#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    #[error(transparent)]
    Download(#[from] cagalintry_net::DownloadError),

    #[error("reading {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("no such Minecraft version: {0}")]
    UnknownVersion(String),

    #[error("version {0} is missing a client download")]
    NoClientJar(String),

    #[error("version {0} does not say which class to run")]
    NoMainClass(String),

    #[error("version {0} has no asset index")]
    NoAssetIndex(String),

    #[error("version inheritance from {0} is too deep or circular")]
    InheritanceLoop(String),

    #[error("parsing metadata for {version}: {source}")]
    Parse {
        version: String,
        #[source]
        source: serde_json::Error,
    },

    #[error("extracting natives from {path}: {source}")]
    Extract {
        path: String,
        #[source]
        source: zip::result::ZipError,
    },
}

/// A native library jar that must be unpacked before launch.
#[derive(Debug, Clone)]
pub struct NativeLibrary {
    pub jar: PathBuf,
    /// Paths inside the jar that must not be extracted.
    pub exclude: Vec<String>,
}

/// Everything needed to launch, once [`Installer::install`] has run.
#[derive(Debug, Clone)]
pub struct ResolvedVersion {
    pub detail: VersionDetail,
    /// In classpath order. The client jar is last, so anything a loader patches
    /// is found first.
    pub classpath: Vec<PathBuf>,
    pub natives: Vec<NativeLibrary>,
    /// The value `${natives_directory}` expands to.
    pub natives_dir: PathBuf,
    /// Where native jars are actually unpacked.
    ///
    /// Not always the same as `natives_dir`: versions from the 1.21.9 era
    /// onwards set `-Djava.library.path=${natives_directory}/java` and use
    /// sibling directories as scratch space for JNA, LWJGL and Netty. Unpacking
    /// to the root on those versions produces `UnsatisfiedLinkError: Failed to
    /// locate library: lwjgl.dll`, so the directory is read out of the version's
    /// own arguments rather than assumed.
    pub natives_extract_dir: PathBuf,
    pub client_jar: PathBuf,
    pub asset_index: AssetIndexRef,
    pub main_class: String,
    /// Files that must exist before launching, excluding assets. Internal:
    /// callers see the summary via [`Installer::plan`] rather than the specs.
    pub(crate) downloads: Vec<DownloadSpec>,
}

impl ResolvedVersion {
    pub fn java_component(&self) -> &str {
        self.detail.java_component()
    }

    pub fn java_major_version(&self) -> u32 {
        self.detail.java_major_version()
    }
}

/// Progress across a whole install, for the UI's task drawer.
#[derive(Debug, Clone, Copy, Default)]
pub struct InstallPlan {
    pub file_count: usize,
    pub total_bytes: u64,
}

pub struct Installer {
    downloader: Downloader,
    dirs: DataDirs,
    platform: Platform,
}

impl Installer {
    pub fn new(downloader: Downloader, dirs: DataDirs) -> Self {
        Self { downloader, dirs, platform: Platform::current() }
    }

    /// Override the platform. Only useful for tests that exercise resolution
    /// for a machine we aren't running on.
    pub fn with_platform(mut self, platform: Platform) -> Self {
        self.platform = platform;
        self
    }

    pub fn dirs(&self) -> &DataDirs {
        &self.dirs
    }

    pub fn downloader(&self) -> &Downloader {
        &self.downloader
    }

    pub async fn version_manifest(&self) -> Result<VersionManifest, InstallError> {
        Ok(self.downloader.fetch_json(VERSION_MANIFEST_URL).await?)
    }

    /// Load a version's JSON, fetching and caching it if absent, then fold in
    /// anything it inherits from.
    pub async fn version_detail(
        &self,
        manifest: &VersionManifest,
        version_id: &str,
    ) -> Result<VersionDetail, InstallError> {
        let mut detail = self.load_version_json(manifest, version_id).await?;

        let mut depth = 0;
        while let Some(parent_id) = detail.inherits_from.clone() {
            depth += 1;
            if depth > MAX_INHERITANCE_DEPTH {
                return Err(InstallError::InheritanceLoop(version_id.to_string()));
            }
            let parent = self.load_version_json(manifest, &parent_id).await?;
            detail = VersionDetail::merge_inherited(&parent, &detail);
        }

        Ok(detail)
    }

    async fn load_version_json(
        &self,
        manifest: &VersionManifest,
        version_id: &str,
    ) -> Result<VersionDetail, InstallError> {
        let path = self.dirs.version_json(version_id);

        // A locally installed loader profile has no manifest entry at all, so a
        // cached file is the only source for it.
        if let Ok(bytes) = tokio::fs::read(&path).await
            && let Ok(detail) = serde_json::from_slice::<VersionDetail>(&bytes)
        {
            return Ok(detail);
        }

        let entry = manifest
            .find(version_id)
            .ok_or_else(|| InstallError::UnknownVersion(version_id.to_string()))?;

        let mut spec = DownloadSpec::new(&entry.url, &path);
        if let Some(sha1) = &entry.sha1 {
            spec = spec.with_sha1(sha1);
        }
        self.downloader.download(&spec, None).await?;

        let bytes = tokio::fs::read(&path)
            .await
            .map_err(|source| InstallError::Io { path: path.display().to_string(), source })?;

        serde_json::from_slice(&bytes).map_err(|source| InstallError::Parse {
            version: version_id.to_string(),
            source,
        })
    }

    /// Work out what this version needs, without fetching any of it.
    pub fn resolve(&self, detail: &VersionDetail) -> Result<ResolvedVersion, InstallError> {
        let version_id = detail.id.clone();

        let main_class = detail
            .main_class
            .clone()
            .ok_or_else(|| InstallError::NoMainClass(version_id.clone()))?;

        let asset_index = detail
            .asset_index
            .clone()
            .ok_or_else(|| InstallError::NoAssetIndex(version_id.clone()))?;

        let client = detail
            .downloads
            .as_ref()
            .and_then(|d| d.client.as_ref())
            .ok_or_else(|| InstallError::NoClientJar(version_id.clone()))?;

        let client_jar = self.dirs.client_jar(&version_id);
        let mut downloads = vec![
            DownloadSpec::new(&client.url, &client_jar)
                .with_sha1(&client.sha1)
                .with_size(client.size),
            DownloadSpec::new(&asset_index.url, self.dirs.asset_index(&asset_index.id))
                .with_sha1(&asset_index.sha1),
        ];

        let mut classpath = Vec::new();
        let mut natives = Vec::new();
        // Loader libraries are prepended to the list by inheritance merging, so
        // taking the first entry per artifact gives the loader's version — which
        // is what must win when it ships a patched copy.
        let mut seen: HashSet<String> = HashSet::new();

        for library in &detail.libraries {
            if !rules_allow(&library.rules, &self.platform) {
                continue;
            }

            for resolved in self.resolve_library(library) {
                if !seen.insert(resolved.key) {
                    continue;
                }
                if let Some(spec) = resolved.download {
                    downloads.push(spec);
                }
                if resolved.native {
                    natives.push(NativeLibrary { jar: resolved.path, exclude: resolved.exclude });
                } else {
                    classpath.push(resolved.path);
                }
            }
        }

        // Vanilla classes come last so a loader's patched copies shadow them.
        classpath.push(client_jar.clone());

        let natives_dir = self.dirs.natives(&version_id);
        let natives_extract_dir = natives_extract_dir(detail, &natives_dir);

        Ok(ResolvedVersion {
            detail: detail.clone(),
            classpath,
            natives,
            natives_extract_dir,
            natives_dir,
            client_jar,
            asset_index,
            main_class,
            downloads,
        })
    }

    /// One library entry can yield a classpath artifact, a native artifact, or
    /// both — the pre-1.19 format put natives in a sibling classifier.
    fn resolve_library(&self, library: &Library) -> Vec<ResolvedLibraryArtifact> {
        let mut out = Vec::new();
        let coord = MavenCoord::parse(&library.name);

        // Native artifacts for other architectures. Mojang's rules only
        // distinguish operating systems, so without this every Windows machine
        // would install the x64, x86 and arm64 builds on top of each other.
        if let Some(arch) = coord.as_ref().and_then(MavenCoord::native_arch)
            && arch != self.platform.arch
        {
            return out;
        }

        let exclude = library
            .extract
            .as_ref()
            .map(|e| e.exclude.clone())
            .unwrap_or_default();

        // Modern format, and the common case: a single artifact whose
        // classifier says whether it is a native.
        if let Some(artifact) = library.downloads.as_ref().and_then(|d| d.artifact.as_ref()) {
            let relative = artifact
                .path
                .clone()
                .or_else(|| coord.as_ref().map(MavenCoord::path));

            if let Some(relative) = relative {
                let path = self.dirs.library(&relative);
                out.push(ResolvedLibraryArtifact {
                    key: relative.clone(),
                    download: Some(
                        DownloadSpec::new(&artifact.url, &path)
                            .with_sha1(&artifact.sha1)
                            .with_size(artifact.size),
                    ),
                    native: coord.as_ref().is_some_and(MavenCoord::is_native),
                    exclude: exclude.clone(),
                    path,
                });
            }
        }

        // Pre-1.19 format: `natives` maps this OS to a key in `classifiers`.
        if let Some(natives) = &library.natives
            && let Some(classifier) = natives.get(&self.platform.os)
        {
            // `${arch}` expands to 32 or 64 in these old entries.
            let classifier = classifier.replace(
                "${arch}",
                if self.platform.arch == "x86" { "32" } else { "64" },
            );

            if let Some(artifact) = library
                .downloads
                .as_ref()
                .and_then(|d| d.classifiers.as_ref())
                .and_then(|c| c.get(&classifier))
            {
                let relative = artifact
                    .path
                    .clone()
                    .or_else(|| coord.as_ref().map(MavenCoord::path))
                    .unwrap_or_else(|| format!("{}-{classifier}.jar", library.name));
                let path = self.dirs.library(&relative);
                out.push(ResolvedLibraryArtifact {
                    key: relative.clone(),
                    download: Some(
                        DownloadSpec::new(&artifact.url, &path)
                            .with_sha1(&artifact.sha1)
                            .with_size(artifact.size),
                    ),
                    native: true,
                    exclude: exclude.clone(),
                    path,
                });
            }
        }

        // Loader libraries frequently carry only a repository URL and expect
        // the path to be derived from the coordinate.
        if out.is_empty()
            && let Some(coord) = coord
        {
            let relative = coord.path();
            let path = self.dirs.library(&relative);
            let download = library.url.as_ref().map(|base| {
                DownloadSpec::new(format!("{}{relative}", ensure_trailing_slash(base)), &path)
            });
            out.push(ResolvedLibraryArtifact {
                key: relative,
                download,
                native: coord.is_native(),
                exclude,
                path,
            });
        }

        out
    }

    /// Total work implied by a resolved version, for progress reporting.
    ///
    /// Fetches the asset index first — it is a single small file, and without
    /// it the asset objects are invisible here, which reports a total far
    /// smaller than the download turns out to be.
    pub async fn plan(&self, resolved: &ResolvedVersion) -> Result<InstallPlan, InstallError> {
        self.ensure_asset_index(resolved).await?;

        let assets = self.asset_downloads(resolved).await?;
        Ok(InstallPlan {
            file_count: resolved.downloads.len() + assets.len(),
            total_bytes: resolved
                .downloads
                .iter()
                .chain(assets.iter())
                .filter_map(|s| s.size)
                .sum(),
        })
    }

    /// Download just the asset index. Cheap, and cached like everything else,
    /// so calling it before `install` costs nothing on the second run.
    async fn ensure_asset_index(&self, resolved: &ResolvedVersion) -> Result<(), InstallError> {
        let spec = DownloadSpec::new(
            &resolved.asset_index.url,
            self.dirs.asset_index(&resolved.asset_index.id),
        )
        .with_sha1(&resolved.asset_index.sha1);

        self.downloader.download(&spec, None).await?;
        Ok(())
    }

    /// Fetch everything the version needs and unpack its natives.
    pub async fn install(
        &self,
        resolved: &ResolvedVersion,
        progress: Option<&ProgressSender>,
    ) -> Result<(), InstallError> {
        self.dirs.ensure().await.map_err(|source| InstallError::Io {
            path: self.dirs.root().display().to_string(),
            source,
        })?;

        // The asset index has to land before its contents can be enumerated.
        self.downloader.download_all(&resolved.downloads, progress).await?;

        let assets = self.asset_downloads(resolved).await?;
        self.downloader.download_all(&assets, progress).await?;

        self.extract_natives(resolved).await?;
        Ok(())
    }

    async fn asset_downloads(
        &self,
        resolved: &ResolvedVersion,
    ) -> Result<Vec<DownloadSpec>, InstallError> {
        let path = self.dirs.asset_index(&resolved.asset_index.id);
        let Ok(bytes) = tokio::fs::read(&path).await else {
            // Not fetched yet; `install` downloads it before calling back here.
            return Ok(Vec::new());
        };

        let index: AssetIndex =
            serde_json::from_slice(&bytes).map_err(|source| InstallError::Parse {
                version: resolved.asset_index.id.clone(),
                source,
            })?;

        Ok(index
            .objects
            .values()
            .map(|object| {
                DownloadSpec::new(object.url(), self.dirs.asset_object(&object.hash))
                    .with_sha1(&object.hash)
                    .with_size(object.size)
            })
            .collect())
    }

    /// Unpack native jars into the version's natives directory.
    ///
    /// Skipped entirely when the directory already holds files: extraction is
    /// deterministic, and repeating it on every launch is pure delay.
    async fn extract_natives(&self, resolved: &ResolvedVersion) -> Result<(), InstallError> {
        let dir = resolved.natives_extract_dir.clone();
        let io_err = |path: &Path| {
            let path = path.display().to_string();
            move |source| InstallError::Io { path: path.clone(), source }
        };

        if directory_has_entries(&dir).await {
            return Ok(());
        }
        tokio::fs::create_dir_all(&dir).await.map_err(io_err(&dir))?;

        for native in &resolved.natives {
            let jar = native.jar.clone();
            let dir = dir.clone();
            let exclude = native.exclude.clone();

            // zip is a blocking API and these are multi-megabyte archives.
            tokio::task::spawn_blocking(move || extract_native_jar(&jar, &dir, &exclude))
                .await
                .map_err(|e| InstallError::Io {
                    path: native.jar.display().to_string(),
                    source: std::io::Error::other(e),
                })??;
        }

        Ok(())
    }
}

struct ResolvedLibraryArtifact {
    key: String,
    path: PathBuf,
    download: Option<DownloadSpec>,
    native: bool,
    exclude: Vec<String>,
}

/// Where this version expects to find its unpacked native libraries.
///
/// Read out of the version's own `-Djava.library.path=` argument so the layout
/// follows Mojang rather than a guess: older versions point straight at
/// `${natives_directory}`, newer ones at `${natives_directory}/java`.
fn natives_extract_dir(detail: &VersionDetail, natives_root: &Path) -> PathBuf {
    const FLAG: &str = "-Djava.library.path=";
    const VAR: &str = "${natives_directory}";

    let Some(arguments) = detail.arguments.as_ref() else {
        // Pre-1.13 versions declare no JVM arguments; the launcher supplies
        // `-Djava.library.path=<root>` itself.
        return natives_root.to_path_buf();
    };

    let value = arguments.jvm.iter().find_map(|argument| {
        let candidates: &[String] = match argument {
            Argument::Literal(value) => std::slice::from_ref(value),
            Argument::Conditional { value, .. } => value.as_slice(),
        };
        candidates
            .iter()
            .find_map(|candidate| candidate.strip_prefix(FLAG))
    });

    match value.and_then(|value| value.strip_prefix(VAR)) {
        // e.g. "/java" — the modern layout.
        Some(suffix) => {
            let mut dir = natives_root.to_path_buf();
            for part in suffix.split(['/', '\\']).filter(|p| !p.is_empty()) {
                dir.push(part);
            }
            dir
        }
        // Either the argument is absent, or it points somewhere we don't model;
        // the root is the long-standing convention and the safe default.
        None => natives_root.to_path_buf(),
    }
}

fn ensure_trailing_slash(base: &str) -> String {
    if base.ends_with('/') { base.to_string() } else { format!("{base}/") }
}

async fn directory_has_entries(dir: &Path) -> bool {
    match tokio::fs::read_dir(dir).await {
        Ok(mut entries) => entries.next_entry().await.ok().flatten().is_some(),
        Err(_) => false,
    }
}

/// Extract the shared libraries from a native jar.
///
/// Only the top-level files are wanted: `META-INF` holds signatures, and the
/// `exclude` list names anything else the version explicitly rejects. Entry
/// names come from an archive we did not build, so traversal is checked here
/// too rather than trusted.
fn extract_native_jar(jar: &Path, dest: &Path, exclude: &[String]) -> Result<(), InstallError> {
    let file = std::fs::File::open(jar).map_err(|source| InstallError::Io {
        path: jar.display().to_string(),
        source,
    })?;

    let mut archive = zip::ZipArchive::new(file).map_err(|source| InstallError::Extract {
        path: jar.display().to_string(),
        source,
    })?;

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|source| InstallError::Extract {
            path: jar.display().to_string(),
            source,
        })?;

        let Some(enclosed) = entry.enclosed_name() else {
            continue; // absolute or traversing path — refuse it
        };
        let name = entry.name().to_string();

        if entry.is_dir()
            || name.starts_with("META-INF/")
            || exclude.iter().any(|prefix| name.starts_with(prefix))
        {
            continue;
        }

        // Flatten: the JVM looks for natives directly in java.library.path.
        let Some(file_name) = enclosed.file_name() else {
            continue;
        };
        let out_path = dest.join(file_name);

        let mut out = std::fs::File::create(&out_path).map_err(|source| InstallError::Io {
            path: out_path.display().to_string(),
            source,
        })?;
        std::io::copy(&mut entry, &mut out).map_err(|source| InstallError::Io {
            path: out_path.display().to_string(),
            source,
        })?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meta::{
        AssetIndexRef, DownloadRef, Downloads, LibraryArtifact, LibraryDownloads, OsRule, Rule,
        RuleAction,
    };
    use std::collections::HashMap;

    fn installer(os: &str, arch: &str) -> Installer {
        let dirs = DataDirs::with_root(if cfg!(windows) { r"C:\data" } else { "/data" });
        Installer::new(Downloader::new().unwrap(), dirs).with_platform(Platform {
            os: os.into(),
            arch: arch.into(),
            version: Some("10.0".into()),
        })
    }

    fn artifact(path: &str) -> LibraryArtifact {
        LibraryArtifact {
            path: Some(path.to_string()),
            sha1: "a".repeat(40),
            size: 100,
            url: format!("https://libraries.minecraft.net/{path}"),
        }
    }

    fn library(name: &str, path: &str) -> Library {
        Library {
            name: name.to_string(),
            downloads: Some(LibraryDownloads {
                artifact: Some(artifact(path)),
                classifiers: None,
            }),
            rules: Vec::new(),
            natives: None,
            extract: None,
            url: None,
        }
    }

    fn version(libraries: Vec<Library>) -> VersionDetail {
        VersionDetail {
            id: "1.21.4".into(),
            inherits_from: None,
            main_class: Some("net.minecraft.client.main.Main".into()),
            asset_index: Some(AssetIndexRef {
                id: "17".into(),
                sha1: "b".repeat(40),
                size: 1,
                url: "https://example.test/17.json".into(),
                total_size: None,
            }),
            assets: Some("17".into()),
            downloads: Some(Downloads {
                client: Some(DownloadRef {
                    sha1: "c".repeat(40),
                    size: 1000,
                    url: "https://example.test/client.jar".into(),
                }),
                server: None,
            }),
            java_version: None,
            libraries,
            arguments: None,
            minecraft_arguments: None,
            kind: None,
        }
    }

    #[test]
    fn the_client_jar_goes_last_on_the_classpath() {
        // A loader ships patched copies of vanilla classes; the JVM takes the
        // first match, so vanilla must be behind everything else.
        let resolved = installer("windows", "x86_64")
            .resolve(&version(vec![library("com.example:lib:1.0", "com/example/lib/1.0/lib-1.0.jar")]))
            .unwrap();

        assert_eq!(resolved.classpath.len(), 2);
        assert!(resolved.classpath.last().unwrap().ends_with("1.21.4.jar"));
    }

    #[test]
    fn libraries_for_other_platforms_are_left_out() {
        let mut windows_only = library("org.lwjgl:lwjgl:3.3.3", "org/lwjgl/lwjgl/3.3.3/lwjgl-3.3.3.jar");
        windows_only.rules = vec![Rule {
            action: RuleAction::Allow,
            os: Some(OsRule { name: Some("windows".into()), version: None, arch: None }),
            features: None,
        }];

        let detail = version(vec![windows_only]);

        // Client jar only.
        assert_eq!(installer("linux", "x86_64").resolve(&detail).unwrap().classpath.len(), 1);
        assert_eq!(installer("windows", "x86_64").resolve(&detail).unwrap().classpath.len(), 2);
    }

    #[test]
    fn native_classifiers_are_extracted_not_put_on_the_classpath() {
        let native = library(
            "org.lwjgl:lwjgl:3.3.3:natives-windows",
            "org/lwjgl/lwjgl/3.3.3/lwjgl-3.3.3-natives-windows.jar",
        );
        let resolved = installer("windows", "x86_64").resolve(&version(vec![native])).unwrap();

        assert_eq!(resolved.natives.len(), 1);
        // Client jar only — the native never joins the classpath.
        assert_eq!(resolved.classpath.len(), 1);
    }

    #[test]
    fn only_the_native_matching_this_architecture_is_installed() {
        // Reproduces the real 26.2 layout: three Windows native entries whose
        // rules are identical and only mention the OS. Installing all three
        // flattens them into one directory and the last one wins, which on an
        // ARM machine is a DLL the JVM cannot load.
        let os_only_rule = vec![Rule {
            action: RuleAction::Allow,
            os: Some(OsRule { name: Some("windows".into()), version: None, arch: None }),
            features: None,
        }];

        let variants: Vec<Library> = [
            ("org.lwjgl:lwjgl:3.4.1:natives-windows", "org/lwjgl/lwjgl/3.4.1/lwjgl-3.4.1-natives-windows.jar"),
            ("org.lwjgl:lwjgl:3.4.1:natives-windows-x86", "org/lwjgl/lwjgl/3.4.1/lwjgl-3.4.1-natives-windows-x86.jar"),
            ("org.lwjgl:lwjgl:3.4.1:natives-windows-arm64", "org/lwjgl/lwjgl/3.4.1/lwjgl-3.4.1-natives-windows-arm64.jar"),
        ]
        .into_iter()
        .map(|(name, path)| Library { rules: os_only_rule.clone(), ..library(name, path) })
        .collect();

        let detail = version(variants);

        let on_arm = installer("windows", "arm64").resolve(&detail).unwrap();
        assert_eq!(on_arm.natives.len(), 1);
        assert!(on_arm.natives[0].jar.to_string_lossy().contains("arm64"));

        let on_x64 = installer("windows", "x86_64").resolve(&detail).unwrap();
        assert_eq!(on_x64.natives.len(), 1);
        let jar = on_x64.natives[0].jar.to_string_lossy().into_owned();
        assert!(jar.contains("natives-windows.jar"), "got {jar}");

        let on_x86 = installer("windows", "x86").resolve(&detail).unwrap();
        assert_eq!(on_x86.natives.len(), 1);
        assert!(on_x86.natives[0].jar.to_string_lossy().contains("x86"));
    }

    #[test]
    fn the_legacy_natives_map_is_understood() {
        // Pre-1.19 versions name a classifier per OS instead of shipping a
        // separate library entry.
        let mut classifiers = HashMap::new();
        classifiers.insert(
            "natives-windows".to_string(),
            artifact("org/lwjgl/lwjgl/2.9.4/lwjgl-2.9.4-natives-windows.jar"),
        );
        let mut natives = HashMap::new();
        natives.insert("windows".to_string(), "natives-windows".to_string());

        let library = Library {
            name: "org.lwjgl:lwjgl:2.9.4".into(),
            downloads: Some(LibraryDownloads {
                artifact: Some(artifact("org/lwjgl/lwjgl/2.9.4/lwjgl-2.9.4.jar")),
                classifiers: Some(classifiers),
            }),
            rules: Vec::new(),
            natives: Some(natives),
            extract: None,
            url: None,
        };

        let resolved = installer("windows", "x86_64").resolve(&version(vec![library.clone()])).unwrap();
        assert_eq!(resolved.natives.len(), 1);
        // The plain artifact still belongs on the classpath alongside it.
        assert_eq!(resolved.classpath.len(), 2);

        // A platform with no entry in the map gets no natives.
        let resolved = installer("linux", "x86_64").resolve(&version(vec![library])).unwrap();
        assert!(resolved.natives.is_empty());
    }

    #[test]
    fn arch_placeholders_in_legacy_classifiers_are_expanded() {
        let mut classifiers = HashMap::new();
        classifiers.insert("natives-windows-64".to_string(), artifact("x/y/1/y-1-natives-windows-64.jar"));
        let mut natives = HashMap::new();
        natives.insert("windows".to_string(), "natives-windows-${arch}".to_string());

        let library = Library {
            name: "x:y:1".into(),
            downloads: Some(LibraryDownloads { artifact: None, classifiers: Some(classifiers) }),
            rules: Vec::new(),
            natives: Some(natives),
            extract: None,
            url: None,
        };

        let resolved = installer("windows", "x86_64").resolve(&version(vec![library])).unwrap();
        assert_eq!(resolved.natives.len(), 1);
    }

    #[test]
    fn duplicate_libraries_appear_on_the_classpath_once() {
        let detail = version(vec![
            library("com.example:lib:1.0", "com/example/lib/1.0/lib-1.0.jar"),
            library("com.example:lib:1.0", "com/example/lib/1.0/lib-1.0.jar"),
        ]);
        let resolved = installer("windows", "x86_64").resolve(&detail).unwrap();
        assert_eq!(resolved.classpath.len(), 2); // one library + client jar
    }

    #[test]
    fn loader_libraries_without_a_download_block_derive_their_path() {
        let library = Library {
            name: "net.fabricmc:fabric-loader:0.16.10".into(),
            downloads: None,
            rules: Vec::new(),
            natives: None,
            extract: None,
            url: Some("https://maven.fabricmc.net/".into()),
        };
        let resolved = installer("windows", "x86_64").resolve(&version(vec![library])).unwrap();
        assert!(
            resolved.classpath[0]
                .ends_with(Path::new("fabric-loader").join("0.16.10").join("fabric-loader-0.16.10.jar"))
        );
    }

    #[test]
    fn a_version_without_a_main_class_is_rejected_rather_than_launched() {
        let mut detail = version(vec![]);
        detail.main_class = None;
        assert!(matches!(
            installer("windows", "x86_64").resolve(&detail),
            Err(InstallError::NoMainClass(_))
        ));
    }

    #[test]
    fn a_version_without_a_client_download_is_rejected() {
        let mut detail = version(vec![]);
        detail.downloads = None;
        assert!(matches!(
            installer("windows", "x86_64").resolve(&detail),
            Err(InstallError::NoClientJar(_))
        ));
    }

    #[test]
    fn maven_base_urls_get_exactly_one_separator() {
        assert_eq!(ensure_trailing_slash("https://maven.test"), "https://maven.test/");
        assert_eq!(ensure_trailing_slash("https://maven.test/"), "https://maven.test/");
    }

    fn with_jvm_args(args: Vec<&str>) -> VersionDetail {
        let mut detail = version(vec![]);
        detail.arguments = Some(crate::meta::Arguments {
            jvm: args.into_iter().map(|a| Argument::Literal(a.to_string())).collect(),
            game: vec![],
        });
        detail
    }

    #[test]
    fn natives_unpack_where_the_version_says_to_look_for_them() {
        // 1.21.9-era versions moved the JNI libraries into a `java`
        // subdirectory and use siblings as scratch space. Unpacking to the root
        // here produces "Failed to locate library: lwjgl.dll" at startup.
        let root = Path::new("C:/data/natives/26.2");
        let detail = with_jvm_args(vec![
            "-Djava.library.path=${natives_directory}/java",
            "-Djna.tmpdir=${natives_directory}/jna",
        ]);
        assert_eq!(natives_extract_dir(&detail, root), root.join("java"));
    }

    #[test]
    fn older_versions_unpack_straight_into_the_natives_root() {
        let root = Path::new("C:/data/natives/1.20.1");
        let detail = with_jvm_args(vec!["-Djava.library.path=${natives_directory}"]);
        assert_eq!(natives_extract_dir(&detail, root), root.to_path_buf());
    }

    #[test]
    fn a_version_with_no_library_path_argument_falls_back_to_the_root() {
        let root = Path::new("C:/data/natives/1.8.9");
        // Pre-1.13: no JVM arguments at all, launcher supplies its own.
        assert_eq!(natives_extract_dir(&version(vec![]), root), root.to_path_buf());
        // Modern shape but no library path declared.
        let detail = with_jvm_args(vec!["-Dfoo=bar"]);
        assert_eq!(natives_extract_dir(&detail, root), root.to_path_buf());
    }

    #[test]
    fn the_library_path_argument_is_found_inside_conditional_groups() {
        let mut detail = version(vec![]);
        detail.arguments = Some(crate::meta::Arguments {
            jvm: vec![Argument::Conditional {
                rules: vec![],
                value: crate::meta::ArgumentValue::Many(vec![
                    "-Dfoo=bar".into(),
                    "-Djava.library.path=${natives_directory}/java".into(),
                ]),
            }],
            game: vec![],
        });
        let root = Path::new("C:/data/natives/x");
        assert_eq!(natives_extract_dir(&detail, root), root.join("java"));
    }
}
