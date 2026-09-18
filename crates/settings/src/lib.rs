//! Application settings: JSON persistence under the config directory and a
//! GPUI global store.
//!
//! The file is the source of truth on disk; the [`SettingsStore`] global is
//! the in-memory working copy. Mutations go through [`SettingsStore::update`],
//! which saves to disk synchronously (the file is small) and notifies
//! observers through `cx.notify` on the store entity when one is attached.

use gpui::{App, AppContext, Global};
use paths::config_dir;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::OnceLock;

/// User-adjustable application settings.
///
/// Unknown fields in the file are ignored (`deny_unknown_fields` is
/// deliberately not set) so newer builds can add fields without bricking
/// older settings files.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Opt-in telemetry: crash reports are uploaded to Sentry on the launch
    /// after a crash. Default off; also requires a DSN to be configured.
    pub telemetry_enabled: bool,
    /// Sentry DSN (project key endpoint). `None`/empty disables reporting
    /// even when `telemetry_enabled` is set.
    pub telemetry_dsn: Option<String>,
    /// Whether the changes (staging) strip starts pinned open per workspace.
    pub keep_changes_strip_open: bool,
    /// Ask for confirmation before closing a workspace tab with unsaved
    /// changes.
    pub confirm_close_with_changes: bool,
    /// Recently opened archives, most recent first (absolute paths). Capped
    /// at [`MAX_RECENT_ARCHIVES`].
    pub recent_archives: Vec<String>,
}

/// How many entries `recent_archives` keeps.
pub const MAX_RECENT_ARCHIVES: usize = 10;

impl Default for Settings {
    fn default() -> Self {
        Self {
            telemetry_enabled: false,
            telemetry_dsn: None,
            keep_changes_strip_open: false,
            confirm_close_with_changes: true,
            recent_archives: Vec::new(),
        }
    }
}

impl Settings {
    /// Moves `path` to the front of the recent list (deduped, capped).
    /// Returns true when the list changed.
    pub fn push_recent_archive(&mut self, path: &str) -> bool {
        if self.recent_archives.first().is_some_and(|first| first == path) {
            return false;
        }
        self.recent_archives.retain(|entry| entry != path);
        self.recent_archives.insert(0, path.to_string());
        self.recent_archives.truncate(MAX_RECENT_ARCHIVES);
        true
    }
}

/// File the settings persist to: `<config_dir>/settings.json`.
pub fn settings_file() -> PathBuf {
    config_dir().join("settings.json")
}

/// In-memory settings store, registered as a GPUI global.
pub struct SettingsStore {
    settings: Settings,
    file: PathBuf,
}

struct GlobalSettingsStore(SettingsStore);

impl Global for GlobalSettingsStore {}

/// Loads (or creates) the settings file and registers the global store.
/// Call once at startup.
pub fn init(cx: &mut App) {
    let file = settings_file();
    let settings = load(&file);
    cx.set_global(GlobalSettingsStore(SettingsStore { settings, file }));
}

/// Read-only access to the current settings.
pub fn global(cx: &App) -> &Settings {
    &cx.global::<GlobalSettingsStore>().0.settings
}

/// Mutates the settings and persists them to disk immediately.
pub fn update<R>(cx: &mut App, f: impl FnOnce(&mut Settings) -> R) -> R {
    let store = &mut cx.global_mut::<GlobalSettingsStore>().0;
    let result = f(&mut store.settings);
    if let Err(err) = save(&store.file, &store.settings) {
        eprintln!("settings save failed: {err}");
    }
    result
}

fn load(file: &PathBuf) -> Settings {
    match std::fs::read_to_string(file) {
        Ok(json) => match serde_json::from_str(&json) {
            Ok(settings) => settings,
            Err(err) => {
                eprintln!("settings parse failed (using defaults): {err}");
                Settings::default()
            }
        },
        Err(_) => Settings::default(),
    }
}

fn save(file: &PathBuf, settings: &Settings) -> Result<(), String> {
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    // Write-then-rename so a crash mid-write cannot truncate the file.
    let tmp = file.with_extension("json.tmp");
    std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, file).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_roundtrip_through_json() {
        let json = serde_json::to_string(&Settings::default()).unwrap();
        let parsed: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, Settings::default());
        assert!(!parsed.telemetry_enabled, "telemetry must default off");
    }

    #[test]
    fn unknown_fields_are_ignored() {
        let json = r#"{"telemetry_enabled": true, "future_field": 42}"#;
        let parsed: Settings = serde_json::from_str(json).unwrap();
        assert!(parsed.telemetry_enabled);
    }

    #[test]
    fn empty_file_yields_defaults() {
        let parsed: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(parsed, Settings::default());
    }

    #[test]
    fn recent_archives_dedupe_and_cap() {
        let mut settings = Settings::default();
        for i in 0..(MAX_RECENT_ARCHIVES + 3) {
            settings.push_recent_archive(&format!("/a/{i}.7z"));
        }
        assert_eq!(settings.recent_archives.len(), MAX_RECENT_ARCHIVES);
        assert_eq!(settings.recent_archives[0], "/a/12.7z");

        settings.push_recent_archive("/a/12.7z");
        assert_eq!(settings.recent_archives[0], "/a/12.7z");
        assert_eq!(settings.recent_archives.len(), MAX_RECENT_ARCHIVES);

        settings.push_recent_archive("/a/12.7z");
        assert_eq!(settings.recent_archives[0], "/a/12.7z");
    }

    #[test]
    fn save_writes_json_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("settings.json");
        let settings = Settings {
            telemetry_enabled: true,
            telemetry_dsn: Some("https://k@example.com/1".into()),
            ..Settings::default()
        };
        save(&file, &settings).unwrap();
        let loaded: Settings =
            serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(loaded.telemetry_dsn.as_deref(), Some("https://k@example.com/1"));
    }
}
