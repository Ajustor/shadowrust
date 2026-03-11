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
    // Quality options for software encoders (CRF = quality-based, no bitrate cap).
    let sw_h264_opts: &[(&str, &str)] = &[("preset", "fast"), ("crf", "23")];
    let sw_h265_opts: &[(&str, &str)] = &[("preset", "fast"), ("crf", "28")];
    // NVENC: constqp quality mode via `cq`.
    let nvenc_h264_opts: &[(&str, &str)] = &[("preset", "p4"), ("cq", "23")];
    let nvenc_h265_opts: &[(&str, &str)] = &[("preset", "p4"), ("cq", "28")];

    match pref {
        // ── Auto: use the system's default encoder for the codec ID ───────────
        // This is identical to the original code path — whatever encoder the
        // system has registered as default for H264/HEVC will be used.
        VideoCodecPref::H264Auto => {
            let codec = encoder::find(codec::Id::H264)
                .context("H.264 encoder not found")?;
            let name = codec_name(codec);
            // libx264 uses CRF; if the system default happens to be NVENC, pass
            // NVENC-appropriate options so it also works.
            let opts = if name.contains("nvenc") { nvenc_h264_opts } else { sw_h264_opts };
            try_open_codec(codec, opts, octx, width, height, fps, time_base)
                .ok_or_else(|| anyhow::anyhow!("H.264 auto encoder ({name}) failed to open"))
                .map(|(enc, idx)| (enc, idx, name))
        }
        VideoCodecPref::H265Auto => {
            let codec = encoder::find(codec::Id::HEVC)
                .context("H.265 encoder not found")?;
            let name = codec_name(codec);
            let opts = if name.contains("nvenc") { nvenc_h265_opts } else { sw_h265_opts };
            try_open_codec(codec, opts, octx, width, height, fps, time_base)
                .ok_or_else(|| anyhow::anyhow!("H.265 auto encoder ({name}) failed to open"))
                .map(|(enc, idx)| (enc, idx, name))
        }

        // ── NVENC: explicitly request GPU encoder, no software fallback ───────
        VideoCodecPref::H264Nvenc => {
            let codec = encoder::find_by_name("h264_nvenc")
                .context("h264_nvenc not found — is an NVIDIA GPU available?")?;
            try_open_codec(codec, nvenc_h264_opts, octx, width, height, fps, time_base)
                .ok_or_else(|| anyhow::anyhow!("h264_nvenc failed to open (no NVENC GPU?)"))
                .map(|(enc, idx)| (enc, idx, "h264_nvenc".to_string()))
        }
        VideoCodecPref::H265Nvenc => {
            let codec = encoder::find_by_name("hevc_nvenc")
                .context("hevc_nvenc not found — is an NVIDIA GPU available?")?;
            try_open_codec(codec, nvenc_h265_opts, octx, width, height, fps, time_base)
                .ok_or_else(|| anyhow::anyhow!("hevc_nvenc failed to open (no NVENC GPU?)"))
                .map(|(enc, idx)| (enc, idx, "hevc_nvenc".to_string()))
        }
    }
}

/// Return the encoder name from an FFmpeg codec object (safe wrapper).
fn codec_name(codec: codec::codec::Codec) -> String {
    unsafe {
        std::ffi::CStr::from_ptr((*codec.as_ptr()).name)
            .to_str()
            .unwrap_or("unknown")
            .to_owned()
    }
}

/// Try to open a codec.  Returns `None` if setup or open fails.
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
            let name = codec_name(codec);
            log::info!("{name} failed to open ({e})");
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
