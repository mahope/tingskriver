//! Move the old provider settings into the shared model.
//!
//! The old app kept keys in plain text in the `settings` and
//! `transcript_settings` tables and the dictation endpoint in the dictation
//! settings. [`plan`] turns them into a [`ProvidersConfig`] plus the keys to
//! store, without losing anything: every key is moved to Credential Manager,
//! also those whose provider is no longer supported (Claude, Deepgram,
//! ElevenLabs). [`apply`] stores the keys and only then clears the old
//! columns.
//!
//! The old summary code still reads keys through `SettingsRepository`; that
//! falls back to [`legacy_read_targets`] once the column is empty.

use std::collections::BTreeMap;

use sqlx::{Row, SqlitePool};

use super::model::{
    self, custom_provider, key_target, ollama_provider, ChainLink, ChatOptions, Function, Provider,
    ProviderKind, ProvidersConfig, LOCAL_ID, OPENAI_BASE_URL, OPENAI_ID, SYV_BASE_URL, SYV_ID,
};
use super::secrets::{ApiKey, KeyStore};
use crate::dictation::settings::{DictationSettings, SYV_DEFAULT_MODEL};

pub const GROQ_BASE_URL: &str = "https://api.groq.com/openai/v1";
pub const OPENROUTER_BASE_URL: &str = "https://openrouter.ai/api/v1";
pub const GEMINI_BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta/openai";

/// Key columns of the old `settings` table (summary providers).
pub const SUMMARY_KEY_COLUMNS: [&str; 6] = [
    "openaiApiKey",
    "anthropicApiKey",
    "groqApiKey",
    "ollamaApiKey",
    "openRouterApiKey",
    "geminiApiKey",
];

/// Key columns of the old `transcript_settings` table.
pub const TRANSCRIPT_KEY_COLUMNS: [&str; 5] = [
    "whisperApiKey",
    "deepgramApiKey",
    "elevenLabsApiKey",
    "groqApiKey",
    "openaiApiKey",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LegacyTable {
    /// `settings`: summary providers.
    Summary,
    /// `transcript_settings`.
    Transcript,
}

/// The old custom OpenAI settings without the key.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LegacyCustomOpenAi {
    pub endpoint: String,
    pub model: String,
    pub options: ChatOptions,
}

/// Everything the old settings held. `Debug` is safe: keys are [`ApiKey`].
#[derive(Clone, Debug, Default)]
pub struct LegacySettings {
    pub has_summary_row: bool,
    pub summary_provider: Option<String>,
    pub summary_model: Option<String>,
    pub ollama_endpoint: Option<String>,
    pub custom_openai: Option<LegacyCustomOpenAi>,
    pub custom_openai_key: Option<ApiKey>,
    /// Column name to key, `settings` table.
    pub summary_keys: BTreeMap<String, ApiKey>,
    pub has_transcript_row: bool,
    pub transcript_provider: Option<String>,
    pub transcript_model: Option<String>,
    /// Column name to key, `transcript_settings` table.
    pub transcript_keys: BTreeMap<String, ApiKey>,
}

impl LegacySettings {
    pub fn is_empty(&self) -> bool {
        !self.has_summary_row && !self.has_transcript_row
    }

    pub fn has_keys(&self) -> bool {
        !self.summary_keys.is_empty()
            || !self.transcript_keys.is_empty()
            || self.custom_openai_key.is_some()
    }
}

/// The Credential Manager target the old settings code reads for `provider`
/// in `table`, in order. Must match what [`plan`] stores.
pub fn legacy_read_targets(table: LegacyTable, provider: &str) -> Vec<String> {
    let id = |id: &str| key_target(id);
    match (table, provider) {
        (LegacyTable::Summary, "openai") => vec![id(OPENAI_ID)],
        (LegacyTable::Summary, "claude") => vec![id("anthropic")],
        (LegacyTable::Summary, "groq") => vec![id("groq")],
        (LegacyTable::Summary, "ollama") => vec![id("ollama")],
        (LegacyTable::Summary, "openrouter") => vec![id("openrouter")],
        (LegacyTable::Summary, "gemini") => vec![id("gemini")],
        (LegacyTable::Summary, "custom-openai") => vec![id("custom-openai")],
        (LegacyTable::Transcript, "openai") => vec![id("openai-transcription"), id(OPENAI_ID)],
        (LegacyTable::Transcript, "groq") => vec![id("groq-transcription"), id("groq")],
        (LegacyTable::Transcript, "localWhisper") => vec![id("legacy-whisper")],
        (LegacyTable::Transcript, "deepgram") => vec![id("legacy-deepgram")],
        (LegacyTable::Transcript, "elevenLabs") => vec![id("legacy-elevenlabs")],
        _ => Vec::new(),
    }
}

/// Where the old settings code stores a key it saves from now on.
pub fn legacy_write_target(table: LegacyTable, provider: &str) -> Option<String> {
    legacy_read_targets(table, provider).into_iter().next()
}

/// The new configuration, the keys to store and notes for the user.
#[derive(Debug)]
pub struct MigrationPlan {
    pub config: ProvidersConfig,
    /// Credential Manager target and key.
    pub keys: Vec<(String, ApiKey)>,
    /// Danish notes about settings that changed meaning.
    pub notes: Vec<String>,
}

struct Planner {
    config: ProvidersConfig,
    keys: BTreeMap<String, ApiKey>,
    notes: Vec<String>,
}

impl Planner {
    fn key(&mut self, provider_id: &str, key: &ApiKey) {
        self.keys.insert(key_target(provider_id), key.clone());
    }

    fn ensure(&mut self, provider: Provider) -> &mut Provider {
        let index = match self
            .config
            .providers
            .iter()
            .position(|p| p.id == provider.id)
        {
            Some(index) => index,
            None => {
                self.config.providers.push(provider);
                self.config.providers.len() - 1
            }
        };
        &mut self.config.providers[index]
    }
}

fn non_empty(value: &Option<String>) -> Option<&str> {
    value.as_deref().map(str::trim).filter(|v| !v.is_empty())
}

/// Turn the old settings into the new model. Pure; see [`apply`].
pub fn plan(legacy: &LegacySettings, dictation: Option<&DictationSettings>) -> MigrationPlan {
    let mut p = Planner {
        config: ProvidersConfig::default(),
        keys: BTreeMap::new(),
        notes: Vec::new(),
    };
    let summary_provider = non_empty(&legacy.summary_provider);
    let summary_model = non_empty(&legacy.summary_model).map(str::to_string);
    let transcript_provider = non_empty(&legacy.transcript_provider);
    let transcript_model = non_empty(&legacy.transcript_model).map(str::to_string);
    let s_key = |c: &str| legacy.summary_keys.get(c);
    let t_key = |c: &str| legacy.transcript_keys.get(c);

    // OpenAI and Groq had a key in both tables. One provider when they are
    // the same, a separate "(transskription)" provider when they differ.
    let mut transcription_ids: BTreeMap<&str, String> = BTreeMap::new();
    for (old, column) in [("openai", "openaiApiKey"), ("groq", "groqApiKey")] {
        let make = |id: &str, name: &str| match old {
            "openai" => Provider {
                id: id.to_string(),
                name: name.to_string(),
                kind: ProviderKind::OpenAi,
                base_url: OPENAI_BASE_URL.to_string(),
                transcription_model: Some("whisper-1".to_string()),
                chat_model: Some("gpt-4o-mini".to_string()),
                diarization: false,
                chat_options: ChatOptions::default(),
            },
            _ => {
                let mut groq = custom_provider(id, name, GROQ_BASE_URL);
                groq.transcription_model = Some("whisper-large-v3".to_string());
                groq
            }
        };
        let (base_name, base_id) = if old == "openai" {
            ("OpenAI", OPENAI_ID)
        } else {
            ("Groq", "groq")
        };
        let summary = s_key(column);
        let transcript = t_key(column);
        let chosen = summary_provider == Some(old) || transcript_provider == Some(old);
        if summary.is_some() || transcript.is_some() || chosen {
            p.ensure(make(base_id, base_name));
        }
        if let Some(key) = summary.or(transcript) {
            p.key(base_id, key);
        }
        transcription_ids.insert(old, base_id.to_string());
        if let (Some(a), Some(b)) = (summary, transcript) {
            if a != b {
                let id = format!("{base_id}-transcription");
                p.ensure(make(&id, &format!("{base_name} (transskription)")));
                p.key(&id, b);
                transcription_ids.insert(old, id);
            }
        }
    }

    if s_key("openRouterApiKey").is_some() || summary_provider == Some("openrouter") {
        p.ensure(custom_provider(
            "openrouter",
            "OpenRouter",
            OPENROUTER_BASE_URL,
        ));
    }
    if let Some(key) = s_key("openRouterApiKey") {
        p.key("openrouter", key);
    }
    if s_key("geminiApiKey").is_some() || summary_provider == Some("gemini") {
        p.ensure(custom_provider("gemini", "Google Gemini", GEMINI_BASE_URL));
    }
    if let Some(key) = s_key("geminiApiKey") {
        p.key("gemini", key);
    }

    let ollama_endpoint = non_empty(&legacy.ollama_endpoint);
    if s_key("ollamaApiKey").is_some()
        || ollama_endpoint.is_some()
        || summary_provider == Some("ollama")
    {
        p.ensure(ollama_provider(ollama_endpoint));
    }
    if let Some(key) = s_key("ollamaApiKey") {
        p.key("ollama", key);
    }

    if let Some(custom) = &legacy.custom_openai {
        let provider = p.ensure(custom_provider(
            "custom-openai",
            "Brugerdefineret",
            &custom.endpoint,
        ));
        provider.chat_model = Some(custom.model.clone()).filter(|m| !m.trim().is_empty());
        provider.chat_options = custom.options.clone();
    }
    if let Some(key) = &legacy.custom_openai_key {
        p.key("custom-openai", key);
    }

    if let Some(key) = s_key("anthropicApiKey") {
        p.key("anthropic", key);
        p.notes.push(
            "Claude understøttes ikke længere, fordi Anthropics API ikke er OpenAI-kompatibelt. \
             Nøglen er gemt i Windows Legitimationsstyring."
                .to_string(),
        );
    }
    for (column, id, name) in [
        ("whisperApiKey", "legacy-whisper", "Whisper-API"),
        ("deepgramApiKey", "legacy-deepgram", "Deepgram"),
        ("elevenLabsApiKey", "legacy-elevenlabs", "ElevenLabs"),
    ] {
        if let Some(key) = t_key(column) {
            p.key(id, key);
            p.notes.push(format!(
                "{name} understøttes ikke længere til transskription. Nøglen er gemt i Windows Legitimationsstyring."
            ));
        }
    }

    // Summary and assistant: the old choice first, the local model as reserve.
    if legacy.has_summary_row {
        let first = match summary_provider {
            Some("builtin-ai") | Some("local-llama") | Some("localllama") => None,
            Some("claude") => {
                p.notes.push(
                    "Referat bruger nu den indbyggede lokale model i stedet for Claude."
                        .to_string(),
                );
                None
            }
            Some("custom-openai") => Some((
                "custom-openai".to_string(),
                legacy.custom_openai.as_ref().map(|c| c.model.clone()),
            )),
            Some(old @ ("openai" | "groq" | "openrouter" | "ollama" | "gemini")) => {
                Some((old.to_string(), summary_model.clone()))
            }
            Some(other) => {
                p.notes.push(format!("Den gamle referat-udbyder \"{other}\" kendes ikke; referat bruger den lokale model."));
                None
            }
            None => None,
        };
        let mut chain = Vec::new();
        if let Some((id, model)) = first {
            if let Some(provider) = p.config.provider(&id) {
                let model = model.filter(|m| !m.trim().is_empty());
                if model.is_some() || provider.chat_model.is_some() {
                    chain.push(ChainLink::new(&id).with_model(model.unwrap_or_default()));
                } else {
                    p.notes.push(format!(
                        "{} havde ingen model valgt; referat bruger den lokale model.",
                        provider.name
                    ));
                }
            }
        }
        let builtin = matches!(
            summary_provider,
            Some("builtin-ai" | "local-llama" | "localllama")
        );
        let local_model = if builtin { summary_model.clone() } else { None };
        chain.push(ChainLink::new(LOCAL_ID).with_model(local_model.unwrap_or_default()));
        p.config.functions.insert(Function::Summary, chain.clone());
        p.config.functions.insert(Function::Assistant, chain);
    }

    // Meeting transcription: keep the old choice. Syv.ai is not switched on
    // for meetings behind the user's back, even if dictation has a key.
    if legacy.has_transcript_row {
        let mut chain = Vec::new();
        match transcript_provider {
            Some(old @ ("openai" | "groq")) => {
                let id = transcription_ids
                    .get(old)
                    .cloned()
                    .unwrap_or_else(|| old.to_string());
                chain.push(
                    ChainLink::new(&id).with_model(transcript_model.clone().unwrap_or_default()),
                );
                chain.push(ChainLink::new(LOCAL_ID));
            }
            Some(old @ ("deepgram" | "elevenLabs")) => {
                p.notes.push(format!(
                    "Transskription med {old} understøttes ikke længere; møder transskriberes med den lokale model."
                ));
                chain.push(ChainLink::new(LOCAL_ID));
            }
            _ => chain.push(
                ChainLink::new(LOCAL_ID).with_model(transcript_model.clone().unwrap_or_default()),
            ),
        }
        p.config.functions.insert(Function::Transcription, chain);
    }

    if let Some(d) = dictation {
        plan_dictation(&mut p, d);
    }

    MigrationPlan {
        config: p.config,
        keys: p.keys.into_iter().collect(),
        notes: p.notes,
    }
}

fn plan_dictation(p: &mut Planner, d: &DictationSettings) {
    let endpoint = model::normalize_base_url(&d.syv_endpoint);
    if !endpoint.is_empty() && endpoint != SYV_BASE_URL {
        if let Some(syv) = p.config.provider_mut(SYV_ID) {
            syv.base_url = endpoint;
        }
    }
    let mut chain = Vec::new();
    if d.engine != "local" {
        let mut link = ChainLink::new(SYV_ID).with_timeout_ms(d.fallback_after_ms.max(1));
        if !d.syv_model.trim().is_empty() && d.syv_model != SYV_DEFAULT_MODEL {
            link = link.with_model(d.syv_model.clone());
        }
        chain.push(link);
    }
    chain.push(ChainLink::new(LOCAL_ID));
    p.config.functions.insert(Function::Dictation, chain);

    let mut polish = Vec::new();
    if d.polish_enabled {
        let budget = d.polish_budget_ms.max(1);
        if d.polish_provider == "ollama" {
            let wanted = ollama_provider(
                Some(&d.polish_endpoint)
                    .filter(|e| !e.trim().is_empty())
                    .map(String::as_str),
            );
            let id = match p.config.provider("ollama") {
                None => {
                    p.config.providers.push(wanted);
                    "ollama".to_string()
                }
                Some(existing)
                    if existing.base_url == wanted.base_url
                        || d.polish_endpoint.trim().is_empty() =>
                {
                    "ollama".to_string()
                }
                Some(_) => {
                    p.config.providers.push(Provider {
                        id: "ollama-polish".to_string(),
                        name: "Ollama (diktering)".to_string(),
                        ..wanted
                    });
                    "ollama-polish".to_string()
                }
            };
            if d.polish_model.trim().is_empty() {
                p.notes.push(
                    "Polering via Ollama havde ingen model valgt og er slået fra.".to_string(),
                );
            } else {
                polish.push(
                    ChainLink::new(&id)
                        .with_model(d.polish_model.clone())
                        .with_timeout_ms(budget),
                );
            }
        } else {
            polish.push(
                ChainLink::new(LOCAL_ID)
                    .with_model(d.polish_model.clone())
                    .with_timeout_ms(budget),
            );
        }
    }
    p.config.functions.insert(Function::DictationPolish, polish);
}

fn optional_string(row: &sqlx::sqlite::SqliteRow, column: &str) -> Option<String> {
    row.try_get::<Option<String>, _>(column).ok().flatten()
}

/// Read the old settings tables. Missing tables, rows or columns count as
/// not set.
pub async fn read_legacy(pool: &SqlitePool) -> Result<LegacySettings, String> {
    let mut legacy = LegacySettings::default();

    let row = sqlx::query("SELECT * FROM settings WHERE id = '1' LIMIT 1")
        .fetch_optional(pool)
        .await;
    if let Ok(Some(row)) = row {
        legacy.has_summary_row = true;
        legacy.summary_provider = optional_string(&row, "provider");
        legacy.summary_model = optional_string(&row, "model");
        legacy.ollama_endpoint = optional_string(&row, "ollamaEndpoint");
        for column in SUMMARY_KEY_COLUMNS {
            if let Some(key) = optional_string(&row, column).and_then(ApiKey::new) {
                legacy.summary_keys.insert(column.to_string(), key);
            }
        }
        if let Some(json) = optional_string(&row, "customOpenAIConfig") {
            match serde_json::from_str::<crate::summary::CustomOpenAIConfig>(&json) {
                Ok(c) => {
                    legacy.custom_openai_key = c.api_key.clone().and_then(ApiKey::new);
                    legacy.custom_openai = Some(LegacyCustomOpenAi {
                        endpoint: c.endpoint,
                        model: c.model,
                        options: ChatOptions {
                            max_tokens: c.max_tokens,
                            temperature: c.temperature,
                            top_p: c.top_p,
                        },
                    });
                }
                Err(e) => log::warn!(
                    "Providers: old custom OpenAI settings unreadable, skipped: {}",
                    e.classify_name()
                ),
            }
        }
    }

    let row = sqlx::query("SELECT * FROM transcript_settings WHERE id = '1' LIMIT 1")
        .fetch_optional(pool)
        .await;
    if let Ok(Some(row)) = row {
        legacy.has_transcript_row = true;
        legacy.transcript_provider = optional_string(&row, "provider");
        legacy.transcript_model = optional_string(&row, "model");
        for column in TRANSCRIPT_KEY_COLUMNS {
            if let Some(key) = optional_string(&row, column).and_then(ApiKey::new) {
                legacy.transcript_keys.insert(column.to_string(), key);
            }
        }
    }
    Ok(legacy)
}

trait ClassifyName {
    fn classify_name(&self) -> &'static str;
}

impl ClassifyName for serde_json::Error {
    /// The category only: the message could quote the JSON, which held a key.
    fn classify_name(&self) -> &'static str {
        match self.classify() {
            serde_json::error::Category::Io => "io",
            serde_json::error::Category::Syntax => "syntax",
            serde_json::error::Category::Data => "data",
            serde_json::error::Category::Eof => "eof",
        }
    }
}

/// Store the keys, then clear them from the old tables. If a key cannot be
/// stored, nothing is cleared and the error is returned, so the migration
/// can run again later without losing a key.
pub async fn apply(
    plan: &MigrationPlan,
    store: &dyn KeyStore,
    pool: Option<&SqlitePool>,
    legacy: &LegacySettings,
) -> Result<(), String> {
    for (target, key) in &plan.keys {
        store.set(target, key).map_err(|e| {
            format!("Nøglen til {target} kunne ikke gemmes i Windows Legitimationsstyring: {e}")
        })?;
    }
    if let Some(pool) = pool {
        clear_legacy_keys(pool, legacy).await?;
    }
    Ok(())
}

/// NULL the key columns that held a value, and drop `apiKey` from the custom
/// OpenAI JSON.
pub async fn clear_legacy_keys(pool: &SqlitePool, legacy: &LegacySettings) -> Result<(), String> {
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    for column in legacy.summary_keys.keys() {
        if SUMMARY_KEY_COLUMNS.contains(&column.as_str()) {
            sqlx::query(&format!(
                "UPDATE settings SET \"{column}\" = NULL WHERE id = '1'"
            ))
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
        }
    }
    for column in legacy.transcript_keys.keys() {
        if TRANSCRIPT_KEY_COLUMNS.contains(&column.as_str()) {
            sqlx::query(&format!(
                "UPDATE transcript_settings SET \"{column}\" = NULL WHERE id = '1'"
            ))
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
        }
    }
    if legacy.custom_openai_key.is_some() {
        let json: Option<String> =
            sqlx::query_scalar("SELECT customOpenAIConfig FROM settings WHERE id = '1'")
                .fetch_optional(&mut *tx)
                .await
                .map_err(|e| e.to_string())?
                .flatten();
        if let Some(json) = json {
            if let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&json) {
                value["apiKey"] = serde_json::Value::Null;
                sqlx::query("UPDATE settings SET customOpenAIConfig = $1 WHERE id = '1'")
                    .bind(value.to_string())
                    .execute(&mut *tx)
                    .await
                    .map_err(|e| e.to_string())?;
            }
        }
    }
    tx.commit().await.map_err(|e| e.to_string())
}

/// For the old settings code (`SettingsRepository`): the key it would have
/// found in its column, now read from Credential Manager.
pub fn legacy_key(table: LegacyTable, provider: &str) -> Option<String> {
    let store = super::secrets::CredentialManager;
    legacy_read_targets(table, provider)
        .iter()
        .find_map(|target| match store.get(target) {
            Ok(key) => key.map(|k| k.expose().to_string()),
            Err(e) => {
                log::warn!("Providers: could not read the key for {provider}: {e}");
                None
            }
        })
}

/// For the old settings code: store a key it saves in Credential Manager.
/// `false` when that is not possible (not Windows); the caller then keeps
/// its old column.
pub fn store_legacy_key(table: LegacyTable, provider: &str, key: &str) -> bool {
    let (Some(target), Some(key)) = (legacy_write_target(table, provider), ApiKey::new(key)) else {
        return false;
    };
    match super::secrets::CredentialManager.set(&target, &key) {
        Ok(()) => true,
        Err(e) => {
            log::debug!("Providers: key for {provider} kept in the database: {e}");
            false
        }
    }
}

/// For the old settings code: delete a key it deletes.
pub fn delete_legacy_key(table: LegacyTable, provider: &str) {
    for target in legacy_read_targets(table, provider) {
        if let Err(e) = super::secrets::CredentialManager.delete(&target) {
            log::warn!("Providers: could not delete the key for {provider}: {e}");
        }
    }
}
