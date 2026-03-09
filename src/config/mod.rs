use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Persistent user settings, saved to the OS config directory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// Name of the preferred video capture device (partial match is accepted)
    pub video_device: Option<String>,
    /// Name of the preferred audio input device (partial match is accepted)
    pub audio_device: Option<String>,
    /// Capture width in pixels
    pub width: u32,
    /// Capture height in pixels
    pub height: u32,
    /// Capture FPS
    pub fps: u32,
    /// Playback volume (0.0 = mute, 1.0 = unity, up to 2.0)
    pub volume: f32,
    /// Default path for recorded videos
    pub record_path: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            video_device: None,
            audio_device: None,
            width: 1920,
            height: 1080,
            fps: 60,
            volume: 1.0,
            record_path: "capture.mp4".to_string(),
        }
    }
}

impl AppConfig {
    /// Returns the path to the config file:
    /// - Linux/macOS: `~/.config/shadowrust/config.json`
    /// - Windows:     `%APPDATA%\shadowrust\config.json`
    fn config_path() -> Option<PathBuf> {
        let base = dirs::config_dir()?;
        Some(base.join("shadowrust").join("config.json"))
    }

    /// Load config from disk, returning `Default` if the file doesn't exist
    /// or can't be parsed (so the app always starts cleanly).
    pub fn load() -> Self {
        let Some(path) = Self::config_path() else {
            return Self::default();
        };
        match std::fs::read_to_string(&path) {
            Ok(contents) => serde_json::from_str(&contents).unwrap_or_else(|e| {
                log::warn!("Config parse error ({e}); using defaults");
                Self::default()
            }),
            Err(_) => Self::default(), // file doesn't exist yet
        }
    }

    /// Persist config to disk. Errors are logged but not propagated so a
    /// save failure never crashes the app.
    pub fn save(&self) {
        let Some(path) = Self::config_path() else {
            return;
        };
        if let Some(dir) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(dir) {
                log::error!("Cannot create config dir: {e}");
                return;
            }
        }
        match serde_json::to_string_pretty(self) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&path, json) {
                    log::error!("Cannot write config: {e}");
                } else {
                    log::debug!("Config saved to {}", path.display());
                }
            }
            Err(e) => log::error!("Config serialize error: {e}"),
        }
    }

    /// Convenience: save only if the new value actually differs from `old`.
    pub fn save_if_changed(old: &Self, new: &Self) -> Result<()> {
        if serde_json::to_string(old).ok() != serde_json::to_string(new).ok() {
            new.save();
        }
        Ok(())
    }
}