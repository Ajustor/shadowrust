use ffmpeg_next::frame;

use super::Recorder;

impl Recorder {
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
}
