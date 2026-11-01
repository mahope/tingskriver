use super::{select_backend, Backend, TrimConfig};
use crate::audio::vad::ContinuousVadProcessor;
use anyhow::{Context, Result};
use std::{
    fs::File,
    io::{BufReader, Read},
    path::Path,
    time::Duration,
};

const FRAME: usize = 480;
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrimSegment {
    /// Inclusive original sample position.
    pub orig_start: usize,
    /// Exclusive original sample position.
    pub orig_end: usize,
    pub trimmed_start: usize,
}
#[derive(Debug, Clone)]
pub struct TrimmedAudio {
    pub samples: Vec<f32>,
    pub segments: Vec<TrimSegment>,
    pub backend: Backend,
}
impl TrimmedAudio {
    /// Right-continuous at joins: a word starting at a join belongs to the next
    /// segment. Out-of-range times are clamped; empty audio maps to zero.
    pub fn to_original_ms(&self, trimmed_ms: f64) -> f64 {
        self.map_ms(trimmed_ms, false)
    }
    /// Endpoints at a join belong to the preceding segment (exclusive end).
    pub fn end_to_original_ms(&self, trimmed_ms: f64) -> f64 {
        self.map_ms(trimmed_ms, true)
    }
    fn map_ms(&self, ms: f64, end: bool) -> f64 {
        let sample = if ms.is_nan() {
            0.0
        } else {
            (ms * 16.0).clamp(0.0, self.samples.len() as f64)
        };
        let found = self.segments.iter().rev().find(|s| {
            if end && sample > 0.0 {
                (s.trimmed_start as f64) < sample
            } else {
                s.trimmed_start as f64 <= sample
            }
        });
        found.map_or(0.0, |s| {
            (s.orig_start as f64
                + (sample - s.trimmed_start as f64).min((s.orig_end - s.orig_start) as f64))
                / 16.0
        })
    }
}

pub fn trim(samples_16k_mono: &[f32], cfg: &TrimConfig) -> Result<TrimmedAudio> {
    trim_with_runtime_probe(samples_16k_mono, cfg, || {
        crate::ensure_onnx_runtime_available().is_ok()
    })
}

pub fn trim_with_runtime_probe(
    samples: &[f32],
    cfg: &TrimConfig,
    probe: impl FnOnce() -> bool,
) -> Result<TrimmedAudio> {
    cfg.validate()?;
    let backend = select_backend(cfg.backend, probe);
    let (ranges, backend) =
        detect_with_fallback(|| Ok(samples.iter().copied().map(Ok)), cfg, backend)?;
    assemble(samples.iter().copied().map(Ok), ranges, backend)
}

/// Two streaming passes over little-endian f32 PCM, 16 kHz mono. The input is
/// never collected; memory holds selected speech plus the segment index only.
/// Container decoding/resampling belongs to the caller, not the capture loop.
pub fn trim_pcm_file(path: &Path, cfg: &TrimConfig) -> Result<TrimmedAudio> {
    cfg.validate()?;
    let open = || -> Result<PcmSamples> {
        let file = File::open(path).context("Cannot open VAD PCM input")?;
        anyhow::ensure!(file.metadata()?.len() % 4 == 0, "Truncated f32 PCM input");
        Ok(PcmSamples(BufReader::new(file)))
    };
    let backend = select_backend(cfg.backend, || {
        crate::ensure_onnx_runtime_available().is_ok()
    });
    let (ranges, backend) = detect_with_fallback(open, cfg, backend)?;
    assemble(open()?, ranges, backend)
}
struct PcmSamples(BufReader<File>);
impl Iterator for PcmSamples {
    type Item = Result<f32>;
    fn next(&mut self) -> Option<Self::Item> {
        let mut bytes = [0; 4];
        match self.0.read(&mut bytes[..1]) {
            Ok(0) => None,
            Ok(_) => Some(
                self.0
                    .read_exact(&mut bytes[1..])
                    .context("Cannot read VAD PCM input")
                    .map(|()| f32::from_le_bytes(bytes)),
            ),
            Err(e) => Some(Err(e).context("Cannot read VAD PCM input")),
        }
    }
}
pub(super) fn detect_with_fallback<I: Iterator<Item = Result<f32>>>(
    open: impl Fn() -> Result<I>,
    cfg: &TrimConfig,
    backend: Backend,
) -> Result<(Vec<(usize, usize)>, Backend)> {
    // Startup errors are recoverable before input is consumed. Never hide I/O or
    // invalid-sample errors by retrying them as another backend.
    let processor = make_processor(backend, cfg);
    let selected = if processor.is_some() {
        Backend::Silero
    } else {
        Backend::Energy
    };
    let ranges = detect(open()?, cfg, processor)?;
    Ok((ranges, selected))
}
pub(super) fn make_processor(backend: Backend, cfg: &TrimConfig) -> Option<ContinuousVadProcessor> {
    if backend == Backend::Silero {
        match silero(cfg) {
            Ok(p) => Some(p),
            Err(_) => {
                log::warn!("VAD backend: energy (Silero session startup failed)");
                None
            }
        }
    } else {
        None
    }
}
pub(super) fn silero(cfg: &TrimConfig) -> Result<ContinuousVadProcessor> {
    let config = silero_rs::VadConfig {
        sample_rate: 16000,
        positive_speech_threshold: cfg.threshold,
        negative_speech_threshold: (cfg.threshold - 0.15).max(0.0),
        min_speech_time: Duration::from_millis(cfg.min_speech_ms.into()),
        redemption_time: Duration::from_millis(cfg.min_silence_ms.into()),
        pre_speech_pad: Duration::ZERO,
        post_speech_pad: Duration::ZERO,
    };
    ContinuousVadProcessor::with_config(16000, config)
}
pub(super) fn detect(
    input: impl Iterator<Item = Result<f32>>,
    cfg: &TrimConfig,
    mut silero: Option<ContinuousVadProcessor>,
) -> Result<Vec<(usize, usize)>> {
    let mut energy = super::energy::EnergyVad::new();
    let mut ranges = Vec::new();
    let mut frame = Vec::with_capacity(FRAME);
    let mut total = 0;
    let mut start = None;
    let mut last_voice = 0;
    let mut voiced = 0;
    let mut consume = |frame: &[f32], total: usize| -> Result<()> {
        if let Some(processor) = silero.as_mut() {
            for s in processor.process_audio(frame)? {
                ranges.push((
                    (s.start_timestamp_ms * 16.0).round() as usize,
                    (s.end_timestamp_ms * 16.0).round() as usize,
                ));
            }
        } else if energy.speech(frame, cfg.threshold) {
            start.get_or_insert(total - frame.len());
            voiced += frame.len();
            last_voice = total;
        } else if start.is_some() && total - last_voice >= cfg.min_silence_ms as usize * 16 {
            if voiced >= cfg.min_speech_ms as usize * 16 {
                if let Some(s) = start {
                    ranges.push((s, last_voice));
                }
            }
            start = None;
            voiced = 0;
        }
        Ok(())
    };
    for sample in input {
        let sample = sample?;
        anyhow::ensure!(sample.is_finite(), "VAD input contains non-finite samples");
        frame.push(sample);
        total += 1;
        if frame.len() == FRAME {
            consume(&frame, total)?;
            frame.clear();
        }
    }
    if !frame.is_empty() {
        consume(&frame, total)?;
    }
    if let Some(processor) = silero.as_mut() {
        for s in processor.flush()? {
            ranges.push((
                (s.start_timestamp_ms * 16.0).round() as usize,
                (s.end_timestamp_ms * 16.0).round() as usize,
            ));
        }
    } else if voiced >= cfg.min_speech_ms as usize * 16 {
        if let Some(s) = start {
            ranges.push((s, last_voice));
        }
    }
    let pad = cfg.speech_pad_ms as usize * 16;
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (s, e) in ranges {
        let s = s.saturating_sub(pad).min(total);
        let e = e.saturating_add(pad).min(total);
        if e <= s {
            continue;
        }
        if let Some(last) = merged.last_mut() {
            if s <= last.1 {
                last.1 = last.1.max(e);
                continue;
            }
        }
        merged.push((s, e));
    }
    Ok(merged)
}
pub(super) fn assemble(
    input: impl Iterator<Item = Result<f32>>,
    ranges: Vec<(usize, usize)>,
    backend: Backend,
) -> Result<TrimmedAudio> {
    let mut result = TrimmedAudio {
        samples: Vec::new(),
        segments: Vec::new(),
        backend,
    };
    for &(start, end) in &ranges {
        let trimmed_start = result.segments.last().map_or(0, |s: &TrimSegment| {
            s.trimmed_start + s.orig_end - s.orig_start
        });
        result.segments.push(TrimSegment {
            orig_start: start,
            orig_end: end,
            trimmed_start,
        });
    }
    let mut range = 0;
    for (index, sample) in input.enumerate() {
        let sample = sample?;
        while range < ranges.len() && index >= ranges[range].1 {
            range += 1;
        }
        if range == ranges.len() {
            break;
        }
        if index >= ranges[range].0 {
            result.samples.push(sample);
        }
    }
    let expected: usize = ranges.iter().map(|(s, e)| e - s).sum();
    anyhow::ensure!(
        result.samples.len() == expected,
        "VAD input changed between passes"
    );
    Ok(result)
}
