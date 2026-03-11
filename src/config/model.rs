use serde::{Deserialize, Serialize};

/// Preferred video codec / encoder for recording.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub enum VideoCodecPref {
    /// H.264 — uses the system's default H.264 encoder (default).
    /// Typically libx264 (software). Identical behavior to the original code.
    #[default]
    H264Auto,
    /// H.264 — force NVIDIA NVENC GPU encoder (requires NVIDIA GPU + drivers).
    H264Nvenc,
    /// H.265/HEVC — uses the system's default H.265 encoder.
    /// ~40% smaller files at same visual quality.
    H265Auto,
    /// H.265/HEVC — force NVIDIA NVENC GPU encoder.
    H265Nvenc,
}

impl VideoCodecPref {
    pub fn label(&self) -> &'static str {
        match self {
            Self::H264Auto  => "H.264 (auto)",
            Self::H264Nvenc => "H.264 NVENC (GPU)",
            Self::H265Auto  => "H.265 (auto)",
            Self::H265Nvenc => "H.265 NVENC (GPU)",
        }
    }

    pub fn is_hevc(&self) -> bool {
        matches!(self, Self::H265Auto | Self::H265Nvenc)
    }
}

/// Preferred audio codec for recording.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub enum AudioCodecPref {
    /// AAC — widely compatible, default.
    #[default]
    Aac,
    /// Opus — better quality at lower bitrates, MKV-native.
    Opus,
}

impl AudioCodecPref {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Aac => "AAC",
            Self::Opus => "Opus",
        }
    }
}

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
    /// Preferred video codec / encoder
    #[serde(default)]
    pub video_codec: VideoCodecPref,
    /// Preferred audio codec
    #[serde(default)]
    pub audio_codec: AudioCodecPref,
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
            record_path: "capture.mkv".to_string(),
            video_codec: VideoCodecPref::default(),
            audio_codec: AudioCodecPref::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_values() {
        let cfg = AppConfig::default();
        assert_eq!(cfg.width, 1920);
        assert_eq!(cfg.height, 1080);
        assert_eq!(cfg.fps, 60);
        assert!((cfg.volume - 1.0).abs() < f32::EPSILON);
        assert_eq!(cfg.record_path, "capture.mkv");
        assert!(cfg.video_device.is_none());
        assert!(cfg.audio_device.is_none());
    }

    #[test]
    fn test_serde_round_trip() {
        let cfg = AppConfig {
            video_device: Some("Genki ShadowCast".to_string()),
            audio_device: Some("USB Audio".to_string()),
            width: 2560,
            height: 1440,
            fps: 30,
            volume: 0.75,
            record_path: "/tmp/video.mp4".to_string(),
            video_codec: VideoCodecPref::default(),
            audio_codec: AudioCodecPref::default(),
        };
        let json = serde_json::to_string(&cfg).unwrap();
        let restored: AppConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.width, 2560);
        assert_eq!(restored.height, 1440);
        assert_eq!(restored.fps, 30);
        assert!((restored.volume - 0.75).abs() < f32::EPSILON);
        assert_eq!(restored.video_device.as_deref(), Some("Genki ShadowCast"));
        assert_eq!(restored.audio_device.as_deref(), Some("USB Audio"));
        assert_eq!(restored.record_path, "/tmp/video.mp4");
    }

    #[test]
    fn test_serde_missing_fields_uses_defaults() {
        // Simulate a config file with only some fields
        let json = r#"{"width": 3840, "height": 2160, "fps": 30, "volume": 0.5, "record_path": "out.mp4"}"#;
        let cfg: AppConfig = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.width, 3840);
        assert!(cfg.video_device.is_none());
        assert!(cfg.audio_device.is_none());
    }

    #[test]
    fn test_serde_invalid_json_fails() {
        let result: Result<AppConfig, _> = serde_json::from_str("not json");
        assert!(result.is_err());
    }

    #[test]
    fn test_clone() {
        let cfg = AppConfig::default();
        let cloned = cfg.clone();
        assert_eq!(cloned.width, cfg.width);
        assert_eq!(cloned.height, cfg.height);
        assert_eq!(cloned.record_path, cfg.record_path);
    }
}
