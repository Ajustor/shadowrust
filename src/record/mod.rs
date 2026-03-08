use anyhow::{Context, Result};
use ffmpeg_next as ffmpeg;
use ffmpeg_next::{
    codec, encoder, format, frame, software::scaling, util::rational::Rational,
    Dictionary,
};

pub struct Recorder {
    octx: format::context::Output,
    encoder: encoder::Video,
    scaler: scaling::Context,
    stream_index: usize,
    pts: i64,
    width: u32,
    height: u32,
}

impl Recorder {
    pub fn new(path: &str, width: u32, height: u32, fps: u32) -> Result<Self> {
        ffmpeg::init().context("ffmpeg init")?;

        let mut octx = format::output(path).context("open output file")?;

        let codec = encoder::find(codec::Id::H264).context("find H.264 encoder")?;
        let mut ost = octx.add_stream(codec).context("add video stream")?;
        let stream_index = ost.index();

        let mut enc = codec::Context::new_with_codec(codec)
            .encoder()
            .video()
            .context("create encoder")?;

        enc.set_width(width);
        enc.set_height(height);
        enc.set_format(ffmpeg_next::util::format::Pixel::YUV420P);
        enc.set_time_base(Rational::new(1, fps as i32));
        enc.set_frame_rate(Some(Rational::new(fps as i32, 1)));
        enc.set_bit_rate(8_000_000); // 8 Mbps default

        let mut opts = Dictionary::new();
        opts.set("preset", "ultrafast");
        opts.set("crf", "18");

        let encoder = enc.open_with(opts).context("open H.264 encoder")?;
        ost.set_parameters(&encoder);

        format::context::output::dump(&octx, 0, Some(path));
        octx.write_header().context("write file header")?;

        let scaler = scaling::Context::get(
            ffmpeg_next::util::format::Pixel::RGBA,
            width,
            height,
            ffmpeg_next::util::format::Pixel::YUV420P,
            width,
            height,
            scaling::Flags::BILINEAR,
        )
        .context("create scaler")?;

        Ok(Self {
            octx,
            encoder,
            scaler,
            stream_index,
            pts: 0,
            width,
            height,
        })
    }

    /// Push a raw RGBA frame into the encoder.
    pub fn push_frame(&mut self, rgba: &[u8], _size: (u32, u32)) {
        let mut src_frame = frame::Video::new(
            ffmpeg_next::util::format::Pixel::RGBA,
            self.width,
            self.height,
        );
        src_frame.data_mut(0).copy_from_slice(rgba);

        let mut dst_frame = frame::Video::new(
            ffmpeg_next::util::format::Pixel::YUV420P,
            self.width,
            self.height,
        );

        if self.scaler.run(&src_frame, &mut dst_frame).is_err() {
            return;
        }

        dst_frame.set_pts(Some(self.pts));
        self.pts += 1;

        if self.encoder.send_frame(&dst_frame).is_err() {
            return;
        }

        let mut packet = ffmpeg_next::Packet::empty();
        while self.encoder.receive_packet(&mut packet).is_ok() {
            packet.set_stream(self.stream_index);
            packet.rescale_ts(
                Rational::new(1, 60),
                self.octx.stream(self.stream_index).unwrap().time_base(),
            );
            packet.write_interleaved(&mut self.octx).ok();
        }
    }

    /// Flush encoder and write file trailer.
    pub fn finish(mut self) -> Result<()> {
        self.encoder.send_eof().ok();
        let mut packet = ffmpeg_next::Packet::empty();
        while self.encoder.receive_packet(&mut packet).is_ok() {
            packet.set_stream(self.stream_index);
            packet.write_interleaved(&mut self.octx).ok();
        }
        self.octx.write_trailer().context("write file trailer")?;
        log::info!("Recording finalised");
        Ok(())
    }
}
