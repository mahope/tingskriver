use std::{future::Future, path::Path, sync::Arc};

use anyhow::Result;
use serde_json::json;

use crate::providers::{
    chain, client::TranscriptSegment, Function, ProviderError, ProviderService, ResolvedLink,
    Transcription, TranscriptionRequest,
};
use crate::transcript_merge::{merge, MergeConfig, Segment};

use super::{Job, Queue, QueueEvent};

impl Queue {
    /// Execute at most one due job. The host polls this method, and forwards
    /// events without opening dialogs. A cancelled future leaves a running job
    /// for startup recovery. Keys are resolved at execution time, never saved.
    pub async fn run_once<L, Fut>(
        &self,
        service: &ProviderService,
        default_config: &crate::providers::ProvidersConfig,
        local: L,
        emit: impl FnMut(QueueEvent),
    ) -> Result<bool>
    where
        L: FnMut(ResolvedLink, TranscriptionRequest) -> Fut,
        Fut: Future<Output = Result<Transcription, ProviderError>>,
    {
        self.run_once_at(
            service,
            default_config,
            local,
            emit,
            chrono::Utc::now().timestamp(),
        )
        .await
    }

    pub(super) async fn run_once_at<L, Fut>(
        &self,
        service: &ProviderService,
        default_config: &crate::providers::ProvidersConfig,
        mut local: L,
        mut emit: impl FnMut(QueueEvent),
        now: i64,
    ) -> Result<bool>
    where
        L: FnMut(ResolvedLink, TranscriptionRequest) -> Fut,
        Fut: Future<Output = Result<Transcription, ProviderError>>,
    {
        let Some(job) = self.claim(now).await? else {
            return Ok(false);
        };
        let started = std::time::Instant::now();
        emit(event(&job, "running", None, None));
        let result = execute(
            self,
            &job,
            service,
            default_config,
            &mut local,
            &mut emit,
            now,
        )
        .await;
        match result {
            Ok(value) => {
                self.complete(&job, &value).await?;
                emit(event(&job, "completed", None, None));
            }
            Err(retry_after) => {
                let finished =
                    now.saturating_add(started.elapsed().as_secs().min(i64::MAX as u64) as i64);
                let next = self.fail(&job, retry_after, finished).await?;
                emit(event(
                    &job,
                    if next.is_some() { "pending" } else { "failed" },
                    next,
                    None,
                ));
            }
        }
        Ok(true)
    }
}

fn event(job: &Job, status: &str, next_attempt: Option<i64>, track: Option<&str>) -> QueueEvent {
    QueueEvent {
        job_id: job.id.clone(),
        meeting_id: job.spec.meeting_id.clone(),
        attempts: job.attempts,
        status: status.into(),
        next_attempt,
        track: track.map(str::to_string),
    }
}

async fn execute<L, Fut>(
    queue: &Queue,
    job: &Job,
    service: &ProviderService,
    default_config: &crate::providers::ProvidersConfig,
    local: &mut L,
    emit: &mut impl FnMut(QueueEvent),
    now: i64,
) -> Result<serde_json::Value, Option<u64>>
where
    L: FnMut(ResolvedLink, TranscriptionRequest) -> Fut,
    Fut: Future<Output = Result<Transcription, ProviderError>>,
{
    let config = job.spec.providers.as_ref().unwrap_or(default_config);
    let started = std::time::Instant::now();
    let clock = || now.saturating_add(started.elapsed().as_secs().min(i64::MAX as u64) as i64);
    let mut tracks = Vec::new();
    for (name, path, diarize) in [
        ("microphone", Some(job.spec.microphone.as_path()), false),
        ("system", job.spec.system.as_deref(), true),
    ] {
        let Some(path) = path else { continue };
        emit(event(job, "running", None, Some(name)));
        let wav = tokio::fs::read(path).await.map_err(|_| None)?;
        let request = TranscriptionRequest {
            wav,
            file_name: "track.wav".into(),
            language: Some(job.spec.language.clone()),
            diarize,
            timestamps: true,
            prompt: job.spec.hotwords.prompt(crate::hotwords::PROMPT_BUDGET),
        };
        let cooldowns = queue.cooldowns(clock()).await.map_err(|_| None)?;
        let links = service
            .resolve(config, Function::Transcription)
            .into_iter()
            .filter(|link| !cooldowns.contains_key(&link.provider_id))
            .collect();
        let result = chain::transcribe(Function::Transcription, links, request, &mut *local).await;
        let attempts = match &result {
            Ok(delivered) => &delivered.attempts,
            Err(error) => &error.attempts,
        };
        for attempt in attempts {
            let wait = match attempt.error {
                ProviderError::RateLimited { retry_after_secs }
                | ProviderError::Http {
                    retry_after_secs, ..
                } => retry_after_secs,
                _ => None,
            };
            if let Some(wait) = wait {
                queue
                    .defer_provider(
                        &attempt.provider_id,
                        clock().saturating_add(wait.min(i64::MAX as u64) as i64),
                    )
                    .await
                    .map_err(|_| None)?;
            }
        }
        let delivered = match result {
            Ok(delivered) => delivered,
            Err(_) => {
                let cooldowns = queue.cooldowns(clock()).await.map_err(|_| None)?;
                return Err(cooldowns
                    .values()
                    .map(|until| until.saturating_sub(clock()).max(0) as u64)
                    .max());
            }
        };
        tracks.push(delivered);
    }
    let mic = &tracks[0].value;
    let system = tracks.get(1).map(|track| &track.value);
    let config = MergeConfig {
        mikrofon_label: if job.spec.microphone_label.trim().is_empty() {
            "Mig".into()
        } else {
            job.spec.microphone_label.clone()
        },
        system_diariseret: system.is_some_and(|t| !t.speakers_missing),
        ..Default::default()
    };
    let mic = segments(mic, job.spec.offset_ms);
    let system = system
        .map(|t| segments(t, job.spec.offset_ms))
        .unwrap_or_default();
    let merged: Vec<_> = merge(&mic, &system, &config).into_iter().map(|s| json!({
        "startMs": s.start_ms, "endMs": s.slut_ms, "text": s.tekst,
        "speaker": s.taler.label(&config), "speakersMissing": s.taler_mangler,
        "overlap": s.overlapper,
        "source": match s.kilde { crate::transcript_merge::Kilde::Mikrofon => "microphone", crate::transcript_merge::Kilde::System => "system" },
    })).collect();
    Ok(json!({"tracks": tracks, "segments": merged, "language": job.spec.language}))
}

fn segments(transcript: &Transcription, offset: u64) -> Vec<Segment> {
    let ms = |t: Option<f64>| (t.unwrap_or(0.0).max(0.0) * 1000.0) as u64;
    transcript
        .segments
        .iter()
        .map(|s| Segment {
            start_ms: offset.saturating_add(ms(s.start)),
            slut_ms: offset.saturating_add(ms(s.end.or(s.start))),
            tekst: s.text.clone(),
            taler: s.speaker.clone(),
        })
        .collect()
}

/// Adapter for an already loaded Parakeet/Whisper provider. The host supplies
/// the model selected by the resolved link; language is passed unchanged.
pub async fn transcribe_local(
    provider: Arc<dyn crate::audio::transcription::TranscriptionProvider>,
    request: TranscriptionRequest,
) -> Result<Transcription, ProviderError> {
    transcribe_local_with_vad(provider, request, crate::vad_trim::TrimConfig::default()).await
}

pub(super) async fn transcribe_local_with_vad(
    provider: Arc<dyn crate::audio::transcription::TranscriptionProvider>,
    request: TranscriptionRequest,
    vad: crate::vad_trim::TrimConfig,
) -> Result<Transcription, ProviderError> {
    let samples = tokio::task::spawn_blocking(move || -> Result<Vec<f32>> {
        let mut file = tempfile::Builder::new().suffix(".wav").tempfile()?;
        std::io::Write::write_all(&mut file, &request.wav)?;
        Ok(crate::audio::decoder::decode_audio_file(Path::new(file.path()))?.to_whisper_format())
    })
    .await
    .map_err(|_| ProviderError::local("Audio decoder stopped"))?
    .map_err(|_| ProviderError::local("Audio could not be decoded"))?;
    let chunks = tokio::task::spawn_blocking(move || {
        crate::vad_trim::trim(&samples, &vad).map(|trimmed| (samples, trimmed.segments))
    })
    .await
    .map_err(|_| ProviderError::local("Speech detection stopped"))?
    .map_err(|_| ProviderError::local("Speech detection failed"))?;
    let mut segments = Vec::new();
    for chunk in chunks.1 {
        let result = provider
            .transcribe(
                chunks.0[chunk.orig_start..chunk.orig_end].to_vec(),
                request.language.clone(),
            )
            .await
            .map_err(|_| ProviderError::local("Local transcription failed"))?;
        if !result.text.trim().is_empty() {
            segments.push(TranscriptSegment {
                start: Some(chunk.orig_start as f64 / 16000.0),
                end: Some(chunk.orig_end as f64 / 16000.0),
                text: result.text,
                speaker: None,
            });
        }
    }
    Ok(Transcription {
        text: segments
            .iter()
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>()
            .join(" "),
        segments,
        ..Default::default()
    })
}
