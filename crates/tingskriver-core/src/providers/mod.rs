//! AI providers shared by meetings and dictation (SPEC D1).
//!
//! - `model`: providers (Syv.ai, OpenAI, OdinCore, custom OpenAI-compatible,
//!   Ollama, the built-in local model) and an ordered chain per function
//! - `secrets`: API keys, only in Windows Credential Manager
//! - `client`: OpenAI-compatible calls with time limits and classified errors
//! - `chain`: fall back to the next link and record who delivered
//! - `migrate`: the old Ollama/OpenAI/Groq/OpenRouter/Claude settings
//! - `service`: `providers.json`, keys, the test call
//!
//! Dictation and meetings look up their providers through the functions
//! below:
//!
//! ```ignore
//! let delivered = providers::transcribe(&host, Function::Dictation, request, |link, req| async move {
//!     // link.kind == ProviderKind::Local: run Parakeet with link.model
//!     local_transcribe(&link.model, req).await.map_err(ProviderError::local)
//! })
//! .await?;
//! log::info!("{}", delivered.delivered_by());
//! ```

pub mod chain;
pub mod client;
pub mod error;
pub mod migrate;
pub mod model;
pub mod secrets;
pub mod service;

#[cfg(test)]
mod fake_server;
#[cfg(test)]
mod tests;

use std::future::Future;

pub use chain::{Attempt, ChainFailed, Delivered};
pub use client::{ChatMessage, ResolvedLink, TestReport, Transcription, TranscriptionRequest};
pub use error::{ProviderError, ProviderFailure};
pub use model::{ChainLink, Function, Provider, ProviderKind, ProvidersConfig};
pub use service::{ProviderService, ProviderStatus};

use crate::host::HostHandle;

/// Why a lookup or a chain gave no result, ready to show in the UI.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum LookupError {
    /// The configuration could not be loaded.
    Config { message: String },
    /// Every link failed, or the function is off.
    Chain(ChainFailed),
}

impl std::fmt::Display for LookupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LookupError::Config { message } => f.write_str(message),
            LookupError::Chain(failed) => failed.fmt(f),
        }
    }
}

impl std::error::Error for LookupError {}

impl From<LookupError> for String {
    fn from(e: LookupError) -> String {
        e.to_string()
    }
}

/// The current configuration (migrating the old settings on first use).
pub async fn config(host: &HostHandle) -> Result<ProvidersConfig, String> {
    ProviderService::for_host(host)?.load_for_host(host).await
}

/// Validate and save the configuration from the settings UI.
pub async fn save_config(host: &HostHandle, config: &ProvidersConfig) -> Result<(), String> {
    ProviderService::for_host(host)?.save(config)
}

/// The providers with key status, for the settings UI.
pub async fn statuses(host: &HostHandle) -> Result<Vec<ProviderStatus>, String> {
    let service = ProviderService::for_host(host)?;
    let config = service.load_for_host(host).await?;
    Ok(service.statuses(&config))
}

/// Store a provider's key in Credential Manager.
pub fn set_key(host: &HostHandle, provider_id: &str, key: &str) -> Result<(), String> {
    ProviderService::for_host(host)?.set_key(provider_id, key)
}

pub fn delete_key(host: &HostHandle, provider_id: &str) -> Result<(), String> {
    ProviderService::for_host(host)?.delete_key(provider_id)
}

/// The test button: a real, small call with the measured round trip.
pub async fn test_provider(
    host: &HostHandle,
    provider_id: &str,
) -> Result<TestReport, ProviderFailure> {
    let as_failure = |message: String| {
        ProviderFailure::new(
            provider_id,
            provider_id,
            ProviderError::Config { detail: message },
        )
    };
    let service = ProviderService::for_host(host).map_err(as_failure)?;
    let config = service.load_for_host(host).await.map_err(as_failure)?;
    service.test(&config, provider_id).await
}

/// The ordered links for `function`, with models and keys filled in.
pub async fn chain_for(host: &HostHandle, function: Function) -> Result<Vec<ResolvedLink>, String> {
    let service = ProviderService::for_host(host)?;
    let config = service.load_for_host(host).await?;
    Ok(service.resolve(&config, function))
}

/// Transcribe with `function`'s chain (`Transcription` or `Dictation`).
/// Remote links are called here; `local` runs the links on the built-in
/// model.
pub async fn transcribe<L, LFut>(
    host: &HostHandle,
    function: Function,
    request: TranscriptionRequest,
    local: L,
) -> Result<Delivered<Transcription>, LookupError>
where
    L: FnMut(ResolvedLink, TranscriptionRequest) -> LFut,
    LFut: Future<Output = Result<Transcription, ProviderError>>,
{
    let links = chain_for(host, function)
        .await
        .map_err(|message| LookupError::Config { message })?;
    chain::transcribe(function, links, request, local)
        .await
        .map_err(LookupError::Chain)
}

/// Chat with `function`'s chain (`Summary`, `Assistant` or
/// `DictationPolish`). Remote links are called here; `local` runs the links
/// on the built-in model.
pub async fn chat<L, LFut>(
    host: &HostHandle,
    function: Function,
    messages: Vec<ChatMessage>,
    local: L,
) -> Result<Delivered<String>, LookupError>
where
    L: FnMut(ResolvedLink, Vec<ChatMessage>) -> LFut,
    LFut: Future<Output = Result<String, ProviderError>>,
{
    let links = chain_for(host, function)
        .await
        .map_err(|message| LookupError::Config { message })?;
    chain::chat(function, links, messages, local)
        .await
        .map_err(LookupError::Chain)
}
