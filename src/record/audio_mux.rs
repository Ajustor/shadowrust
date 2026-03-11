use ffmpeg_next::{channel_layout::ChannelLayout, frame, util::rational::Rational};

use super::Recorder;

impl Recorder {
    /// Push interleaved f32 audio samples (L,R,L,R,…) from the capture card.
    ///
    /// Samples are resampled to 48 kHz stereo via SWR, appended to `audio_buf`
    /// (interleaved, 48 kHz stereo), and encoded in chunks of `audio_frame_size`
    /// stereo pairs.  Leftover samples stay in the buffer for the next call so
    /// no samples are ever discarded mid-recording.
    pub fn push_audio(&mut self, samples: &[f32]) {
        if samples.is_empty() || self.audio_enc.is_none() {
            return;
        }

        // Take encoder out to avoid double-mut-borrow.
        let mut enc = match self.audio_enc.take() {
            Some(e) => e,
            None => return,
        };

        self.resample_into_buf(samples);
        self.drain_buf_to_encoder(&mut enc);

        self.audio_enc = Some(enc);
    }

    /// Convert `samples` to 48 kHz interleaved stereo and append to `audio_buf`.
    fn resample_into_buf(&mut self, samples: &[f32]) {
        let in_ch = self.audio_in_channels.max(1);
        let nb_in = samples.len() / in_ch;

        let Some(swr) = self.swr.as_mut() else {
            // SWR unavailable — buffer raw samples as-is (best-effort fallback).
            self.audio_buf.extend_from_slice(samples);
            return;
        };

        // ── Build packed input frame ─────────────────────────────────────────
        let in_layout = if in_ch >= 2 { ChannelLayout::STEREO } else { ChannelLayout::MONO };
        let mut in_frame = frame::Audio::new(
            ffmpeg_next::util::format::Sample::F32(
                ffmpeg_next::util::format::sample::Type::Packed,
            ),
            nb_in,
            in_layout,
        );
        in_frame.set_rate(self.audio_in_rate);
        {
            let dst: &mut [f32] = bytemuck::cast_slice_mut(in_frame.data_mut(0));
            let n = samples.len().min(dst.len());
            dst[..n].copy_from_slice(&samples[..n]);
        }

        // ── Build packed output frame (stereo, TARGET_RATE) ──────────────────
        // Over-allocate to guarantee SWR has room for all output samples.
        let nb_out_max =
            (nb_in as f64 * self.audio_sample_rate as f64 / self.audio_in_rate as f64).ceil()
                as usize
                + 512;
        let mut out_frame = frame::Audio::new(
            ffmpeg_next::util::format::Sample::F32(
                // Packed (interleaved) — appended directly to audio_buf below.
                ffmpeg_next::util::format::sample::Type::Packed,
            ),
            nb_out_max,
            ChannelLayout::STEREO,
        );
        out_frame.set_rate(self.audio_sample_rate);

        // ── Resample ─────────────────────────────────────────────────────────
        // swr.run() returns Ok(actual_samples_written).  IMPORTANT: do NOT use
        // out_frame.samples() — that returns the allocated capacity, not the
        // number actually produced, which would inject silence and desync audio.
        match swr.run(&in_frame, &mut out_frame) {
            Ok(_) => {
                // swr_convert_frame sets out_frame->nb_samples to the actual count.
                let nb_out = out_frame.samples();
                if nb_out > 0 {
                    // Packed stereo: nb_out samples/ch × 2 channels = nb_out*2 f32 values.
                    let floats: &[f32] = bytemuck::cast_slice(out_frame.data(0));
                    let len = (nb_out * 2).min(floats.len());
                    self.audio_buf.extend_from_slice(&floats[..len]);
                }
            }
            Err(e) => log::warn!("SWR error: {e}"),
        }
    }

    /// Drain `audio_buf` into the encoder in chunks of `audio_frame_size` stereo pairs.
    /// Handles both planar formats (AAC) and packed formats (Opus).
    /// Leftover samples (< one full frame) stay in the buffer.
    fn drain_buf_to_encoder(&mut self, enc: &mut ffmpeg_next::encoder::Audio) {
        // Each AAC frame = audio_frame_size samples per channel.
        // Interleaved stereo ⇒ audio_frame_size × 2 f32 values per chunk.
        let chunk = self.audio_frame_size * 2;

        let is_planar = matches!(
            self.audio_enc_format,
            ffmpeg_next::util::format::Sample::F32(ffmpeg_next::util::format::sample::Type::Planar)
                | ffmpeg_next::util::format::Sample::I16(
                    ffmpeg_next::util::format::sample::Type::Planar
                )
        );

        while self.audio_buf.len() >= chunk {
            let mut af = frame::Audio::new(
                self.audio_enc_format,
                self.audio_frame_size,
                ChannelLayout::STEREO,
            );
            af.set_pts(Some(self.audio_pts));
            self.audio_pts += self.audio_frame_size as i64;

            let src = &self.audio_buf[..chunk];
            if is_planar {
                // Deinterleave: buf[L0,R0,L1,R1,…] → plane 0 = L, plane 1 = R
                for ch in 0..2usize {
                    let dst: &mut [f32] = bytemuck::cast_slice_mut(af.data_mut(ch));
                    for (i, s) in dst[..self.audio_frame_size].iter_mut().enumerate() {
                        *s = src[i * 2 + ch];
                    }
                }
            } else {
                // Packed: audio_buf is already interleaved stereo — copy directly.
                let dst: &mut [f32] = bytemuck::cast_slice_mut(af.data_mut(0));
                let n = chunk.min(dst.len());
                dst[..n].copy_from_slice(&src[..n]);
            }

            self.audio_buf.drain(..chunk);
            self.write_audio_packet(enc, &mut af);
        }
    }

    pub(crate) fn write_audio_packet(
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
            pkt.rescale_ts(Rational::new(1, self.audio_sample_rate as i32), audio_tb);
            pkt.write_interleaved(&mut self.octx).ok();
        }
    }
}

