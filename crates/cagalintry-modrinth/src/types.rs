//! Modrinth API v2 response types, and the bridge into our own pack manifest.
//!
//! Only the fields the launcher uses are modelled; the API returns a great deal
//! more and being exhaustive would just mean breaking whenever they add a
//! field.

use cagalintry_proto::{ContentSource, EntryKind, Hashes, PackEntry, Side};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all(serialize = "camelCase"))]
pub struct SearchResults {
    pub hits: Vec<SearchHit>,
    pub offset: u32,
    pub limit: u32,
    pub total_hits: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
// Asymmetric on purpose. Modrinth sends snake_case, which matches the field
// names, so deserialisation needs no renaming. But these same structs are
// handed to the frontend, where everything else is camelCase — serialising as
// snake_case there silently yields `undefined` for every multi-word field.
#[serde(rename_all(serialize = "camelCase"))]
pub struct SearchHit {
    pub project_id: String,
    pub slug: String,
    pub title: String,
    pub description: String,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub icon_url: Option<String>,
    #[serde(default)]
    pub downloads: u64,
    #[serde(default)]
    pub follows: u64,
    #[serde(default)]
    pub categories: Vec<String>,
    /// `required`, `optional`, `unsupported`, or `unknown`.
    #[serde(default)]
    pub client_side: Option<String>,
    #[serde(default)]
    pub server_side: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
// Asymmetric on purpose. Modrinth sends snake_case, which matches the field
// names, so deserialisation needs no renaming. But these same structs are
// handed to the frontend, where everything else is camelCase — serialising as
// snake_case there silently yields `undefined` for every multi-word field.
#[serde(rename_all(serialize = "camelCase"))]
pub struct Project {
    pub id: String,
    pub slug: String,
    pub title: String,
    pub description: String,
    /// The full description, in Markdown. Rendered and sanitised before it
    /// reaches the UI — see [`crate::markdown`].
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub icon_url: Option<String>,
    #[serde(default)]
    pub downloads: u64,
    #[serde(default)]
    pub followers: u64,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub client_side: Option<String>,
    #[serde(default)]
    pub server_side: Option<String>,
    #[serde(default)]
    pub source_url: Option<String>,
    #[serde(default)]
    pub issues_url: Option<String>,
    #[serde(default)]
    pub wiki_url: Option<String>,
    #[serde(default)]
    pub discord_url: Option<String>,
    #[serde(default)]
    pub gallery: Vec<GalleryImage>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all(serialize = "camelCase"))]
pub struct GalleryImage {
    pub url: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub featured: bool,
}

/// A project plus its description already rendered to safe HTML.
///
/// Rendering happens here rather than in the frontend so untrusted Markdown is
/// sanitised before crossing into the webview.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectPage {
    #[serde(flatten)]
    pub project: Project,
    pub body_html: String,
}

impl From<Project> for ProjectPage {
    fn from(project: Project) -> Self {
        let body_html = project
            .body
            .as_deref()
            .map(crate::markdown::to_safe_html)
            .unwrap_or_default();
        Self { project, body_html }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
// Asymmetric on purpose. Modrinth sends snake_case, which matches the field
// names, so deserialisation needs no renaming. But these same structs are
// handed to the frontend, where everything else is camelCase — serialising as
// snake_case there silently yields `undefined` for every multi-word field.
#[serde(rename_all(serialize = "camelCase"))]
pub struct Version {
    pub id: String,
    pub project_id: String,
    pub name: String,
    pub version_number: String,
    #[serde(default)]
    pub changelog: Option<String>,
    /// `release`, `beta` or `alpha`.
    pub version_type: String,
    #[serde(default)]
    pub game_versions: Vec<String>,
    #[serde(default)]
    pub loaders: Vec<String>,
    #[serde(default)]
    pub downloads: u64,
    #[serde(default)]
    pub dependencies: Vec<Dependency>,
    pub files: Vec<VersionFile>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
// Asymmetric on purpose. Modrinth sends snake_case, which matches the field
// names, so deserialisation needs no renaming. But these same structs are
// handed to the frontend, where everything else is camelCase — serialising as
// snake_case there silently yields `undefined` for every multi-word field.
#[serde(rename_all(serialize = "camelCase"))]
pub struct Dependency {
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub version_id: Option<String>,
    /// `required`, `optional`, `incompatible` or `embedded`.
    pub dependency_type: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
// Asymmetric on purpose. Modrinth sends snake_case, which matches the field
// names, so deserialisation needs no renaming. But these same structs are
// handed to the frontend, where everything else is camelCase — serialising as
// snake_case there silently yields `undefined` for every multi-word field.
#[serde(rename_all(serialize = "camelCase"))]
pub struct VersionFile {
    pub hashes: FileHashes,
    pub url: String,
    pub filename: String,
    /// A version may ship several files — sources, javadoc, the mod itself.
    /// Exactly one is marked primary, and that is the one to install.
    #[serde(default)]
    pub primary: bool,
    #[serde(default)]
    pub size: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FileHashes {
    pub sha1: String,
    pub sha512: String,
}

#[cfg(test)]
mod wire_format_tests {
    use super::*;

    /// The types cross two boundaries with opposite conventions, and getting
    /// this wrong is invisible in Rust: the app compiles, the API parses, and
    /// only the UI quietly shows blanks.
    #[test]
    fn responses_deserialize_from_snake_case_and_serialize_to_camel_case() {
        let hit: SearchHit = serde_json::from_str(
            r#"{"project_id":"AANobbMI","slug":"sodium","title":"Sodium",
                "description":"d","icon_url":"https://cdn.modrinth.com/icon.png",
                "client_side":"required","server_side":"unsupported"}"#,
        )
        .unwrap();

        assert_eq!(hit.project_id, "AANobbMI");
        assert_eq!(hit.icon_url.as_deref(), Some("https://cdn.modrinth.com/icon.png"));

        let json = serde_json::to_value(&hit).unwrap();
        assert_eq!(json["projectId"], "AANobbMI");
        assert!(json["iconUrl"].is_string());
        assert_eq!(json["clientSide"], "required");
        // The snake_case spellings must be gone, not merely duplicated.
        assert!(json.get("project_id").is_none());
        assert!(json.get("icon_url").is_none());
    }

    #[test]
    fn search_results_expose_a_camel_case_total() {
        let results: SearchResults =
            serde_json::from_str(r#"{"hits":[],"offset":0,"limit":20,"total_hits":42}"#).unwrap();

        let json = serde_json::to_value(&results).unwrap();
        assert_eq!(json["totalHits"], 42);
        assert!(json.get("total_hits").is_none());
    }

    #[test]
    fn versions_expose_camel_case_fields() {
        let version: Version = serde_json::from_str(
            r#"{"id":"v","project_id":"p","name":"n","version_number":"1.0",
                "version_type":"release","game_versions":["1.21.4"],
                "loaders":["fabric"],"files":[]}"#,
        )
        .unwrap();

        let json = serde_json::to_value(&version).unwrap();
        assert_eq!(json["projectId"], "p");
        assert_eq!(json["versionNumber"], "1.0");
        assert_eq!(json["versionType"], "release");
        assert_eq!(json["gameVersions"][0], "1.21.4");
    }
}

impl Version {
    /// The file to actually install.
    ///
    /// Falls back to the first file when nothing is flagged primary, which some
    /// older versions do.
    pub fn primary_file(&self) -> Option<&VersionFile> {
        self.files
            .iter()
            .find(|file| file.primary)
            .or_else(|| self.files.first())
    }

    pub fn is_release(&self) -> bool {
        self.version_type == "release"
    }

    /// Project ids of the dependencies that must also be installed.
    ///
    /// `optional` is left to the player, `incompatible` is a warning, and
    /// `embedded` is already inside the jar.
    pub fn required_dependencies(&self) -> impl Iterator<Item = &str> {
        self.dependencies
            .iter()
            .filter(|dependency| dependency.dependency_type == "required")
            .filter_map(|dependency| dependency.project_id.as_deref())
    }

    /// Convert into a pack manifest entry, ready to be published or installed.
    ///
    /// This is the join between browsing Modrinth and syncing a pack: the same
    /// entry describes what to download now and what a friend's launcher will
    /// reconcile against later.
    pub fn to_pack_entry(&self, kind: EntryKind, client_side: Option<&str>) -> Option<PackEntry> {
        let file = self.primary_file()?;

        Some(PackEntry {
            kind,
            source: ContentSource::Modrinth {
                project_id: self.project_id.clone(),
                version_id: self.id.clone(),
            },
            path: format!("{}/{}", kind.directory(), sanitise_filename(&file.filename)),
            hashes: Hashes {
                sha1: file.hashes.sha1.clone(),
                sha512: file.hashes.sha512.clone(),
            },
            size: file.size,
            downloads: vec![file.url.clone()],
            // A mod the server refuses is a client-side mod; anything else is
            // assumed to belong on both, which is the safe default for sync.
            side: match client_side {
                Some("unsupported") => Side::Client,
                _ => Side::Both,
            },
            enabled: true,
            name: Some(self.name.clone()),
            version_number: Some(self.version_number.clone()),
        })
    }
}

/// Filenames come from user-uploaded content, so anything that could escape the
/// target directory is replaced rather than trusted.
fn sanitise_filename(filename: &str) -> String {
    let name = filename
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(filename)
        .trim_matches(['.', ' '].as_slice());

    let cleaned: String = name
        .chars()
        .map(|c| if c.is_control() || matches!(c, ':' | '*' | '?' | '"' | '<' | '>' | '|') { '_' } else { c })
        .collect();

    if cleaned.is_empty() { "unnamed.jar".to_string() } else { cleaned }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(files: Vec<VersionFile>) -> Version {
        Version {
            id: "abcd1234".into(),
            project_id: "AANobbMI".into(),
            name: "Sodium 0.6.0".into(),
            version_number: "mc1.21.4-0.6.0".into(),
            changelog: None,
            version_type: "release".into(),
            game_versions: vec!["1.21.4".into()],
            loaders: vec!["fabric".into()],
            downloads: 100,
            dependencies: Vec::new(),
            files,
        }
    }

    fn file(filename: &str, primary: bool) -> VersionFile {
        VersionFile {
            hashes: FileHashes { sha1: "a".repeat(40), sha512: "b".repeat(128) },
            url: format!("https://cdn.modrinth.com/data/AANobbMI/versions/x/{filename}"),
            filename: filename.to_string(),
            primary,
            size: 1234,
        }
    }

    #[test]
    fn the_primary_file_is_the_one_installed() {
        // A version often ships sources and javadoc alongside the mod itself.
        let v = version(vec![
            file("sodium-sources.jar", false),
            file("sodium-fabric-0.6.0.jar", true),
        ]);
        assert_eq!(v.primary_file().unwrap().filename, "sodium-fabric-0.6.0.jar");
    }

    #[test]
    fn falls_back_to_the_first_file_when_none_is_primary() {
        let v = version(vec![file("only.jar", false)]);
        assert_eq!(v.primary_file().unwrap().filename, "only.jar");
    }

    #[test]
    fn converts_into_a_pack_entry() {
        let v = version(vec![file("sodium-fabric-0.6.0.jar", true)]);
        let entry = v.to_pack_entry(EntryKind::Mod, Some("required")).unwrap();

        assert_eq!(entry.path, "mods/sodium-fabric-0.6.0.jar");
        assert_eq!(entry.side, Side::Both);
        assert_eq!(entry.name.as_deref(), Some("Sodium 0.6.0"));
        assert_eq!(entry.version_number.as_deref(), Some("mc1.21.4-0.6.0"));
        assert!(matches!(entry.source, ContentSource::Modrinth { .. }));
        // Must survive the manifest's own validation unchanged.
        entry.validate().unwrap();
    }

    #[test]
    fn server_unsupported_mods_are_marked_client_side() {
        let v = version(vec![file("iris.jar", true)]);
        let entry = v.to_pack_entry(EntryKind::Mod, Some("unsupported")).unwrap();
        assert_eq!(entry.side, Side::Client);
    }

    #[test]
    fn shaderpacks_and_resourcepacks_land_in_their_own_directories() {
        let v = version(vec![file("BSL_v8.2.zip", true)]);
        assert_eq!(
            v.to_pack_entry(EntryKind::ShaderPack, None).unwrap().path,
            "shaderpacks/BSL_v8.2.zip"
        );
        assert_eq!(
            v.to_pack_entry(EntryKind::ResourcePack, None).unwrap().path,
            "resourcepacks/BSL_v8.2.zip"
        );
    }

    #[test]
    fn filenames_that_would_escape_the_directory_are_neutralised() {
        // Uploaded by strangers; a path here writes wherever it says.
        let v = version(vec![file("../../evil.jar", true)]);
        let entry = v.to_pack_entry(EntryKind::Mod, None).unwrap();
        assert_eq!(entry.path, "mods/evil.jar");
        entry.validate().unwrap();

        let v = version(vec![file("bad:name*.jar", true)]);
        let entry = v.to_pack_entry(EntryKind::Mod, None).unwrap();
        assert_eq!(entry.path, "mods/bad_name_.jar");
        entry.validate().unwrap();
    }

    #[test]
    fn only_required_dependencies_are_pulled_in() {
        let mut v = version(vec![file("x.jar", true)]);
        v.dependencies = vec![
            Dependency { project_id: Some("req".into()), version_id: None, dependency_type: "required".into() },
            Dependency { project_id: Some("opt".into()), version_id: None, dependency_type: "optional".into() },
            Dependency { project_id: Some("emb".into()), version_id: None, dependency_type: "embedded".into() },
            Dependency { project_id: Some("bad".into()), version_id: None, dependency_type: "incompatible".into() },
        ];

        let required: Vec<&str> = v.required_dependencies().collect();
        assert_eq!(required, ["req"]);
    }

    #[test]
    fn a_version_with_no_files_yields_no_entry() {
        assert!(version(vec![]).to_pack_entry(EntryKind::Mod, None).is_none());
    }

    #[test]
    fn search_results_parse() {
        let json = r#"{
          "hits": [{
            "project_id": "AANobbMI", "slug": "sodium", "title": "Sodium",
            "description": "A modern rendering engine", "author": "jellysquid3",
            "icon_url": "https://cdn.modrinth.com/data/AANobbMI/icon.png",
            "downloads": 30000000, "follows": 5000,
            "categories": ["optimization", "fabric"],
            "client_side": "required", "server_side": "unsupported",
            "some_new_field": 1
          }],
          "offset": 0, "limit": 20, "total_hits": 1
        }"#;

        let results: SearchResults = serde_json::from_str(json).unwrap();
        assert_eq!(results.total_hits, 1);
        assert_eq!(results.hits[0].title, "Sodium");
        assert_eq!(results.hits[0].server_side.as_deref(), Some("unsupported"));
    }
}
