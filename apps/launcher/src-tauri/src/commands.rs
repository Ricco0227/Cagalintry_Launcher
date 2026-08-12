//! The IPC surface the frontend calls.
//!
//! Commands stay thin: claim the modpack, delegate to the crates that do the
//! real work, and translate progress into events the UI can render. Anything
//! worth testing lives below this layer, where it can be tested without a
//! webview.

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use cagalintry_mc::launch::{self, GameOutput, LaunchOptions, LaunchSession};
use cagalintry_net::DownloadEvent;
use cagalintry_mc::LoaderVersion;
use cagalintry_modrinth::{
    ProjectPage, SearchQuery, SearchResults, SearchSort, Version as ModrinthVersion, VersionFilter,
};
use cagalintry_proto::{EntryKind, LoaderKind, LoaderSpec, PackEntry};
use serde::Serialize;
use tauri::{AppHandle, Emitter as _, State};
use time::OffsetDateTime;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::pack::Pack;
use crate::primary_action::{PackStatus, PrimaryAction, resolve};
use crate::settings::{Settings, SettingsPatch};
use crate::state::{AppState, CommandError, CommandResult};

/// Progress is emitted on a timer rather than per chunk: a full install is
/// hundreds of thousands of byte events, and forwarding each one to the webview
/// costs more than the download does.
const PROGRESS_INTERVAL: std::time::Duration = std::time::Duration::from_millis(100);

// ---------------------------------------------------------------------------
// Views
// ---------------------------------------------------------------------------

/// A modpack as the Library renders it, with the button state already
/// resolved so the frontend never has to reimplement those rules.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackView {
    #[serde(flatten)]
    pub pack: Pack,
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
    pub pack_id: Uuid,
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
    pub pack_id: Uuid,
    pub line: String,
    pub is_stderr: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameExit {
    pub pack_id: Uuid,
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
pub async fn list_packs(state: State<'_, Arc<AppState>>) -> CommandResult<Vec<PackView>> {
    let packs = state.packs.list().await?;

    let mut views = Vec::with_capacity(packs.len());
    for pack in packs {
        let action = pack_action(&state, &pack).await;
        views.push(PackView { pack, action });
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

/// Loader builds available for a Minecraft version, newest first.
#[tauri::command]
pub async fn list_loader_versions(
    state: State<'_, Arc<AppState>>,
    kind: LoaderKind,
    mc_version: String,
) -> CommandResult<Vec<LoaderVersion>> {
    Ok(state.loaders().list_versions(kind, &mc_version).await?)
}

#[tauri::command]
pub async fn create_pack(
    state: State<'_, Arc<AppState>>,
    name: String,
    mc_version: String,
    loader: Option<LoaderSpec>,
) -> CommandResult<PackView> {
    let mut pack = Pack::new(name, mc_version, loader.unwrap_or_else(LoaderSpec::vanilla));
    // New packs start from the launcher-wide default; the pack's own
    // setting takes over from then on.
    pack.max_memory_mb = state.settings().await.default_max_memory_mb;

    state.packs.save(&pack).await?;

    let action = pack_action(&state, &pack).await;
    Ok(PackView { pack, action })
}

#[tauri::command]
pub async fn get_pack(state: State<'_, Arc<AppState>>, id: Uuid) -> CommandResult<PackView> {
    let pack = state.packs.get(id).await?;
    let action = pack_action(&state, &pack).await;
    Ok(PackView { pack, action })
}

/// Per-pack settings. Every field optional — the frontend sends only what
/// changed, so two panels editing different fields can't clobber each other.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackPatch {
    pub name: Option<String>,
    pub max_memory_mb: Option<u32>,
    pub java_path: Option<String>,
    pub extra_jvm_args: Option<Vec<String>>,
}

#[tauri::command]
pub async fn update_pack(
    state: State<'_, Arc<AppState>>,
    id: Uuid,
    patch: PackPatch,
) -> CommandResult<PackView> {
    let mut pack = state.packs.get(id).await?;

    if let Some(name) = patch.name {
        pack.name = name;
    }
    if let Some(memory) = patch.max_memory_mb {
        // Below 512 MB the game cannot start; the upper bound just stops a
        // stray keystroke asking for a terabyte of heap.
        pack.max_memory_mb = memory.clamp(512, 65536);
    }
    if let Some(java_path) = patch.java_path {
        // An emptied text field means "use the default", not "use ''".
        pack.java_path = (!java_path.trim().is_empty()).then(|| java_path.into());
    }
    if let Some(args) = patch.extra_jvm_args {
        pack.extra_jvm_args = args.into_iter().filter(|a| !a.trim().is_empty()).collect();
    }

    state.packs.save(&pack).await?;
    let action = pack_action(&state, &pack).await;
    Ok(PackView { pack, action })
}

// ---------------------------------------------------------------------------
// Content
// ---------------------------------------------------------------------------

/// Search Modrinth, scoped to what a pack can actually use.
#[tauri::command]
pub async fn search_content(
    state: State<'_, Arc<AppState>>,
    kind: EntryKind,
    query: String,
    mc_version: Option<String>,
    loader: Option<LoaderKind>,
    sort: Option<SearchSort>,
    offset: Option<u32>,
    limit: Option<u32>,
) -> CommandResult<SearchResults> {
    let search = SearchQuery {
        text: query,
        kind,
        mc_version,
        loader,
        sort: sort.unwrap_or_default(),
        offset: offset.unwrap_or(0),
        limit: limit.unwrap_or(20),
    };
    Ok(state.modrinth().search(&search).await?)
}

/// A project's full page: metadata, links, gallery, and its description
/// rendered to sanitised HTML.
#[tauri::command]
pub async fn get_project(
    state: State<'_, Arc<AppState>>,
    project_id: String,
) -> CommandResult<ProjectPage> {
    Ok(state.modrinth().project(&project_id).await?.into())
}

/// Versions of a project, newest first.
///
/// With a pack id, narrowed to what that pack could actually install. Without
/// one — browsing from Discover, where nothing is installable — every version
/// is listed, because there is no pack whose compatibility could define
/// "usable".
#[tauri::command]
pub async fn list_project_versions(
    state: State<'_, Arc<AppState>>,
    id: Option<Uuid>,
    project_id: String,
    kind: EntryKind,
) -> CommandResult<Vec<ModrinthVersion>> {
    let filter = match id {
        Some(id) => {
            let pack = state.packs.get(id).await?;
            VersionFilter {
                mc_version: Some(pack.mc_version),
                loader: Some(pack.loader.kind),
                apply_loader: kind == EntryKind::Mod,
            }
        }
        None => VersionFilter::default(),
    };

    Ok(state.modrinth().versions(&project_id, &filter).await?)
}

#[tauri::command]
pub async fn list_content(
    state: State<'_, Arc<AppState>>,
    id: Uuid,
) -> CommandResult<Vec<PackEntry>> {
    Ok(state.content().list(id).await?)
}

/// Install content into a pack, together with anything it requires.
///
/// `version_id` pins an exact build; without it the newest compatible release
/// is chosen. Required dependencies are resolved transitively — installing a
/// mod whose library is missing produces a crash on launch, not a clear error,
/// so leaving that to the player is not a kindness.
#[tauri::command]
pub async fn install_content(
    state: State<'_, Arc<AppState>>,
    id: Uuid,
    project_id: String,
    kind: EntryKind,
    version_id: Option<String>,
) -> CommandResult<Vec<PackEntry>> {
    let pack = state.packs.get(id).await?;
    let client = state.modrinth();
    let content = state.content();

    let filter = VersionFilter {
        mc_version: Some(pack.mc_version.clone()),
        loader: Some(pack.loader.kind),
        // Resource and shader packs declare no loader; filtering by one would
        // reject every version they have.
        apply_loader: kind == EntryKind::Mod,
    };

    let mut queue = vec![(project_id, kind, version_id)];
    let mut seen: HashSet<String> = HashSet::new();

    while let Some((project_id, kind, version_id)) = queue.pop() {
        if !seen.insert(project_id.clone()) {
            continue; // already handled, and dependency graphs do contain cycles
        }

        let version = match version_id {
            Some(version_id) => client.version(&version_id).await?,
            None => client.best_version(&project_id, &filter).await?,
        };

        let project = client.project(&project_id).await?;
        let Some(entry) = version.to_pack_entry(kind, project.client_side.as_deref()) else {
            continue; // a version with no downloadable file
        };

        content.install(id, entry, &state.downloader).await?;

        // A dependency of anything is a mod: a shader pack requiring Iris
        // requires the mod, not another shader pack.
        for dependency in version.required_dependencies() {
            queue.push((dependency.to_string(), EntryKind::Mod, None));
        }
    }

    Ok(content.list(id).await?)
}

#[tauri::command]
pub async fn remove_content(
    state: State<'_, Arc<AppState>>,
    id: Uuid,
    path: String,
) -> CommandResult<Vec<PackEntry>> {
    let content = state.content();
    content.remove(id, &path).await?;
    Ok(content.list(id).await?)
}

#[tauri::command]
pub async fn set_content_enabled(
    state: State<'_, Arc<AppState>>,
    id: Uuid,
    path: String,
    enabled: bool,
) -> CommandResult<Vec<PackEntry>> {
    let content = state.content();
    content.set_enabled(id, &path, enabled).await?;
    Ok(content.list(id).await?)
}

#[tauri::command]
pub async fn get_settings(state: State<'_, Arc<AppState>>) -> CommandResult<Settings> {
    Ok(state.settings().await)
}

#[tauri::command]
pub async fn update_settings(
    state: State<'_, Arc<AppState>>,
    patch: SettingsPatch,
) -> CommandResult<Settings> {
    Ok(state.update_settings(patch).await?)
}

/// Absolute path of the launcher's data directory, for the Settings page.
#[tauri::command]
pub fn data_directory(state: State<'_, Arc<AppState>>) -> String {
    state.dirs.root().display().to_string()
}

#[tauri::command]
pub async fn delete_pack(state: State<'_, Arc<AppState>>, id: Uuid) -> CommandResult<()> {
    if state.is_running(id).await {
        return Err(CommandError::new(
            "running",
            "close the game before deleting this pack",
        ));
    }
    state.packs.delete(id).await?;
    Ok(())
}

#[tauri::command]
pub async fn open_pack_folder(state: State<'_, Arc<AppState>>, id: Uuid) -> CommandResult<String> {
    Ok(state.packs.game_dir(id).display().to_string())
}

/// Install whatever is missing and start the game.
///
/// Safe to call twice: the second call finds the pack claimed and returns
/// immediately rather than racing the first over the same files.
#[tauri::command]
pub async fn launch_pack(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    id: Uuid,
) -> CommandResult<()> {
    if state.is_running(id).await {
        return Err(CommandError::new("running", "this pack is already running"));
    }
    if !state.try_claim(id).await {
        return Err(CommandError::new("busy", "this pack is already being prepared"));
    }

    let result = prepare_and_launch(&app, &state, id).await;
    state.release(id).await;
    result
}

async fn prepare_and_launch(app: &AppHandle, state: &Arc<AppState>, id: Uuid) -> CommandResult<()> {
    let mut pack = state.packs.get(id).await?;
    let session = session_for_launch()?;

    emit_stage(app, id, "Resolving version");

    let installer = state.installer();
    let manifest = installer.version_manifest().await?;

    // Vanilla is resolved and installed first regardless of loader. Every
    // loader profile inherits from it, and NeoForge additionally needs the
    // vanilla client jar and a JVM on disk before its install can even run —
    // its patched client is derived from them locally.
    let vanilla = installer.resolve(&installer.version_detail(&manifest, &pack.mc_version).await?)?;

    let plan = installer.plan(&vanilla).await?;
    let (progress, pump) =
        progress_channel(app.clone(), id, plan.file_count as u64, plan.total_bytes);

    emit_stage(app, id, "Downloading Minecraft");
    installer.install(&vanilla, Some(&progress)).await?;

    emit_stage(app, id, "Preparing Java");
    let java_override = state.java_override(pack.java_path.as_deref()).await;
    let java = state
        .java()
        .provide(
            vanilla.java_component(),
            vanilla.java_major_version(),
            java_override.as_deref(),
            Some(&progress),
        )
        .await?;

    // With vanilla in place, the loader profile can be produced and then
    // resolved like any other version document — everything below this point
    // is loader-agnostic.
    let resolved = if pack.loader.kind == LoaderKind::Vanilla {
        vanilla
    } else {
        emit_stage(app, id, &format!("Installing {}", pack.loader.kind.display_name()));

        let version_id = state
            .loaders()
            .ensure_profile(
                pack.loader.kind,
                &pack.mc_version,
                pack.loader.version.as_deref().unwrap_or_default(),
                &java.executable,
                &vanilla.client_jar,
                Some(&progress),
            )
            .await?;

        let detail = installer.version_detail(&manifest, &version_id).await?;
        let resolved = installer.resolve(&detail)?;

        emit_stage(app, id, "Downloading mod loader");
        installer.install(&resolved, Some(&progress)).await?;
        resolved
    };

    // Stop the aggregator before launching so no stale progress arrives after
    // the game window is up.
    drop(progress);
    let _ = pump.await;

    let mut options = LaunchOptions::new(session, state.packs.game_dir(id));
    options.max_memory_mb = pack.max_memory_mb;
    options.extra_jvm_args = pack.extra_jvm_args.clone();

    let command = launch::build_command(
        &resolved,
        &java,
        &options,
        &state.dirs.assets(),
        &cagalintry_mc::Platform::current(),
    );
    tracing::info!(pack = %id, command = %command.to_redacted_string(), "launching");

    let (log_tx, log_rx) = mpsc::unbounded_channel();
    let child = launch::spawn(&command, Some(log_tx)).await?;

    forward_logs(app.clone(), id, log_rx);

    let child = state.register_running(id, child).await;
    watch_for_exit(app.clone(), Arc::clone(state), id, child);

    pack.last_played = Some(OffsetDateTime::now_utc());
    state.packs.save(&pack).await?;

    Ok(())
}

#[tauri::command]
pub async fn kill_pack(state: State<'_, Arc<AppState>>, id: Uuid) -> CommandResult<()> {
    let Some(child) = state.running_child(id).await else {
        return Ok(());
    };
    child.lock().await.kill().await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

async fn pack_action(state: &AppState, pack: &Pack) -> PrimaryAction {
    resolve(PackStatus {
        has_linked_minecraft_account: has_linked_account(),
        busy: state.is_busy(pack.id).await,
        running: state.is_running(pack.id).await,
        installed_revision: pack.installed_revision,
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

fn emit_stage(app: &AppHandle, pack_id: Uuid, stage: &str) {
    let _ = app.emit(
        "install://progress",
        InstallProgress {
            pack_id,
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
    pack_id: Uuid,
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
                    pack_id,
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

fn forward_logs(app: AppHandle, pack_id: Uuid, mut rx: mpsc::UnboundedReceiver<GameOutput>) {
    tokio::spawn(async move {
        while let Some(output) = rx.recv().await {
            let _ = app.emit(
                "game://log",
                GameLogLine {
                    pack_id,
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
    pack_id: Uuid,
    child: Arc<tokio::sync::Mutex<tokio::process::Child>>,
) {
    tokio::spawn(async move {
        let status = child.lock().await.wait().await;
        let code = status.as_ref().ok().and_then(std::process::ExitStatus::code);

        // Clear the running flag before announcing the exit, so a UI that
        // refreshes on the event sees Play rather than a stale Running.
        state.forget_running(pack_id).await;

        let _ = app.emit(
            "game://exit",
            GameExit {
                pack_id,
                code,
                crashed: code != Some(0),
            },
        );
    });
}
