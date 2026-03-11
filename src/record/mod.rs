mod audio_mux;
mod finalize;
mod setup;
mod thread;
mod video;

use std::time::Instant;

use ffmpeg_next::{encoder, format, software::resampling, software::scaling, util::rational::Rational};

pub use thread::RecordThread;

pub struct Recorder {
    pub(crate) octx: format::context::Output,
    // ── Video ──
    pub(crate) video_enc: encoder::Video,
    pub(crate) video_scaler: scaling::Context,
    pub(crate) video_stream_idx: usize,
    pub(crate) video_pts: i64,
    pub(crate) video_time_base: Rational,
    pub(crate) video_start: Instant,
    pub(crate) width: u32,
    pub(crate) height: u32,
    // ── Audio ──
    pub(crate) audio_enc: Option<encoder::Audio>,
    pub(crate) audio_stream_idx: usize,
    pub(crate) audio_pts: i64,
    pub(crate) audio_sample_rate: u32,
    pub(crate) audio_in_channels: usize,
    pub(crate) audio_frame_size: usize,
    pub(crate) audio_buf: Vec<f32>,
    /// Format of the audio encoder frame (Planar for AAC, Packed for Opus).
    pub(crate) audio_enc_format: ffmpeg_next::util::format::Sample,
    /// Software resampler: converts raw CPAL input to the encoder's format.
    /// None if no resampling is needed (rates and channels already match).
    pub(crate) swr: Option<resampling::Context>,
    /// Channel layout of the raw CPAL input (before resampling).
    pub(crate) audio_in_rate: u32,
}

#[cfg(test)]
mod integration_tests {
    use super::*;
    use crate::config::{AudioCodecPref, VideoCodecPref};

    #[test]
    fn test_basic_recording() {
        let path = "/tmp/shadowrust_test_recording.mkv";
        let _ = std::fs::remove_file(path);

        let w = 320u32;
        let h = 240u32;
        let fps = 30u32;
        let audio_rate = 48000u32;
        // ~1600 samples per frame at 48kHz for 30fps (= 48000/30)
        let samples_per_frame = (audio_rate / fps) as usize * 2; // *2 for stereo

        let mut rec = Recorder::new(
            path, w, h, fps,
            audio_rate, 2,
            &VideoCodecPref::H264Auto,
            &AudioCodecPref::Aac,
        ).expect("Recorder::new should succeed");

        // Push 60 frames with interleaved audio (simulating real capture loop)
        let frame = vec![128u8; (w * h * 4) as usize]; // grey frame (not pure black — more realistic)
        let silence = vec![0.0f32; samples_per_frame];
        for _ in 0..60 {
            rec.push_audio(&silence);
            rec.push_frame(&frame, (w, h));
        }

        rec.finish().expect("finish should succeed");

        let meta = std::fs::metadata(path).expect("file should exist");
        println!("Test file size: {} bytes", meta.len());
        // Silence + uniform grey compresses aggressively — just verify the file
        // has actual content (both H.264 and AAC tracks) beyond a bare header.
        assert!(meta.len() > 2_000, "Recording should be > 2KB (video+audio), got {} bytes", meta.len());

        let _ = std::fs::remove_file(path);
    }
}
