use bytemuck::cast_slice_mut;
use ffmpeg_next::{frame, util::rational::Rational};

use super::Recorder;

impl Recorder {
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
}
