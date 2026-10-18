//! The setup panel's choices, kept between runs in the user's config directory.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use un_core::Language;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Saved {
    pub camera: Option<u32>,
    pub mirror: Option<bool>,
    pub detector: Option<String>,
    pub strategy: Option<String>,
    pub language: Option<Language>,
    pub child_name: Option<String>,
    /// The chosen child's profile (see `profiles_path`).
    pub child: Option<String>,
    pub total: Option<u32>,
    pub tasks: Option<u32>,
    /// auto | piper | console | off
    pub voice: Option<String>,
    pub mic: Option<bool>,
    /// A microphone name; `None` for the system default.
    pub mic_device: Option<String>,
    pub chime: Option<bool>,
    /// What the lessons asked to remember between sessions (names, where a lesson stopped);
    /// handed back as `SessionConfig::extra`.
    pub extra: HashMap<String, String>,
}

/// `~/Library/Application Support/un/desktop.json` on macOS, `$XDG_CONFIG_HOME/un/desktop.json`
/// on Linux, `%APPDATA%\un\desktop.json` on Windows; `UN_CONFIG` overrides the directory.
/// The children's profiles, next to the settings (as on the phones).
pub fn profiles_path() -> PathBuf {
    path().and_then(|p| p.parent().map(|d| d.join("profiles.json"))).unwrap_or_else(|| PathBuf::from("profiles.json"))
}

pub fn path() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("UN_CONFIG") {
        return Some(PathBuf::from(dir).join("desktop.json"));
    }
    let base = if cfg!(target_os = "windows") {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
    };
    base.map(|b| b.join("un").join("desktop.json"))
}

/// `lesson.log` next to the settings: one line per thing the engine does, for looking into a
/// session afterwards.
pub fn log_path() -> Option<PathBuf> {
    path().map(|p| p.with_file_name("lesson.log"))
}

pub fn load() -> Saved {
    let Some(path) = path() else { return Saved::default() };
    match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_else(|error| {
            log::warn!("{}: {error}; using defaults", path.display());
            Saved::default()
        }),
        Err(_) => Saved::default(),
    }
}

pub fn save(saved: &Saved) {
    let Some(path) = path() else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    match serde_json::to_string_pretty(saved) {
        Ok(text) => {
            if let Err(error) = std::fs::write(&path, text + "\n") {
                log::warn!("could not save {}: {error}", path.display());
            }
        }
        Err(error) => log::warn!("settings: {error}"),
    }
}
