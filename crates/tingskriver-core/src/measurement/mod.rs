//! Explicit, independent model comparisons. Never uses the fallback chain.
use crate::{
    providers::{
        self,
        model::{Capability, ChainLink, Function, ProviderKind},
        ProviderService, ProvidersConfig,
    },
    wer::{self, runner::ModelEngine, Score},
};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    time::Instant,
};

pub mod bench;

const MAX_SECONDS: usize = 90;

pub struct Sample {
    samples: Vec<f32>,
    wav: Vec<u8>,
}

impl Sample {
    /// Import a bounded mono 16 kHz PCM16 WAV. No decoder downloads or subprocesses.
    pub fn load(path: &Path) -> Result<Self, String> {
        let samples =
            wer::runner::read_pcm16_with_limit(path, MAX_SECONDS as u64).map_err(|_| {
                "Importér en WAV-fil med mono, 16 kHz, PCM16 og højst 90 sekunder.".to_string()
            })?;
        Self::from_samples(samples)
    }

    pub fn from_samples(samples: Vec<f32>) -> Result<Self, String> {
        if samples.is_empty()
            || samples.len() > MAX_SECONDS * 16_000
            || samples.iter().any(|s| !s.is_finite())
        {
            return Err("Prøven skal indeholde 0–90 sekunders gyldig lyd ved 16 kHz.".into());
        }
        let wav = crate::dictation::logic::audio::wav_bytes(&samples, 16_000);
        // Local models receive exactly the PCM values sent to remote models.
        let samples = wav[44..]
            .chunks_exact(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0)
            .collect();
        Ok(Self { samples, wav })
    }

    pub fn duration_seconds(&self) -> f64 {
        self.samples.len() as f64 / 16_000.0
    }
}

/// User-entered tariff per audio hour; not a live vendor price or electricity estimate.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Rate {
    pub per_hour: f64,
    pub currency: String,
}
impl Rate {
    pub fn new(per_hour: f64, currency: &str) -> Result<Self, String> {
        let rate = Self {
            per_hour,
            currency: currency.into(),
        };
        rate.validate()?;
        Ok(rate)
    }
    fn validate(&self) -> Result<(), String> {
        if !self.per_hour.is_finite()
            || self.per_hour < 0.0
            || self.currency.len() != 3
            || !self.currency.bytes().all(|b| b.is_ascii_uppercase())
        {
            return Err("Angiv en ikke-negativ timepris og en valutakode som DKK.".into());
        }
        Ok(())
    }
    pub fn sample_cost(&self, seconds: f64) -> f64 {
        self.per_hour * (seconds / 3600.0)
    }
}

#[derive(Clone, Debug)]
pub struct Selection {
    pub provider: String,
    pub model: String,
    pub rate: Option<Rate>,
}
impl Selection {
    pub fn new(provider: &str, model: &str) -> Self {
        Self {
            provider: provider.into(),
            model: model.into(),
            rate: None,
        }
    }
}

/// Default models and every transcription/dictation override, with no implicit selection.
pub fn candidates(config: &ProvidersConfig) -> Vec<Selection> {
    let mut result = Vec::new();
    for provider in &config.providers {
        if let Some(model) = provider.default_model(Capability::Transcription) {
            result.push(Selection::new(&provider.id, model));
        }
    }
    for function in [Function::Transcription, Function::Dictation] {
        for link in config.chain(function) {
            if config.provider(&link.provider).is_none() {
                continue;
            }
            if let Some(model) = link.model.as_ref().filter(|m| !m.trim().is_empty()) {
                if !result
                    .iter()
                    .any(|s| s.provider == link.provider && s.model == *model)
                {
                    result.push(Selection::new(&link.provider, model));
                }
            }
        }
    }
    // Offer both installed local engines even if only one is in the chain.
    if config.provider(providers::model::LOCAL_ID).is_some() {
        for model in [
            crate::config::DEFAULT_PARAKEET_MODEL,
            crate::config::DEFAULT_WHISPER_MODEL,
        ] {
            if !result
                .iter()
                .any(|s| s.provider == providers::model::LOCAL_ID && s.model == model)
            {
                result.push(Selection::new(providers::model::LOCAL_ID, model));
            }
        }
    }
    result
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Status {
    Completed,
    Skipped,
    Failed,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Row {
    pub provider: String,
    pub model: String,
    pub status: Status,
    pub message: String,
    pub score: Option<Score>,
    pub wer: Option<f64>,
    pub latency_ms: Option<u64>,
    pub rate: Option<Rate>,
    pub sample_cost: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Report {
    pub schema_version: u32,
    pub id: String,
    pub created_at: String,
    pub normalization: String,
    pub duration_seconds: f64,
    pub results: Vec<Row>,
}
impl Report {
    /// Persist metrics only: no audio, reference, hypothesis, URLs or keys.
    #[allow(clippy::disallowed_methods)]
    pub fn save(&self, app_data: &Path) -> Result<PathBuf, String> {
        uuid::Uuid::parse_str(&self.id).map_err(|_| "Ugyldigt resultat-id.".to_string())?;
        let dir = app_data.join("measurements");
        std::fs::create_dir_all(&dir)
            .map_err(|_| "Resultatmappen kunne ikke oprettes.".to_string())?;
        let path = dir.join(format!("{}.json", self.id));
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|_| "Resultatet kunne ikke gemmes.".to_string())?;
        serde_json::to_writer_pretty(&mut file, self)
            .map_err(|_| "Resultatet kunne ikke skrives.".to_string())?;
        file.sync_all()
            .map_err(|_| "Resultatet kunne ikke gemmes på disken.".to_string())?;
        Ok(path)
    }
}

pub async fn run(
    config: &ProvidersConfig,
    service: &ProviderService,
    selected: &[Selection],
    sample: &Sample,
    reference: &str,
    models_dir: &Path,
) -> Result<Report, String> {
    if selected.is_empty() {
        return Err("Vælg mindst én udbyder og model.".into());
    }
    if wer::normalize(reference).is_empty() || reference.chars().count() > 20_000 {
        return Err("Skriv facit med 1–20.000 tegn og mindst ét ord.".into());
    }
    // Validate the complete plan before allowing the first upload.
    let mut plan = config.clone();
    let mut links = Vec::new();
    for selection in selected {
        if let Some(rate) = &selection.rate {
            rate.validate()?;
        }
        if selection.model.trim().is_empty() {
            return Err("Vælg en model.".into());
        }
        let link = ChainLink::new(&selection.provider).with_model(&selection.model);
        config.validate_link(Function::Transcription, &link)?;
        let provider = config
            .provider(&selection.provider)
            .ok_or("Udbyderen findes ikke.")?;
        providers::model::validate_provider(provider)?;
        links.push(link);
    }
    plan.functions.insert(Function::Transcription, links);
    let links = service.resolve(&plan, Function::Transcription);
    let mut report = Report {
        schema_version: 1,
        id: uuid::Uuid::new_v4().to_string(),
        created_at: chrono::Utc::now().to_rfc3339(),
        normalization: "unicode-alphanumeric-lower-v1".into(),
        duration_seconds: sample.duration_seconds(),
        results: Vec::new(),
    };
    // A selected endpoint must not redirect the sample to an unselected host.
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "Netværksklienten kunne ikke oprettes.".to_string())?;
    let request = providers::TranscriptionRequest {
        wav: sample.wav.clone(),
        file_name: "proeve.wav".into(),
        language: Some("da".into()),
        prompt: None,
        diarize: false,
        timestamps: false,
    };
    // Sequential calls avoid competing local models and retain only one loaded engine.
    for (link, selection) in links.into_iter().zip(selected) {
        let mut row = Row {
            provider: link.provider_id.clone(),
            model: link.model.clone(),
            status: Status::Skipped,
            message: String::new(),
            score: None,
            wer: None,
            latency_ms: None,
            rate: selection.rate.clone(),
            sample_cost: selection
                .rate
                .as_ref()
                .map(|r| r.sample_cost(sample.duration_seconds())),
        };
        if link.kind.requires_key() && link.key.is_none() {
            row.message = "Sprunget over: API-nøgle mangler.".into();
        } else if link.kind == ProviderKind::Local {
            let engine = bench::engine_for(&link.model);
            // Loading is excluded from inference latency and never downloads models.
            match ModelEngine::load(engine, models_dir.to_path_buf(), link.model.clone()).await {
                Err(_) => {
                    row.message =
                        "Sprunget over: den lokale model kunne ikke indlæses. Installér den først."
                            .into()
                }
                Ok(engine) => {
                    let start = Instant::now();
                    let result = match &engine {
                        ModelEngine::Parakeet(engine, _) => {
                            engine.transcribe_audio(sample.samples.clone()).await
                        }
                        ModelEngine::Whisper(engine, _) => engine
                            .transcribe_audio_with_confidence(
                                sample.samples.clone(),
                                Some("da".into()),
                            )
                            .await
                            .map(|(text, _, _)| text),
                    };
                    finish(&mut row, reference, result.map_err(|_| ()), start);
                }
            }
        } else {
            let start = Instant::now();
            let result = providers::client::transcribe(&client, &link, &request).await;
            finish(
                &mut row,
                reference,
                result.map(|t| t.text).map_err(|_| ()),
                start,
            );
        }
        report.results.push(row);
    }
    Ok(report)
}

fn finish(row: &mut Row, reference: &str, result: Result<String, ()>, start: Instant) {
    row.latency_ms = Some(start.elapsed().as_millis().min(u64::MAX as u128) as u64);
    match result {
        Ok(text) => {
            let score = wer::score(reference, &text);
            row.wer = score.wer();
            row.score = Some(score);
            row.status = Status::Completed;
            row.message = "Målt".into();
        }
        Err(()) => {
            row.status = Status::Failed;
            row.message = "Målingen fejlede. Kontrollér forbindelsen og modellen; ingen reserveudbyder blev kaldt.".into();
        }
    }
}

#[cfg(test)]
#[allow(clippy::disallowed_methods)]
mod tests;
