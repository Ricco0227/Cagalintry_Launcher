//! Process-wide state shared by every command.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use cagalintry_mc::{DataDirs, Installer, JavaProvisioner, LoaderInstaller};
use cagalintry_net::Downloader;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::instance::InstanceStore;
use crate::settings::{Settings, SettingsPatch};

pub struct AppState {
    pub dirs: DataDirs,
    pub downloader: Downloader,
    pub instances: InstanceStore,

    /// Cached so the UI can read settings without touching the disk on every
    /// render; the file stays the source of truth across restarts.
    settings: Mutex<Settings>,

    /// Instances with an install or update in flight. The primary button reads
    /// this to render as busy, and command handlers read it to refuse starting
    /// the same work twice.
    busy: Mutex<HashSet<Uuid>>,

    /// Instances with a live game process, so a second Play can't start a
    /// second copy writing to the same world files.
    running: Mutex<HashMap<Uuid, Arc<Mutex<tokio::process::Child>>>>,
}

impl AppState {
    pub async fn new() -> anyhow::Result<Self> {
        let dirs = DataDirs::discover()?;
        let settings = Settings::load(&dirs).await;
        // Concurrency comes from settings, so the downloader is built after
        // them rather than reconfigured later.
        let downloader = Downloader::with_concurrency(settings.download_concurrency)?;

        Ok(Self {
            instances: InstanceStore::new(dirs.clone()),
            dirs,
            downloader,
            settings: Mutex::new(settings),
            busy: Mutex::new(HashSet::new()),
            running: Mutex::new(HashMap::new()),
        })
    }

    pub async fn settings(&self) -> Settings {
        self.settings.lock().await.clone()
    }

    /// Apply and persist. Concurrency changes take effect on the next restart,
    /// since the running downloader's permit count is fixed at construction.
    pub async fn update_settings(&self, patch: SettingsPatch) -> anyhow::Result<Settings> {
        let mut settings = self.settings.lock().await;
        settings.apply(patch);
        settings.save(&self.dirs).await?;
        Ok(settings.clone())
    }

    /// The Java override to use for an instance: the instance's own setting
    /// first, then the global one, then automatic selection.
    pub async fn java_override(&self, instance: Option<&std::path::Path>) -> Option<PathBuf> {
        match instance {
            Some(path) => Some(path.to_path_buf()),
            None => self.settings.lock().await.java_path.clone(),
        }
    }

    pub fn installer(&self) -> Installer {
        Installer::new(self.downloader.clone(), self.dirs.clone())
    }

    pub fn loaders(&self) -> LoaderInstaller {
        LoaderInstaller::new(self.downloader.clone(), self.dirs.clone())
    }

    pub fn java(&self) -> JavaProvisioner {
        JavaProvisioner::new(self.downloader.clone(), self.dirs.clone())
    }

    /// Claim an instance for work. `false` means something is already running
    /// for it and the caller should do nothing.
    pub async fn try_claim(&self, id: Uuid) -> bool {
        self.busy.lock().await.insert(id)
    }

    pub async fn release(&self, id: Uuid) {
        self.busy.lock().await.remove(&id);
    }

    pub async fn is_busy(&self, id: Uuid) -> bool {
        self.busy.lock().await.contains(&id)
    }

    pub async fn is_running(&self, id: Uuid) -> bool {
        self.running.lock().await.contains_key(&id)
    }

    pub async fn register_running(&self, id: Uuid, child: tokio::process::Child) -> Arc<Mutex<tokio::process::Child>> {
        let child = Arc::new(Mutex::new(child));
        self.running.lock().await.insert(id, Arc::clone(&child));
        child
    }

    pub async fn forget_running(&self, id: Uuid) {
        self.running.lock().await.remove(&id);
    }

    pub async fn running_child(&self, id: Uuid) -> Option<Arc<Mutex<tokio::process::Child>>> {
        self.running.lock().await.get(&id).cloned()
    }
}

/// Errors as the frontend sees them: a plain message, plus a stable code the UI
/// can branch on without matching prose.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandError {
    pub code: &'static str,
    pub message: String,
}

impl CommandError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self { code, message: message.into() }
    }

}

/// A blanket `impl<E: Display>` would collide with the reflexive `From<T> for T`,
/// so the conversions are spelled out for the error types commands actually
/// return. Each keeps its own code so the UI can react to it specifically.
macro_rules! command_error_from {
    ($($ty:path => $code:literal),* $(,)?) => {
        $(
            impl From<$ty> for CommandError {
                fn from(error: $ty) -> Self {
                    Self::new($code, error.to_string())
                }
            }
        )*
    };
}

command_error_from! {
    crate::instance::InstanceError => "instance",
    cagalintry_mc::InstallError => "install",
    cagalintry_mc::LoaderError => "loader",
    cagalintry_mc::JavaError => "java",
    cagalintry_mc::LaunchError => "launch",
    cagalintry_net::DownloadError => "download",
    std::io::Error => "io",
    anyhow::Error => "internal",
}

pub type CommandResult<T> = Result<T, CommandError>;
