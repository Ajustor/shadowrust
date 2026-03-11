use anyhow::{Context as _, Result};
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

        let input_device =
            find_input_device(&host, input_device_name).with_context(
                || match input_device_name {
                    Some(hint) => format!(
                        "Audio input device not found for hint '{hint}'. \
                         Check that the capture card is connected and its audio \
                         device name contains '{hint}'."
                    ),
                    None => "No default audio input device available".to_string(),
                },
            )?;
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

        // ── Choose input config ───────────────────────────────────────────────
        // Prefer 48 000 Hz stereo (standard for video capture cards / UAC
        // devices).  Some cards declare a rate via WASAPI that differs from
        // their real hardware rate, which causes pitch shift.  By explicitly
        // requesting 48 kHz we force WASAPI to do the conversion (if any)
        // and we always know the rate we're actually working with.
        const PREFERRED_RATE: u32 = 48_000;

        let in_cfg = {
            let found = input_device
                .supported_input_configs()
                .ok()
                .and_then(|mut it| {
                    it.find(|c| {
                        c.min_sample_rate().0 <= PREFERRED_RATE
                            && c.max_sample_rate().0 >= PREFERRED_RATE
                    })
                })
                .map(|c| c.with_sample_rate(cpal::SampleRate(PREFERRED_RATE)));

            match found {
                Some(c) => {
                    log::info!(
                        "Audio input: using preferred {PREFERRED_RATE} Hz \
                         ({}ch, {:?})",
                        c.channels(),
                        c.sample_format()
                    );
                    c
                }
                None => {
                    log::warn!(
                        "Audio input: device does not support {PREFERRED_RATE} Hz — \
                         falling back to default_input_config"
                    );
                    input_device
                        .default_input_config()
                        .context("no default input config")?
                }
            }
        };

        let in_channels = in_cfg.channels() as usize;
        let in_rate = in_cfg.sample_rate().0;

        // Try to open the output device at the SAME rate as the input so the
        // ring buffer doesn't drift (pitch/speed distortion).
        let out_cfg = {
            let same_rate = output_device
                .supported_output_configs()
                .ok()
                .and_then(|mut it| {
                    it.find(|c| {
                        c.min_sample_rate().0 <= in_rate && c.max_sample_rate().0 >= in_rate
                    })
                })
                .map(|r| r.with_sample_rate(cpal::SampleRate(in_rate)));
            match same_rate {
                Some(c) => c,
                None => output_device
                    .default_output_config()
                    .context("no default output config")?,
            }
        };

        let out_channels = out_cfg.channels() as usize;
        let out_rate = out_cfg.sample_rate().0;

        if out_rate != in_rate {
            log::warn!(
                "Audio rate mismatch — in: {in_rate} Hz, out: {out_rate} Hz. \
                 Playback may sound distorted. Recording is unaffected (uses in_rate)."
            );
        }

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
        // If out_rate != in_rate we do a simple nearest-neighbour resample so
        // the ring buffer drains proportionally and no pitch/speed distortion
        // occurs during live playback.  Recording bypasses this path entirely
        // (uses record_tx/rx at in_rate) so the file is unaffected.
        let out_stream_cfg = out_cfg.config();
        let output_stream = output_device
            .build_output_stream(
                &out_stream_cfg,
                move |out: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    let vol = f32::from_bits(volume_reader.load(Ordering::Relaxed));

                    if in_rate == out_rate && in_channels == out_channels {
                        // Fast path: rates and channels match — direct copy.
                        let filled = pb_cons.pop_slice(out);
                        for s in &mut out[..filled] {
                            *s *= vol;
                        }
                        out[filled..].fill(0.0);
                    } else {
                        // Rates or channel counts differ.  Pull the right number
                        // of input frames and resample with nearest-neighbour.
                        let out_frames = out.len() / out_channels.max(1);
                        let ratio = in_rate as f64 / out_rate as f64;
                        let in_frames_need = ((out_frames as f64 * ratio).ceil() as usize).max(1);
                        let mut in_buf = vec![0.0f32; in_frames_need * in_channels];
                        let popped = pb_cons.pop_slice(&mut in_buf);
                        let avail = popped / in_channels.max(1);

                        for out_i in 0..out_frames {
                            let in_i =
                                ((out_i as f64 * ratio) as usize).min(avail.saturating_sub(1));
                            let out_frame =
                                &mut out[out_i * out_channels..(out_i + 1) * out_channels];
                            for (ch, s) in out_frame.iter_mut().enumerate() {
                                let src_ch = ch.min(in_channels.saturating_sub(1));
                                *s = in_buf
                                    .get(in_i * in_channels + src_ch)
                                    .copied()
                                    .unwrap_or(0.0)
                                    * vol;
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
