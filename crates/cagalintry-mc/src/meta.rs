//! Mojang's launcher metadata, as published on piston-meta.
//!
//! These types mirror JSON we don't control, so they are deliberately lenient:
//! unknown fields are ignored and anything not needed to launch the game is
//! left unmodelled. Being strict here would mean the launcher breaks the day
//! Mojang adds a field.

use serde::{Deserialize, Serialize};

/// Index of every published version.
pub const VERSION_MANIFEST_URL: &str =
    "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct VersionManifest {
    pub latest: LatestVersions,
    pub versions: Vec<VersionEntry>,
}

impl VersionManifest {
    pub fn find(&self, id: &str) -> Option<&VersionEntry> {
        self.versions.iter().find(|v| v.id == id)
    }

    /// Releases only, newest first — snapshots and the April Fools' versions
    /// are noise in a launcher's version picker by default.
    pub fn releases(&self) -> impl Iterator<Item = &VersionEntry> {
        self.versions.iter().filter(|v| v.kind == VersionKind::Release)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LatestVersions {
    pub release: String,
    pub snapshot: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionEntry {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: VersionKind,
    pub url: String,
    /// SHA-1 of the version JSON at `url`. Lets a cached copy be trusted
    /// without refetching it.
    #[serde(default)]
    pub sha1: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VersionKind {
    Release,
    Snapshot,
    OldBeta,
    OldAlpha,
}

// ---------------------------------------------------------------------------
// Version detail
// ---------------------------------------------------------------------------

/// A single version's full description: what to download, and how to run it.
///
/// Mod loaders publish documents in this same shape with `inheritsFrom` set,
/// which is what makes [`merge_inherited`](Self::merge_inherited) the whole of
/// loader support at this layer.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionDetail {
    pub id: String,

    /// Set by loader profiles: this document is a patch over the named vanilla
    /// version rather than a complete description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inherits_from: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub main_class: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asset_index: Option<AssetIndexRef>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assets: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub downloads: Option<Downloads>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub java_version: Option<JavaVersionRef>,

    #[serde(default)]
    pub libraries: Vec<Library>,

    /// Modern format (1.13+).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Arguments>,

    /// Pre-1.13 format: a single space-separated template string.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minecraft_arguments: Option<String>,

    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<VersionKind>,
}

impl VersionDetail {
    /// Fold a child (loader) document over its parent (vanilla) document.
    ///
    /// Child scalars win when present. Libraries are *prepended* rather than
    /// appended: the JVM takes the first match on the classpath, and a loader
    /// ships patched versions of classes that must shadow the vanilla ones.
    /// Arguments are appended, since loaders add to the command line.
    pub fn merge_inherited(parent: &VersionDetail, child: &VersionDetail) -> VersionDetail {
        let mut libraries = child.libraries.clone();
        libraries.extend(parent.libraries.iter().cloned());

        let arguments = match (&parent.arguments, &child.arguments) {
            (Some(p), Some(c)) => Some(Arguments {
                game: [p.game.clone(), c.game.clone()].concat(),
                jvm: [p.jvm.clone(), c.jvm.clone()].concat(),
            }),
            (some, None) | (None, some) => some.clone(),
        };

        VersionDetail {
            id: child.id.clone(),
            inherits_from: None,
            main_class: child.main_class.clone().or_else(|| parent.main_class.clone()),
            asset_index: child.asset_index.clone().or_else(|| parent.asset_index.clone()),
            assets: child.assets.clone().or_else(|| parent.assets.clone()),
            downloads: child.downloads.clone().or_else(|| parent.downloads.clone()),
            java_version: child.java_version.clone().or_else(|| parent.java_version.clone()),
            libraries,
            arguments,
            minecraft_arguments: child
                .minecraft_arguments
                .clone()
                .or_else(|| parent.minecraft_arguments.clone()),
            kind: child.kind.or(parent.kind),
        }
    }

    /// Mojang's JRE component name, defaulting to the pre-1.17 runtime for old
    /// versions that predate the field.
    pub fn java_component(&self) -> &str {
        self.java_version
            .as_ref()
            .map(|j| j.component.as_str())
            .unwrap_or("jre-legacy")
    }

    pub fn java_major_version(&self) -> u32 {
        self.java_version.as_ref().map(|j| j.major_version).unwrap_or(8)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AssetIndexRef {
    pub id: String,
    pub sha1: String,
    pub size: u64,
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_size: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Downloads {
    pub client: Option<DownloadRef>,
    pub server: Option<DownloadRef>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DownloadRef {
    pub sha1: String,
    pub size: u64,
    pub url: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaVersionRef {
    pub component: String,
    pub major_version: u32,
}

// ---------------------------------------------------------------------------
// Libraries
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Library {
    /// Maven coordinates: `group:artifact:version[:classifier]`.
    pub name: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub downloads: Option<LibraryDownloads>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<Rule>,

    /// Pre-1.19 native layout: maps an OS to a key in `downloads.classifiers`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub natives: Option<std::collections::HashMap<String, String>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extract: Option<ExtractRules>,

    /// Loader libraries frequently carry only a Maven repository URL and
    /// expect the launcher to derive the path from `name`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LibraryDownloads {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact: Option<LibraryArtifact>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub classifiers: Option<std::collections::HashMap<String, LibraryArtifact>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LibraryArtifact {
    /// Repository-relative path, e.g. `org/lwjgl/lwjgl/3.3.3/lwjgl-3.3.3.jar`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub sha1: String,
    pub size: u64,
    pub url: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ExtractRules {
    #[serde(default)]
    pub exclude: Vec<String>,
}

// ---------------------------------------------------------------------------
// Rules and arguments
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Rule {
    pub action: RuleAction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os: Option<OsRule>,
    /// Optional launcher features (demo mode, custom resolution). We enable
    /// none of them, so any rule gated on one does not apply.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub features: Option<std::collections::HashMap<String, bool>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RuleAction {
    Allow,
    Disallow,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OsRule {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// A regex against the OS version. Only ever used to single out Windows 10.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arch: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Arguments {
    #[serde(default)]
    pub game: Vec<Argument>,
    #[serde(default)]
    pub jvm: Vec<Argument>,
}

/// Either a bare string, or a conditional group guarded by rules.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum Argument {
    Literal(String),
    Conditional {
        rules: Vec<Rule>,
        value: ArgumentValue,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum ArgumentValue {
    Single(String),
    Many(Vec<String>),
}

impl ArgumentValue {
    pub fn as_slice(&self) -> &[String] {
        match self {
            Self::Single(s) => std::slice::from_ref(s),
            Self::Many(v) => v,
        }
    }
}

// ---------------------------------------------------------------------------
// Asset index
// ---------------------------------------------------------------------------

/// Mojang's asset CDN. Objects are addressed by hash, not by name.
pub const RESOURCES_BASE_URL: &str = "https://resources.download.minecraft.net";

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AssetIndex {
    pub objects: std::collections::HashMap<String, AssetObject>,
    /// Pre-1.7 versions expect assets laid out by name under `resources/`
    /// instead of hashed into an object store.
    #[serde(default)]
    pub map_to_resources: bool,
    #[serde(default)]
    pub r#virtual: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AssetObject {
    pub hash: String,
    pub size: u64,
}

impl AssetObject {
    /// Assets live under the first two characters of their hash, both on
    /// Mojang's CDN and in the local store.
    pub fn relative_path(&self) -> String {
        format!("{}/{}", &self.hash[..2], self.hash)
    }

    pub fn url(&self) -> String {
        format!("{RESOURCES_BASE_URL}/{}", self.relative_path())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_version_manifest_entry() {
        let json = r#"{
            "latest": {"release": "1.21.4", "snapshot": "25w02a"},
            "versions": [
                {"id":"1.21.4","type":"release","url":"https://example.test/1.21.4.json",
                 "time":"2024-12-03T10:12:57+00:00","releaseTime":"2024-12-03T10:12:57+00:00",
                 "sha1":"abc123","complianceLevel":1},
                {"id":"25w02a","type":"snapshot","url":"https://example.test/s.json"}
            ]
        }"#;
        let manifest: VersionManifest = serde_json::from_str(json).unwrap();
        assert_eq!(manifest.latest.release, "1.21.4");
        assert_eq!(manifest.releases().count(), 1);
        assert_eq!(manifest.find("25w02a").unwrap().kind, VersionKind::Snapshot);
        // Unmodelled fields like complianceLevel must not break parsing.
        assert_eq!(manifest.find("1.21.4").unwrap().sha1.as_deref(), Some("abc123"));
    }

    #[test]
    fn parses_both_argument_shapes() {
        let json = r#"{
            "id": "1.21.4",
            "arguments": {
                "game": ["--username", "${auth_player_name}",
                         {"rules":[{"action":"allow","features":{"is_demo_user":true}}],"value":"--demo"}],
                "jvm": [{"rules":[{"action":"allow","os":{"name":"windows"}}],
                         "value":["-Dos.name=Windows 10","-Dos.version=10.0"]}]
            },
            "libraries": []
        }"#;
        let detail: VersionDetail = serde_json::from_str(json).unwrap();
        let args = detail.arguments.unwrap();
        assert!(matches!(args.game[0], Argument::Literal(ref s) if s == "--username"));
        assert!(matches!(args.game[2], Argument::Conditional { .. }));
        match &args.jvm[0] {
            Argument::Conditional { value, .. } => assert_eq!(value.as_slice().len(), 2),
            other => panic!("expected conditional, got {other:?}"),
        }
    }

    #[test]
    fn parses_the_legacy_argument_string() {
        let json = r#"{"id":"1.8.9","minecraftArguments":"--username ${auth_player_name}","libraries":[]}"#;
        let detail: VersionDetail = serde_json::from_str(json).unwrap();
        assert!(detail.arguments.is_none());
        assert!(detail.minecraft_arguments.is_some());
    }

    #[test]
    fn defaults_java_to_the_legacy_runtime_when_unspecified() {
        let json = r#"{"id":"1.8.9","libraries":[]}"#;
        let detail: VersionDetail = serde_json::from_str(json).unwrap();
        assert_eq!(detail.java_component(), "jre-legacy");
        assert_eq!(detail.java_major_version(), 8);
    }

    #[test]
    fn loader_libraries_shadow_vanilla_ones_on_the_classpath() {
        // Order matters: the JVM takes the first match, and a loader ships
        // patched classes that must win over the vanilla originals.
        let parent: VersionDetail = serde_json::from_str(
            r#"{"id":"1.21.4","mainClass":"net.minecraft.client.main.Main",
                "libraries":[{"name":"vanilla:lib:1"}],"assets":"17"}"#,
        )
        .unwrap();
        let child: VersionDetail = serde_json::from_str(
            r#"{"id":"fabric-loader-1.21.4","inheritsFrom":"1.21.4",
                "mainClass":"net.fabricmc.loader.impl.launch.knot.KnotClient",
                "libraries":[{"name":"fabric:loader:1"}]}"#,
        )
        .unwrap();

        let merged = VersionDetail::merge_inherited(&parent, &child);
        assert_eq!(merged.id, "fabric-loader-1.21.4");
        assert_eq!(merged.main_class.as_deref(), Some("net.fabricmc.loader.impl.launch.knot.KnotClient"));
        assert_eq!(merged.libraries[0].name, "fabric:loader:1");
        assert_eq!(merged.libraries[1].name, "vanilla:lib:1");
        // Inherited from the parent, which the child never mentions.
        assert_eq!(merged.assets.as_deref(), Some("17"));
        assert!(merged.inherits_from.is_none());
    }

    #[test]
    fn assets_are_addressed_by_hash_prefix() {
        let object = AssetObject { hash: "abcdef0123456789".into(), size: 10 };
        assert_eq!(object.relative_path(), "ab/abcdef0123456789");
        assert!(object.url().ends_with("/ab/abcdef0123456789"));
    }
}
