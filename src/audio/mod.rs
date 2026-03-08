use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ringbuf::HeapRb;

/// Live audio pass-through: UAC input device → system default output.
///
/// Dropping this struct stops both streams cleanly.
pub struct AudioPassthrough {
    _input_stream: cpal::Stream,
    _output_stream: cpal::Stream,
}

impl AudioPassthrough {
    /// Start audio pass-through.
    ///
    /// `input_device_name` — optional substring to match against the audio
    /// input device name (e.g. "ShadowCast", "Genki"). Falls back to the
    /// system default input if no match is found.
    pub fn start(input_device_name: Option<&str>) -> Result<Self> {
        let host = cpal::default_host();

        // ── Input device ─────────────────────────────────────────────────────
        let input_device = find_input_device(&host, input_device_name)
            .context("no audio input device available")?;
        log::info!("Audio input: {}", input_device.name().unwrap_or_default());

        // ── Output device ────────────────────────────────────────────────────
        let output_device = host
            .default_output_device()
            .context("no audio output device")?;
        log::info!(
            "Audio output: {}",
            output_device.name().unwrap_or_default()
        );

        // Use the input's native config (sample rate, channels) to avoid
        // any resampling on the capture path.
        let input_supported = input_device
            .default_input_config()
            .context("no default input config")?;

        let in_channels = input_supported.channels() as usize;
        let sample_rate = input_supported.sample_rate();
        let stream_cfg = cpal::StreamConfig {
            channels: input_supported.channels(),
            sample_rate,
            buffer_size: cpal::BufferSize::Default,
        };

        // Try the same rate on the output; if unsupported the OS mixer
        // handles the conversion transparently.
        let out_supported = output_device
            .default_output_config()
            .context("no default output config")?;
        let out_channels = out_supported.channels() as usize;
        let out_cfg = cpal::StreamConfig {
            channels: out_supported.channels(),
            sample_rate,
            buffer_size: cpal::BufferSize::Default,
        };

        // Ring buffer: ~200 ms of interleaved f32 samples.
        let buf_size = sample_rate.0 as usize * in_channels / 5;
        let rb = HeapRb::<f32>::new(buf_size * 4);
        let (mut prod, mut cons) = rb.split();

        // ── Input stream: capture card → ring buffer ──────────────────────────
        let input_stream = input_device
            .build_input_stream(
                &stream_cfg,
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    // push_slice returns the number of samples written; we
                    // silently drop any excess (back-pressure, ring is full).
                    prod.push_slice(data);
                },
                |e| log::error!("Audio input error: {e}"),
                None,
            )
            .context("build audio input stream")?;

        // ── Output stream: ring buffer → speakers ─────────────────────────────
        let output_stream = output_device
            .build_output_stream(
                &out_cfg,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    if in_channels == out_channels {
                        // Happy path: same layout, bulk copy.
                        let filled = cons.pop_slice(data);
                        // Silence any unfilled samples (ring underrun).
                        data[filled..].fill(0.0);
                    } else {
                        // Channel count mismatch: process frame-by-frame.
                        for out_frame in data.chunks_mut(out_channels) {
                            let mut in_frame = [0.0f32; 8]; // up to 7.1
                            let n = cons.pop_slice(&mut in_frame[..in_channels]);
                            for (i, out_s) in out_frame.iter_mut().enumerate() {
                                *out_s = if i < n {
                                    in_frame[i]
                                } else {
                                    // Upmix: repeat last captured channel.
                                    in_frame[n.saturating_sub(1)]
                                };
                            }
                        }
                    }
                },
                |e| log::error!("Audio output error: {e}"),
                None,
            )
            .context("build audio output stream")?;

        input_stream.play().context("start audio input")?;
        output_stream.play().context("start audio output")?;

        log::info!(
            "Audio pass-through active: {in_channels}ch in, {out_channels}ch out @ {} Hz",
            sample_rate.0
        );

        Ok(Self {
            _input_stream: input_stream,
            _output_stream: output_stream,
        })
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
    let hint_lower = name_hint.map(|h| h.to_lowercase());

    if let Some(ref hint) = hint_lower {
        if let Ok(mut devs) = host.input_devices() {
            let matched = devs.find(|d| {
                d.name()
                    .map(|n| n.to_lowercase().contains(hint.as_str()))
                    .unwrap_or(false)
            });
            if matched.is_some() {
                return matched;
            }
        }
    }

    host.default_input_device()
}
