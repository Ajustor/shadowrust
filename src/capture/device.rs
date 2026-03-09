use nokhwa::{
    Camera,
    pixel_format::RgbAFormat,
    utils::{CameraIndex, RequestedFormat, RequestedFormatType},
};

/// List available UVC devices.
pub fn list_devices() -> Vec<String> {
    nokhwa::query(nokhwa::utils::ApiBackend::Auto)
        .unwrap_or_default()
        .into_iter()
        .map(|info| format!("[{}] {}", info.index(), info.human_name()))
        .collect()
}

/// A unique (width × height) entry with the highest FPS the device supports
/// at that resolution.
#[derive(Clone)]
pub struct DeviceResolution {
    pub width: u32,
    pub height: u32,
    pub max_fps: u32,
    pub label: String,
}

/// Query every resolution the capture device's driver actually exposes.
///
/// Opens the camera briefly (without streaming) to read its compatible
/// format list, then returns deduplicated resolutions sorted from smallest
/// to largest with their maximum supported FPS.
///
/// Returns an empty vec on any error (device busy, wrong index, etc.).
pub fn query_device_resolutions(device_index: usize) -> Vec<DeviceResolution> {
    let index = CameraIndex::Index(device_index as u32);
    let fmt = RequestedFormat::new::<RgbAFormat>(RequestedFormatType::AbsoluteHighestResolution);

    let mut camera = match Camera::new(index, fmt) {
        Ok(c) => c,
        Err(e) => {
            log::warn!("Cannot open device {device_index} for format query: {e}");
            return vec![];
        }
    };

    let formats = match camera.compatible_camera_formats() {
        Ok(f) => f,
        Err(e) => {
            log::warn!("Cannot query formats for device {device_index}: {e}");
            return vec![];
        }
    };

    // Aggregate: for each (width, height) keep the maximum FPS seen.
    use std::collections::HashMap;
    let mut map: HashMap<(u32, u32), u32> = HashMap::new();
    for fmt in &formats {
        let res = fmt.resolution();
        let (w, h) = (res.width(), res.height());
        let fps = fmt.frame_rate();
        let entry = map.entry((w, h)).or_insert(0);
        *entry = (*entry).max(fps);
    }

    let mut resolutions: Vec<DeviceResolution> = map
        .into_iter()
        .map(|((w, h), max_fps)| {
            let label = format!("{w}×{h} @ {max_fps} fps");
            DeviceResolution {
                width: w,
                height: h,
                max_fps,
                label,
            }
        })
        .collect();

    // Sort smallest → largest pixel count.
    resolutions.sort_by_key(|r| r.width * r.height);
    resolutions
}

/// Format a resolution label from width, height, and max FPS.
pub(crate) fn format_resolution_label(width: u32, height: u32, max_fps: u32) -> String {
    format!("{width}×{height} @ {max_fps} fps")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_resolution_label_1080p() {
        let label = format_resolution_label(1920, 1080, 60);
        assert_eq!(label, "1920×1080 @ 60 fps");
    }

    #[test]
    fn test_format_resolution_label_4k() {
        let label = format_resolution_label(3840, 2160, 30);
        assert_eq!(label, "3840×2160 @ 30 fps");
    }

    #[test]
    fn test_format_resolution_label_720p() {
        let label = format_resolution_label(1280, 720, 120);
        assert_eq!(label, "1280×720 @ 120 fps");
    }

    #[test]
    fn test_device_resolution_clone() {
        let res = DeviceResolution {
            width: 1920,
            height: 1080,
            max_fps: 60,
            label: "1920×1080 @ 60 fps".to_string(),
        };
        let cloned = res.clone();
        assert_eq!(cloned.width, 1920);
        assert_eq!(cloned.height, 1080);
        assert_eq!(cloned.max_fps, 60);
        assert_eq!(cloned.label, "1920×1080 @ 60 fps");
    }
}
