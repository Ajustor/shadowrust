use bytemuck::cast_slice;
use ffmpeg_next::{
    channel_layout::ChannelLayout, frame, util::rational::Rational,
};

use super::Recorder;

impl Recorder {
    /// Push interleaved f32 audio samples (L,R,L,R,…) from the capture card.
    ///
    /// The samples are first passed through the SWR resampler (which converts
    /// from the CPAL input rate/channels to 48 kHz stereo), then fed to the
    /// AAC encoder.  This eliminates any pitch/speed distortion that would
    /// result from a mismatch between the declared and actual sample rate.
    pub fn push_audio(&mut self, samples: &[f32]) {
        if samples.is_empty() || self.audio_enc.is_none() {
            return;
        }

        let in_ch = self.audio_in_channels.max(1);
        let nb_in_frames = samples.len() / in_ch;

        // Take the encoder out to avoid simultaneous mutable borrows.
        let mut enc = match self.audio_enc.take() {
            Some(e) => e,
            None => return,
        };

        if let Some(swr) = self.swr.as_mut() {
            // Build a packed input frame at the CPAL rate.
            let in_layout = if in_ch >= 2 {
                ChannelLayout::STEREO
            } else {
                ChannelLayout::MONO
            };
            let mut in_frame = frame::Audio::new(
                ffmpeg_next::util::format::Sample::F32(
                    ffmpeg_next::util::format::sample::Type::Packed,
                ),
                nb_in_frames,
                in_layout,
            );
            in_frame.set_rate(self.audio_in_rate);

            let plane = in_frame.data_mut(0);
            let dst: &mut [f32] = bytemuck::cast_slice_mut(plane);
            let copy = samples.len().min(dst.len());
            dst[..copy].copy_from_slice(&samples[..copy]);

            // Allocate the output frame (planar f32 stereo at TARGET_RATE).
            let nb_out_max = (nb_in_frames as f64
                * self.audio_sample_rate as f64
                / self.audio_in_rate as f64)
                .ceil() as usize
                + 1024;

            let mut out_frame = frame::Audio::new(
                ffmpeg_next::util::format::Sample::F32(
                    ffmpeg_next::util::format::sample::Type::Planar,
                ),
                nb_out_max,
                ChannelLayout::STEREO,
            );
            out_frame.set_rate(self.audio_sample_rate);

            match swr.run(&in_frame, &mut out_frame) {
                Ok(_) => {
                    let nb_out = out_frame.samples();
                    if nb_out > 0 {
                        self.encode_planar_frame(&out_frame, nb_out, &mut enc);
                    }
                }
                Err(e) => {
                    log::warn!("SWR run error: {e} — falling back to raw samples");
                    self.push_audio_raw_inner(samples, &mut enc);
                }
            }
        } else {
            self.push_audio_raw_inner(samples, &mut enc);
        }

        self.audio_enc = Some(enc);
    }

    /// Encode audio from a planar f32 frame in chunks of `audio_frame_size`.
    fn encode_planar_frame(
        &mut self,
        src: &frame::Audio,
        nb_samples: usize,
        enc: &mut ffmpeg_next::encoder::Audio,
    ) {
        let enc_ch = enc.channel_layout().channels() as usize;
        let frame_size = self.audio_frame_size;
        let mut offset = 0;

        while offset + frame_size <= nb_samples {
            let mut af = frame::Audio::new(
                ffmpeg_next::util::format::Sample::F32(
                    ffmpeg_next::util::format::sample::Type::Planar,
                ),
                frame_size,
                ChannelLayout::STEREO,
            );
            af.set_pts(Some(self.audio_pts));
            self.audio_pts += frame_size as i64;

            for plane in 0..enc_ch {
                let src_plane: &[f32] = cast_slice(src.data(plane));
                let dst_plane = af.data_mut(plane);
                let dst_f32: &mut [f32] = bytemuck::cast_slice_mut(dst_plane);
                if let (Some(s), Some(d)) = (
                    src_plane.get(offset..offset + frame_size),
                    dst_f32.get_mut(..frame_size),
                ) {
                    d.copy_from_slice(s);
                }
            }

            self.write_encoded_packet(enc, &mut af);
            offset += frame_size;
        }
    }

    /// Legacy path: deinterleave raw f32 samples and encode directly.
    /// Used only when SWR is unavailable (should rarely happen).
    fn push_audio_raw_inner(&mut self, samples: &[f32], enc: &mut ffmpeg_next::encoder::Audio) {
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

            for plane in 0..enc_ch {
                let src_ch = plane.min(in_ch - 1);
                let plane_bytes = af.data_mut(plane);
                let plane_f32: &mut [f32] = bytemuck::cast_slice_mut(plane_bytes);
                if let Some(out) = plane_f32.get_mut(..self.audio_frame_size) {
                    for (i, s) in out.iter_mut().enumerate() {
                        *s = chunk
                            .get(i * in_ch + src_ch)
                            .copied()
                            .unwrap_or(0.0);
                    }
                }
            }

            self.write_encoded_packet(enc, &mut af);
        }
    }

    fn write_encoded_packet(
        &mut self,
        enc: &mut ffmpeg_next::encoder::Audio,
        af: &mut frame::Audio,
    ) {
        if enc.send_frame(af).is_err() {
            return;
        }
        let audio_tb = match self.octx.stream(self.audio_stream_idx) {
            Some(s) => s.time_base(),
            None => return,
        };
        let mut pkt = ffmpeg_next::Packet::empty();
        while enc.receive_packet(&mut pkt).is_ok() {
            pkt.set_stream(self.audio_stream_idx);
            pkt.rescale_ts(
                Rational::new(1, self.audio_sample_rate as i32),
                audio_tb,
            );
            pkt.write_interleaved(&mut self.octx).ok();
        }
    }
}
