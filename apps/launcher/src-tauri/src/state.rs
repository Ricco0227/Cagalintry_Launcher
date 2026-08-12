//! Process-wide state shared by every command.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use cagalintry_mc::{DataDirs, Installer, JavaProvisioner};
use cagalintry_net::Downloader;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::instance::InstanceStore;

pub struct AppState {
    pub dirs: DataDirs,
    pub downloader: Downloader,
    pub instances: InstanceStore,

    /// Instances with an install or update in flight. The primary button reads
    /// this to render as busy, and command handlers read it to refuse starting
    /// the same work twice.
    busy: Mutex<HashSet<Uuid>>,

    /// Instances with a live game process, so a second Play can't start a
    /// second copy writing to the same world files.
    running: Mutex<HashMap<Uuid, Arc<Mutex<tokio::process::Child>>>>,
}

impl AppState {
    pub fn new() -> anyhow::Result<Self> {
        let dirs = DataDirs::discover()?;
        let downloader = Downloader::new()?;
        Ok(Self {
            instances: InstanceStore::new(dirs.clone()),
            dirs,
            downloader,
            busy: Mutex::new(HashSet::new()),
            running: Mutex::new(HashMap::new()),
        })
    }

    pub fn installer(&self) -> Installer {
        Installer::new(self.downloader.clone(), self.dirs.clone())
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
    cagalintry_mc::JavaError => "java",
    cagalintry_mc::LaunchError => "launch",
    cagalintry_net::DownloadError => "download",
    std::io::Error => "io",
    anyhow::Error => "internal",
}

pub type CommandResult<T> = Result<T, CommandError>;
