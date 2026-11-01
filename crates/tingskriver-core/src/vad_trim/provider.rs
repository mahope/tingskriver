use super::{trim, TrimConfig, TrimmedAudio};
use crate::audio::transcription::provider::{
    TranscriptResult, TranscriptionError, TranscriptionProvider,
};

pub struct TrimmedTranscript {
    pub transcript: TranscriptResult,
    pub audio: TrimmedAudio,
}
/// One call preserves linguistic context across pauses and avoids per-segment
/// request overhead. The returned map remains available to timed adapters.
pub async fn transcribe_trimmed(
    provider: &dyn TranscriptionProvider,
    audio: &[f32],
    cfg: &TrimConfig,
    language: Option<String>,
) -> Result<TrimmedTranscript, TranscriptionError> {
    let audio = trim(audio, cfg)
        .map_err(|_| TranscriptionError::EngineFailed("VAD preprocessing failed".into()))?;
    let transcript = if audio.samples.is_empty() {
        TranscriptResult {
            text: String::new(),
            confidence: None,
            is_partial: false,
        }
    } else {
        provider.transcribe(audio.samples.clone(), language).await?
    };
    Ok(TrimmedTranscript { transcript, audio })
}

#[derive(Debug, Clone)]
pub struct TimedSpan {
    pub text: String,
    pub start_ms: f64,
    pub end_ms: f64,
}
#[derive(Debug, Clone, Default)]
pub struct TimedTranscript {
    pub text: String,
    pub words: Vec<TimedSpan>,
    pub segments: Vec<TimedSpan>,
}
/// Optional adapter for engines that expose timestamps. The existing core
/// TranscriptionProvider returns text only and remains unchanged.
pub trait TimedTranscriptionProvider: Send + Sync {
    fn transcribe_timed<'a>(
        &'a self,
        audio: Vec<f32>,
        language: Option<String>,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<TimedTranscript, TranscriptionError>>
                + Send
                + 'a,
        >,
    >;
}
pub async fn transcribe_trimmed_timed(
    provider: &dyn TimedTranscriptionProvider,
    audio: &[f32],
    cfg: &TrimConfig,
    language: Option<String>,
) -> Result<TimedTranscript, TranscriptionError> {
    let audio = trim(audio, cfg)
        .map_err(|_| TranscriptionError::EngineFailed("VAD preprocessing failed".into()))?;
    if audio.samples.is_empty() {
        return Ok(TimedTranscript::default());
    }
    let mut result = provider
        .transcribe_timed(audio.samples.clone(), language)
        .await?;
    for span in result.words.iter_mut().chain(result.segments.iter_mut()) {
        if !span.start_ms.is_finite() || !span.end_ms.is_finite() || span.end_ms < span.start_ms {
            return Err(TranscriptionError::EngineFailed(
                "Provider returned invalid timestamps".into(),
            ));
        }
        span.start_ms = audio.to_original_ms(span.start_ms);
        span.end_ms = audio.end_to_original_ms(span.end_ms).max(span.start_ms);
    }
    Ok(result)
}
