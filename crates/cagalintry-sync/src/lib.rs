//! Client half of pack sync: talking to the sync server, tracking head
//! revisions, and applying a [`PackDiff`] to a pack directory through a
//! staging area so an interrupted update is retried rather than half-applied.
//!
//! [`PackDiff`]: cagalintry_proto::PackDiff
//!
//! Filled in during Phases 7 and 8.
