//! Microsoft account authentication for Minecraft.
//!
//! Device-code flow: MSA -> Xbox Live -> XSTS -> Minecraft services ->
//! entitlement check -> profile. Refresh tokens live in the OS credential store.
//!
//! There is deliberately no offline-account path: a Microsoft account that owns
//! the game is the only way in, and the entitlement check is not skippable.
//!
//! Filled in during Phase 2.
