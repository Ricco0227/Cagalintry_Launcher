//! Download engine: hash-verified, concurrency-limited fetching that never
//! leaves a partial file where a finished one should be, and never refetches
//! something already on disk and correct.

pub mod download;
pub mod hash;

pub use download::{
    Checksum, DownloadError, DownloadEvent, DownloadSpec, Downloader, Outcome, ProgressSender,
    USER_AGENT,
};
