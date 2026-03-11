use anyhow::{Context, Result};
use ffmpeg_next::{channel_layout::ChannelLayout, frame, util::rational::Rational};

use super::Recorder;

impl Recorder {
    /// Flush encoders and write file trailer.
    pub fn finish(mut self) -> Result<()> {
        // ── Flush remaining buffered audio samples ────────────────────────────
        // audio_buf may hold up to (frame_size - 1) stereo pairs that didn't
        // fill the last AAC frame.  Pad with silence and encode them so the
        // audio track doesn't end early relative to the video.
        if !self.audio_buf.is_empty() {
            if let Some(mut enc) = self.audio_enc.take() {
                let chunk = self.audio_frame_size * 2;
                while self.audio_buf.len() < chunk {
                    self.audio_buf.push(0.0);
                }

                let is_planar = matches!(
                    self.audio_enc_format,
                    ffmpeg_next::util::format::Sample::F32(
                        ffmpeg_next::util::format::sample::Type::Planar
                    ) | ffmpeg_next::util::format::Sample::I16(
                        ffmpeg_next::util::format::sample::Type::Planar
                    )
                );

                let mut af = frame::Audio::new(
                    self.audio_enc_format,
                    self.audio_frame_size,
                    ChannelLayout::STEREO,
                );
                af.set_pts(Some(self.audio_pts));
                let src = &self.audio_buf[..chunk];
                if is_planar {
                    let n = self.audio_frame_size;
                    for ch in 0..2usize {
                        unsafe {
                            let plane = std::slice::from_raw_parts_mut(
                                (*af.as_mut_ptr()).data[ch] as *mut f32,
                                n,
                            );
                            for (i, s) in plane.iter_mut().enumerate() {
                                *s = src[i * 2 + ch];
                            }
                        }
                    }
                } else {
                    let dst: &mut [f32] = bytemuck::cast_slice_mut(af.data_mut(0));
                    let n = chunk.min(dst.len());
                    dst[..n].copy_from_slice(&src[..n]);
                }
                self.write_audio_packet(&mut enc, &mut af);
                // Put the encoder back so the EOF flush below can drain it.
                self.audio_enc = Some(enc);
            }
        }

        // ── Flush video encoder ───────────────────────────────────────────────
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

        // ── Flush audio encoder ───────────────────────────────────────────────
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
