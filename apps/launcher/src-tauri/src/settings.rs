//! Launcher-wide settings.
//!
//! A single JSON file in the data root. Small enough that it is read and
//! written whole, and readable enough that a person can fix it by hand if the
//! launcher ever refuses to start because of it.

use std::path::PathBuf;

use cagalintry_mc::DataDirs;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Theme {
    /// Follow the operating system.
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    #[serde(default)]
    pub theme: Theme,

    /// Simultaneous downloads. Higher is not always faster — an install is
    /// thousands of small asset files and past a point the bottleneck becomes
    /// the disk and the CDN's patience rather than bandwidth.
    #[serde(default = "default_concurrency")]
    pub download_concurrency: usize,

    /// Applied to newly created instances; existing ones keep their own value.
    #[serde(default = "default_memory")]
    pub default_max_memory_mb: u32,

    /// Overrides Java selection everywhere. Left unset, the launcher uses
    /// Mojang's runtime for the version and falls back to a system JVM new
    /// enough to run it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub java_path: Option<PathBuf>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: Theme::default(),
            download_concurrency: default_concurrency(),
            default_max_memory_mb: default_memory(),
            java_path: None,
        }
    }
}

const fn default_concurrency() -> usize {
    8
}

const fn default_memory() -> u32 {
    4096
}

/// Changes to apply. Every field optional so the frontend can send only what
/// the player actually touched.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsPatch {
    pub theme: Option<Theme>,
    pub download_concurrency: Option<usize>,
    pub default_max_memory_mb: Option<u32>,
    /// `Some(None)` clears the override; absent leaves it alone.
    #[serde(default, with = "self::double_option")]
    pub java_path: Option<Option<PathBuf>>,
}

impl Settings {
    pub fn apply(&mut self, patch: SettingsPatch) {
        if let Some(theme) = patch.theme {
            self.theme = theme;
        }
        if let Some(concurrency) = patch.download_concurrency {
            // Zero would deadlock the download engine; absurdly high just
            // wastes file handles.
            self.download_concurrency = concurrency.clamp(1, 32);
        }
        if let Some(memory) = patch.default_max_memory_mb {
            // Below 512 MB the game cannot start at all.
            self.default_max_memory_mb = memory.clamp(512, 65536);
        }
        if let Some(java_path) = patch.java_path {
            self.java_path = java_path.filter(|p| !p.as_os_str().is_empty());
        }
    }

    pub async fn load(dirs: &DataDirs) -> Self {
        let path = dirs.root().join("settings.json");
        match tokio::fs::read(&path).await {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|err| {
                // A corrupt settings file must not stop the launcher opening —
                // defaults are always a usable configuration.
                tracing::warn!(path = %path.display(), error = %err, "settings unreadable, using defaults");
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }

    pub async fn save(&self, dirs: &DataDirs) -> std::io::Result<()> {
        let path = dirs.root().join("settings.json");
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let json = serde_json::to_vec_pretty(self)?;
        tokio::fs::write(&path, json).await
    }
}

/// `Option<Option<T>>` where an absent key and an explicit `null` mean
/// different things: leave alone, versus clear.
mod double_option {
    use serde::{Deserialize, Deserializer};

    pub fn deserialize<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
    where
        D: Deserializer<'de>,
        T: Deserialize<'de>,
    {
        Option::<T>::deserialize(deserializer).map(Some)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_a_usable_configuration() {
        let settings = Settings::default();
        assert_eq!(settings.theme, Theme::System);
        assert!(settings.download_concurrency >= 1);
        assert!(settings.default_max_memory_mb >= 512);
        assert!(settings.java_path.is_none());
    }

    #[test]
    fn a_patch_only_changes_what_it_mentions() {
        let mut settings = Settings::default();
        settings.apply(SettingsPatch { theme: Some(Theme::Dark), ..Default::default() });

        assert_eq!(settings.theme, Theme::Dark);
        assert_eq!(settings.download_concurrency, default_concurrency());
        assert_eq!(settings.default_max_memory_mb, default_memory());
    }

    #[test]
    fn concurrency_is_clamped_to_something_workable() {
        let mut settings = Settings::default();

        // Zero would stall every download waiting for a permit.
        settings.apply(SettingsPatch { download_concurrency: Some(0), ..Default::default() });
        assert_eq!(settings.download_concurrency, 1);

        settings.apply(SettingsPatch { download_concurrency: Some(9999), ..Default::default() });
        assert_eq!(settings.download_concurrency, 32);
    }

    #[test]
    fn memory_cannot_be_set_below_what_the_game_needs() {
        let mut settings = Settings::default();
        settings.apply(SettingsPatch { default_max_memory_mb: Some(64), ..Default::default() });
        assert_eq!(settings.default_max_memory_mb, 512);
    }

    #[test]
    fn an_explicit_null_java_path_clears_the_override() {
        let mut settings = Settings { java_path: Some("/opt/jdk/bin/java".into()), ..Default::default() };

        settings.apply(SettingsPatch { java_path: Some(None), ..Default::default() });
        assert!(settings.java_path.is_none());
    }

    #[test]
    fn an_absent_java_path_leaves_the_override_alone() {
        let mut settings = Settings { java_path: Some("/opt/jdk/bin/java".into()), ..Default::default() };

        settings.apply(SettingsPatch::default());
        assert!(settings.java_path.is_some());
    }

    #[test]
    fn an_empty_java_path_is_treated_as_cleared() {
        // What a text field sends when the player deletes its contents.
        let mut settings = Settings { java_path: Some("/opt/jdk/bin/java".into()), ..Default::default() };
        settings.apply(SettingsPatch { java_path: Some(Some(PathBuf::new())), ..Default::default() });
        assert!(settings.java_path.is_none());
    }

    #[test]
    fn unknown_and_missing_fields_fall_back_to_defaults() {
        // Forward compatibility: a settings file written by a newer build must
        // not stop an older one from starting.
        let json = r#"{"theme":"dark","somethingNew":42}"#;
        let settings: Settings = serde_json::from_str(json).unwrap();
        assert_eq!(settings.theme, Theme::Dark);
        assert_eq!(settings.download_concurrency, default_concurrency());
    }
}
