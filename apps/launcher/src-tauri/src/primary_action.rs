// Wired to real pack state in Phase 3; the rules and their tests are here
// first because they are the contract the rest of the UI is built against.
#![allow(dead_code)]

//! What the one big button on a pack says and does.
//!
//! Derived state, never a stored flag — the button is recomputed from the
//! pack's current facts every time they change. That is what makes it flip
//! from Link Minecraft to Play the instant a link succeeds, and from Play to
//! Update the instant the sync server announces a new revision, with no
//! refresh and no chance of the label disagreeing with what the click does.

use serde::{Deserialize, Serialize};

/// Everything the decision depends on. Assembled by the caller from the account
/// store, the running-process table, and the pack's sync state.
#[derive(Debug, Clone, Copy, Default)]
pub struct PackStatus {
    /// Whether the signed-in Cagalintry account has any Minecraft account
    /// linked. Without one there is no session to launch with.
    pub has_linked_minecraft_account: bool,
    /// An install or update is in flight for this pack.
    pub busy: bool,
    /// The game is already running for this pack.
    pub running: bool,
    /// Revision currently on disk, if this pack is bound to a synced pack.
    pub installed_revision: Option<u64>,
    /// Latest revision the sync server has, if known. `None` while offline —
    /// which deliberately leaves the button on Play rather than blocking a
    /// launch behind an unreachable server.
    pub head_revision: Option<u64>,
    /// Content has never been installed for this pack.
    pub needs_install: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum PrimaryAction {
    /// No Minecraft account is linked. Opens the Microsoft login dialog.
    LinkMinecraft,
    /// Work in progress; the button is disabled and shows progress instead.
    Busy,
    /// Already running.
    Running,
    /// First-time install of a pack that has been added but never downloaded.
    Install,
    /// A newer revision is available. `changes` drives the badge.
    Update { changes: u64 },
    Play,
}

impl PrimaryAction {
    /// Clicking does nothing useful in these states.
    pub fn is_disabled(self) -> bool {
        matches!(self, Self::Busy | Self::Running)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::LinkMinecraft => "Link Minecraft",
            Self::Busy => "Working…",
            Self::Running => "Running",
            Self::Install => "Install",
            Self::Update { .. } => "Update",
            Self::Play => "Play",
        }
    }
}

/// Resolve the button state. Order matters and is the whole point:
///
/// Linking comes first because there is no sense downloading mods for a session
/// that cannot start. Busy and running come next so a click can't start a second
/// copy of work already underway. Only then do install and update apply, and
/// Play is the fallback.
pub fn resolve(status: PackStatus) -> PrimaryAction {
    if !status.has_linked_minecraft_account {
        return PrimaryAction::LinkMinecraft;
    }
    if status.busy {
        return PrimaryAction::Busy;
    }
    if status.running {
        return PrimaryAction::Running;
    }
    if status.needs_install {
        return PrimaryAction::Install;
    }

    // Only claim an update when the server has actually told us about a newer
    // revision. Offline (`head_revision: None`) must stay playable.
    if let (Some(installed), Some(head)) = (status.installed_revision, status.head_revision)
        && head > installed
    {
        return PrimaryAction::Update { changes: head - installed };
    }

    PrimaryAction::Play
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A linked, installed, idle pack — the ordinary case.
    fn ready() -> PackStatus {
        PackStatus {
            has_linked_minecraft_account: true,
            ..PackStatus::default()
        }
    }

    #[test]
    fn without_a_linked_account_the_button_offers_linking() {
        let status = PackStatus { has_linked_minecraft_account: false, ..ready() };
        assert_eq!(resolve(status), PrimaryAction::LinkMinecraft);
        assert_eq!(resolve(status).label(), "Link Minecraft");
    }

    #[test]
    fn linking_outranks_an_available_update() {
        // Downloading mods for a session that cannot start is wasted work, so
        // the prompt to link wins even when the pack is out of date.
        let status = PackStatus {
            has_linked_minecraft_account: false,
            installed_revision: Some(1),
            head_revision: Some(9),
            ..ready()
        };
        assert_eq!(resolve(status), PrimaryAction::LinkMinecraft);
    }

    #[test]
    fn linking_outranks_a_pending_install() {
        let status = PackStatus {
            has_linked_minecraft_account: false,
            needs_install: true,
            ..ready()
        };
        assert_eq!(resolve(status), PrimaryAction::LinkMinecraft);
    }

    #[test]
    fn a_newer_head_revision_becomes_an_update_with_a_badge() {
        let status = PackStatus {
            installed_revision: Some(4),
            head_revision: Some(7),
            ..ready()
        };
        assert_eq!(resolve(status), PrimaryAction::Update { changes: 3 });
    }

    #[test]
    fn matching_revisions_are_playable() {
        let status = PackStatus {
            installed_revision: Some(7),
            head_revision: Some(7),
            ..ready()
        };
        assert_eq!(resolve(status), PrimaryAction::Play);
    }

    #[test]
    fn being_offline_does_not_block_playing() {
        // head_revision is unknown, not "zero". Treating unknown as out-of-date
        // would make the launcher useless whenever the sync server is down.
        let status = PackStatus {
            installed_revision: Some(4),
            head_revision: None,
            ..ready()
        };
        assert_eq!(resolve(status), PrimaryAction::Play);
    }

    #[test]
    fn a_local_pack_with_no_sync_link_just_plays() {
        assert_eq!(resolve(ready()), PrimaryAction::Play);
    }

    #[test]
    fn an_installed_revision_ahead_of_head_is_not_an_update() {
        // Can happen briefly after a rollback on the server. Offering "Update"
        // to go backwards would be misleading.
        let status = PackStatus {
            installed_revision: Some(9),
            head_revision: Some(4),
            ..ready()
        };
        assert_eq!(resolve(status), PrimaryAction::Play);
    }

    #[test]
    fn work_in_progress_disables_the_button() {
        let busy = PackStatus { busy: true, ..ready() };
        assert_eq!(resolve(busy), PrimaryAction::Busy);
        assert!(resolve(busy).is_disabled());

        let running = PackStatus { running: true, ..ready() };
        assert_eq!(resolve(running), PrimaryAction::Running);
        assert!(resolve(running).is_disabled());
    }

    #[test]
    fn busy_outranks_running_and_update() {
        let status = PackStatus {
            busy: true,
            running: true,
            installed_revision: Some(1),
            head_revision: Some(2),
            ..ready()
        };
        assert_eq!(resolve(status), PrimaryAction::Busy);
    }

    #[test]
    fn a_never_installed_pack_offers_install() {
        let status = PackStatus { needs_install: true, ..ready() };
        assert_eq!(resolve(status), PrimaryAction::Install);
    }

    #[test]
    fn serializes_as_a_tagged_object_for_the_frontend() {
        let json = serde_json::to_value(PrimaryAction::Update { changes: 3 }).unwrap();
        assert_eq!(json["kind"], "update");
        assert_eq!(json["changes"], 3);

        let json = serde_json::to_value(PrimaryAction::LinkMinecraft).unwrap();
        assert_eq!(json["kind"], "linkMinecraft");
    }
}
