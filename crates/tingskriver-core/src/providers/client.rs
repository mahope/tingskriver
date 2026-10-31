//! OpenAI-compatible client for Syv.ai, OpenAI, OdinCore, Ollama and custom
//! endpoints: `/audio/transcriptions`, `/chat/completions` and `/models`.
//!
//! Every call has a time limit that also covers a server that accepts the
//! connection and then never answers. Errors are classified
//! ([`ProviderError`]) and never contain the key.

use std::time::{Duration, Instant};

use once_cell::sync::Lazy;
use reqwest::multipart::{Form, Part};
use reqwest::{RequestBuilder, Response, StatusCode};
use serde::{Deserialize, Serialize};

use super::error::{parse_retry_after, ProviderError};
use super::model::{ChatOptions, Function, ProviderKind};
use super::secrets::{redact, ApiKey};

/// A chain link with everything needed to call it, including the key.
/// `Debug` is safe to log: [`ApiKey`] hides its value.
#[derive(Clone, Debug)]
pub struct ResolvedLink {
    pub function: Function,
    pub provider_id: String,
    pub provider_name: String,
    pub kind: ProviderKind,
    pub base_url: String,
    pub model: String,
    pub key: Option<ApiKey>,
    /// Per-link limit from the chain, `None` for the function's default
    /// (remote) or no limit (local).
    pub timeout: Option<Duration>,
    pub diarization: bool,
    pub chat_options: ChatOptions,
}

impl ResolvedLink {
    /// The limit for an HTTP call on this link.
    pub fn remote_timeout(&self) -> Duration {
        self.timeout
            .unwrap_or_else(|| self.function.default_remote_timeout())
    }

    /// The limit the chain enforces around the whole attempt.
    pub fn attempt_timeout(&self) -> Option<Duration> {
        if self.kind.is_remote() {
            Some(self.remote_timeout())
        } else {
            self.timeout
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}/{}", self.base_url.trim_end_matches('/'), path)
    }

    fn ensure_remote(&self) -> Result<(), ProviderError> {
        if self.kind.is_remote() {
            Ok(())
        } else {
            Err(ProviderError::Config {
                detail: "den lokale model kaldes ikke over HTTP".to_string(),
            })
        }
    }
}

/// Shared HTTP client (connection pool) for all providers.
pub fn http_client() -> reqwest::Client {
    static CLIENT: Lazy<reqwest::Client> = Lazy::new(|| {
        reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_else(|e| {
                log::warn!(
                    "Provider HTTP client with connect timeout failed ({e}); using defaults"
                );
                reqwest::Client::new()
            })
    });
    CLIENT.clone()
}

#[derive(Clone, Debug, Default)]
pub struct TranscriptionRequest {
    pub wav: Vec<u8>,
    pub file_name: String,
    /// ISO 639-1, e.g. `da`. `None` lets the service detect it.
    pub language: Option<String>,
    /// Dictionary words as hints.
    pub prompt: Option<String>,
    /// Ask for speaker labels. Only sent to providers with diarization.
    pub diarize: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptSegment {
    pub start: Option<f64>,
    pub end: Option<f64>,
    pub text: String,
    pub speaker: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Transcription {
    pub text: String,
    pub segments: Vec<TranscriptSegment>,
    /// Speaker labels were asked for but this link does not deliver them.
    /// The UI shows the speakers as missing instead of guessing.
    pub speakers_missing: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> ChatMessage {
        ChatMessage {
            role: "system".to_string(),
            content: content.into(),
        }
    }

    pub fn user(content: impl Into<String>) -> ChatMessage {
        ChatMessage {
            role: "user".to_string(),
            content: content.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestReport {
    pub latency_ms: u64,
    /// Number of models the service listed.
    pub models: usize,
}

#[derive(Deserialize)]
struct RawTranscription {
    #[serde(default)]
    text: String,
    #[serde(default)]
    segments: Vec<RawSegment>,
}

#[derive(Deserialize)]
struct RawSegment {
    #[serde(default)]
    start: Option<f64>,
    #[serde(default)]
    end: Option<f64>,
    #[serde(default)]
    text: String,
    #[serde(default, alias = "speaker_id", alias = "speaker_label")]
    speaker: Option<serde_json::Value>,
}

/// Transcribe a WAV file.
pub async fn transcribe(
    client: &reqwest::Client,
    link: &ResolvedLink,
    request: &TranscriptionRequest,
) -> Result<Transcription, ProviderError> {
    link.ensure_remote()?;
    let diarize = request.diarize && link.diarization;
    let file_name = if request.file_name.is_empty() {
        "lyd.wav"
    } else {
        &request.file_name
    };
    let file = Part::bytes(request.wav.clone())
        .file_name(file_name.to_string())
        .mime_str("audio/wav")
        .map_err(|e| ProviderError::Config {
            detail: e.to_string(),
        })?;
    let mut form = Form::new()
        .part("file", file)
        .text("model", link.model.clone())
        .text(
            "response_format",
            if diarize { "verbose_json" } else { "json" },
        );
    if let Some(language) = request.language.as_deref().filter(|l| !l.trim().is_empty()) {
        form = form.text("language", language.to_string());
    }
    if let Some(prompt) = request.prompt.as_deref().filter(|p| !p.trim().is_empty()) {
        form = form.text("prompt", prompt.to_string());
    }
    if diarize {
        form = form.text("diarize", "true");
    }

    let builder = client
        .post(link.url("audio/transcriptions"))
        .multipart(form);
    let raw: RawTranscription = call_json(link, builder).await?;
    let segments: Vec<TranscriptSegment> = raw
        .segments
        .into_iter()
        .map(|s| TranscriptSegment {
            start: s.start,
            end: s.end,
            text: s.text.trim().to_string(),
            speaker: s.speaker.and_then(speaker_label),
        })
        .collect();
    let has_speakers = !segments.is_empty() && segments.iter().all(|s| s.speaker.is_some());
    Ok(Transcription {
        text: raw.text.trim().to_string(),
        speakers_missing: request.diarize && !has_speakers,
        segments,
    })
}

fn speaker_label(value: serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(s) if !s.trim().is_empty() => Some(s.trim().to_string()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

#[derive(Deserialize)]
struct RawChat {
    choices: Vec<RawChoice>,
}

#[derive(Deserialize)]
struct RawChoice {
    message: RawMessage,
}

#[derive(Deserialize)]
struct RawMessage {
    #[serde(default)]
    content: Option<String>,
}

/// One non-streaming chat completion; returns the assistant's text.
pub async fn chat(
    client: &reqwest::Client,
    link: &ResolvedLink,
    messages: &[ChatMessage],
) -> Result<String, ProviderError> {
    link.ensure_remote()?;
    let mut body = serde_json::json!({
        "model": link.model,
        "messages": messages,
        "stream": false,
    });
    let options = &link.chat_options;
    if let Some(v) = options.max_tokens {
        body["max_tokens"] = v.into();
    }
    if let Some(v) = options.temperature {
        body["temperature"] = v.into();
    }
    if let Some(v) = options.top_p {
        body["top_p"] = v.into();
    }
    let builder = client.post(link.url("chat/completions")).json(&body);
    let raw: RawChat = call_json(link, builder).await?;
    raw.choices
        .into_iter()
        .next()
        .and_then(|c| c.message.content)
        .ok_or_else(|| ProviderError::BadResponse {
            detail: "svaret indeholdt ingen tekst".to_string(),
        })
}

#[derive(Deserialize)]
struct RawModels {
    #[serde(default)]
    data: Vec<serde_json::Value>,
}

/// A real, small call (`GET /models`) with the measured round trip.
pub async fn test(
    client: &reqwest::Client,
    link: &ResolvedLink,
) -> Result<TestReport, ProviderError> {
    link.ensure_remote()?;
    let start = Instant::now();
    let raw: RawModels = call_json(link, client.get(link.url("models"))).await?;
    Ok(TestReport {
        latency_ms: start.elapsed().as_millis() as u64,
        models: raw.data.len(),
    })
}

/// Send, check the status and decode the JSON body, all within the link's
/// time limit.
async fn call_json<T: serde::de::DeserializeOwned>(
    link: &ResolvedLink,
    builder: RequestBuilder,
) -> Result<T, ProviderError> {
    let timeout = link.remote_timeout();
    let builder = match &link.key {
        Some(key) => builder.bearer_auth(key.expose()),
        None => builder,
    }
    .timeout(timeout);

    let work = async {
        let response = builder
            .send()
            .await
            .map_err(|e| map_reqwest(e, timeout, link))?;
        let response = check_status(response)?;
        let bytes = response
            .bytes()
            .await
            .map_err(|e| map_reqwest(e, timeout, link))?;
        serde_json::from_slice::<T>(&bytes).map_err(|e| ProviderError::BadResponse {
            detail: redact(&e.to_string(), link.key.as_ref()),
        })
    };
    match tokio::time::timeout(timeout, work).await {
        Ok(result) => result,
        Err(_) => Err(ProviderError::timeout(timeout)),
    }
}

fn check_status(response: Response) -> Result<Response, ProviderError> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    Err(classify_status(
        status,
        response
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|v| v.to_str().ok()),
    ))
}

/// Map an HTTP error status to an error class.
pub fn classify_status(status: StatusCode, retry_after: Option<&str>) -> ProviderError {
    match status.as_u16() {
        401 | 403 => ProviderError::Unauthorized {
            status: status.as_u16(),
        },
        404 => ProviderError::NotFound,
        429 => ProviderError::RateLimited {
            retry_after_secs: retry_after.and_then(|v| parse_retry_after(v, chrono::Utc::now())),
        },
        code => ProviderError::Http { status: code },
    }
}

fn map_reqwest(error: reqwest::Error, timeout: Duration, link: &ResolvedLink) -> ProviderError {
    if error.is_timeout() {
        return ProviderError::timeout(timeout);
    }
    let body_error = error.is_decode() || error.is_body();
    let detail = redact(&error.without_url().to_string(), link.key.as_ref());
    if body_error {
        ProviderError::BadResponse { detail }
    } else {
        ProviderError::Unreachable { detail }
    }
}
