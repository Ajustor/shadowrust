use anyhow::{Context, Result};
use bytemuck::cast_slice;
use ffmpeg_next as ffmpeg;
use ffmpeg_next::{
    Dictionary, channel_layout::ChannelLayout, codec, encoder, format, frame, software::scaling,
    util::rational::Rational,
};

pub struct Recorder {
    octx: format::context::Output,
    // ── Video ──
    video_enc: encoder::Video,
    video_scaler: scaling::Context,
    video_stream_idx: usize,
    video_pts: i64,
    video_time_base: Rational,
    width: u32,
    height: u32,
    // ── Audio ──
    audio_enc: Option<encoder::Audio>,
    audio_stream_idx: usize,
    audio_pts: i64,
    audio_sample_rate: u32,
    audio_in_channels: usize,
    /// AAC frame size (samples per channel, typically 1024)
    audio_frame_size: usize,
    /// Pending interleaved f32 samples awaiting a full AAC frame
    audio_buf: Vec<f32>,
}

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
        let mut vst = octx.add_stream(video_codec).context("add video stream")?;
        let video_stream_idx = vst.index();
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
        vst.set_parameters(&video_enc);

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
                let mut ast = octx.add_stream(audio_codec).context("add audio stream")?;
                let audio_stream_idx = ast.index();

                let mut aenc_ctx = codec::Context::new_with_codec(audio_codec)
                    .encoder()
                    .audio()
                    .context("create audio encoder ctx")?;

                aenc_ctx.set_rate(audio_sample_rate as i32);
                aenc_ctx.set_channel_layout(if audio_channels >= 2 {
                    ChannelLayout::STEREO
                } else {
                    ChannelLayout::MONO
                });
                aenc_ctx.set_format(ffmpeg_next::util::format::Sample::F32(
                    ffmpeg_next::util::format::sample::Type::Planar,
                ));
                aenc_ctx.set_bit_rate(128_000);
                aenc_ctx.set_time_base(Rational::new(1, audio_sample_rate as i32));

                let audio_enc = aenc_ctx.open().context("open AAC encoder")?;

                let frame_size = audio_enc.frame_size() as usize;
                ast.set_parameters(&audio_enc);
                log::info!(
                    "Audio recording: AAC {}ch @ {}Hz, frame_size={frame_size}",
                    audio_channels,
                    audio_sample_rate
                );
                (Some(audio_enc), audio_stream_idx, frame_size.max(1024))
            }
            None => {
                log::warn!("AAC encoder not found — recording without audio");
                (None, usize::MAX, 1024)
            }
        };

        format::context::output::dump(&octx, 0, Some(path));
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

    /// Push a raw RGBA video frame.
    pub fn push_frame(&mut self, rgba: &[u8], _size: (u32, u32)) {
        let mut src = frame::Video::new(
            ffmpeg_next::util::format::Pixel::RGBA,
            self.width,
            self.height,
        );
        src.data_mut(0).copy_from_slice(rgba);

        let mut dst = frame::Video::new(
            ffmpeg_next::util::format::Pixel::YUV420P,
            self.width,
            self.height,
        );
        if self.video_scaler.run(&src, &mut dst).is_err() {
            return;
        }

        dst.set_pts(Some(self.video_pts));
        self.video_pts += 1;

        if self.video_enc.send_frame(&dst).is_err() {
            return;
        }

        let mut pkt = ffmpeg_next::Packet::empty();
        while self.video_enc.receive_packet(&mut pkt).is_ok() {
            pkt.set_stream(self.video_stream_idx);
            pkt.rescale_ts(
                self.video_time_base,
                self.octx.stream(self.video_stream_idx).unwrap().time_base(),
            );
            pkt.write_interleaved(&mut self.octx).ok();
        }
    }

    /// Push interleaved f32 audio samples (L, R, L, R, …) from the capture card.
    ///
    /// Samples are buffered internally and encoded in AAC frames of
    /// `frame_size` samples per channel. Call this every video frame with
    /// whatever was drained from `AudioPassthrough::drain_recording_samples`.
    pub fn push_audio(&mut self, samples: &[f32]) {
        let Some(enc) = self.audio_enc.as_mut() else {
            return;
        };

        self.audio_buf.extend_from_slice(samples);

        let in_ch = self.audio_in_channels;
        // Encode-as-stereo: if mono input, duplicate; if >2ch, take first two
        let enc_ch: usize = enc.channel_layout().channels() as usize;
        let frame_samples = self.audio_frame_size * in_ch; // interleaved samples for one AAC frame

        while self.audio_buf.len() >= frame_samples {
            let chunk: Vec<f32> = self.audio_buf.drain(..frame_samples).collect();

            let mut af = frame::Audio::new(
                ffmpeg_next::util::format::Sample::F32(
                    ffmpeg_next::util::format::sample::Type::Planar,
                ),
                self.audio_frame_size,
                enc.channel_layout(),
            );
            af.set_pts(Some(self.audio_pts));
            self.audio_pts += self.audio_frame_size as i64;

            // Deinterleave: chunk = [L0,R0,L1,R1,...] → plane 0 = L, plane 1 = R
            for plane in 0..enc_ch {
                let src_ch = plane.min(in_ch - 1); // clamp for mono→stereo upmix
                let plane_bytes = af.data_mut(plane);
                let plane_f32: &mut [f32] = cast_slice_mut(plane_bytes);
                for (i, s) in plane_f32[..self.audio_frame_size].iter_mut().enumerate() {
                    *s = chunk.get(i * in_ch + src_ch).copied().unwrap_or(0.0);
                }
            }

            if enc.send_frame(&af).is_err() {
                break;
            }

            let audio_tb = self.octx.stream(self.audio_stream_idx).unwrap().time_base();
            let mut pkt = ffmpeg_next::Packet::empty();
            while enc.receive_packet(&mut pkt).is_ok() {
                pkt.set_stream(self.audio_stream_idx);
                pkt.rescale_ts(Rational::new(1, self.audio_sample_rate as i32), audio_tb);
                pkt.write_interleaved(&mut self.octx).ok();
            }
        }
    }

    /// Flush encoders and write file trailer.
    pub fn finish(mut self) -> Result<()> {
        // Flush video
        self.video_enc.send_eof().ok();
        let mut pkt = ffmpeg_next::Packet::empty();
        while self.video_enc.receive_packet(&mut pkt).is_ok() {
            pkt.set_stream(self.video_stream_idx);
            pkt.write_interleaved(&mut self.octx).ok();
        }

        // Flush audio
        if let Some(enc) = self.audio_enc.as_mut() {
            enc.send_eof().ok();
            let audio_tb = self.octx.stream(self.audio_stream_idx).unwrap().time_base();
            while enc.receive_packet(&mut pkt).is_ok() {
                pkt.set_stream(self.audio_stream_idx);
                pkt.rescale_ts(Rational::new(1, self.audio_sample_rate as i32), audio_tb);
                pkt.write_interleaved(&mut self.octx).ok();
            }
        }

        self.octx.write_trailer().context("write file trailer")?;
        log::info!("Recording finalised");
        Ok(())
    }
}

use bytemuck::cast_slice_mut;
