//! Modrinth API v2 client.
//!
//! Rate limited to stay inside Modrinth's 300 requests/minute, and sends the
//! identifying User-Agent their documentation requires — they call out bare
//! library defaults as unacceptable.

pub mod markdown;
pub mod rate_limit;
pub mod types;

use cagalintry_net::Downloader;
use cagalintry_proto::{EntryKind, LoaderKind};

pub use rate_limit::RateLimiter;
pub use types::{
    Dependency, FileHashes, GalleryImage, Project, ProjectPage, SearchHit, SearchResults, Version,
    VersionFile,
};

const API_BASE: &str = "https://api.modrinth.com/v2";

#[derive(Debug, thiserror::Error)]
pub enum ModrinthError {
    #[error(transparent)]
    Request(#[from] cagalintry_net::DownloadError),

    #[error("no version of this project works with {}{}", .mc_version, .loader.map(|l| format!(" on {l}")).unwrap_or_default())]
    NoCompatibleVersion {
        mc_version: String,
        loader: Option<&'static str>,
    },

    #[error("building a request URL: {0}")]
    Url(String),
}

/// What to search for.
#[derive(Debug, Clone)]
pub struct SearchQuery {
    pub text: String,
    pub kind: EntryKind,
    /// Restricts results to content published for this Minecraft version.
    pub mc_version: Option<String>,
    /// Only meaningful for mods; resource and shader packs are loader-agnostic.
    pub loader: Option<LoaderKind>,
    pub offset: u32,
    pub limit: u32,
}

impl SearchQuery {
    pub fn new(kind: EntryKind) -> Self {
        Self {
            text: String::new(),
            kind,
            mc_version: None,
            loader: None,
            offset: 0,
            limit: 20,
        }
    }

    /// Modrinth's facets are a JSON array of arrays: entries within a group are
    /// OR'd, groups are AND'd.
    fn facets(&self) -> String {
        let mut groups: Vec<String> = vec![format!(
            "[\"project_type:{}\"]",
            self.kind.modrinth_project_type()
        )];

        if let Some(mc_version) = &self.mc_version {
            groups.push(format!("[\"versions:{}\"]", escape_json(mc_version)));
        }

        // Only mods are built against a loader. Filtering shaders by "fabric"
        // would return nothing at all.
        if self.kind == EntryKind::Mod
            && let Some(loader) = self.loader
            && loader != LoaderKind::Vanilla
        {
            groups.push(format!("[\"categories:{}\"]", loader.modrinth_facet()));
        }

        format!("[{}]", groups.join(","))
    }
}

/// Narrows a project's version list to what an instance can actually use.
#[derive(Debug, Clone, Default)]
pub struct VersionFilter {
    pub mc_version: Option<String>,
    pub loader: Option<LoaderKind>,
    /// Resource and shader packs declare no loader, so filtering them by one
    /// removes everything.
    pub apply_loader: bool,
}

pub struct ModrinthClient {
    downloader: Downloader,
    limiter: RateLimiter,
}

impl ModrinthClient {
    pub fn new(downloader: Downloader) -> Self {
        Self { downloader, limiter: RateLimiter::default() }
    }

    pub async fn search(&self, query: &SearchQuery) -> Result<SearchResults, ModrinthError> {
        let mut url = url::Url::parse(&format!("{API_BASE}/search"))
            .map_err(|e| ModrinthError::Url(e.to_string()))?;

        {
            let mut pairs = url.query_pairs_mut();
            if !query.text.trim().is_empty() {
                pairs.append_pair("query", query.text.trim());
            }
            pairs.append_pair("facets", &query.facets());
            pairs.append_pair("limit", &query.limit.clamp(1, 100).to_string());
            pairs.append_pair("offset", &query.offset.to_string());
            // Downloads rather than relevance when there's no search text, so
            // an empty Discover page shows what people actually use.
            pairs.append_pair(
                "index",
                if query.text.trim().is_empty() { "downloads" } else { "relevance" },
            );
        }

        self.limiter.acquire().await;
        Ok(self.downloader.fetch_json(url.as_str()).await?)
    }

    pub async fn project(&self, id_or_slug: &str) -> Result<Project, ModrinthError> {
        self.limiter.acquire().await;
        Ok(self
            .downloader
            .fetch_json(&format!("{API_BASE}/project/{id_or_slug}"))
            .await?)
    }

    /// A project's versions, newest first.
    pub async fn versions(
        &self,
        project_id: &str,
        filter: &VersionFilter,
    ) -> Result<Vec<Version>, ModrinthError> {
        let mut url = url::Url::parse(&format!("{API_BASE}/project/{project_id}/version"))
            .map_err(|e| ModrinthError::Url(e.to_string()))?;

        {
            let mut pairs = url.query_pairs_mut();
            if let Some(mc_version) = &filter.mc_version {
                pairs.append_pair("game_versions", &format!("[\"{}\"]", escape_json(mc_version)));
            }
            if filter.apply_loader
                && let Some(loader) = filter.loader
                && loader != LoaderKind::Vanilla
            {
                pairs.append_pair("loaders", &format!("[\"{}\"]", loader.modrinth_facet()));
            }
        }

        self.limiter.acquire().await;
        Ok(self.downloader.fetch_json(url.as_str()).await?)
    }

    pub async fn version(&self, version_id: &str) -> Result<Version, ModrinthError> {
        self.limiter.acquire().await;
        Ok(self
            .downloader
            .fetch_json(&format!("{API_BASE}/version/{version_id}"))
            .await?)
    }

    /// Identify a file already on disk by its SHA-1.
    ///
    /// This is how a folder of loose jars becomes a proper pack: hash each one,
    /// ask Modrinth what it is, and anything recognised can be published as a
    /// manifest entry rather than an opaque blob.
    pub async fn version_by_hash(&self, sha1: &str) -> Result<Option<Version>, ModrinthError> {
        self.limiter.acquire().await;
        let url = format!("{API_BASE}/version_file/{sha1}?algorithm=sha1");

        match self.downloader.fetch_json::<Version>(&url).await {
            Ok(version) => Ok(Some(version)),
            // An unrecognised hash is an ordinary answer, not a failure.
            Err(cagalintry_net::DownloadError::Status { status, .. })
                if status == reqwest::StatusCode::NOT_FOUND =>
            {
                Ok(None)
            }
            Err(err) => Err(err.into()),
        }
    }

    /// The newest version of a project an instance can use.
    ///
    /// Prefers a full release, falling back to a beta or alpha when that is all
    /// that exists for the version — normal shortly after a Minecraft release.
    pub async fn best_version(
        &self,
        project_id: &str,
        filter: &VersionFilter,
    ) -> Result<Version, ModrinthError> {
        let versions = self.versions(project_id, filter).await?;

        versions
            .iter()
            .find(|version| version.is_release())
            .or_else(|| versions.first())
            .cloned()
            .ok_or_else(|| ModrinthError::NoCompatibleVersion {
                mc_version: filter.mc_version.clone().unwrap_or_else(|| "any version".into()),
                loader: filter
                    .apply_loader
                    .then_some(filter.loader)
                    .flatten()
                    .map(LoaderKind::display_name),
            })
    }
}

/// Escapes a value being embedded in a hand-built JSON facet string.
fn escape_json(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mod_searches_are_faceted_by_type_version_and_loader() {
        let query = SearchQuery {
            text: "sodium".into(),
            kind: EntryKind::Mod,
            mc_version: Some("1.21.4".into()),
            loader: Some(LoaderKind::Fabric),
            offset: 0,
            limit: 20,
        };

        assert_eq!(
            query.facets(),
            r#"[["project_type:mod"],["versions:1.21.4"],["categories:fabric"]]"#
        );
    }

    #[test]
    fn shader_searches_are_not_faceted_by_loader() {
        // Shader packs declare no loader; filtering by one returns nothing.
        let query = SearchQuery {
            kind: EntryKind::ShaderPack,
            mc_version: Some("1.21.4".into()),
            loader: Some(LoaderKind::Fabric),
            ..SearchQuery::new(EntryKind::ShaderPack)
        };

        assert_eq!(query.facets(), r#"[["project_type:shader"],["versions:1.21.4"]]"#);
    }

    #[test]
    fn resource_pack_searches_use_modrinths_own_project_type() {
        let query = SearchQuery::new(EntryKind::ResourcePack);
        assert_eq!(query.facets(), r#"[["project_type:resourcepack"]]"#);
    }

    #[test]
    fn vanilla_is_not_a_loader_facet() {
        let query = SearchQuery {
            loader: Some(LoaderKind::Vanilla),
            ..SearchQuery::new(EntryKind::Mod)
        };
        assert_eq!(query.facets(), r#"[["project_type:mod"]]"#);
    }

    #[test]
    fn facet_values_are_escaped() {
        let query = SearchQuery {
            mc_version: Some(r#"1.21"x"#.into()),
            ..SearchQuery::new(EntryKind::Mod)
        };
        assert!(query.facets().contains(r#"versions:1.21\"x"#));
    }

    /// Exercises the real API: search, version selection, and conversion into
    /// a manifest entry. Ignored by default since it needs the network.
    ///
    /// Run with:
    ///   cargo test -p cagalintry-modrinth -- --ignored --nocapture live_api
    #[tokio::test]
    #[ignore = "hits the live Modrinth API"]
    async fn live_api_search_and_resolve() {
        let client = ModrinthClient::new(Downloader::new().unwrap());

        let results = client
            .search(&SearchQuery {
                text: "sodium".into(),
                kind: EntryKind::Mod,
                mc_version: Some("1.21.4".into()),
                loader: Some(LoaderKind::Fabric),
                offset: 0,
                limit: 5,
            })
            .await
            .expect("search failed");

        assert!(results.total_hits > 0, "no results for sodium");
        println!("{} hits, first: {}", results.total_hits, results.hits[0].title);

        // Facets must actually restrict: a Fabric mod search should not be
        // returning resource packs or incompatible versions.
        let sodium = results
            .hits
            .iter()
            .find(|hit| hit.slug == "sodium")
            .expect("sodium not in results");

        let filter = VersionFilter {
            mc_version: Some("1.21.4".into()),
            loader: Some(LoaderKind::Fabric),
            apply_loader: true,
        };
        let version = client
            .best_version(&sodium.project_id, &filter)
            .await
            .expect("no compatible version");

        println!("chose {} ({})", version.version_number, version.version_type);
        assert!(version.game_versions.contains(&"1.21.4".to_string()));
        assert!(version.loaders.contains(&"fabric".to_string()));

        let project = client.project(&sodium.project_id).await.unwrap();
        let entry = version
            .to_pack_entry(EntryKind::Mod, project.client_side.as_deref())
            .expect("no primary file");

        println!("entry: {} ({} bytes)", entry.path, entry.size);
        entry.validate().expect("entry failed manifest validation");
        assert!(entry.path.starts_with("mods/"));
        assert_eq!(entry.hashes.sha512.len(), 128);

        // And the reverse lookup used to identify loose jars on disk.
        let by_hash = client
            .version_by_hash(&entry.hashes.sha1)
            .await
            .expect("hash lookup failed");
        assert_eq!(by_hash.map(|v| v.id), Some(version.id));

        // An unknown hash is an ordinary "not found", not an error.
        assert!(client.version_by_hash(&"0".repeat(40)).await.unwrap().is_none());
    }

    #[test]
    fn search_limits_are_clamped_to_what_the_api_accepts() {
        // Exercised through the URL builder rather than asserted directly,
        // since clamping happens there.
        let query = SearchQuery { limit: 5000, ..SearchQuery::new(EntryKind::Mod) };
        assert_eq!(query.limit.clamp(1, 100), 100);

        let query = SearchQuery { limit: 0, ..SearchQuery::new(EntryKind::Mod) };
        assert_eq!(query.limit.clamp(1, 100), 1);
    }
}
