//! Run a function's chain: try each link in order, fall back on an error or
//! a time limit, and record which provider delivered ("leveret af ...").

use std::future::Future;

use serde::Serialize;

use super::client::{self, ChatMessage, ResolvedLink, Transcription, TranscriptionRequest};
use super::error::ProviderError;
use super::model::{Function, ProviderKind};

/// A link that failed before another one delivered (or before the chain
/// gave up).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Attempt {
    pub provider_id: String,
    pub provider_name: String,
    pub model: String,
    pub error: ProviderError,
    /// Danish explanation of `error`.
    pub message: String,
}

/// The result and who delivered it.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Delivered<T> {
    pub value: T,
    pub provider_id: String,
    pub provider_name: String,
    pub kind: ProviderKind,
    pub model: String,
    /// Links tried before this one, in order.
    pub attempts: Vec<Attempt>,
}

impl<T> Delivered<T> {
    /// "leveret af Syv.ai (syv-transcribe)", for the UI.
    pub fn delivered_by(&self) -> String {
        format!("leveret af {} ({})", self.provider_name, self.model)
    }

    /// A reserve link delivered, not the first choice.
    pub fn fell_back(&self) -> bool {
        !self.attempts.is_empty()
    }

    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Delivered<U> {
        Delivered {
            value: f(self.value),
            provider_id: self.provider_id,
            provider_name: self.provider_name,
            kind: self.kind,
            model: self.model,
            attempts: self.attempts,
        }
    }
}

/// Every link failed, or the chain is empty (the function is off).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChainFailed {
    pub function: Function,
    pub attempts: Vec<Attempt>,
}

impl ChainFailed {
    pub fn is_off(&self) -> bool {
        self.attempts.is_empty()
    }
}

impl std::fmt::Display for ChainFailed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.attempts.is_empty() {
            return write!(
                f,
                "{} er slået fra: der er ikke valgt nogen udbyder.",
                self.function.label()
            );
        }
        write!(f, "{} lykkedes ikke.", self.function.label())?;
        for attempt in &self.attempts {
            write!(f, " {}", attempt.message)?;
        }
        Ok(())
    }
}

impl std::error::Error for ChainFailed {}

/// Try `links` in order with `op`. A link that needs a key it does not have
/// is skipped without a call. A link fails on an error or when its time
/// limit runs out; then the next one is used. Fallbacks and the delivering
/// provider are logged (never the key).
pub async fn run<T, F, Fut>(
    function: Function,
    links: Vec<ResolvedLink>,
    mut op: F,
) -> Result<Delivered<T>, ChainFailed>
where
    F: FnMut(ResolvedLink) -> Fut,
    Fut: Future<Output = Result<T, ProviderError>>,
{
    let mut attempts = Vec::new();
    let fn_name = function_log_name(function);
    for link in links {
        let result = if link.kind.requires_key() && link.key.is_none() {
            Err(ProviderError::MissingKey)
        } else {
            let attempt = op(link.clone());
            match link.attempt_timeout() {
                Some(limit) => tokio::time::timeout(limit, attempt)
                    .await
                    .unwrap_or_else(|_| Err(ProviderError::timeout(limit))),
                None => attempt.await,
            }
        };
        match result {
            Ok(value) => {
                if attempts.is_empty() {
                    log::info!(
                        "{fn_name}: delivered by {} ({})",
                        link.provider_id,
                        link.model
                    );
                } else {
                    log::warn!(
                        "{fn_name}: delivered by reserve {} ({}) after {} failed link(s)",
                        link.provider_id,
                        link.model,
                        attempts.len()
                    );
                }
                return Ok(Delivered {
                    value,
                    provider_id: link.provider_id,
                    provider_name: link.provider_name,
                    kind: link.kind,
                    model: link.model,
                    attempts,
                });
            }
            Err(error) => {
                let message = error.message(&link.provider_name);
                if error == ProviderError::MissingKey {
                    log::info!("{fn_name}: skipping {}, no API key", link.provider_id);
                } else {
                    log::warn!(
                        "{fn_name}: {} ({}) failed: {message}",
                        link.provider_id,
                        link.model
                    );
                }
                attempts.push(Attempt {
                    provider_id: link.provider_id,
                    provider_name: link.provider_name,
                    model: link.model,
                    error,
                    message,
                });
            }
        }
    }
    if attempts.is_empty() {
        log::info!("{fn_name}: no provider chosen");
    } else {
        log::error!("{fn_name}: all {} link(s) failed", attempts.len());
    }
    Err(ChainFailed { function, attempts })
}

fn function_log_name(function: Function) -> &'static str {
    match function {
        Function::Transcription => "Providers/transcription",
        Function::Dictation => "Providers/dictation",
        Function::Summary => "Providers/summary",
        Function::Assistant => "Providers/assistant",
        Function::DictationPolish => "Providers/dictation-polish",
    }
}

/// Transcribe with the chain. Remote links are called here; `local` handles
/// links on the built-in model (`link.model` names the local model).
pub async fn transcribe<L, LFut>(
    function: Function,
    links: Vec<ResolvedLink>,
    request: TranscriptionRequest,
    mut local: L,
) -> Result<Delivered<Transcription>, ChainFailed>
where
    L: FnMut(ResolvedLink, TranscriptionRequest) -> LFut,
    LFut: Future<Output = Result<Transcription, ProviderError>>,
{
    let http = client::http_client();
    run(function, links, |link| {
        let request = request.clone();
        let http = http.clone();
        let local_call = (!link.kind.is_remote()).then(|| local(link.clone(), request.clone()));
        async move {
            match local_call {
                Some(call) => {
                    let mut transcription = call.await?;
                    transcription.speakers_missing |= request.diarize;
                    Ok(transcription)
                }
                None => client::transcribe(&http, &link, &request).await,
            }
        }
    })
    .await
}

/// Chat with the chain. Remote links are called here; `local` handles links
/// on the built-in model.
pub async fn chat<L, LFut>(
    function: Function,
    links: Vec<ResolvedLink>,
    messages: Vec<ChatMessage>,
    mut local: L,
) -> Result<Delivered<String>, ChainFailed>
where
    L: FnMut(ResolvedLink, Vec<ChatMessage>) -> LFut,
    LFut: Future<Output = Result<String, ProviderError>>,
{
    let http = client::http_client();
    run(function, links, |link| {
        let messages = messages.clone();
        let http = http.clone();
        let local_call = (!link.kind.is_remote()).then(|| local(link.clone(), messages.clone()));
        async move {
            match local_call {
                Some(call) => call.await,
                None => client::chat(&http, &link, &messages).await,
            }
        }
    })
    .await
}
