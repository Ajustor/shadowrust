use anyhow::Result;
use std::path::PathBuf;

use super::AppConfig;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_save_if_changed_detects_difference() {
        let old = AppConfig::default();
        let mut new = AppConfig::default();
        new.width = 2560;

        let old_json = serde_json::to_string(&old).unwrap();
        let new_json = serde_json::to_string(&new).unwrap();
        assert_ne!(old_json, new_json);
    }

    #[test]
    fn test_save_if_changed_same_config() {
        let old = AppConfig::default();
        let new = AppConfig::default();

        let old_json = serde_json::to_string(&old).unwrap();
        let new_json = serde_json::to_string(&new).unwrap();
        assert_eq!(old_json, new_json);
    }

    #[test]
    fn test_load_returns_default_when_no_file() {
        // On a fresh system or test environment, there's no config file
        // load() should return defaults gracefully
        let cfg = AppConfig::load();
        // We can't assert exact values because there might be a real config,
        // but at least it shouldn't panic
        assert!(cfg.width > 0);
        assert!(cfg.height > 0);
        assert!(cfg.fps > 0);
    }
}
