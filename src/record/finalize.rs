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
                let mut af = frame::Audio::new(
                    ffmpeg_next::util::format::Sample::F32(
                        ffmpeg_next::util::format::sample::Type::Planar,
                    ),
                    self.audio_frame_size,
                    ChannelLayout::STEREO,
                );
                af.set_pts(Some(self.audio_pts));
                let src = &self.audio_buf[..chunk];
                for ch in 0..2usize {
                    let dst: &mut [f32] = bytemuck::cast_slice_mut(af.data_mut(ch));
                    for (i, s) in dst[..self.audio_frame_size].iter_mut().enumerate() {
                        *s = src[i * 2 + ch];
                    }
                }
                self.write_audio_packet(&mut enc, &mut af);
                // enc dropped here (finishing)
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
