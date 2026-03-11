use crate::app::UiAction;
use crate::audio::AudioPassthrough;
use crate::capture::{DeviceResolution, list_devices};
use crate::config::{AudioCodecPref, VideoCodecPref};

#[derive(Default)]
pub struct UiState {
    pub capturing: bool,
    pub recording: bool,
    pub audio_active: bool,
    pub muted: bool,
    pub menu_visible: bool,
    pub selected_device: usize,
    pub selected_audio_device: usize,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub fps_display: f32,
    pub volume: f32,
    pub record_path: String,
    pub video_codec: VideoCodecPref,
    pub audio_codec: AudioCodecPref,
    pub latency_ms: f32,
    pub frames_dropped: u64,
    pub pending_actions: Vec<UiAction>,
    /// Preferred device names restored from config (used for auto-selection)
    pub preferred_video_device: Option<String>,
    pub preferred_audio_device: Option<String>,
    /// Background update checker (started once)
    pub update_checker: Option<crate::updater::UpdateChecker>,
    /// Whether the user dismissed the update notification
    pub update_dismissed: bool,
    pub(super) devices: Vec<String>,
    pub(super) devices_loaded: bool,
    pub(super) audio_devices: Vec<String>,
    pub(super) audio_devices_loaded: bool,
    pub(super) device_resolutions: Vec<DeviceResolution>,
    pub(super) selected_resolution_idx: usize,
    pub(super) custom_resolution: bool,
}

impl UiState {
    pub fn set_device_resolutions(&mut self, resolutions: Vec<DeviceResolution>) {
        self.device_resolutions = resolutions;
        // Try to restore the saved resolution; fall back to 1080p, then last entry.
        self.selected_resolution_idx = self
            .device_resolutions
            .iter()
            .position(|r| r.width == self.width && r.height == self.height)
            .or_else(|| {
                self.device_resolutions
                    .iter()
                    .rposition(|r| r.height == 1080)
            })
            .unwrap_or(self.device_resolutions.len().saturating_sub(1));
        if let Some(r) = self.device_resolutions.get(self.selected_resolution_idx) {
            self.width = r.width;
            self.height = r.height;
            self.fps = r.max_fps;
        }
        self.custom_resolution = false;
    }

    /// Name of the currently selected video capture device, if known.
    pub fn selected_video_device_name(&self) -> Option<String> {
        self.devices.get(self.selected_device).cloned()
    }

    /// Name of the currently selected audio input device, if known.
    pub fn selected_audio_device_name(&self) -> Option<String> {
        self.audio_devices.get(self.selected_audio_device).cloned()
    }

    pub(super) fn load_video_devices(&mut self) {
        if !self.devices_loaded {
            self.devices = list_devices();
            self.devices_loaded = true;
            // Restore preferred device from saved config
            if let Some(ref pref) = self.preferred_video_device.clone() {
                let pref_lower = pref.to_lowercase();
                if let Some(idx) = self
                    .devices
                    .iter()
                    .position(|d| d.to_lowercase().contains(&pref_lower))
                {
                    self.selected_device = idx;
                }
            }
        }
    }

    pub(super) fn load_audio_devices(&mut self) {
        if !self.audio_devices_loaded {
            self.audio_devices = AudioPassthrough::list_input_devices();
            self.audio_devices_loaded = true;
            // Restore preferred audio device from saved config
            if let Some(ref pref) = self.preferred_audio_device.clone() {
                let pref_lower = pref.to_lowercase();
                if let Some(idx) = self
                    .audio_devices
                    .iter()
                    .position(|d| d.to_lowercase().contains(&pref_lower))
                {
                    self.selected_audio_device = idx;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_state() {
        let state = UiState::default();
        assert!(!state.capturing);
        assert!(!state.recording);
        assert!(!state.audio_active);
        assert!(!state.muted);
        assert!(!state.menu_visible);
        assert_eq!(state.selected_device, 0);
        assert_eq!(state.selected_audio_device, 0);
        assert_eq!(state.width, 0);
        assert_eq!(state.height, 0);
        assert_eq!(state.fps, 0);
        assert_eq!(state.volume, 0.0);
        assert!(state.pending_actions.is_empty());
    }

    #[test]
    fn test_set_device_resolutions_empty() {
        let mut state = UiState::default();
        state.width = 1920;
        state.height = 1080;
        state.set_device_resolutions(vec![]);
        assert_eq!(state.selected_resolution_idx, 0);
        assert!(!state.custom_resolution);
    }

    #[test]
    fn test_set_device_resolutions_finds_matching() {
        let mut state = UiState::default();
        state.width = 1920;
        state.height = 1080;

        let resolutions = vec![
            DeviceResolution {
                width: 1280,
                height: 720,
                max_fps: 60,
                label: "1280×720 @ 60 fps".to_string(),
            },
            DeviceResolution {
                width: 1920,
                height: 1080,
                max_fps: 60,
                label: "1920×1080 @ 60 fps".to_string(),
            },
            DeviceResolution {
                width: 3840,
                height: 2160,
                max_fps: 30,
                label: "3840×2160 @ 30 fps".to_string(),
            },
        ];
        state.set_device_resolutions(resolutions);
        assert_eq!(state.selected_resolution_idx, 1);
        assert_eq!(state.width, 1920);
        assert_eq!(state.height, 1080);
        assert_eq!(state.fps, 60);
    }

    #[test]
    fn test_set_device_resolutions_fallback_to_1080p() {
        let mut state = UiState::default();
        state.width = 2560;
        state.height = 1440;

        let resolutions = vec![
            DeviceResolution {
                width: 1280,
                height: 720,
                max_fps: 60,
                label: "720p".to_string(),
            },
            DeviceResolution {
                width: 1920,
                height: 1080,
                max_fps: 60,
                label: "1080p".to_string(),
            },
        ];
        state.set_device_resolutions(resolutions);
        // 2560x1440 not found, falls back to 1080p
        assert_eq!(state.selected_resolution_idx, 1);
        assert_eq!(state.width, 1920);
    }

    #[test]
    fn test_selected_video_device_name_empty() {
        let state = UiState::default();
        assert!(state.selected_video_device_name().is_none());
    }

    #[test]
    fn test_selected_audio_device_name_empty() {
        let state = UiState::default();
        assert!(state.selected_audio_device_name().is_none());
    }

    #[test]
    fn test_selected_video_device_name_with_devices() {
        let mut state = UiState::default();
        state.devices = vec!["Device A".to_string(), "Device B".to_string()];
        state.selected_device = 1;
        assert_eq!(
            state.selected_video_device_name(),
            Some("Device B".to_string())
        );
    }

    #[test]
    fn test_selected_audio_device_name_with_devices() {
        let mut state = UiState::default();
        state.audio_devices = vec!["Mic 1".to_string(), "Mic 2".to_string()];
        state.selected_audio_device = 0;
        assert_eq!(
            state.selected_audio_device_name(),
            Some("Mic 1".to_string())
        );
    }

    #[test]
    fn test_selected_device_out_of_bounds() {
        let mut state = UiState::default();
        state.devices = vec!["Only Device".to_string()];
        state.selected_device = 5;
        assert!(state.selected_video_device_name().is_none());
    }
}
