use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_channel::{Receiver, Sender, bounded};
use ringbuf::HeapRb;
use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};

use super::device::{find_input_device, list_input_devices};

/// Live audio pass-through: UAC input device → system default output.
///
/// The same input audio is simultaneously forwarded to the playback ring
/// buffer (low-latency output) and to a recording channel so the Recorder
/// can mux it into the video file.
pub struct AudioPassthrough {
    _input_stream: cpal::Stream,
    _output_stream: cpal::Stream,
    volume: Arc<AtomicU32>,
    /// Receives audio chunks (interleaved f32) for recording.
    record_rx: Receiver<Vec<f32>>,
    /// Native sample rate of the capture device.
    pub sample_rate: u32,
    /// Channel count of the capture device.
    pub channels: u16,
}

impl AudioPassthrough {
    pub fn start(input_device_name: Option<&str>, initial_volume: f32) -> Result<Self> {
        let host = cpal::default_host();

        let input_device = find_input_device(&host, input_device_name)
            .context("no audio input device available")?;
        log::info!(
            "Audio input device: {}",
            input_device.name().unwrap_or_default()
        );

        let output_device = host
            .default_output_device()
            .context("no audio output device")?;
        log::info!(
            "Audio output device: {}",
            output_device.name().unwrap_or_default()
        );

        let in_cfg = input_device
            .default_input_config()
            .context("no default input config")?;
        let out_cfg = output_device
            .default_output_config()
            .context("no default output config")?;

        let in_channels = in_cfg.channels() as usize;
        let out_channels = out_cfg.channels() as usize;
        let in_rate = in_cfg.sample_rate().0;
        let out_rate = out_cfg.sample_rate().0;

        log::info!(
            "Audio config — in: {in_channels}ch @ {in_rate}Hz  |  out: {out_channels}ch @ {out_rate}Hz"
        );

        // Playback ring buffer (~400 ms)
        let pb_buf_samples = in_rate as usize * in_channels * 2 / 5;
        let pb_rb = HeapRb::<f32>::new(pb_buf_samples.max(8192));
        let (mut pb_prod, mut pb_cons) = pb_rb.split();

        // Recording channel: bounded so a slow recorder never blocks the callback
        let (record_tx, record_rx): (Sender<Vec<f32>>, Receiver<Vec<f32>>) = bounded(512);

        let volume = Arc::new(AtomicU32::new(initial_volume.to_bits()));
        let volume_reader = Arc::clone(&volume);

        // ── Input stream: capture card → playback buffer + recording channel ──
        let in_stream_cfg = in_cfg.config();
        let input_stream = input_device
            .build_input_stream(
                &in_stream_cfg,
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    pb_prod.push_slice(data);
                    // Non-blocking send; drop chunk if channel is full
                    let _ = record_tx.try_send(data.to_vec());
                },
                |e| log::error!("Audio input error: {e}"),
                None,
            )
            .context("build audio input stream")?;

        // ── Output stream: playback buffer → speakers ─────────────────────────
        let out_stream_cfg = out_cfg.config();
        let output_stream = output_device
            .build_output_stream(
                &out_stream_cfg,
                move |out: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    let vol = f32::from_bits(volume_reader.load(Ordering::Relaxed));
                    if in_channels == out_channels {
                        let filled = pb_cons.pop_slice(out);
                        for s in &mut out[..filled] {
                            *s *= vol;
                        }
                        out[filled..].fill(0.0);
                    } else {
                        for out_frame in out.chunks_mut(out_channels) {
                            let mut in_buf = [0.0f32; 8];
                            let n = pb_cons.pop_slice(&mut in_buf[..in_channels.min(8)]);
                            for (i, s) in out_frame.iter_mut().enumerate() {
                                *s = if i < n {
                                    in_buf[i] * vol
                                } else if n > 0 {
                                    in_buf[n - 1] * vol
                                } else {
                                    0.0
                                };
                            }
                        }
                    }
                },
                |e| log::error!("Audio output error: {e}"),
                None,
            )
            .context("build audio output stream")?;

        input_stream.play().context("start audio input stream")?;
        output_stream.play().context("start audio output stream")?;

        log::info!("Audio pass-through active (vol={initial_volume:.2})");

        Ok(Self {
            _input_stream: input_stream,
            _output_stream: output_stream,
            volume,
            record_rx,
            sample_rate: in_rate,
            channels: in_cfg.channels(),
        })
    }

    /// Set playback volume (0.0 = mute, 1.0 = unity, up to 2.0). Thread-safe.
    pub fn set_volume(&self, v: f32) {
        self.volume.store(v.to_bits(), Ordering::Relaxed);
    }

    /// Drain all audio samples accumulated since the last call.
    /// Returns interleaved f32 samples (L, R, L, R, …).
    /// Call this every video frame to feed audio into the Recorder.
    pub fn drain_recording_samples(&self) -> Vec<f32> {
        let mut out = Vec::new();
        while let Ok(chunk) = self.record_rx.try_recv() {
            out.extend_from_slice(&chunk);
        }
        out
    }

    /// List all available audio input device names.
    pub fn list_input_devices() -> Vec<String> {
        list_input_devices()
    }
}
