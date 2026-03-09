use anyhow::{Context, Result};
use ffmpeg_next as ffmpeg;
use ffmpeg_next::{
    Dictionary, channel_layout::ChannelLayout, codec, encoder, format, software::scaling,
    util::rational::Rational,
};

use super::Recorder;

impl Recorder {
    pub fn new(
        path: &str,
        width: u32,
        height: u32,
        fps: u32,
        audio_sample_rate: u32,
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
        venc_ctx.set_bit_rate(8_000_000);

        let mut vopts = Dictionary::new();
        vopts.set("preset", "ultrafast");
        vopts.set("crf", "18");

        let video_enc = venc_ctx.open_with(vopts).context("open H.264 encoder")?;

        {
            let mut vst = octx.add_stream(video_codec).context("add video stream")?;
            vst.set_parameters(&video_enc);
        }
        let video_stream_idx = 0usize; // video is always first stream

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

        // ── Audio stream (AAC) ────────────────────────────────────────────────
        let (audio_enc, audio_stream_idx, audio_frame_size) = match encoder::find(codec::Id::AAC) {
            Some(audio_codec) => {
                let mut aenc_ctx = codec::Context::new_with_codec(audio_codec)
                    .encoder()
                    .audio()
                    .context("create audio encoder ctx")?;

                let layout = if audio_channels >= 2 {
                    ChannelLayout::STEREO
                } else {
                    ChannelLayout::MONO
                };
                aenc_ctx.set_rate(audio_sample_rate as i32);
                aenc_ctx.set_channel_layout(layout);
                aenc_ctx.set_format(ffmpeg_next::util::format::Sample::F32(
                    ffmpeg_next::util::format::sample::Type::Planar,
                ));
                aenc_ctx.set_bit_rate(128_000);
                aenc_ctx.set_time_base(Rational::new(1, audio_sample_rate as i32));

                let audio_enc = aenc_ctx.open().context("open AAC encoder")?;
                let frame_size = (audio_enc.frame_size() as usize).max(1024);

                {
                    let mut ast = octx.add_stream(audio_codec).context("add audio stream")?;
                    ast.set_parameters(&audio_enc);
                }
                let audio_stream_idx = 1usize; // audio is second stream

                log::info!(
                    "Audio recording: AAC {}ch @ {}Hz, frame_size={frame_size}",
                    audio_channels,
                    audio_sample_rate
                );
                (Some(audio_enc), audio_stream_idx, frame_size)
            }
            None => {
                log::warn!("AAC encoder not found — recording without audio");
                (None, usize::MAX, 1024)
            }
        };

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
            audio_sample_rate,
            audio_in_channels: audio_channels as usize,
            audio_frame_size,
            audio_buf: Vec::new(),
        })
    }
}
