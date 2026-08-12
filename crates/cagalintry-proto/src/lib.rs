//! Types shared between the launcher and the self-hosted sync server.
//!
//! Everything that crosses the wire is defined exactly once, here, and compiled
//! into both sides. That makes a schema change a compile error rather than a
//! runtime surprise on somebody else's machine.

pub mod api;
pub mod diff;
pub mod manifest;
pub mod validate;

pub use diff::{EntryUpdate, PackDiff};
pub use manifest::{
    ContentSource, EntryKind, Hashes, LoaderKind, LoaderSpec, OverridesRef, PackEntry,
    PackManifest, Side, MANIFEST_SCHEMA_VERSION,
};
pub use validate::{ValidationError, ALLOWED_DOWNLOAD_HOSTS};
