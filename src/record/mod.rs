mod audio_mux;
mod finalize;
mod setup;
mod thread;
mod video;

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
    /// Software resampler: converts raw CPAL input to the encoder's format.
    /// None if no resampling is needed (rates and channels already match).
    pub(crate) swr: Option<resampling::Context>,
    /// Channel layout of the raw CPAL input (before resampling).
    pub(crate) audio_in_rate: u32,
}
