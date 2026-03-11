use std::{
    sync::Arc,
    thread::{self, JoinHandle},
};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, bounded};

use super::Recorder;

/// Messages sent from the main thread to the encode thread.
enum RecordMsg {
    /// A decoded RGBA video frame.
    Frame {
        data: Arc<Vec<u8>>,
        size: (u32, u32),
    },
    /// Interleaved f32 audio samples from the capture card's UAC.
    Audio(Vec<f32>),
    /// Flush encoders and finalise the file.
    Stop,
}

/// Handle to the background encode thread.
///
/// Dropping this handle calls `finish()` implicitly (best-effort).
pub struct RecordThread {
    tx: Sender<RecordMsg>,
    handle: Option<JoinHandle<()>>,
}

impl RecordThread {
    /// Spawn the encode thread.  The thread will call `Recorder::new` itself
    /// (so FFmpeg contexts are created on the encode thread and never cross
    /// thread boundaries) and process messages until `Stop` is received.
    pub fn start(
        path: String,
        width: u32,
        height: u32,
        fps: u32,
        audio_rate: u32,
        audio_channels: u16,
    ) -> anyhow::Result<Self> {
        // Enough headroom to absorb a few slow I-frames without blocking.
        // Arc<Vec<u8>> frames are cheap to queue (pointer, not 33 MB copy).
        let (tx, rx) = bounded::<RecordMsg>(128);

        let handle = thread::Builder::new()
            .name("shadowrust-encode".into())
            .spawn(move || encode_loop(rx, path, width, height, fps, audio_rate, audio_channels))
            .map_err(|e| anyhow::anyhow!("spawn encode thread: {e}"))?;

        Ok(Self {
            tx,
            handle: Some(handle),
        })
    }

    /// Send a video frame (non-blocking — drops frame if encoder is busy).
    pub fn push_frame(&self, data: Arc<Vec<u8>>, size: (u32, u32)) {
        let _ = self.tx.try_send(RecordMsg::Frame { data, size });
    }

    /// Send audio samples (non-blocking — drops if channel is full).
    pub fn push_audio(&self, samples: Vec<f32>) {
        if !samples.is_empty() {
            let _ = self.tx.try_send(RecordMsg::Audio(samples));
        }
    }

    /// Signal the encode thread to flush and write the file trailer, then
    /// wait for it to finish.  Blocks the caller until the file is safe.
    pub fn finish(mut self) {
        let _ = self.tx.send(RecordMsg::Stop);
        if let Some(h) = self.handle.take() {
            if let Err(e) = h.join() {
                log::error!("Encode thread panicked: {e:?}");
            }
        }
    }
}

impl Drop for RecordThread {
    fn drop(&mut self) {
        // Best-effort stop (already called by finish(), this covers the
        // case where the handle is dropped without calling finish()).
        let _ = self.tx.try_send(RecordMsg::Stop);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

fn encode_loop(
    rx: Receiver<RecordMsg>,
    path: String,
    width: u32,
    height: u32,
    fps: u32,
    audio_rate: u32,
    audio_channels: u16,
) {
    let mut rec = match Recorder::new(&path, width, height, fps, audio_rate, audio_channels) {
        Ok(r) => r,
        Err(e) => {
            log::error!("Recorder init failed: {e}");
            return;
        }
    };

    log::info!("Encode thread started → {path}");

    loop {
        // Use a timeout so the thread can check for a Stop that arrives
        // while the channel is otherwise empty (rare but possible).
        let msg = match rx.recv_timeout(std::time::Duration::from_millis(500)) {
            Ok(m) => m,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => break,
        };

        match msg {
            RecordMsg::Frame { data, size } => rec.push_frame(&data, size),
            RecordMsg::Audio(samples) => rec.push_audio(&samples),
            RecordMsg::Stop => break,
        }
    }

    if let Err(e) = rec.finish() {
        log::error!("Recording finalise failed: {e}");
    }

    log::info!("Encode thread exited");
}
