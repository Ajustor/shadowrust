/// Configuration for a capture session.
pub struct CaptureConfig {
    pub device_index: usize,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capture_config_creation() {
        let config = CaptureConfig {
            device_index: 0,
            width: 1920,
            height: 1080,
            fps: 60,
        };
        assert_eq!(config.device_index, 0);
        assert_eq!(config.width, 1920);
        assert_eq!(config.height, 1080);
        assert_eq!(config.fps, 60);
    }

    #[test]
    fn test_capture_config_4k() {
        let config = CaptureConfig {
            device_index: 1,
            width: 3840,
            height: 2160,
            fps: 30,
        };
        assert_eq!(config.width, 3840);
        assert_eq!(config.height, 2160);
        assert_eq!(config.fps, 30);
    }
}
