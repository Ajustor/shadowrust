use anyhow::{Context, Result};
use ffmpeg_next as ffmpeg;
use ffmpeg_next::{
    Dictionary,
    channel_layout::ChannelLayout,
    codec, encoder, format,
    software::{resampling, scaling},
    util::rational::Rational,
};

use crate::config::{AudioCodecPref, VideoCodecPref};
use super::Recorder;

/// The sample rate the audio encoder always runs at.
const TARGET_RATE: u32 = 48_000;

impl Recorder {
    pub fn new(
        path: &str,
        width: u32,
        height: u32,
        fps: u32,
        audio_in_rate: u32,
        audio_channels: u16,
        video_codec_pref: &VideoCodecPref,
        audio_codec_pref: &AudioCodecPref,
    ) -> Result<Self> {
        ffmpeg::init().context("ffmpeg init")?;

        let mut octx = format::output(path).context("open output file")?;

        // ── Video stream ──────────────────────────────────────────────────────
        let video_time_base = Rational::new(1, fps as i32);
        let (video_enc, video_stream_idx, encoder_name) =
            open_video_encoder(video_codec_pref, &mut octx, width, height, fps, video_time_base)?;
        log::info!("Video encoder: {encoder_name} ({width}×{height} @ {fps}fps)");

        let video_scaler = scaling::Context::get(
            ffmpeg_next::util::format::Pixel::RGBA,
            width,
            height,
            ffmpeg_next::util::format::Pixel::YUV420P,
            width,
            height,
            scaling::Flags::BILINEAR,
        )
        .context("create scaler")?;

        // ── Audio stream ──────────────────────────────────────────────────────
        let (audio_enc, audio_stream_idx, audio_frame_size, audio_enc_format) =
            open_audio_encoder(audio_codec_pref, &mut octx)?;

        // ── SWR resampler ─────────────────────────────────────────────────────
        let in_layout = if audio_channels >= 2 {
            ChannelLayout::STEREO
        } else {
            ChannelLayout::MONO
        };
        // SWR always outputs packed (interleaved) stereo — audio_buf stores L,R,L,R,...
        // drain_buf_to_encoder then deinterleaves for planar encoders (AAC) or
        // copies directly for packed encoders (Opus).
        let swr = resampling::Context::get(
            ffmpeg_next::util::format::Sample::F32(
                ffmpeg_next::util::format::sample::Type::Packed,
            ),
            in_layout,
            audio_in_rate,
            ffmpeg_next::util::format::Sample::F32(
                ffmpeg_next::util::format::sample::Type::Packed,
            ),
            ChannelLayout::STEREO,
            TARGET_RATE,
        )
        .map_err(|e| log::warn!("SWR init failed ({e}) — recording without resampling"))
        .ok();

        octx.write_header().context("write file header")?;

        Ok(Self {
            octx,
            video_enc,
            video_scaler,
            video_stream_idx,
            video_pts: 0,
            video_time_base,
            width,
            height,
            audio_enc,
            audio_stream_idx,
            audio_pts: 0,
            audio_sample_rate: TARGET_RATE,
            audio_in_channels: audio_channels as usize,
            audio_frame_size,
            audio_buf: Vec::new(),
            audio_enc_format,
            swr,
            audio_in_rate,
        })
    }
}

// ── Video encoder selection ────────────────────────────────────────────────────

/// Try video encoders in preference order.  Returns `(encoder, stream_idx, name)`.
fn open_video_encoder(
    pref: &VideoCodecPref,
    octx: &mut format::context::Output,
    width: u32,
    height: u32,
    fps: u32,
    time_base: Rational,
) -> Result<(encoder::Video, usize, String)> {
    // Software fallback options (same as before the codec-pref feature).
    // Used as the last-resort entry so the fallback is byte-for-byte identical
    // to the pre-feature code path.
    let sw_h264_opts: &[(&str, &str)] = &[("preset", "fast"), ("crf", "23")];
    let sw_h265_opts: &[(&str, &str)] = &[("preset", "fast"), ("crf", "28")];
    // NVENC options: constqp mode via `cq` (simplest, works on all NVENC gens).
    let nvenc_h264_opts: &[(&str, &str)] = &[("preset", "p4"), ("cq", "23")];
    let nvenc_h265_opts: &[(&str, &str)] = &[("preset", "p4"), ("cq", "28")];

    // Named-encoder candidates (try in order; first success wins).
    let hw_candidates: &[(&str, &[(&str, &str)])] = match pref {
        VideoCodecPref::H264Auto => &[("h264_nvenc", nvenc_h264_opts)],
        VideoCodecPref::H265Auto => &[("hevc_nvenc", nvenc_h265_opts)],
        VideoCodecPref::H264Sw | VideoCodecPref::H265Sw => &[],
    };

    // Software codec ID used for the final fallback (same as original code).
    let sw_fallback: Option<(codec::Id, &[(&str, &str)])> = match pref {
        VideoCodecPref::H264Auto | VideoCodecPref::H264Sw => {
            Some((codec::Id::H264, sw_h264_opts))
        }
        VideoCodecPref::H265Auto | VideoCodecPref::H265Sw => {
            Some((codec::Id::HEVC, sw_h265_opts))
        }
    };

    // ── Try hardware encoders ─────────────────────────────────────────────────
    for (name, opts_pairs) in hw_candidates {
        if let Some((enc, idx)) = try_open_named(name, opts_pairs, octx, width, height, fps, time_base) {
            return Ok((enc, idx, name.to_string()));
        }
    }

    // ── Software fallback (by codec::Id — identical to pre-feature code) ──────
    if let Some((id, opts_pairs)) = sw_fallback {
        let codec = encoder::find(id).with_context(|| format!("no software encoder for {id:?}"))?;
        let name = unsafe {
            std::ffi::CStr::from_ptr((*codec.as_ptr()).name)
                .to_str()
                .unwrap_or("unknown")
                .to_owned()
        };
        if let Some((enc, idx)) = try_open_codec(codec, opts_pairs, octx, width, height, fps, time_base) {
            return Ok((enc, idx, name));
        }
    }

    anyhow::bail!("No video encoder available for {pref:?}")
}

/// Try to open a named encoder.  Returns `None` if not found or fails to open.
fn try_open_named(
    name: &str,
    opts: &[(&str, &str)],
    octx: &mut format::context::Output,
    width: u32,
    height: u32,
    fps: u32,
    time_base: Rational,
) -> Option<(encoder::Video, usize)> {
    let codec = encoder::find_by_name(name)?;
    try_open_codec(codec, opts, octx, width, height, fps, time_base)
}

/// Try to open a codec object.  Returns `None` if setup or open fails.
fn try_open_codec(
    codec: codec::codec::Codec,
    opts: &[(&str, &str)],
    octx: &mut format::context::Output,
    width: u32,
    height: u32,
    fps: u32,
    time_base: Rational,
) -> Option<(encoder::Video, usize)> {
    let mut ctx = codec::Context::new_with_codec(codec).encoder().video().ok()?;
    ctx.set_width(width);
    ctx.set_height(height);
    ctx.set_format(ffmpeg_next::util::format::Pixel::YUV420P);
    ctx.set_time_base(time_base);
    ctx.set_frame_rate(Some(Rational::new(fps as i32, 1)));

    let mut dict = Dictionary::new();
    for (k, v) in opts {
        dict.set(k, v);
    }

    match ctx.open_with(dict) {
        Ok(enc) => {
            // Add stream only after encoder opens successfully.
            match octx.add_stream(codec) {
                Ok(mut vst) => {
                    vst.set_parameters(&enc);
                    let idx = vst.index();
                    Some((enc, idx))
                }
                Err(e) => {
                    log::warn!("add_stream failed: {e}");
                    None
                }
            }
        }
        Err(e) => {
            let name = unsafe {
                std::ffi::CStr::from_ptr((*codec.as_ptr()).name)
                    .to_str()
                    .unwrap_or("?")
            };
            log::info!("{name} failed to open ({e}), trying next encoder");
            None
        }
    }
}

// ── Audio encoder selection ────────────────────────────────────────────────────

/// Open audio encoder.  Returns `(encoder, stream_idx, frame_size, enc_format)`.
fn open_audio_encoder(
    pref: &AudioCodecPref,
    octx: &mut format::context::Output,
) -> Result<(Option<encoder::Audio>, usize, usize, ffmpeg_next::util::format::Sample)> {
    // (codec_id_or_name, bitrate, frame_format)
    let (codec, bitrate, frame_fmt) = match pref {
        AudioCodecPref::Aac => (
            encoder::find(codec::Id::AAC),
            192_000usize,
            ffmpeg_next::util::format::Sample::F32(
                ffmpeg_next::util::format::sample::Type::Planar,
            ),
        ),
        AudioCodecPref::Opus => (
            encoder::find_by_name("libopus")
                .or_else(|| encoder::find(codec::Id::OPUS)),
            128_000usize,
            ffmpeg_next::util::format::Sample::F32(
                ffmpeg_next::util::format::sample::Type::Packed,
            ),
        ),
    };

    let codec = match codec {
        Some(c) => c,
        None => {
            log::warn!("{pref:?} encoder not found — recording without audio");
            let default_fmt = ffmpeg_next::util::format::Sample::F32(
                ffmpeg_next::util::format::sample::Type::Planar,
            );
            return Ok((None, usize::MAX, 1024, default_fmt));
        }
    };

    let mut aenc_ctx = codec::Context::new_with_codec(codec)
        .encoder()
        .audio()
        .context("create audio encoder ctx")?;

    aenc_ctx.set_rate(TARGET_RATE as i32);
    aenc_ctx.set_channel_layout(ChannelLayout::STEREO);
    aenc_ctx.set_format(frame_fmt);
    aenc_ctx.set_bit_rate(bitrate);
    aenc_ctx.set_time_base(Rational::new(1, TARGET_RATE as i32));

    let audio_enc = aenc_ctx.open().context("open audio encoder")?;
    let default_frame_size = if matches!(pref, AudioCodecPref::Opus) { 960 } else { 1024 };
    let frame_size = (audio_enc.frame_size() as usize).max(default_frame_size);

    let idx = {
        let mut ast = octx.add_stream(codec).context("add audio stream")?;
        ast.set_parameters(&audio_enc);
        ast.index()
    };

    log::info!(
        "Audio encoder: {pref:?} stereo @ {TARGET_RATE} Hz, frame_size={frame_size}, bitrate={bitrate}"
    );
    Ok((Some(audio_enc), idx, frame_size, frame_fmt))
}
