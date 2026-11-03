use super::PlaybackError;
use std::{fs::File, path::Path, time::Duration};
use symphonia::core::{
    audio::SampleBuffer,
    codecs::{Decoder, DecoderOptions},
    formats::{FormatOptions, FormatReader, SeekMode, SeekTo},
    io::MediaSourceStream,
    meta::MetadataOptions,
    probe::Hint,
    units::Time,
};

pub(super) struct Track {
    path: std::path::PathBuf,
    format: Box<dyn FormatReader>,
    decoder: Box<dyn Decoder>,
    id: u32,
    pub rate: u32,
    samples: Vec<f32>,
    cursor: usize,
    frame: u64,
    offset: f64,
}
impl Track {
    #[allow(clippy::disallowed_methods)] // Synchronous disk worker.
    pub fn open(path: &Path) -> Result<Self, PlaybackError> {
        let file = File::open(path).map_err(|_| PlaybackError::Open)?;
        let mut hint = Hint::new();
        if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
            hint.with_extension(ext);
        }
        let format = symphonia::default::get_probe()
            .format(
                &hint,
                MediaSourceStream::new(Box::new(file), Default::default()),
                &FormatOptions {
                    enable_gapless: true,
                    ..Default::default()
                },
                &MetadataOptions::default(),
            )
            .map_err(|_| PlaybackError::Unsupported)?
            .format;
        let track = format.default_track().ok_or(PlaybackError::Unsupported)?;
        let rate = track
            .codec_params
            .sample_rate
            .ok_or(PlaybackError::Unsupported)?;
        if rate == 0 {
            return Err(PlaybackError::Unsupported);
        }
        let decoder = symphonia::default::get_codecs()
            .make(&track.codec_params, &DecoderOptions::default())
            .map_err(|_| PlaybackError::Unsupported)?;
        let edit = super::mp4::media_offset(path, track.id)?;
        let offset = if edit == 0 {
            0.0
        } else {
            let t = track
                .codec_params
                .time_base
                .ok_or(PlaybackError::Unsupported)?
                .calc_time(edit);
            t.seconds as f64 + t.frac
        };
        Ok(Self {
            offset,
            path: path.into(),
            id: track.id,
            rate,
            format,
            decoder,
            samples: Vec::new(),
            cursor: 0,
            frame: 0,
        })
    }
    fn packet(&mut self) -> Result<bool, PlaybackError> {
        loop {
            let packet = match self.format.next_packet() {
                Ok(p) => p,
                Err(symphonia::core::errors::Error::IoError(e))
                    if e.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    return Ok(false)
                }
                Err(_) => return Err(PlaybackError::Decode),
            };
            if packet.track_id() != self.id {
                continue;
            }
            let decoded = self
                .decoder
                .decode(&packet)
                .map_err(|_| PlaybackError::Decode)?;
            let channels = decoded.spec().channels.count();
            if channels == 0 || decoded.spec().rate != self.rate {
                return Err(PlaybackError::Unsupported);
            }
            let mut buf = SampleBuffer::<f32>::new(decoded.capacity() as u64, *decoded.spec());
            buf.copy_interleaved_ref(decoded);
            self.samples.clear();
            self.samples.extend(
                buf.samples()
                    .chunks_exact(channels)
                    .map(|c| c.iter().sum::<f32>() / channels as f32),
            );
            self.cursor = 0;
            // Packet timestamps use the stream timebase, not necessarily samples.
            let tb = self
                .format
                .tracks()
                .iter()
                .find(|t| t.id == self.id)
                .and_then(|t| t.codec_params.time_base)
                .ok_or(PlaybackError::Unsupported)?;
            let t = tb.calc_time(packet.ts());
            self.frame = ((t.seconds as f64 + t.frac) * self.rate as f64).round() as u64;
            if !self.samples.is_empty() {
                return Ok(true);
            }
        }
    }
    pub fn seeked(&self, time: Duration) -> Result<Self, PlaybackError> {
        let mut track = Self::open(&self.path)?;
        track.seek(time)?;
        Ok(track)
    }
    fn seek(&mut self, time: Duration) -> Result<(), PlaybackError> {
        let seconds = time.as_secs_f64() + self.offset;
        // Warm transform codecs and MP3's bit reservoir before the requested frame.
        let time = Duration::try_from_secs_f64(seconds)
            .map_err(|_| PlaybackError::Seek)?
            .saturating_sub(Duration::from_millis(250));
        if self
            .format
            .seek(
                SeekMode::Accurate,
                SeekTo::Time {
                    time: Time::new(time.as_secs(), time.subsec_nanos() as f64 / 1e9),
                    track_id: Some(self.id),
                },
            )
            .is_err()
        {
            // Indexless streams are reopened and decoded forward with bounded memory.
            *self = Self::open(&self.path)?;
            return Ok(());
        }
        self.decoder.reset();
        self.samples.clear();
        self.cursor = 0;
        self.frame = 0;
        Ok(())
    }
    pub fn at(&mut self, seconds: f64) -> Result<Option<f32>, PlaybackError> {
        let target = ((seconds + self.offset) * self.rate as f64 + 1e-7).floor() as u64;
        loop {
            if self.cursor == self.samples.len() && !self.packet()? {
                return Ok(None);
            }
            let current = self.frame + self.cursor as u64;
            if current > target {
                return Ok(Some(0.0));
            }
            let skip =
                (target - current).min((self.samples.len() - self.cursor - 1) as u64) as usize;
            self.cursor += skip;
            if self.frame + self.cursor as u64 == target {
                return Ok(Some(self.samples[self.cursor]));
            }
            self.cursor = self.samples.len();
        }
    }
}
