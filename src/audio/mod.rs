use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ringbuf::HeapRb;
use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};

/// Live audio pass-through: UAC input device → system default output.
///
/// Each device uses its own native sample rate and channel count.
/// The ring buffer handles any rate/channel mismatch between the two.
/// Dropping this struct stops both streams cleanly.
pub struct AudioPassthrough {
    _input_stream: cpal::Stream,
    _output_stream: cpal::Stream,
    volume: Arc<AtomicU32>,
}

impl AudioPassthrough {
    /// Start audio pass-through.
    ///
    /// `input_device_name` — optional substring to match against the audio
    /// input device name (e.g. "ShadowCast", "Genki"). Falls back to the
    /// system default input if no match is found.
    /// `initial_volume` — linear gain, 1.0 = unity.
    pub fn start(input_device_name: Option<&str>, initial_volume: f32) -> Result<Self> {
        let host = cpal::default_host();

        let input_device = find_input_device(&host, input_device_name)
            .context("no audio input device available")?;
        log::info!("Audio input device: {}", input_device.name().unwrap_or_default());

        let output_device = host
            .default_output_device()
            .context("no audio output device")?;
        log::info!("Audio output device: {}", output_device.name().unwrap_or_default());

        // Each device uses its own native config — never force the input rate
        // onto the output, or ALSA/CoreAudio may silently refuse and produce silence.
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

        // Ring buffer sized for ~400 ms of input audio (generous headroom for
        // any rate difference between input and output).
        let buf_samples = in_rate as usize * in_channels * 2 / 5;
        let rb = HeapRb::<f32>::new(buf_samples.max(8192));
        let (mut prod, mut cons) = rb.split();

        let volume = Arc::new(AtomicU32::new(initial_volume.to_bits()));
        let volume_reader = Arc::clone(&volume);

        // ── Input stream: capture card → ring buffer ──────────────────────────
        // Use the device's native config (converted to StreamConfig).
        let in_stream_cfg = in_cfg.config();
        let input_stream = input_device
            .build_input_stream(
                &in_stream_cfg,
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    prod.push_slice(data);
                },
                |e| log::error!("Audio input error: {e}"),
                None,
            )
            .context("build audio input stream")?;

        // ── Output stream: ring buffer → speakers ─────────────────────────────
        let out_stream_cfg = out_cfg.config();
        let output_stream = output_device
            .build_output_stream(
                &out_stream_cfg,
                move |out: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    let vol = f32::from_bits(volume_reader.load(Ordering::Relaxed));

                    if in_channels == out_channels {
                        // Same channel layout: bulk copy + volume
                        let filled = cons.pop_slice(out);
                        for s in &mut out[..filled] {
                            *s *= vol;
                        }
                        out[filled..].fill(0.0);
                    } else {
                        // Different channel counts: frame-by-frame conversion
                        for out_frame in out.chunks_mut(out_channels) {
                            let mut in_buf = [0.0f32; 8]; // supports up to 7.1
                            let n = cons.pop_slice(&mut in_buf[..in_channels.min(8)]);
                            for (i, s) in out_frame.iter_mut().enumerate() {
                                *s = if i < n {
                                    in_buf[i] * vol
                                } else if n > 0 {
                                    // Upmix: repeat last available channel
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
        })
    }

    /// Set playback volume (linear gain). 0.0 = mute, 1.0 = unity, up to 2.0.
    /// Thread-safe — can be called from the UI thread at any time.
    pub fn set_volume(&self, v: f32) {
        self.volume.store(v.to_bits(), Ordering::Relaxed);
    }

    /// List all available audio input device names.
    pub fn list_input_devices() -> Vec<String> {
        let host = cpal::default_host();
        host.input_devices()
            .map(|devs| devs.filter_map(|d| d.name().ok()).collect())
            .unwrap_or_default()
    }
}

fn find_input_device(host: &cpal::Host, name_hint: Option<&str>) -> Option<cpal::Device> {
    if let Some(hint) = name_hint {
        let hint_lower = hint.to_lowercase();
        if let Ok(mut devs) = host.input_devices() {
            let matched = devs.find(|d| {
                d.name()
                    .map(|n| n.to_lowercase().contains(&hint_lower))
                    .unwrap_or(false)
            });
            if let Some(dev) = matched {
                return Some(dev);
            }
        }
    }
    // Fallback: system default input
    host.default_input_device()
}
