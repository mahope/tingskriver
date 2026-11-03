//! Evaluation adapters call the existing model APIs, not microphone capture,
//! VAD, chunking or polishing. Full-chain VAD experiments remain manual.
use super::{
    manifest::{relative_file, Clip, Manifest},
    report::Report,
};
use anyhow::{bail, Context, Result};
use std::{
    future::Future,
    path::{Path, PathBuf},
    pin::Pin,
};

pub type Transcription<'a> = Pin<Box<dyn Future<Output = Result<String>> + Send + 'a>>;

pub trait Engine: Send + Sync {
    fn name(&self) -> String;
    fn transcribe<'a>(&'a self, clip: &'a Clip, root: &'a Path) -> Transcription<'a>;
}

pub struct FixtureEngine {
    pub hypotheses: PathBuf,
}

impl Engine for FixtureEngine {
    fn name(&self) -> String {
        "fixture".into()
    }
    fn transcribe<'a>(&'a self, clip: &'a Clip, _root: &'a Path) -> Transcription<'a> {
        Box::pin(async move {
            let path = relative_file(&self.hypotheses, &format!("{}.txt", clip.id))?;
            std::fs::read_to_string(path).context("Cannot read fixture hypothesis")
        })
    }
}

pub enum ModelEngine {
    Parakeet(crate::parakeet_engine::ParakeetEngine, String),
    Whisper(crate::whisper_engine::WhisperEngine, String),
}

impl ModelEngine {
    /// Loads installed models only. Never downloads models.
    pub async fn load(engine: &str, models: PathBuf, model: String) -> Result<Self> {
        match engine {
            "parakeet" => {
                let engine =
                    crate::parakeet_engine::ParakeetEngine::new_with_models_dir(Some(models))
                        .map_err(|_| anyhow::anyhow!("Cannot initialize Parakeet"))?;
                engine
                    .load_model(&model)
                    .await
                    .map_err(|_| anyhow::anyhow!("Cannot load installed Parakeet model"))?;
                Ok(Self::Parakeet(engine, model))
            }
            "whisper" => {
                let engine =
                    crate::whisper_engine::WhisperEngine::new_with_models_dir(Some(models))
                        .map_err(|_| anyhow::anyhow!("Cannot initialize Whisper"))?;
                engine
                    .load_model(&model)
                    .await
                    .map_err(|_| anyhow::anyhow!("Cannot load installed Whisper model"))?;
                Ok(Self::Whisper(engine, model))
            }
            _ => bail!("Unknown model engine"),
        }
    }
}

impl Engine for ModelEngine {
    fn name(&self) -> String {
        match self {
            Self::Parakeet(_, model) => format!("parakeet:{model}"),
            Self::Whisper(_, model) => format!("whisper:{model}:da"),
        }
    }
    fn transcribe<'a>(&'a self, clip: &'a Clip, root: &'a Path) -> Transcription<'a> {
        Box::pin(async move {
            let samples = read_pcm16(&relative_file(root, &clip.file)?)?;
            let result = match self {
                Self::Parakeet(engine, _) => engine.transcribe_audio(samples).await,
                Self::Whisper(engine, _) => engine
                    .transcribe_audio_with_confidence(samples, Some("da".into()))
                    .await
                    .map(|(text, _, _)| text),
            };
            result.map_err(|_| anyhow::anyhow!("Model transcription failed"))
        })
    }
}

/// Strict RIFF/WAVE PCM16, mono 16 kHz, 0 < duration <= 8 s. No resampling.
pub fn read_pcm16(path: &Path) -> Result<Vec<f32>> {
    read_pcm16_with_limit(path, 8)
}

pub(crate) fn read_pcm16_with_limit(path: &Path, seconds: u64) -> Result<Vec<f32>> {
    let max_data = seconds * 32_000;
    if std::fs::metadata(path).context("Cannot inspect WAV")?.len() > max_data + 44_000 {
        bail!("WAV exceeds short-clip size limit");
    }
    let bytes = std::fs::read(path).context("Cannot read WAV")?;
    if bytes.len() < 12 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        bail!("Expected RIFF/WAVE PCM16");
    }
    let u16_at = |p| u16::from_le_bytes([bytes[p], bytes[p + 1]]);
    let u32_at = |p| u32::from_le_bytes([bytes[p], bytes[p + 1], bytes[p + 2], bytes[p + 3]]);
    if u32_at(4) as usize + 8 != bytes.len() {
        bail!("Invalid RIFF size");
    }
    let mut position = 12;
    let mut format = false;
    let mut data = None;
    while position + 8 <= bytes.len() {
        let length = u32_at(position + 4) as usize;
        let start = position + 8;
        let end = start.checked_add(length).context("Invalid WAV size")?;
        if end > bytes.len() {
            bail!("Truncated WAV chunk");
        }
        match &bytes[position..position + 4] {
            b"fmt " => {
                if format
                    || length < 16
                    || u16_at(start) != 1
                    || u16_at(start + 2) != 1
                    || u32_at(start + 4) != 16_000
                    || u32_at(start + 8) != 32_000
                    || u16_at(start + 12) != 2
                    || u16_at(start + 14) != 16
                {
                    bail!("Expected mono 16 kHz PCM16 WAV");
                }
                format = true;
            }
            b"data" => {
                if data.is_some() {
                    bail!("Duplicate WAV data chunk");
                }
                data = Some(&bytes[start..end]);
            }
            _ => {}
        }
        position = end + length % 2;
    }
    let data = data.context("Missing WAV data")?;
    if !format
        || position != bytes.len()
        || data.is_empty()
        || data.len() % 2 != 0
        || data.len() as u64 > max_data
    {
        bail!("Invalid WAV or clip exceeds duration limit");
    }
    Ok(data
        .chunks_exact(2)
        .map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0)
        .collect())
}

pub async fn run(
    manifest: &Manifest,
    root: &Path,
    engine: &dyn Engine,
    git_sha: String,
) -> Result<Report> {
    if (engine.name() == "fixture") != (manifest.kind == "fixture-text") {
        bail!("Engine and dataset kind do not match");
    }
    // Validate all audio before loading hypotheses or producing a partial report.
    if manifest.kind == "audio" {
        for clip in &manifest.clips {
            read_pcm16(&relative_file(root, &clip.file)?)?;
        }
    }
    let mut report = Report::new(engine.name(), manifest.kind.clone(), git_sha);
    for clip in &manifest.clips {
        let hypothesis = engine.transcribe(clip, root).await?;
        report.push(clip.id.clone(), clip.reference.clone(), hypothesis);
    }
    Ok(report)
}
