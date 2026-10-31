//! The shared provider model: which AI services exist, and which ordered
//! chain of them each function uses.
//!
//! The configuration never holds an API key. Keys live in Windows Credential
//! Manager under [`key_target`], see [`super::secrets`].

use std::collections::BTreeMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::config::DEFAULT_PARAKEET_MODEL;
use crate::dictation::secrets::SYV_TARGET;

pub const CONFIG_VERSION: u32 = 1;

pub const SYV_ID: &str = "syv-ai";
pub const OPENAI_ID: &str = "openai";
pub const ODINCORE_ID: &str = "odincore";
pub const LOCAL_ID: &str = "local";

pub const SYV_BASE_URL: &str = "https://platform.syv.ai/v1";
pub const OPENAI_BASE_URL: &str = "https://api.openai.com/v1";
pub const ODINCORE_BASE_URL: &str = "https://api.odincore.ai/v1";
pub const OLLAMA_BASE_URL: &str = "http://localhost:11434/v1";

/// What kind of service a provider is. Everything except `Local` speaks the
/// OpenAI-compatible API (`/audio/transcriptions`, `/chat/completions`,
/// `/models`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderKind {
    SyvAi,
    OpenAi,
    OdinCore,
    /// User-defined OpenAI-compatible endpoint (Groq, OpenRouter, vLLM, ...).
    OpenAiCompatible,
    Ollama,
    /// The built-in local models: Parakeet/Whisper for speech, llama-helper
    /// for text. Runs in the app, never over the network.
    Local,
}

impl ProviderKind {
    pub fn label(self) -> &'static str {
        match self {
            ProviderKind::SyvAi => "Syv.ai",
            ProviderKind::OpenAi => "OpenAI",
            ProviderKind::OdinCore => "OdinCore",
            ProviderKind::OpenAiCompatible => "Brugerdefineret OpenAI-kompatibel",
            ProviderKind::Ollama => "Ollama",
            ProviderKind::Local => "Indbygget lokal model",
        }
    }

    pub fn is_remote(self) -> bool {
        self != ProviderKind::Local
    }

    /// A call without a key is pointless: the service always wants one.
    /// Custom endpoints and Ollama may run without authentication.
    pub fn requires_key(self) -> bool {
        matches!(
            self,
            ProviderKind::SyvAi | ProviderKind::OpenAi | ProviderKind::OdinCore
        )
    }

    fn allows_plain_http(self) -> bool {
        matches!(self, ProviderKind::OpenAiCompatible | ProviderKind::Ollama)
    }
}

/// What the app uses AI for. Each function has its own chain.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Function {
    /// Meeting transcription.
    Transcription,
    /// Speech-to-text for dictation (short, latency sensitive).
    Dictation,
    /// Meeting summaries.
    Summary,
    /// Ask-the-meeting and the assistant.
    Assistant,
    /// Polishing dictated text. An empty chain means "off".
    DictationPolish,
}

impl Function {
    pub const ALL: [Function; 5] = [
        Function::Transcription,
        Function::Dictation,
        Function::Summary,
        Function::Assistant,
        Function::DictationPolish,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Function::Transcription => "Transskription",
            Function::Dictation => "Diktering",
            Function::Summary => "Referat",
            Function::Assistant => "Assistent",
            Function::DictationPolish => "Dikterings-polering",
        }
    }

    pub fn capability(self) -> Capability {
        match self {
            Function::Transcription | Function::Dictation => Capability::Transcription,
            Function::Summary | Function::Assistant | Function::DictationPolish => Capability::Chat,
        }
    }

    /// Timeout for a remote link without its own `timeout_ms`. Dictation
    /// falls back to the local model after 2 s (SPEC D2).
    pub fn default_remote_timeout(self) -> Duration {
        match self {
            Function::Dictation | Function::DictationPolish => Duration::from_millis(2_000),
            Function::Transcription => Duration::from_secs(300),
            Function::Summary | Function::Assistant => Duration::from_secs(180),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Capability {
    Transcription,
    Chat,
}

/// Optional sampling parameters for chat calls (kept from the old custom
/// OpenAI settings).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ChatOptions {
    pub max_tokens: Option<i32>,
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
}

impl ChatOptions {
    pub fn is_empty(&self) -> bool {
        self == &ChatOptions::default()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Provider {
    /// Stable id, lowercase letters, digits and `-`. Also names the key in
    /// Credential Manager.
    pub id: String,
    pub name: String,
    pub kind: ProviderKind,
    /// Base URL including `/v1`. Empty for `Local`.
    #[serde(default)]
    pub base_url: String,
    /// Model used for transcription when a chain link names none.
    #[serde(default)]
    pub transcription_model: Option<String>,
    /// Model used for chat when a chain link names none.
    #[serde(default)]
    pub chat_model: Option<String>,
    /// The service labels speakers (`diarize`).
    #[serde(default)]
    pub diarization: bool,
    #[serde(default, skip_serializing_if = "ChatOptions::is_empty")]
    pub chat_options: ChatOptions,
}

impl Provider {
    pub fn default_model(&self, capability: Capability) -> Option<&str> {
        match capability {
            Capability::Transcription => self.transcription_model.as_deref(),
            Capability::Chat => self.chat_model.as_deref(),
        }
        .filter(|m| !m.trim().is_empty())
    }

    /// The local provider can do both; a remote one only what it has a model for
    /// (a link may still name a model explicitly).
    pub fn supports(&self, capability: Capability) -> bool {
        self.kind == ProviderKind::Local || self.default_model(capability).is_some()
    }
}

/// One step of a function's chain.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChainLink {
    pub provider: String,
    /// Overrides the provider's default model for this function.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Give up on this link and use the next after this long.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
}

impl ChainLink {
    pub fn new(provider: &str) -> Self {
        ChainLink {
            provider: provider.to_string(),
            model: None,
            timeout_ms: None,
        }
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        let model = model.into();
        self.model = (!model.trim().is_empty()).then_some(model);
        self
    }

    pub fn with_timeout_ms(mut self, ms: u64) -> Self {
        self.timeout_ms = Some(ms);
        self
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProvidersConfig {
    pub version: u32,
    pub providers: Vec<Provider>,
    /// Ordered chain per function. A function that is missing or empty is off.
    pub functions: BTreeMap<Function, Vec<ChainLink>>,
}

impl Default for ProvidersConfig {
    /// Syv.ai first where it fits, the built-in local model as reserve, and
    /// summaries and the assistant on the local model (SPEC D1, D2).
    fn default() -> Self {
        let mut functions = BTreeMap::new();
        functions.insert(
            Function::Transcription,
            vec![ChainLink::new(SYV_ID), ChainLink::new(LOCAL_ID)],
        );
        functions.insert(
            Function::Dictation,
            vec![ChainLink::new(SYV_ID), ChainLink::new(LOCAL_ID)],
        );
        functions.insert(Function::Summary, vec![ChainLink::new(LOCAL_ID)]);
        functions.insert(Function::Assistant, vec![ChainLink::new(LOCAL_ID)]);
        functions.insert(Function::DictationPolish, Vec::new());
        ProvidersConfig {
            version: CONFIG_VERSION,
            providers: presets(),
            functions,
        }
    }
}

impl ProvidersConfig {
    pub fn provider(&self, id: &str) -> Option<&Provider> {
        self.providers.iter().find(|p| p.id == id)
    }

    pub fn provider_mut(&mut self, id: &str) -> Option<&mut Provider> {
        self.providers.iter_mut().find(|p| p.id == id)
    }

    pub fn chain(&self, function: Function) -> &[ChainLink] {
        self.functions
            .get(&function)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// Add or replace a provider by id.
    pub fn upsert_provider(&mut self, provider: Provider) {
        match self.provider_mut(&provider.id) {
            Some(existing) => *existing = provider,
            None => self.providers.push(provider),
        }
    }

    /// Check what the settings UI may save. Errors are Danish and meant for
    /// the user.
    pub fn validate(&self) -> Result<(), String> {
        let mut seen = std::collections::HashSet::new();
        for p in &self.providers {
            validate_id(&p.id)?;
            if !seen.insert(p.id.as_str()) {
                return Err(format!("Udbyderen \"{}\" findes to gange", p.id));
            }
            if p.name.trim().is_empty() {
                return Err(format!("Udbyderen \"{}\" mangler et navn", p.id));
            }
            if p.kind.is_remote() {
                validate_base_url(p)?;
            }
        }
        for (function, chain) in &self.functions {
            for link in chain {
                let Some(provider) = self.provider(&link.provider) else {
                    return Err(format!(
                        "{} bruger udbyderen \"{}\", som ikke findes",
                        function.label(),
                        link.provider
                    ));
                };
                let capability = function.capability();
                let has_model = link.model.as_deref().is_some_and(|m| !m.trim().is_empty())
                    || provider.supports(capability);
                if !has_model {
                    return Err(format!(
                        "{} bruger {}, men der er ikke valgt nogen model",
                        function.label(),
                        provider.name
                    ));
                }
                if link.timeout_ms == Some(0) {
                    return Err(format!(
                        "{}: tidsgrænsen skal være over 0 ms",
                        function.label()
                    ));
                }
            }
        }
        Ok(())
    }
}

fn validate_id(id: &str) -> Result<(), String> {
    let ok = !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if ok {
        Ok(())
    } else {
        Err(format!(
            "Ugyldigt udbyder-id \"{id}\": brug små bogstaver, tal og bindestreg"
        ))
    }
}

fn validate_base_url(p: &Provider) -> Result<(), String> {
    let url = p.base_url.trim();
    let local_http = ["http://localhost", "http://127.0.0.1", "http://[::1]"]
        .iter()
        .any(|prefix| url.starts_with(prefix));
    if url.starts_with("https://")
        || local_http
        || (p.kind.allows_plain_http() && url.starts_with("http://"))
    {
        Ok(())
    } else {
        Err(format!("Adressen til {} skal starte med https://", p.name))
    }
}

/// The name of `provider_id`'s API key in Credential Manager. Syv.ai keeps
/// the target dictation already used, so an existing key carries over.
pub fn key_target(provider_id: &str) -> String {
    if provider_id == SYV_ID {
        SYV_TARGET.to_string()
    } else {
        format!("Tingskriver/udbyder/{provider_id}")
    }
}

/// The ready-made providers: Syv.ai, OpenAI, OdinCore and the local model.
pub fn presets() -> Vec<Provider> {
    vec![
        Provider {
            id: SYV_ID.to_string(),
            name: "Syv.ai".to_string(),
            kind: ProviderKind::SyvAi,
            base_url: SYV_BASE_URL.to_string(),
            transcription_model: Some(crate::dictation::settings::SYV_DEFAULT_MODEL.to_string()),
            chat_model: None,
            diarization: true,
            chat_options: ChatOptions::default(),
        },
        Provider {
            id: OPENAI_ID.to_string(),
            name: "OpenAI".to_string(),
            kind: ProviderKind::OpenAi,
            base_url: OPENAI_BASE_URL.to_string(),
            transcription_model: Some("whisper-1".to_string()),
            chat_model: Some("gpt-4o-mini".to_string()),
            diarization: false,
            chat_options: ChatOptions::default(),
        },
        Provider {
            id: ODINCORE_ID.to_string(),
            name: "OdinCore".to_string(),
            kind: ProviderKind::OdinCore,
            base_url: ODINCORE_BASE_URL.to_string(),
            transcription_model: Some("ordbogen/whisper".to_string()),
            chat_model: Some("odin-2".to_string()),
            diarization: false,
            chat_options: ChatOptions::default(),
        },
        local_provider(),
    ]
}

pub fn local_provider() -> Provider {
    Provider {
        id: LOCAL_ID.to_string(),
        name: "Indbygget lokal model".to_string(),
        kind: ProviderKind::Local,
        base_url: String::new(),
        transcription_model: Some(DEFAULT_PARAKEET_MODEL.to_string()),
        chat_model: None,
        diarization: false,
        chat_options: ChatOptions::default(),
    }
}

/// A user-defined OpenAI-compatible provider (also Groq, OpenRouter, ...).
pub fn custom_provider(id: &str, name: &str, base_url: &str) -> Provider {
    Provider {
        id: id.to_string(),
        name: name.to_string(),
        kind: ProviderKind::OpenAiCompatible,
        base_url: normalize_base_url(base_url),
        transcription_model: None,
        chat_model: None,
        diarization: false,
        chat_options: ChatOptions::default(),
    }
}

/// An Ollama server. `endpoint` may be given with or without `/v1`.
pub fn ollama_provider(endpoint: Option<&str>) -> Provider {
    let base = endpoint
        .map(str::trim)
        .filter(|e| !e.is_empty())
        .map(|e| {
            let e = normalize_base_url(e);
            if e.ends_with("/v1") {
                e
            } else {
                format!("{e}/v1")
            }
        })
        .unwrap_or_else(|| OLLAMA_BASE_URL.to_string());
    Provider {
        id: "ollama".to_string(),
        name: "Ollama".to_string(),
        kind: ProviderKind::Ollama,
        base_url: base,
        transcription_model: None,
        chat_model: None,
        diarization: false,
        chat_options: ChatOptions::default(),
    }
}

pub fn normalize_base_url(url: &str) -> String {
    url.trim().trim_end_matches('/').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_valid_and_prefers_syv_with_local_reserve() {
        let config = ProvidersConfig::default();
        config.validate().unwrap();
        let ids = |f| {
            config
                .chain(f)
                .iter()
                .map(|l| l.provider.as_str())
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(Function::Dictation), [SYV_ID, LOCAL_ID]);
        assert_eq!(ids(Function::Transcription), [SYV_ID, LOCAL_ID]);
        assert_eq!(ids(Function::Summary), [LOCAL_ID]);
        assert_eq!(ids(Function::Assistant), [LOCAL_ID]);
        assert!(ids(Function::DictationPolish).is_empty());
    }

    #[test]
    fn odincore_preset_uses_the_ai_domain_and_has_no_diarization() {
        let config = ProvidersConfig::default();
        let odin = config.provider(ODINCORE_ID).unwrap();
        assert_eq!(odin.base_url, "https://api.odincore.ai/v1");
        assert_eq!(
            odin.transcription_model.as_deref(),
            Some("ordbogen/whisper")
        );
        assert_eq!(odin.chat_model.as_deref(), Some("odin-2"));
        assert!(!odin.diarization);
        assert!(config.provider(SYV_ID).unwrap().diarization);
    }

    #[test]
    fn json_roundtrip_uses_stable_names() {
        let config = ProvidersConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("\"dictation-polish\":[]"));
        assert!(json.contains("\"kind\":\"syv-ai\""));
        assert!(json.contains("\"baseUrl\""));
        let back: ProvidersConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back, config);
    }

    #[test]
    fn validation_rejects_bad_input() {
        let mut config = ProvidersConfig::default();
        config
            .functions
            .insert(Function::Summary, vec![ChainLink::new("nope")]);
        assert!(config.validate().unwrap_err().contains("som ikke findes"));

        let mut config = ProvidersConfig::default();
        config
            .functions
            .insert(Function::Summary, vec![ChainLink::new(SYV_ID)]);
        assert!(config
            .validate()
            .unwrap_err()
            .contains("ikke valgt nogen model"));
        config.functions.insert(
            Function::Summary,
            vec![ChainLink::new(SYV_ID).with_model("syv-chat")],
        );
        config.validate().unwrap();

        let mut config = ProvidersConfig::default();
        config.providers.push(custom_provider(
            "Groq!",
            "Groq",
            "https://api.groq.com/openai/v1",
        ));
        assert!(config
            .validate()
            .unwrap_err()
            .contains("Ugyldigt udbyder-id"));

        let mut config = ProvidersConfig::default();
        config.provider_mut(OPENAI_ID).unwrap().base_url = "http://api.openai.com/v1".into();
        assert!(config.validate().unwrap_err().contains("https://"));

        let mut config = ProvidersConfig::default();
        config
            .providers
            .push(custom_provider("lan", "LAN", "http://10.0.0.5:8000/v1/"));
        config.validate().unwrap();
        assert_eq!(
            config.provider("lan").unwrap().base_url,
            "http://10.0.0.5:8000/v1"
        );

        let mut config = ProvidersConfig::default();
        config.providers.push(local_provider());
        assert!(config.validate().unwrap_err().contains("to gange"));
    }

    #[test]
    fn key_targets_keep_the_existing_syv_entry() {
        assert_eq!(key_target(SYV_ID), "Tingskriver/syv.ai");
        assert_eq!(key_target("groq"), "Tingskriver/udbyder/groq");
    }

    #[test]
    fn ollama_endpoint_gets_v1() {
        assert_eq!(
            ollama_provider(Some("http://localhost:11434/")).base_url,
            "http://localhost:11434/v1"
        );
        assert_eq!(
            ollama_provider(Some("http://gpu:11434/v1")).base_url,
            "http://gpu:11434/v1"
        );
        assert_eq!(ollama_provider(None).base_url, OLLAMA_BASE_URL);
    }
}
