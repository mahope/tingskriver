use super::{select_backend, TrimConfig, TrimmedAudio};
use anyhow::{Context, Result};
use std::{collections::VecDeque, fs::File, path::Path};
use symphonia::core::{
    audio::SampleBuffer,
    codecs::{Decoder, DecoderOptions, CODEC_TYPE_NULL},
    errors::Error,
    formats::{FormatOptions, FormatReader},
    io::MediaSourceStream,
    meta::MetadataOptions,
    probe::Hint,
};

/// Stream a container file in decoder packets. Input must decode to 16 kHz mono;
/// other rates/channels are rejected, never silently interpreted on a wrong clock.
/// Uses two passes; only selected speech and one decoder packet stay in memory.
pub fn trim_file(path: &Path, cfg: &TrimConfig) -> Result<TrimmedAudio> {
    cfg.validate()?;
    let backend = select_backend(cfg.backend, || {
        crate::ensure_onnx_runtime_available().is_ok()
    });
    let open = || AudioSamples::open(path);
    let (ranges, backend) = super::trim::detect_with_fallback(open, cfg, backend)?;
    super::trim::assemble(open()?, ranges, backend)
}
struct AudioSamples {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn Decoder>,
    track_id: u32,
    samples: VecDeque<f32>,
    done: bool,
}
impl AudioSamples {
    #[allow(clippy::disallowed_methods)] // Synchronous disk worker.
    fn open(path: &Path) -> Result<Self> {
        let file = File::open(path).context("Cannot open VAD audio input")?;
        let stream = MediaSourceStream::new(Box::new(file), Default::default());
        let mut hint = Hint::new();
        if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
            hint.with_extension(ext);
        }
        let format = symphonia::default::get_probe()
            .format(
                &hint,
                stream,
                &FormatOptions::default(),
                &MetadataOptions::default(),
            )
            .map_err(|_| anyhow::anyhow!("Cannot probe VAD audio input"))?
            .format;
        let track = format
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
            .context("VAD audio input has no audio track")?;
        let decoder = symphonia::default::get_codecs()
            .make(&track.codec_params, &DecoderOptions::default())
            .map_err(|_| anyhow::anyhow!("Cannot initialize VAD audio decoder"))?;
        let track_id = track.id;
        Ok(Self {
            format,
            decoder,
            track_id,
            samples: VecDeque::new(),
            done: false,
        })
    }
    fn fill(&mut self) -> Result<bool> {
        loop {
            let packet = match self.format.next_packet() {
                Ok(packet) => packet,
                Err(Error::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                    return Ok(false)
                }
                Err(_) => anyhow::bail!("Cannot read VAD audio packet"),
            };
            if packet.track_id() != self.track_id {
                continue;
            }
            let decoded = self
                .decoder
                .decode(&packet)
                .map_err(|_| anyhow::anyhow!("Cannot decode VAD audio packet"))?;
            anyhow::ensure!(
                decoded.spec().rate == 16000 && decoded.spec().channels.count() == 1,
                "VAD file input must decode to 16 kHz mono"
            );
            let mut buffer = SampleBuffer::<f32>::new(decoded.capacity() as u64, *decoded.spec());
            buffer.copy_interleaved_ref(decoded);
            self.samples.extend(buffer.samples());
            if !self.samples.is_empty() {
                return Ok(true);
            }
        }
    }
}
impl Iterator for AudioSamples {
    type Item = Result<f32>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        if self.samples.is_empty() {
            match self.fill() {
                Ok(true) => {}
                Ok(false) => {
                    self.done = true;
                    return None;
                }
                Err(error) => {
                    self.done = true;
                    return Some(Err(error));
                }
            }
        }
        self.samples.pop_front().map(Ok)
    }
}
