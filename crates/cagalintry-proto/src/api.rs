//! Request and response bodies for the self-hosted sync API.
//!
//! Permission predicates ([`MemberRole::can_push`] and friends) live here on
//! purpose: the launcher uses them to grey out buttons and the server uses the
//! same code to actually enforce access, so the two can't drift into disagreeing
//! about who is allowed to do what.

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::manifest::{LoaderSpec, PackManifest};

// ---------------------------------------------------------------------------
// Accounts
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
    /// Shown in the account's session list so a stolen session can be spotted
    /// and revoked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenPair {
    pub access_token: String,
    pub refresh_token: String,
    /// Lifetime of the access token in seconds.
    pub expires_in: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshRequest {
    pub refresh_token: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UserRole {
    Admin,
    User,
}

impl UserRole {
    pub fn is_admin(self) -> bool {
        matches!(self, Self::Admin)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserProfile {
    pub id: Uuid,
    pub username: String,
    pub display_name: String,
    pub role: UserRole,
    #[serde(default)]
    pub minecraft_accounts: Vec<LinkedMinecraftAccount>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

/// A Minecraft account tied to a Cagalintry account. One person may link
/// several — an alt, or a second account for a family member.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkedMinecraftAccount {
    pub uuid: Uuid,
    pub username: String,
    #[serde(with = "time::serde::rfc3339")]
    pub linked_at: OffsetDateTime,
}

/// Proves ownership of a Minecraft account by handing the server a live token,
/// which it verifies against Mojang rather than taking the client's word for it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkMinecraftRequest {
    pub minecraft_access_token: String,
}

/// Admin-only. There is no self-signup.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateUserRequest {
    pub username: String,
    pub display_name: String,
    pub password: String,
    #[serde(default = "default_user_role")]
    pub role: UserRole,
}

fn default_user_role() -> UserRole {
    UserRole::User
}

// ---------------------------------------------------------------------------
// Packs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Visibility {
    /// Visible only to the owner and explicitly added members. Absent from
    /// everyone else's listing entirely, not merely greyed out.
    Private,
    /// Listable and installable by every authenticated Cagalintry user.
    /// Publishing still stays restricted to the owner and editors.
    Public,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MemberRole {
    Viewer,
    Editor,
    Owner,
}

impl MemberRole {
    /// May publish a new revision.
    pub fn can_push(self) -> bool {
        matches!(self, Self::Owner | Self::Editor)
    }

    /// May rename the pack, change its visibility, or change its icon.
    pub fn can_edit_settings(self) -> bool {
        matches!(self, Self::Owner | Self::Editor)
    }

    /// May add or remove members, or delete the pack. Owner only — an editor
    /// must not be able to lock the owner out of their own pack.
    pub fn can_administer(self) -> bool {
        matches!(self, Self::Owner)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackSummary {
    pub id: Uuid,
    pub slug: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon_sha256: Option<String>,
    pub owner: PackOwner,
    pub visibility: Visibility,
    pub mc_version: String,
    pub loader: LoaderSpec,
    pub head_revision: u64,
    /// The caller's own role, so the UI knows which controls to show without a
    /// second round trip.
    pub your_role: MemberRole,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackOwner {
    pub id: Uuid,
    pub display_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackDetail {
    #[serde(flatten)]
    pub summary: PackSummary,
    pub members: Vec<PackMember>,
    pub recent_revisions: Vec<RevisionSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackMember {
    pub user_id: Uuid,
    pub display_name: String,
    pub role: MemberRole,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatePackRequest {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    pub mc_version: String,
    pub loader: LoaderSpec,
    pub visibility: Visibility,
}

/// Every field optional — absent means "leave unchanged".
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdatePackRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visibility: Option<Visibility>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon_sha256: Option<String>,
}

/// The cheap poll the launcher makes to decide whether Play becomes Update.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HeadResponse {
    pub revision: u64,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PushRevisionRequest {
    /// The revision this edit was based on. If head has moved past it the server
    /// rejects the push with [`ApiErrorCode::RevisionConflict`] instead of
    /// silently overwriting whatever the other person just published.
    pub base_revision: u64,
    pub manifest: PackManifest,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub changelog: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PushRevisionResponse {
    pub revision: u64,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevisionSummary {
    pub revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub changelog: Option<String>,
    pub author_display_name: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlobUploadResponse {
    pub sha256: String,
    pub size: u64,
}

// ---------------------------------------------------------------------------
// Live events
// ---------------------------------------------------------------------------

/// Pushed over the `/events` WebSocket so a waiting launcher flips to "Update
/// available" the moment someone publishes, without polling.
///
/// Events are filtered server-side by visibility — a client is never told that a
/// private pack it cannot see has changed.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum ServerEvent {
    PackCreated { pack_id: Uuid },
    PackUpdated { pack_id: Uuid, revision: u64 },
    PackDeleted { pack_id: Uuid },
    PackVisibilityChanged { pack_id: Uuid, visibility: Visibility },
    MembershipChanged { pack_id: Uuid },
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Machine-readable error codes, so the launcher can react to a conflict or an
/// expired session without string-matching on prose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiErrorCode {
    BadRequest,
    InvalidCredentials,
    SessionExpired,
    Forbidden,
    NotFound,
    /// Head moved while the caller was editing. Carries the current revision so
    /// the client can rebase without another request.
    RevisionConflict,
    ManifestInvalid,
    RateLimited,
    Internal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiError {
    pub code: ApiErrorCode,
    pub message: String,
    /// Present on [`ApiErrorCode::RevisionConflict`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_revision: Option<u64>,
}

impl ApiError {
    pub fn new(code: ApiErrorCode, message: impl Into<String>) -> Self {
        Self { code, message: message.into(), current_revision: None }
    }

    pub fn conflict(current_revision: u64) -> Self {
        Self {
            code: ApiErrorCode::RevisionConflict,
            message: format!(
                "someone else published revision {current_revision} while you were editing"
            ),
            current_revision: Some(current_revision),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_owners_and_editors_can_publish() {
        assert!(MemberRole::Owner.can_push());
        assert!(MemberRole::Editor.can_push());
        assert!(!MemberRole::Viewer.can_push());
    }

    #[test]
    fn only_the_owner_can_administer_a_pack() {
        // An editor must not be able to remove the owner from their own pack.
        assert!(MemberRole::Owner.can_administer());
        assert!(!MemberRole::Editor.can_administer());
        assert!(!MemberRole::Viewer.can_administer());
    }

    #[test]
    fn roles_order_from_least_to_most_privileged() {
        assert!(MemberRole::Viewer < MemberRole::Editor);
        assert!(MemberRole::Editor < MemberRole::Owner);
    }

    #[test]
    fn conflict_errors_carry_the_revision_to_rebase_onto() {
        let err = ApiError::conflict(7);
        assert_eq!(err.code, ApiErrorCode::RevisionConflict);
        assert_eq!(err.current_revision, Some(7));
    }

    #[test]
    fn events_serialize_with_a_type_tag() {
        let json = serde_json::to_value(ServerEvent::PackUpdated {
            pack_id: Uuid::nil(),
            revision: 3,
        })
        .unwrap();
        assert_eq!(json["type"], "packUpdated");
        assert_eq!(json["revision"], 3);
    }
}
