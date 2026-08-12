//! The IPC surface the frontend calls.
//!
//! Commands stay thin: claim the instance, delegate to the crates that do the
//! real work, and translate progress into events the UI can render. Anything
//! worth testing lives below this layer, where it can be tested without a
//! webview.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use cagalintry_mc::launch::{self, GameOutput, LaunchOptions, LaunchSession};
use cagalintry_net::DownloadEvent;
use cagalintry_proto::LoaderSpec;
use serde::Serialize;
use tauri::{AppHandle, Emitter as _, State};
use time::OffsetDateTime;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::instance::Instance;
use crate::primary_action::{InstanceStatus, PrimaryAction, resolve};
use crate::state::{AppState, CommandError, CommandResult};

/// Progress is emitted on a timer rather than per chunk: a full install is
/// hundreds of thousands of byte events, and forwarding each one to the webview
/// costs more than the download does.
const PROGRESS_INTERVAL: std::time::Duration = std::time::Duration::from_millis(100);

// ---------------------------------------------------------------------------
// Views
// ---------------------------------------------------------------------------

/// An instance as the Library renders it, with the button state already
/// resolved so the frontend never has to reimplement those rules.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceView {
    #[serde(flatten)]
    pub instance: Instance,
    pub action: PrimaryAction,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionSummary {
    pub id: String,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallProgress {
    pub instance_id: Uuid,
    /// Human-readable stage: "Resolving", "Downloading", "Preparing Java".
    pub stage: String,
    pub completed_files: u64,
    pub total_files: u64,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameLogLine {
    pub instance_id: Uuid,
    pub line: String,
    pub is_stderr: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameExit {
    pub instance_id: Uuid,
    pub code: Option<i32>,
    /// Anything other than a clean zero is surfaced to the player rather than
    /// disappearing silently.
    pub crashed: bool,
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn app_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[tauri::command]
pub async fn list_instances(state: State<'_, Arc<AppState>>) -> CommandResult<Vec<InstanceView>> {
    let instances = state.instances.list().await?;

    let mut views = Vec::with_capacity(instances.len());
    for instance in instances {
        let action = instance_action(&state, &instance).await;
        views.push(InstanceView { instance, action });
    }
    Ok(views)
}

/// Releases only. Snapshots are noise in a launcher whose whole point is a
/// stable shared modpack.
#[tauri::command]
pub async fn list_minecraft_versions(
    state: State<'_, Arc<AppState>>,
) -> CommandResult<Vec<VersionSummary>>
{
    let manifest = state.installer().version_manifest().await?;
    Ok(manifest
        .releases()
        .map(|entry| VersionSummary { id: entry.id.clone(), kind: "release".to_string() })
        .collect())
}

#[tauri::command]
pub async fn create_instance(
    state: State<'_, Arc<AppState>>,
    name: String,
    mc_version: String,
) -> CommandResult<InstanceView> {
    let instance = Instance::new(name, mc_version, LoaderSpec::vanilla());
    state.instances.save(&instance).await?;

    let action = instance_action(&state, &instance).await;
    Ok(InstanceView { instance, action })
}

#[tauri::command]
pub async fn delete_instance(state: State<'_, Arc<AppState>>, id: Uuid) -> CommandResult<()> {
    if state.is_running(id).await {
        return Err(CommandError::new(
            "running",
            "close the game before deleting this instance",
        ));
    }
    state.instances.delete(id).await?;
    Ok(())
}

#[tauri::command]
pub async fn open_instance_folder(state: State<'_, Arc<AppState>>, id: Uuid) -> CommandResult<String> {
    Ok(state.instances.game_dir(id).display().to_string())
}

/// Install whatever is missing and start the game.
///
/// Safe to call twice: the second call finds the instance claimed and returns
/// immediately rather than racing the first over the same files.
#[tauri::command]
pub async fn launch_instance(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    id: Uuid,
) -> CommandResult<()> {
    if state.is_running(id).await {
        return Err(CommandError::new("running", "this instance is already running"));
    }
    if !state.try_claim(id).await {
        return Err(CommandError::new("busy", "this instance is already being prepared"));
    }

    let result = prepare_and_launch(&app, &state, id).await;
    state.release(id).await;
    result
}

async fn prepare_and_launch(app: &AppHandle, state: &Arc<AppState>, id: Uuid) -> CommandResult<()> {
    let mut instance = state.instances.get(id).await?;
    let session = session_for_launch()?;

    emit_stage(app, id, "Resolving version");

    let installer = state.installer();
    let manifest = installer.version_manifest().await?;
    let detail = installer.version_detail(&manifest, &instance.version_id()).await?;
    let resolved = installer.resolve(&detail)?;

    let plan = installer.plan(&resolved).await?;
    let (progress, pump) = progress_channel(app.clone(), id, plan.file_count as u64, plan.total_bytes);

    emit_stage(app, id, "Downloading");
    installer.install(&resolved, Some(&progress)).await?;

    emit_stage(app, id, "Preparing Java");
    let java = state
        .java()
        .provide(
            resolved.java_component(),
            resolved.java_major_version(),
            instance.java_path.as_deref(),
            Some(&progress),
        )
        .await?;

    // Stop the aggregator before launching so no stale progress arrives after
    // the game window is up.
    drop(progress);
    let _ = pump.await;

    let mut options = LaunchOptions::new(session, state.instances.game_dir(id));
    options.max_memory_mb = instance.max_memory_mb;
    options.extra_jvm_args = instance.extra_jvm_args.clone();

    let command = launch::build_command(
        &resolved,
        &java,
        &options,
        &state.dirs.assets(),
        &cagalintry_mc::Platform::current(),
    );
    tracing::info!(instance = %id, command = %command.to_redacted_string(), "launching");

    let (log_tx, log_rx) = mpsc::unbounded_channel();
    let child = launch::spawn(&command, Some(log_tx)).await?;

    forward_logs(app.clone(), id, log_rx);

    let child = state.register_running(id, child).await;
    watch_for_exit(app.clone(), Arc::clone(state), id, child);

    instance.last_played = Some(OffsetDateTime::now_utc());
    state.instances.save(&instance).await?;

    Ok(())
}

#[tauri::command]
pub async fn kill_instance(state: State<'_, Arc<AppState>>, id: Uuid) -> CommandResult<()> {
    let Some(child) = state.running_child(id).await else {
        return Ok(());
    };
    child.lock().await.kill().await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

async fn instance_action(state: &AppState, instance: &Instance) -> PrimaryAction {
    resolve(InstanceStatus {
        has_linked_minecraft_account: has_linked_account(),
        busy: state.is_busy(instance.id).await,
        running: state.is_running(instance.id).await,
        installed_revision: instance.pack.as_ref().map(|p| p.installed_revision),
        // Populated once the sync client lands; until then there is no pack
        // head to compare against and the button stays on Play.
        head_revision: None,
        needs_install: false,
    })
}

/// Whether a Minecraft account is linked and usable.
///
/// Phase 2 replaces this with the real account store, populated by the
/// Microsoft sign-in chain. Until then nothing is linked, so the primary button
/// reads "Link Minecraft".
fn has_linked_account() -> bool {
    false
}

/// The authenticated session passed to the game.
///
/// There is exactly one way to obtain one: signing in with a Microsoft account
/// that owns Minecraft, verified against Mojang's entitlement endpoint. No
/// offline mode, no placeholder credentials, and no build configuration that
/// produces a session any other way — a launch without a verified session is
/// refused here rather than started and left to fail later.
fn session_for_launch() -> CommandResult<LaunchSession> {
    Err(CommandError::new(
        "noAccount",
        "sign in with a Microsoft account that owns Minecraft before playing",
    ))
}

fn emit_stage(app: &AppHandle, instance_id: Uuid, stage: &str) {
    let _ = app.emit(
        "install://progress",
        InstallProgress {
            instance_id,
            stage: stage.to_string(),
            completed_files: 0,
            total_files: 0,
            downloaded_bytes: 0,
            total_bytes: 0,
        },
    );
}

/// Aggregate raw download events into periodic UI updates.
///
/// Returns the sender to hand to the downloader, and a handle that completes
/// once the sender is dropped and the final total has been emitted.
fn progress_channel(
    app: AppHandle,
    instance_id: Uuid,
    total_files: u64,
    total_bytes: u64,
) -> (cagalintry_net::ProgressSender, tokio::task::JoinHandle<()>) {
    let (tx, mut rx) = mpsc::unbounded_channel();

    let handle = tokio::spawn(async move {
        let bytes = Arc::new(AtomicU64::new(0));
        let files = Arc::new(AtomicU64::new(0));
        let mut ticker = tokio::time::interval(PROGRESS_INTERVAL);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        let emit = |stage: &str, bytes: &AtomicU64, files: &AtomicU64| {
            let _ = app.emit(
                "install://progress",
                InstallProgress {
                    instance_id,
                    stage: stage.to_string(),
                    completed_files: files.load(Ordering::Relaxed),
                    total_files,
                    downloaded_bytes: bytes.load(Ordering::Relaxed),
                    total_bytes,
                },
            );
        };

        loop {
            tokio::select! {
                event = rx.recv() => match event {
                    Some(DownloadEvent::Bytes(n)) => {
                        bytes.fetch_add(n, Ordering::Relaxed);
                    }
                    Some(DownloadEvent::FileComplete { .. }) => {
                        files.fetch_add(1, Ordering::Relaxed);
                    }
                    // Sender dropped: work is finished.
                    None => {
                        emit("Downloading", &bytes, &files);
                        return;
                    }
                },
                _ = ticker.tick() => emit("Downloading", &bytes, &files),
            }
        }
    });

    (tx, handle)
}

fn forward_logs(app: AppHandle, instance_id: Uuid, mut rx: mpsc::UnboundedReceiver<GameOutput>) {
    tokio::spawn(async move {
        while let Some(output) = rx.recv().await {
            let _ = app.emit(
                "game://log",
                GameLogLine {
                    instance_id,
                    line: output.line,
                    is_stderr: output.is_stderr,
                },
            );
        }
    });
}

fn watch_for_exit(
    app: AppHandle,
    state: Arc<AppState>,
    instance_id: Uuid,
    child: Arc<tokio::sync::Mutex<tokio::process::Child>>,
) {
    tokio::spawn(async move {
        let status = child.lock().await.wait().await;
        let code = status.as_ref().ok().and_then(std::process::ExitStatus::code);

        // Clear the running flag before announcing the exit, so a UI that
        // refreshes on the event sees Play rather than a stale Running.
        state.forget_running(instance_id).await;

        let _ = app.emit(
            "game://exit",
            GameExit {
                instance_id,
                code,
                crashed: code != Some(0),
            },
        );
    });
}
