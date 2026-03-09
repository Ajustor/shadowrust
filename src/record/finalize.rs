use anyhow::{Context, Result};
use ffmpeg_next::util::rational::Rational;

use super::Recorder;

impl Recorder {
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
