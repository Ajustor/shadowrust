use anyhow::{Context, Result};
use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, bounded};
use nokhwa::{
    Camera,
    pixel_format::RgbAFormat,
    utils::{CameraIndex, RequestedFormat, RequestedFormatType, Resolution},
};
use std::{
    sync::Arc,
    thread::{self, JoinHandle},
    time::Duration,
};

use super::CaptureConfig;

pub struct CaptureThread {
    /// USB capture thread (blocks on `camera.frame()`).
    usb_handle: Option<JoinHandle<()>>,
    /// MJPEG decode thread (decodes in parallel with USB).
    decode_handle: Option<JoinHandle<()>>,
    stop_tx: Sender<()>,
}

impl CaptureThread {
    /// Start a two-stage capture pipeline:
    ///   1. USB thread  — waits for MJPEG frames from the device (blocking I/O)
    ///   2. Decode thread — decodes MJPEG → RGBA in parallel with the next USB frame
    ///
    /// Frames returned are `Arc<Vec<u8>>` so they can be shared between the
    /// renderer and the recorder without an extra 33 MB copy.
    pub fn start(config: CaptureConfig) -> Result<(Self, Receiver<Arc<Vec<u8>>>)> {
        // Stage 1: raw compressed MJPEG bytes (~500 KB – 3 MB each at 4K).
        // capacity=8 gives ~24 MB of headroom without unbounded growth.
        let (mjpeg_tx, mjpeg_rx) = bounded::<Vec<u8>>(8);
        // Stage 2: decoded RGBA frames (33 MB each at 4K).
        // capacity=4 ≈ 132 MB — keeps one I-frame ahead of the renderer.
        let (rgba_tx, rgba_rx) = bounded::<Arc<Vec<u8>>>(4);
        let (stop_tx, stop_rx) = bounded::<()>(1);

        // ── Thread 1: USB capture ─────────────────────────────────────────────
        // Only copies raw compressed bytes; no CPU-heavy decode here.
        let usb_handle = thread::Builder::new()
            .name("shadowrust-capture".into())
            .spawn(move || {
                if let Err(e) = usb_capture_loop(config, mjpeg_tx, stop_rx) {
                    log::error!("USB capture thread error: {e}");
                }
            })
            .context("spawn USB capture thread")?;

        // ── Thread 2: MJPEG decode ────────────────────────────────────────────
        // Runs in parallel with the USB thread; decode latency doesn't affect
        // the USB polling cadence and vice-versa.
        let decode_handle = thread::Builder::new()
            .name("shadowrust-decode".into())
            .spawn(move || {
                decode_loop(mjpeg_rx, rgba_tx);
            })
            .context("spawn decode thread")?;

        Ok((
            CaptureThread {
                usb_handle: Some(usb_handle),
                decode_handle: Some(decode_handle),
                stop_tx,
            },
            rgba_rx,
        ))
    }
}

impl Drop for CaptureThread {
    fn drop(&mut self) {
        // Signal the USB thread to stop; it will close mjpeg_tx, which makes
        // the decode thread exit on its own via RecvTimeoutError::Disconnected.
        let _ = self.stop_tx.send(());
        if let Some(h) = self.usb_handle.take() {
            let _ = h.join();
        }
        if let Some(h) = self.decode_handle.take() {
            let _ = h.join();
        }
    }
}

/// Stage 1: poll the UVC device and forward raw compressed MJPEG bytes.
fn usb_capture_loop(
    config: CaptureConfig,
    mjpeg_tx: Sender<Vec<u8>>,
    stop_rx: Receiver<()>,
) -> Result<()> {
    let index = CameraIndex::Index(config.device_index as u32);
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
                // Copy raw MJPEG bytes — cheap (~2 MB), no decode here.
                let raw = frame.buffer().to_vec();
                // Drop if the decode thread is still busy (non-blocking).
                let _ = mjpeg_tx.try_send(raw);
            }
            Err(e) => log::warn!("Frame capture error: {e}"),
        }
    }

    camera.stop_stream().ok();
    log::info!("Capture stream closed");
    Ok(())
}

/// Stage 2: decode MJPEG → RGBA on a dedicated thread so USB polling and
/// CPU decode run in parallel.
fn decode_loop(mjpeg_rx: Receiver<Vec<u8>>, rgba_tx: Sender<Arc<Vec<u8>>>) {
    loop {
        let raw = match mjpeg_rx.recv_timeout(Duration::from_millis(200)) {
            Ok(b) => b,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => break,
        };

        match image::load_from_memory(&raw) {
            Ok(img) => {
                let bytes = img.into_rgba8().into_raw();
                // Wrap in Arc — allows zero-copy sharing between renderer and recorder.
                let _ = rgba_tx.try_send(Arc::new(bytes));
            }
            Err(e) => log::warn!("MJPEG decode error: {e}"),
        }
    }

    log::info!("Decode thread exited");
}
