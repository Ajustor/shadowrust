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
