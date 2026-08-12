//! Verified, concurrent, resumable-by-restart downloading.
//!
//! Three properties matter here and everything else follows from them:
//!
//! 1. **Nothing is trusted until it hashes correctly.** A truncated download
//!    and a corrupted one are indistinguishable on disk, and the symptom
//!    surfaces much later as an unexplained crash on launch.
//! 2. **A file that is already correct is never fetched again.** This is what
//!    makes a second modpack on the same version nearly free, and what makes a
//!    failed install cheap to retry.
//! 3. **A partial file never appears at its destination.** Everything is
//!    written to a temporary name and renamed into place only after it
//!    verifies, so an interrupted run leaves no half-written jar behind.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use futures::StreamExt as _;
use sha1::Digest as _;
use tokio::io::AsyncWriteExt as _;
use tokio::sync::{Semaphore, mpsc};

use crate::hash::hashes_match;

/// Mojang and Modrinth both ask for an identifying User-Agent, and Modrinth's
/// documentation calls out bare library defaults as unacceptable.
pub const USER_AGENT: &str =
    concat!("Ricco0227/cagalintry-launcher/", env!("CARGO_PKG_VERSION"), " (github.com/Ricco0227/Cagalintry_Launcher)");

const DEFAULT_CONCURRENCY: usize = 8;
const DEFAULT_RETRIES: u32 = 3;

#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error("could not build the HTTP client: {0}")]
    Client(#[source] reqwest::Error),

    #[error("requesting {url}: {source}")]
    Request {
        url: String,
        #[source]
        source: reqwest::Error,
    },

    #[error("{url} returned {status}")]
    Status { url: String, status: reqwest::StatusCode },

    #[error("writing {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("{url} did not match its expected {algorithm}: expected {expected}, got {actual}")]
    Checksum {
        url: String,
        algorithm: &'static str,
        expected: String,
        actual: String,
    },

    #[error("{url} was {actual} bytes, expected {expected}")]
    Size { url: String, expected: u64, actual: u64 },

    #[error("decoding JSON from {url}: {source}")]
    Json {
        url: String,
        #[source]
        source: serde_json::Error,
    },
}

impl DownloadError {
    /// Whether retrying could plausibly succeed. A 404 will still be a 404;
    /// a truncated body or a dropped connection may well not repeat.
    fn is_retryable(&self) -> bool {
        match self {
            Self::Request { .. } | Self::Checksum { .. } | Self::Size { .. } => true,
            Self::Status { status, .. } => {
                status.is_server_error() || *status == reqwest::StatusCode::TOO_MANY_REQUESTS
            }
            _ => false,
        }
    }
}

/// The hash a downloaded file must match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Checksum {
    /// What Mojang publishes for libraries, assets and client jars.
    Sha1(String),
    /// What Modrinth publishes, and what pack manifests carry.
    Sha512(String),
}

impl Checksum {
    fn algorithm(&self) -> &'static str {
        match self {
            Self::Sha1(_) => "sha1",
            Self::Sha512(_) => "sha512",
        }
    }

    fn expected(&self) -> &str {
        match self {
            Self::Sha1(v) | Self::Sha512(v) => v,
        }
    }

    fn hasher(&self) -> Hasher {
        match self {
            Self::Sha1(_) => Hasher::Sha1(Box::new(sha1::Sha1::new())),
            Self::Sha512(_) => Hasher::Sha512(Box::new(sha2::Sha512::new())),
        }
    }

    /// Verify a file already on disk against this checksum.
    pub async fn verify_file(&self, path: &Path) -> bool {
        let actual = match self {
            Self::Sha1(_) => crate::hash::sha1_file(path).await,
            Self::Sha512(_) => crate::hash::sha512_file(path).await,
        };
        actual.is_ok_and(|actual| hashes_match(&actual, self.expected()))
    }
}

/// Boxed because Sha512's state is an order of magnitude larger than Sha1's,
/// and this enum is held per in-flight download.
enum Hasher {
    Sha1(Box<sha1::Sha1>),
    Sha512(Box<sha2::Sha512>),
}

impl Hasher {
    fn update(&mut self, bytes: &[u8]) {
        match self {
            Self::Sha1(h) => h.update(bytes),
            Self::Sha512(h) => h.update(bytes),
        }
    }

    fn finish(self) -> String {
        match self {
            Self::Sha1(h) => hex::encode(h.finalize()),
            Self::Sha512(h) => hex::encode(h.finalize()),
        }
    }
}

/// One file to fetch.
#[derive(Debug, Clone)]
pub struct DownloadSpec {
    pub url: String,
    pub dest: PathBuf,
    /// Absent only where upstream publishes no hash. Prefer to have one.
    pub checksum: Option<Checksum>,
    pub size: Option<u64>,
}

impl DownloadSpec {
    pub fn new(url: impl Into<String>, dest: impl Into<PathBuf>) -> Self {
        Self { url: url.into(), dest: dest.into(), checksum: None, size: None }
    }

    pub fn with_sha1(mut self, sha1: impl Into<String>) -> Self {
        self.checksum = Some(Checksum::Sha1(sha1.into()));
        self
    }

    pub fn with_sha512(mut self, sha512: impl Into<String>) -> Self {
        self.checksum = Some(Checksum::Sha512(sha512.into()));
        self
    }

    pub fn with_size(mut self, size: u64) -> Self {
        self.size = Some(size);
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Already on disk and verified — no request was made.
    Cached,
    Downloaded { bytes: u64 },
}

/// Emitted as work proceeds. The consumer is expected to aggregate and throttle
/// these; a large install produces byte events continuously.
#[derive(Debug, Clone)]
pub enum DownloadEvent {
    /// Bytes received since the last event, across all in-flight downloads.
    Bytes(u64),
    /// One file finished, whether fetched or already cached.
    FileComplete { dest: PathBuf, outcome: Outcome },
}

pub type ProgressSender = mpsc::UnboundedSender<DownloadEvent>;

#[derive(Debug, Clone)]
pub struct Downloader {
    client: reqwest::Client,
    /// Bounds concurrent requests. Minecraft installs are thousands of tiny
    /// asset files; unbounded concurrency exhausts file handles and gets the
    /// CDN annoyed at us without going any faster.
    permits: Arc<Semaphore>,
    retries: u32,
}

impl Downloader {
    pub fn new() -> Result<Self, DownloadError> {
        Self::with_concurrency(DEFAULT_CONCURRENCY)
    }

    pub fn with_concurrency(concurrency: usize) -> Result<Self, DownloadError> {
        let client = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .connect_timeout(std::time::Duration::from_secs(15))
            // No overall request timeout: an asset file takes milliseconds but a
            // Java runtime is 40 MB on whatever connection the player has.
            .pool_idle_timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(DownloadError::Client)?;

        Ok(Self {
            client,
            permits: Arc::new(Semaphore::new(concurrency.max(1))),
            retries: DEFAULT_RETRIES,
        })
    }

    pub fn client(&self) -> &reqwest::Client {
        &self.client
    }

    /// Fetch a small document into memory. Used for manifests, not content.
    pub async fn fetch_bytes(&self, url: &str) -> Result<Vec<u8>, DownloadError> {
        let _permit = self.permits.acquire().await.expect("semaphore is never closed");

        let mut attempt = 0;
        loop {
            match self.fetch_bytes_once(url).await {
                Ok(bytes) => return Ok(bytes),
                Err(err) if err.is_retryable() && attempt < self.retries => {
                    backoff(attempt).await;
                    attempt += 1;
                }
                Err(err) => return Err(err),
            }
        }
    }

    async fn fetch_bytes_once(&self, url: &str) -> Result<Vec<u8>, DownloadError> {
        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|source| DownloadError::Request { url: url.to_string(), source })?;

        let status = response.status();
        if !status.is_success() {
            return Err(DownloadError::Status { url: url.to_string(), status });
        }

        response
            .bytes()
            .await
            .map(|b| b.to_vec())
            .map_err(|source| DownloadError::Request { url: url.to_string(), source })
    }

    pub async fn fetch_json<T: serde::de::DeserializeOwned>(
        &self,
        url: &str,
    ) -> Result<T, DownloadError> {
        let bytes = self.fetch_bytes(url).await?;
        serde_json::from_slice(&bytes)
            .map_err(|source| DownloadError::Json { url: url.to_string(), source })
    }

    /// Fetch one file, skipping the request entirely if it is already present
    /// and verifies.
    pub async fn download(
        &self,
        spec: &DownloadSpec,
        progress: Option<&ProgressSender>,
    ) -> Result<Outcome, DownloadError> {
        if let Some(outcome) = self.check_cached(spec).await {
            if let Some(tx) = progress {
                let _ = tx.send(DownloadEvent::FileComplete {
                    dest: spec.dest.clone(),
                    outcome,
                });
            }
            return Ok(outcome);
        }

        let _permit = self.permits.acquire().await.expect("semaphore is never closed");

        let mut attempt = 0;
        let bytes = loop {
            match self.download_once(spec, progress).await {
                Ok(bytes) => break bytes,
                Err(err) if err.is_retryable() && attempt < self.retries => {
                    tracing::debug!(url = %spec.url, attempt, error = %err, "retrying download");
                    backoff(attempt).await;
                    attempt += 1;
                }
                Err(err) => return Err(err),
            }
        };

        let outcome = Outcome::Downloaded { bytes };
        if let Some(tx) = progress {
            let _ = tx.send(DownloadEvent::FileComplete { dest: spec.dest.clone(), outcome });
        }
        Ok(outcome)
    }

    /// `Some` when the destination already holds the right bytes.
    ///
    /// With a checksum this is exact. Without one, size is the only available
    /// signal — weaker, but it still avoids refetching an unchanged file.
    async fn check_cached(&self, spec: &DownloadSpec) -> Option<Outcome> {
        let metadata = tokio::fs::metadata(&spec.dest).await.ok()?;
        if !metadata.is_file() {
            return None;
        }

        match (&spec.checksum, spec.size) {
            (Some(checksum), _) => checksum
                .verify_file(&spec.dest)
                .await
                .then_some(Outcome::Cached),
            (None, Some(size)) => (metadata.len() == size).then_some(Outcome::Cached),
            (None, None) => None,
        }
    }

    async fn download_once(
        &self,
        spec: &DownloadSpec,
        progress: Option<&ProgressSender>,
    ) -> Result<u64, DownloadError> {
        let io_err = |path: &Path| {
            let path = path.display().to_string();
            move |source| DownloadError::Io { path: path.clone(), source }
        };

        if let Some(parent) = spec.dest.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(io_err(parent))?;
        }

        let response = self
            .client
            .get(&spec.url)
            .send()
            .await
            .map_err(|source| DownloadError::Request { url: spec.url.clone(), source })?;

        let status = response.status();
        if !status.is_success() {
            return Err(DownloadError::Status { url: spec.url.clone(), status });
        }

        // Unique per attempt so two concurrent downloads of the same
        // destination — or a leftover file from a killed process — can't
        // collide on the temporary name.
        let temp = temp_path(&spec.dest);

        let mut hasher = spec.checksum.as_ref().map(Checksum::hasher);
        let mut written: u64 = 0;

        {
            let mut file = tokio::fs::File::create(&temp).await.map_err(io_err(&temp))?;
            let mut stream = response.bytes_stream();

            while let Some(chunk) = stream.next().await {
                let chunk = chunk
                    .map_err(|source| DownloadError::Request { url: spec.url.clone(), source })?;

                if let Some(hasher) = &mut hasher {
                    hasher.update(&chunk);
                }
                file.write_all(&chunk).await.map_err(io_err(&temp))?;
                written += chunk.len() as u64;

                if let Some(tx) = progress {
                    let _ = tx.send(DownloadEvent::Bytes(chunk.len() as u64));
                }
            }

            file.flush().await.map_err(io_err(&temp))?;
        }

        // Verify before the file is allowed to take its real name, so a
        // corrupt download is never visible as a finished one.
        if let Err(err) = self.verify(spec, hasher, written).await {
            let _ = tokio::fs::remove_file(&temp).await;
            return Err(err);
        }

        // Windows refuses to rename onto an existing file.
        let _ = tokio::fs::remove_file(&spec.dest).await;
        tokio::fs::rename(&temp, &spec.dest).await.map_err(io_err(&spec.dest))?;

        Ok(written)
    }

    async fn verify(
        &self,
        spec: &DownloadSpec,
        hasher: Option<Hasher>,
        written: u64,
    ) -> Result<(), DownloadError> {
        if let Some(expected) = spec.size
            && written != expected
        {
            return Err(DownloadError::Size {
                url: spec.url.clone(),
                expected,
                actual: written,
            });
        }

        if let (Some(hasher), Some(checksum)) = (hasher, &spec.checksum) {
            let actual = hasher.finish();
            if !hashes_match(&actual, checksum.expected()) {
                return Err(DownloadError::Checksum {
                    url: spec.url.clone(),
                    algorithm: checksum.algorithm(),
                    expected: checksum.expected().to_string(),
                    actual,
                });
            }
        }

        Ok(())
    }

    /// Fetch many files concurrently, bounded by this downloader's permit count.
    ///
    /// Fails on the first error rather than pressing on: a half-installed
    /// version that appears to have succeeded is worse than a clean failure the
    /// caller can retry, and retrying is cheap because verified files are kept.
    pub async fn download_all(
        &self,
        specs: &[DownloadSpec],
        progress: Option<&ProgressSender>,
    ) -> Result<Vec<Outcome>, DownloadError> {
        // Futures are built up front rather than in a closure. A closure here
        // produces a higher-ranked signature that callers inside an async
        // command cannot satisfy, and futures are lazy, so nothing starts until
        // the stream polls them anyway.
        let mut pending = Vec::with_capacity(specs.len());
        for spec in specs {
            pending.push(self.download(spec, progress));
        }

        // `buffered` preserves result order; the semaphore remains the real
        // concurrency limit.
        futures::stream::iter(pending)
            .buffered(self.permits.available_permits().max(1) * 2)
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect()
    }
}

/// Exponential, starting at a quarter second: 250ms, 500ms, 1s.
async fn backoff(attempt: u32) {
    tokio::time::sleep(std::time::Duration::from_millis(250 << attempt)).await;
}

fn temp_path(dest: &Path) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut name = dest.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".{}.{n}.part", std::process::id()));
    dest.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join("cagalintry-download-tests").join(name);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_user_agent_identifies_this_launcher() {
        // Modrinth's docs single out bare library defaults as unacceptable.
        assert!(USER_AGENT.starts_with("Ricco0227/cagalintry-launcher/"));
        assert!(!USER_AGENT.contains("reqwest"));
    }

    #[test]
    fn temporary_names_are_unique_per_call() {
        let dest = Path::new("C:/data/libraries/foo.jar");
        let a = temp_path(dest);
        let b = temp_path(dest);
        assert_ne!(a, b);
        // And they sit beside the destination, so the final rename stays on one
        // volume rather than degrading into a cross-device copy.
        assert_eq!(a.parent(), dest.parent());
    }

    #[test]
    fn client_and_network_errors_are_classified_correctly() {
        let not_found = DownloadError::Status {
            url: "https://example.test/x".into(),
            status: reqwest::StatusCode::NOT_FOUND,
        };
        assert!(!not_found.is_retryable(), "a 404 will still be a 404");

        let server_error = DownloadError::Status {
            url: "https://example.test/x".into(),
            status: reqwest::StatusCode::BAD_GATEWAY,
        };
        assert!(server_error.is_retryable());

        let rate_limited = DownloadError::Status {
            url: "https://example.test/x".into(),
            status: reqwest::StatusCode::TOO_MANY_REQUESTS,
        };
        assert!(rate_limited.is_retryable());

        // A bad hash is usually a truncated body, which often succeeds on retry.
        let bad_hash = DownloadError::Checksum {
            url: "https://example.test/x".into(),
            algorithm: "sha1",
            expected: "a".into(),
            actual: "b".into(),
        };
        assert!(bad_hash.is_retryable());
    }

    #[tokio::test]
    async fn an_already_correct_file_is_not_refetched() {
        let dir = temp_dir("cached");
        let dest = dir.join("present.bin");
        tokio::fs::write(&dest, b"abc").await.unwrap();

        let downloader = Downloader::new().unwrap();
        let spec = DownloadSpec::new("https://example.invalid/unreachable", &dest)
            .with_sha1("a9993e364706816aba3e25717850c26c9cd0d89d");

        // The URL is deliberately unresolvable: if this succeeds, no request was
        // made, which is the whole point.
        assert_eq!(downloader.download(&spec, None).await.unwrap(), Outcome::Cached);

        tokio::fs::remove_dir_all(&dir).await.ok();
    }

    #[tokio::test]
    async fn a_file_with_the_wrong_contents_is_not_treated_as_cached() {
        let dir = temp_dir("stale");
        let dest = dir.join("stale.bin");
        tokio::fs::write(&dest, b"wrong").await.unwrap();

        let downloader = Downloader::new().unwrap();
        let spec = DownloadSpec::new("https://example.invalid/unreachable", &dest)
            .with_sha1("a9993e364706816aba3e25717850c26c9cd0d89d");

        // Must attempt a real download, and therefore fail on the bad host.
        assert!(downloader.download(&spec, None).await.is_err());

        tokio::fs::remove_dir_all(&dir).await.ok();
    }

    #[tokio::test]
    async fn size_alone_is_accepted_as_a_cache_check_when_no_hash_is_published() {
        let dir = temp_dir("size-only");
        let dest = dir.join("sized.bin");
        tokio::fs::write(&dest, b"12345").await.unwrap();

        let downloader = Downloader::new().unwrap();
        let spec = DownloadSpec::new("https://example.invalid/unreachable", &dest).with_size(5);
        assert_eq!(downloader.download(&spec, None).await.unwrap(), Outcome::Cached);

        let wrong = DownloadSpec::new("https://example.invalid/unreachable", &dest).with_size(6);
        assert!(downloader.download(&wrong, None).await.is_err());

        tokio::fs::remove_dir_all(&dir).await.ok();
    }

    #[tokio::test]
    async fn checksums_verify_files_on_disk() {
        let dir = temp_dir("verify");
        let path = dir.join("f.bin");
        tokio::fs::write(&path, b"abc").await.unwrap();

        let sha1 = Checksum::Sha1("a9993e364706816aba3e25717850c26c9cd0d89d".into());
        assert!(sha1.verify_file(&path).await);

        // Case-insensitive: manifests in the wild use both.
        let upper = Checksum::Sha1("A9993E364706816ABA3E25717850C26C9CD0D89D".into());
        assert!(upper.verify_file(&path).await);

        assert!(!Checksum::Sha1("0".repeat(40)).verify_file(&path).await);
        assert!(!sha1.verify_file(&dir.join("missing.bin")).await);

        tokio::fs::remove_dir_all(&dir).await.ok();
    }
}
