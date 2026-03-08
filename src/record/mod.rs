use anyhow::{Context, Result};
use bytemuck::cast_slice_mut;
use ffmpeg_next as ffmpeg;
use ffmpeg_next::{
    Dictionary, channel_layout::ChannelLayout, codec, encoder, format, frame,
    software::scaling, util::rational::Rational,
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
    audio_frame_size: usize,
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
        let video_codec =
            encoder::find(codec::Id::H264).context("find H.264 encoder")?;
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
        let (audio_enc, audio_stream_idx, audio_frame_size) =
            match encoder::find(codec::Id::AAC) {
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
                        let mut ast =
                            octx.add_stream(audio_codec).context("add audio stream")?;
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

    /// Push a raw RGBA video frame.
    pub fn push_frame(&mut self, rgba: &[u8], _size: (u32, u32)) {
        let expected = self.width as usize * self.height as usize * 4;
        if rgba.len() != expected {
            log::warn!(
                "push_frame: got {} bytes, expected {} ({} x {} x 4) — skipping",
                rgba.len(),
                expected,
                self.width,
                self.height
            );
            return;
        }

        let mut src = frame::Video::new(
            ffmpeg_next::util::format::Pixel::RGBA,
            self.width,
            self.height,
        );

        // Copy row by row to handle FFmpeg's internal stride/line-size padding.
        let row_bytes = self.width as usize * 4;
        let stride = src.stride(0);
        if stride == row_bytes {
            // Fast path: no padding, direct copy
            src.data_mut(0)[..expected].copy_from_slice(rgba);
        } else {
            for row in 0..self.height as usize {
                src.data_mut(0)[row * stride..row * stride + row_bytes]
                    .copy_from_slice(&rgba[row * row_bytes..(row + 1) * row_bytes]);
            }
        }

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

        let video_tb = match self.octx.stream(self.video_stream_idx) {
            Some(s) => s.time_base(),
            None => return,
        };
        let mut pkt = ffmpeg_next::Packet::empty();
        while self.video_enc.receive_packet(&mut pkt).is_ok() {
            pkt.set_stream(self.video_stream_idx);
            pkt.rescale_ts(self.video_time_base, video_tb);
            pkt.write_interleaved(&mut self.octx).ok();
        }
    }

    /// Push interleaved f32 audio samples (L,R,L,R,…) from the capture card.
    pub fn push_audio(&mut self, samples: &[f32]) {
        let Some(enc) = self.audio_enc.as_mut() else {
            return;
        };

        self.audio_buf.extend_from_slice(samples);

        let in_ch = self.audio_in_channels.max(1);
        let enc_ch = enc.channel_layout().channels() as usize;
        let frame_samples = self.audio_frame_size * in_ch;

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

            // Deinterleave: chunk = [L0,R0,L1,R1,…] → plane 0 = L, plane 1 = R
            for plane in 0..enc_ch {
                let src_ch = plane.min(in_ch - 1);
                let plane_bytes = af.data_mut(plane);
                // Safely cast byte slice to f32 slice
                if plane_bytes.len() < self.audio_frame_size * 4 {
                    log::warn!(
                        "Audio plane {} too small: {} bytes < {} expected",
                        plane,
                        plane_bytes.len(),
                        self.audio_frame_size * 4
                    );
                    break;
                }
                let plane_f32: &mut [f32] = cast_slice_mut(plane_bytes);
                let out = match plane_f32.get_mut(..self.audio_frame_size) {
                    Some(s) => s,
                    None => break,
                };
                for (i, s) in out.iter_mut().enumerate() {
                    *s = chunk.get(i * in_ch + src_ch).copied().unwrap_or(0.0);
                }
            }

            if enc.send_frame(&af).is_err() {
                break;
            }

            let audio_tb = match self.octx.stream(self.audio_stream_idx) {
                Some(s) => s.time_base(),
                None => break,
            };
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
        let video_tb = self
            .octx
            .stream(self.video_stream_idx)
            .map(|s| s.time_base());
        let mut pkt = ffmpeg_next::Packet::empty();
        while self.video_enc.receive_packet(&mut pkt).is_ok() {
            pkt.set_stream(self.video_stream_idx);
            if let Some(tb) = video_tb {
                pkt.rescale_ts(self.video_time_base, tb);
            }
            pkt.write_interleaved(&mut self.octx).ok();
        }

        // Flush audio
        if let Some(enc) = self.audio_enc.as_mut() {
            enc.send_eof().ok();
            let audio_tb = self
                .octx
                .stream(self.audio_stream_idx)
                .map(|s| s.time_base());
            while enc.receive_packet(&mut pkt).is_ok() {
                pkt.set_stream(self.audio_stream_idx);
                if let Some(tb) = audio_tb {
                    pkt.rescale_ts(Rational::new(1, self.audio_sample_rate as i32), tb);
                }
                pkt.write_interleaved(&mut self.octx).ok();
            }
        }

        self.octx.write_trailer().context("write file trailer")?;
        log::info!("Recording finalised");
        Ok(())
    }
}
