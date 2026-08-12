//! Minecraft itself: version metadata, library and asset resolution, Java
//! provisioning, launch argument construction, and process supervision.

pub mod install;
pub mod java;
pub mod launch;
pub mod loader;
pub mod maven;
pub mod meta;
pub mod neoforge;
pub mod paths;
pub mod rules;

pub use install::{InstallError, Installer, ResolvedVersion};
pub use java::{JavaError, JavaProvisioner, JavaRuntime, JavaSource};
pub use launch::{LaunchCommand, LaunchError, LaunchOptions, LaunchSession};
pub use loader::{LoaderError, LoaderInstaller, LoaderVersion};
pub use paths::{DataDirs, InstanceDirs};
pub use rules::Platform;
