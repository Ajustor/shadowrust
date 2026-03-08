use anyhow::{Context, Result};
use crossbeam_channel::{Receiver, Sender, bounded};
use nokhwa::{
    Camera,
    pixel_format::RgbAFormat,
    utils::{CameraIndex, RequestedFormat, RequestedFormatType, Resolution},
};
use std::thread::{self, JoinHandle};

pub struct CaptureConfig {
    pub device_index: usize,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
}

pub struct CaptureThread {
    handle: Option<JoinHandle<()>>,
    stop_tx: Sender<()>,
}

impl CaptureThread {
    /// Start capture in a dedicated OS thread.
    /// Returns (thread handle, frame receiver).
    /// Frames are raw RGBA bytes of size width*height*4.
    pub fn start(config: CaptureConfig) -> Result<(Self, Receiver<Vec<u8>>)> {
        let (frame_tx, frame_rx) = bounded::<Vec<u8>>(4);
        let (stop_tx, stop_rx) = bounded::<()>(1);

        let handle = thread::Builder::new()
            .name("shadowrust-capture".into())
            .spawn(move || {
                if let Err(e) = capture_loop(config, frame_tx, stop_rx) {
                    log::error!("Capture thread error: {e}");
                }
            })
            .context("spawn capture thread")?;

        Ok((
            CaptureThread {
                handle: Some(handle),
                stop_tx,
            },
            frame_rx,
        ))
    }
}

impl Drop for CaptureThread {
    fn drop(&mut self) {
        let _ = self.stop_tx.send(());
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

fn capture_loop(
    config: CaptureConfig,
    frame_tx: Sender<Vec<u8>>,
    stop_rx: Receiver<()>,
) -> Result<()> {
    let index = CameraIndex::Index(config.device_index as u32);
    // Request MJPEG — the format used by virtually all UVC capture cards
    // (Genki ShadowCast 2, Elgato, etc.). nokhwa will decode to RGBA via
    // decode_image::<RgbAFormat>(). Closest() picks the nearest supported
    // resolution/fps if the requested one is unavailable.
    let format = RequestedFormat::new::<RgbAFormat>(RequestedFormatType::Closest(
        nokhwa::utils::CameraFormat::new(
            Resolution::new(config.width, config.height),
            nokhwa::utils::FrameFormat::MJPEG,
            config.fps,
        ),
    ));

    let mut camera = Camera::new(index, format).context("open camera")?;
    camera.open_stream().context("open camera stream")?;

    log::info!(
        "Capture stream open: {}x{}@{}fps",
        config.width,
        config.height,
        config.fps
    );

    loop {
        if stop_rx.try_recv().is_ok() {
            break;
        }

        match camera.frame() {
            Ok(frame) => {
                let rgba = frame.decode_image::<RgbAFormat>().context("decode frame")?;
                let bytes = rgba.into_raw();
                // Drop frame if consumer is slow (non-blocking send)
                let _ = frame_tx.try_send(bytes);
            }
            Err(e) => {
                log::warn!("Frame capture error: {e}");
            }
        }
    }

    camera.stop_stream().ok();
    log::info!("Capture stream closed");
    Ok(())
}

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
            DeviceResolution { width: w, height: h, max_fps, label }
        })
        .collect();

    // Sort smallest → largest pixel count.
    resolutions.sort_by_key(|r| r.width * r.height);
    resolutions
}
