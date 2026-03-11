use anyhow::{Context, Result};
use ffmpeg_next as ffmpeg;
use ffmpeg_next::{
    Dictionary,
    channel_layout::ChannelLayout,
    codec, encoder, format,
    software::{resampling, scaling},
    util::rational::Rational,
};

use super::Recorder;

/// The sample rate the AAC encoder always runs at.
/// Normalising to a single rate via SWR eliminates any CPAL/driver rate drift.
const TARGET_RATE: u32 = 48_000;

impl Recorder {
    pub fn new(
        path: &str,
        width: u32,
        height: u32,
        fps: u32,
        audio_in_rate: u32,
        audio_channels: u16,
    ) -> Result<Self> {
        ffmpeg::init().context("ffmpeg init")?;

        let mut octx = format::output(path).context("open output file")?;

        // ── Video stream ──────────────────────────────────────────────────────
        let video_codec = encoder::find(codec::Id::H264).context("find H.264 encoder")?;
        let video_time_base = Rational::new(1, fps as i32);

        let mut venc_ctx = codec::Context::new_with_codec(video_codec)
            .encoder()
            .video()
            .context("create video encoder ctx")?;
        venc_ctx.set_width(width);
        venc_ctx.set_height(height);
        venc_ctx.set_format(ffmpeg_next::util::format::Pixel::YUV420P);
        venc_ctx.set_time_base(video_time_base);
        venc_ctx.set_frame_rate(Some(Rational::new(fps as i32, 1)));
        // Do NOT set bit_rate — we use CRF mode (quality-based) which produces
        // much smaller files than CBR at 8 Mbps.  Setting bit_rate would
        // override CRF and force a constant-bitrate mode.

        let mut vopts = Dictionary::new();
        vopts.set("preset", "fast");   // good compression, still real-time
        vopts.set("crf", "23");         // visually lossless (FFmpeg default quality)

        let video_enc = venc_ctx.open_with(vopts).context("open H.264 encoder")?;

        {
            let mut vst = octx.add_stream(video_codec).context("add video stream")?;
            vst.set_parameters(&video_enc);
        }
        let video_stream_idx = 0usize;

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

        // ── Audio stream (AAC @ TARGET_RATE Hz stereo) ────────────────────────
        // The encoder always runs at TARGET_RATE Hz stereo.  If the CPAL input
        // is at a different rate or channel count, the SWR resampler below
        // converts it transparently — this is the root fix for "audio too fast".
        let (audio_enc, audio_stream_idx, audio_frame_size) = match encoder::find(codec::Id::AAC) {
            Some(audio_codec) => {
                let mut aenc_ctx = codec::Context::new_with_codec(audio_codec)
                    .encoder()
                    .audio()
                    .context("create audio encoder ctx")?;

                aenc_ctx.set_rate(TARGET_RATE as i32);
                aenc_ctx.set_channel_layout(ChannelLayout::STEREO);
                aenc_ctx.set_format(ffmpeg_next::util::format::Sample::F32(
                    ffmpeg_next::util::format::sample::Type::Planar,
                ));
                aenc_ctx.set_bit_rate(192_000);
                aenc_ctx.set_time_base(Rational::new(1, TARGET_RATE as i32));

                let audio_enc = aenc_ctx.open().context("open AAC encoder")?;
                let frame_size = (audio_enc.frame_size() as usize).max(1024);

                {
                    let mut ast = octx.add_stream(audio_codec).context("add audio stream")?;
                    ast.set_parameters(&audio_enc);
                }
                let audio_stream_idx = 1usize;

                log::info!(
                    "Audio recording: AAC stereo @ {TARGET_RATE} Hz, frame_size={frame_size} \
                     (input: {}ch @ {audio_in_rate} Hz)",
                    audio_channels
                );
                (Some(audio_enc), audio_stream_idx, frame_size)
            }
            None => {
                log::warn!("AAC encoder not found — recording without audio");
                (None, usize::MAX, 1024)
            }
        };

        // ── SWR resampler: CPAL input → encoder format ────────────────────────
        // Even if the rates happen to match, we always normalise to stereo
        // f32-planar so the encoder never sees unexpected formats.
        let in_layout = if audio_channels >= 2 {
            ChannelLayout::STEREO
        } else {
            ChannelLayout::MONO
        };
        let swr = resampling::Context::get(
            // source: interleaved f32 at CPAL's rate
            ffmpeg_next::util::format::Sample::F32(
                ffmpeg_next::util::format::sample::Type::Packed,
            ),
            in_layout,
            audio_in_rate,
            // destination: planar f32 stereo at TARGET_RATE
            ffmpeg_next::util::format::Sample::F32(
                ffmpeg_next::util::format::sample::Type::Planar,
            ),
            ChannelLayout::STEREO,
            TARGET_RATE,
        )
        .map_err(|e| log::warn!("SWR init failed ({e}) — recording without resampling"))
        .ok();

        // Write the file header.  The encode thread calls rec.finish() which
        // always writes the trailer, so standard (non-fragmented) MP4 works
        // correctly.  MKV is natively progressive; no special flags needed.
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
            swr,
            audio_in_rate,
        })
    }
}
