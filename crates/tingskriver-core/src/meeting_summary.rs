//! Making, stopping and editing the summary of one meeting, and the saved
//! choices behind the Opsummering settings tab.
//!
//! The GPUI app calls these functions where the Tauri app calls its
//! commands (`useSummaryGeneration.ts`, `ModelSettingsModal.tsx`). Nothing
//! here knows about a UI: the app polls the stored summary
//! ([`crate::meeting_document::load`]) while a run is going.
//!
//! * The model, the keys and the Ollama address are the same database rows
//!   the Tauri app writes ([`SettingsRepository`]).
//! * "Automatisk opsummering" and the quick-pick languages were kept in the
//!   Tauri webview's `localStorage`, which only the webview can read. This app
//!   keeps them in `summary_preferences.json` in the app data folder.

use std::path::Path;

use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::database::manager::DatabaseManager;
use crate::database::models::Transcript;
use crate::database::repositories::{
    meeting::MeetingsRepository, setting::SettingsRepository, summary::SummaryProcessesRepository,
};
use crate::host::HostHandle;
use crate::state::AppState;
use crate::strings::meeting_summary as text;
use crate::summary::commands;
use crate::summary::language_detection::{detect_summary_language, SummaryLanguageDetectionReason};
use crate::summary::summary_engine::{ModelManagerState, ModelStatus};

/// The settings file for the choices Tauri kept in `localStorage`.
pub const PREFERENCES_FILE: &str = "summary_preferences.json";

/// The summary providers, in Tauri's order (`ModelSettingsModal.tsx:878-884`).
pub const PROVIDERS: &[&str] = &[
    "builtin-ai",
    "claude",
    "custom-openai",
    "groq",
    "ollama",
    "openai",
    "openrouter",
];

/// Tauri's fallback when nothing is saved (`ConfigContext.tsx:101`).
pub const DEFAULT_PROVIDER: &str = "ollama";
pub const DEFAULT_MODEL: &str = "llama3.2:latest";
/// The whisper column of the settings row, kept for the old schema.
const DEFAULT_WHISPER: &str = "large-v3";
/// The template Tauri starts with (`useTemplates.ts`).
pub const DEFAULT_TEMPLATE: &str = "standard";
/// The model Tauri falls back to when auto-summary finds nothing saved.
const GEMMA: &str = "gemma3:1b";
/// How many quick-pick languages there can be (`useRecentLanguages.ts`).
pub const MAX_RECENT_LANGUAGES: usize = 5;
/// Tauri's chunking (`useSummaryGeneration.ts`).
const CHUNK_SIZE: i32 = 40_000;
const OVERLAP: i32 = 1_000;

/// Summary languages with their Danish names (`lib/summary-languages.ts`).
pub const LANGUAGES: &[(&str, &str)] = &[
    ("en", "Engelsk"),
    ("zh", "Kinesisk"),
    ("zh-tw", "Traditionelt kinesisk"),
    ("de", "Tysk"),
    ("es", "Spansk"),
    ("ru", "Russisk"),
    ("ko", "Koreansk"),
    ("fr", "Fransk"),
    ("ja", "Japansk"),
    ("pt", "Portugisisk"),
    ("it", "Italiensk"),
    ("nl", "Hollandsk"),
    ("pl", "Polsk"),
    ("ar", "Arabisk"),
    ("hi", "Hindi"),
    ("ta", "Tamil"),
    ("tr", "Tyrkisk"),
    ("vi", "Vietnamesisk"),
    ("th", "Thailandsk"),
    ("id", "Indonesisk"),
    ("sv", "Svensk"),
    ("cs", "Tjekkisk"),
    ("da", "Dansk"),
    ("fi", "Finsk"),
    ("el", "Græsk"),
    ("he", "Hebraisk"),
    ("hu", "Ungarsk"),
    ("no", "Norsk"),
    ("ro", "Rumænsk"),
    ("uk", "Ukrainsk"),
];

/// Danish name of a summary language code; an unknown code is shown as is.
pub fn language_label(code: &str) -> String {
    LANGUAGES
        .iter()
        .find(|(c, _)| *c == code)
        .map(|(_, label)| (*label).to_string())
        .unwrap_or_else(|| code.to_string())
}

/// A raw locale (`pt-BR`, `en_GB`) as a supported code, else `None`
/// (`normaliseLanguageCode`).
pub fn normalise_language(raw: &str) -> Option<String> {
    let lower = raw.trim().to_lowercase().replace('_', "-");
    let known = |code: &str| LANGUAGES.iter().any(|(c, _)| *c == code);
    if known(&lower) {
        return Some(lower);
    }
    let base = lower.split('-').next().unwrap_or_default();
    known(base).then(|| base.to_string())
}

/// Providers that keep an API key in the settings (not the built-in model,
/// Ollama or the own server, whose key is part of its settings).
pub fn needs_key(provider: &str) -> bool {
    matches!(provider, "claude" | "groq" | "openai" | "openrouter")
}

// ---------------------------------------------------------------------------
// Preferences kept in the app data folder
// ---------------------------------------------------------------------------

/// What Tauri kept in `localStorage`: `isAutoSummary`,
/// `summaryLanguageRecents` and `summaryLanguageDefault`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Preferences {
    /// Make the summary when a recording has been transcribed. Off by
    /// default, as in Tauri (`ConfigContext.tsx`).
    pub auto_summary: bool,
    /// Quick-pick languages, newest first, at most five.
    pub language_recents: Vec<String>,
    /// The language pinned as the default for new meetings.
    pub language_default: Option<String>,
}

impl Preferences {
    /// Read the file. A missing file is the defaults; a file that cannot be
    /// read is an error, so a broken file is never mistaken for "off".
    #[allow(clippy::disallowed_methods)] // Synchronous disk worker.
    pub fn load(dir: &Path) -> Result<Preferences, String> {
        let path = dir.join(PREFERENCES_FILE);
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Preferences::default())
            }
            Err(error) => return Err(error.to_string()),
        };
        let mut preferences: Preferences =
            serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
        preferences.clean();
        Ok(preferences)
    }

    /// Write the file through a temporary file, so a crash never leaves half
    /// a file behind.
    #[allow(clippy::disallowed_methods)] // Synchronous disk worker.
    pub fn save(&self, dir: &Path) -> Result<(), String> {
        let mut clean = self.clone();
        clean.clean();
        let json = serde_json::to_vec_pretty(&clean).map_err(|error| error.to_string())?;
        std::fs::create_dir_all(dir).map_err(|error| error.to_string())?;
        let path = dir.join(PREFERENCES_FILE);
        let temporary = dir.join(format!("{PREFERENCES_FILE}.tmp"));
        std::fs::write(&temporary, json).map_err(|error| error.to_string())?;
        std::fs::rename(&temporary, &path).map_err(|error| {
            let _ = std::fs::remove_file(&temporary);
            error.to_string()
        })
    }

    pub async fn load_async(dir: std::path::PathBuf) -> Result<Preferences, String> {
        tokio::task::spawn_blocking(move || Preferences::load(&dir))
            .await
            .map_err(|error| error.to_string())?
    }

    pub async fn save_async(self, dir: std::path::PathBuf) -> Result<(), String> {
        tokio::task::spawn_blocking(move || self.save(&dir))
            .await
            .map_err(|error| error.to_string())?
    }

    /// Only supported codes, no repeats, at most five; the pinned language
    /// is one of the supported ones.
    fn clean(&mut self) {
        let mut recents: Vec<String> = Vec::new();
        for code in &self.language_recents {
            if let Some(code) = normalise_language(code) {
                if !recents.contains(&code) {
                    recents.push(code);
                }
            }
            if recents.len() == MAX_RECENT_LANGUAGES {
                break;
            }
        }
        self.language_recents = recents;
        self.language_default = self
            .language_default
            .as_deref()
            .and_then(normalise_language);
    }

    /// `addRecent`: to the front, without repeats.
    pub fn add_recent(&mut self, code: &str) {
        let Some(code) = normalise_language(code) else {
            return;
        };
        self.language_recents.retain(|c| *c != code);
        self.language_recents.insert(0, code);
        self.language_recents.truncate(MAX_RECENT_LANGUAGES);
    }

    /// `removeRecent`: also clears the pin when it was this language.
    pub fn remove_recent(&mut self, code: &str) {
        let code = normalise_language(code).unwrap_or_else(|| code.to_string());
        self.language_recents.retain(|c| *c != code);
        if self.language_default.as_deref() == Some(code.as_str()) {
            self.language_default = None;
        }
    }

    /// `setPinned`: clicking the pinned language again removes the pin.
    pub fn toggle_pinned(&mut self, code: &str) {
        let Some(code) = normalise_language(code) else {
            return;
        };
        if self.language_default.as_deref() == Some(code.as_str()) {
            self.language_default = None;
        } else {
            self.language_default = Some(code);
        }
    }
}

// ---------------------------------------------------------------------------
// The saved model
// ---------------------------------------------------------------------------

/// The provider and model summaries are made with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelChoice {
    pub provider: String,
    pub model: String,
    /// Address of the Ollama server; `None` is the local one.
    pub ollama_endpoint: Option<String>,
}

/// The saved choice, or `None` when nothing is saved. For the own server the
/// model is the one in its settings, as Tauri's `ConfigContext` reads it.
pub async fn read_model(pool: &SqlitePool) -> Result<Option<ModelChoice>, String> {
    let Some(setting) = SettingsRepository::get_model_config(pool)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };
    if setting.provider.trim().is_empty() {
        return Ok(None);
    }
    let mut model = setting.model;
    if setting.provider == "custom-openai" {
        if let Some(config) = SettingsRepository::get_custom_openai_config(pool)
            .await
            .map_err(|error| error.to_string())?
        {
            model = config.model;
        }
    }
    Ok(Some(ModelChoice {
        provider: setting.provider,
        model,
        ollama_endpoint: setting
            .ollama_endpoint
            .filter(|endpoint| !endpoint.trim().is_empty()),
    }))
}

/// The choice a run uses: the saved one, else Tauri's default.
pub async fn effective_model(pool: &SqlitePool) -> Result<ModelChoice, String> {
    Ok(read_model(pool).await?.unwrap_or(ModelChoice {
        provider: DEFAULT_PROVIDER.into(),
        model: DEFAULT_MODEL.into(),
        ollama_endpoint: None,
    }))
}

/// Save provider and model (`api_save_model_config`). The old whisper column
/// and, unless `choice` brings one, the Ollama address stay as they are.
pub async fn save_model(pool: &SqlitePool, choice: &ModelChoice) -> Result<(), String> {
    if !PROVIDERS.contains(&choice.provider.as_str()) {
        return Err(format!("Ukendt udbyder: {}", choice.provider));
    }
    let existing = SettingsRepository::get_model_config(pool)
        .await
        .map_err(|error| error.to_string())?;
    let whisper = existing
        .as_ref()
        .map(|s| s.whisper_model.clone())
        .filter(|w| !w.is_empty())
        .unwrap_or_else(|| DEFAULT_WHISPER.to_string());
    let endpoint = choice
        .ollama_endpoint
        .clone()
        .or_else(|| existing.and_then(|s| s.ollama_endpoint))
        .filter(|endpoint| !endpoint.trim().is_empty());
    SettingsRepository::save_model_config(
        pool,
        &choice.provider,
        &choice.model,
        &whisper,
        endpoint.as_deref(),
    )
    .await
    .map_err(|error| error.to_string())
}

/// Whether a key is stored for the provider. The key itself never leaves
/// core.
pub async fn has_key(pool: &SqlitePool, provider: &str) -> Result<bool, String> {
    Ok(SettingsRepository::get_api_key(pool, provider)
        .await
        .map_err(|error| error.to_string())?
        .is_some_and(|key| !key.trim().is_empty()))
}

/// Store the provider's key (Windows Credential Manager when possible).
pub async fn save_key(pool: &SqlitePool, provider: &str, key: &str) -> Result<(), String> {
    if !needs_key(provider) {
        return Err(format!("{provider} bruger ingen API-nøgle her"));
    }
    if key.trim().is_empty() {
        return Err(text::SUMMARY_KEY_SAVE_FAILED.to_string());
    }
    SettingsRepository::save_api_key(pool, provider, key.trim())
        .await
        .map_err(|error| error.to_string())
}

/// The own OpenAI-compatible server: address and model (the key is only
/// reported as present).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CustomServer {
    pub endpoint: String,
    pub model: String,
    pub has_key: bool,
}

pub async fn read_custom_server(pool: &SqlitePool) -> Result<CustomServer, String> {
    Ok(SettingsRepository::get_custom_openai_config(pool)
        .await
        .map_err(|error| error.to_string())?
        .map(|config| CustomServer {
            endpoint: config.endpoint,
            model: config.model,
            has_key: config.api_key.is_some_and(|key| !key.trim().is_empty()),
        })
        .unwrap_or_default())
}

/// Save the own server (`api_save_custom_openai_config`). An empty `key`
/// keeps the stored one; the other limits are left as they were.
pub async fn save_custom_server(
    pool: &SqlitePool,
    endpoint: &str,
    model: &str,
    key: &str,
) -> Result<(), String> {
    if endpoint.trim().is_empty() || model.trim().is_empty() {
        return Err(text::SUMMARY_CUSTOM_REQUIRED.to_string());
    }
    let endpoint = endpoint.trim();
    if !endpoint.starts_with("http://") && !endpoint.starts_with("https://") {
        return Err(text::SUMMARY_CUSTOM_SCHEME.to_string());
    }
    let existing = SettingsRepository::get_custom_openai_config(pool)
        .await
        .map_err(|error| error.to_string())?;
    let api_key = match key.trim() {
        "" => existing.as_ref().and_then(|c| c.api_key.clone()),
        key => Some(key.to_string()),
    };
    let (max_tokens, temperature, top_p) = existing
        .map(|c| (c.max_tokens, c.temperature, c.top_p))
        .unwrap_or_default();
    let config = crate::summary::CustomOpenAIConfig {
        endpoint: endpoint.to_string(),
        api_key,
        model: model.trim().to_string(),
        max_tokens,
        temperature,
        top_p,
    };
    SettingsRepository::save_custom_openai_config(pool, &config)
        .await
        .map_err(|error| error.to_string())
}

/// One entry of the model list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelOption {
    pub id: String,
    pub label: String,
}

/// The list for one provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelList {
    Models(Vec<ModelOption>),
    /// The provider needs a key before it lists models.
    NeedsKey,
    Failed(String),
}

static BUILTIN_MODELS: std::sync::LazyLock<ModelManagerState> = std::sync::LazyLock::new(|| {
    ModelManagerState(std::sync::Arc::new(tokio::sync::Mutex::new(None)))
});

/// The models the provider offers (Tauri's `get_ollama_models`,
/// `get_openrouter_models`, `get_groq_models`, `get_anthropic_models`,
/// `get_openai_models` and `builtin_ai_list_models`). Needs the network for
/// every provider but the built-in one.
pub async fn model_list(
    host: HostHandle,
    pool: &SqlitePool,
    provider: &str,
    ollama_endpoint: Option<String>,
) -> ModelList {
    let plain = |ids: Vec<String>| {
        ModelList::Models(
            ids.into_iter()
                .map(|id| ModelOption {
                    label: id.clone(),
                    id,
                })
                .collect(),
        )
    };
    let key = match provider {
        "claude" | "groq" | "openai" => match SettingsRepository::get_api_key(pool, provider).await
        {
            Ok(Some(key)) if !key.trim().is_empty() => Some(key),
            Ok(_) => return ModelList::NeedsKey,
            Err(error) => return ModelList::Failed(error.to_string()),
        },
        _ => None,
    };
    match provider {
        "ollama" => match crate::ollama::get_ollama_models(ollama_endpoint).await {
            Ok(models) => plain(models.into_iter().map(|m| m.name).collect()),
            Err(error) => ModelList::Failed(error),
        },
        "openrouter" => match crate::openrouter::get_openrouter_models().await {
            Ok(models) => ModelList::Models(
                models
                    .into_iter()
                    .map(|m| ModelOption {
                        label: if m.name.is_empty() {
                            m.id.clone()
                        } else {
                            m.name
                        },
                        id: m.id,
                    })
                    .collect(),
            ),
            Err(error) => ModelList::Failed(error),
        },
        "groq" => match crate::groq::groq::get_groq_models(key).await {
            Ok(models) => plain(models.into_iter().map(|m| m.id).collect()),
            Err(error) => ModelList::Failed(error),
        },
        "claude" => match crate::anthropic::anthropic::get_anthropic_models(key).await {
            Ok(models) => plain(models.into_iter().map(|m| m.id).collect()),
            Err(error) => ModelList::Failed(error),
        },
        "openai" => match crate::openai::openai::get_openai_models(key).await {
            Ok(models) => plain(models.into_iter().map(|m| m.id).collect()),
            Err(error) => ModelList::Failed(error),
        },
        "builtin-ai" => {
            match crate::summary::summary_engine::commands::builtin_ai_list_models(
                host,
                &BUILTIN_MODELS,
            )
            .await
            {
                Ok(models) => ModelList::Models(
                    models
                        .into_iter()
                        .map(|m| ModelOption {
                            label: match m.status {
                                ModelStatus::Available => m.display_name,
                                ModelStatus::Downloading { .. } => format!(
                                    "{} ({})",
                                    m.display_name,
                                    text::SUMMARY_BUILTIN_DOWNLOADING.to_lowercase()
                                ),
                                _ => format!(
                                    "{} ({})",
                                    m.display_name,
                                    text::SUMMARY_BUILTIN_NOT_DOWNLOADED.to_lowercase()
                                ),
                            },
                            id: m.name,
                        })
                        .collect(),
                ),
                Err(error) => ModelList::Failed(error),
            }
        }
        _ => ModelList::Models(Vec::new()),
    }
}

// ---------------------------------------------------------------------------
// Templates and the language of one meeting
// ---------------------------------------------------------------------------

/// A summary template (`api_list_templates`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
    pub id: String,
    pub name: String,
    pub description: String,
}

pub async fn templates() -> Result<Vec<Template>, String> {
    Ok(crate::summary::template_commands::api_list_templates()
        .await?
        .into_iter()
        .map(|t| Template {
            id: t.id,
            name: t.name,
            description: t.description,
        })
        .collect())
}

/// The template to start with: the chosen one if it exists, else `standard`,
/// else the first (`useTemplates.ts`).
pub fn pick_template(templates: &[Template], current: &str) -> String {
    if templates.iter().any(|t| t.id == current) {
        return current.to_string();
    }
    templates
        .iter()
        .find(|t| t.id == DEFAULT_TEMPLATE)
        .or_else(|| templates.first())
        .map(|t| t.id.clone())
        .unwrap_or_else(|| current.to_string())
}

/// The summary language saved for a meeting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeetingLanguage {
    /// `None` is Automatisk.
    pub language: Option<String>,
    /// The meeting has no folder, so the choice cannot be stored with it.
    pub local_only: bool,
}

pub async fn read_language(
    db: &DatabaseManager,
    meeting_id: &str,
) -> Result<MeetingLanguage, String> {
    let state = AppState {
        db_manager: db.clone(),
    };
    let saved = commands::api_get_meeting_summary_language(&state, meeting_id.to_string()).await?;
    Ok(MeetingLanguage {
        language: saved.language.as_deref().and_then(normalise_language),
        local_only: saved.storage == commands::SummaryLanguageStorage::LocalFallback,
    })
}

/// Save (or clear, with `None`) the language of a meeting
/// (`saveMeetingSummaryLanguage`).
pub async fn save_language(
    db: &DatabaseManager,
    meeting_id: &str,
    language: Option<&str>,
) -> Result<MeetingLanguage, String> {
    let normalised = language.and_then(normalise_language);
    let state = AppState {
        db_manager: db.clone(),
    };
    let saved = commands::api_save_meeting_summary_language(
        &state,
        meeting_id.to_string(),
        normalised.clone(),
    )
    .await?;
    let local_only = saved.storage == commands::SummaryLanguageStorage::LocalFallback;
    Ok(MeetingLanguage {
        language: if local_only {
            normalised
        } else {
            saved.language.as_deref().and_then(normalise_language)
        },
        local_only,
    })
}

// ---------------------------------------------------------------------------
// Running a summary
// ---------------------------------------------------------------------------

/// The transcript as Tauri sends it to the model
/// (`buildSummaryTranscriptPayload`): `[MM:SS] speaker: text` per line, and
/// the bare texts for the language detection.
pub fn transcript_payload(rows: &[Transcript]) -> (String, Vec<String>) {
    let time = |row: &Transcript| match row.audio_start_time {
        Some(seconds) => {
            let total = seconds.max(0.0).floor() as u64;
            format!("[{:02}:{:02}]", total / 60, total % 60)
        }
        None => row.timestamp.clone(),
    };
    let text = rows
        .iter()
        .map(|row| {
            let speaker = row
                .speaker
                .as_deref()
                .filter(|s| !s.is_empty() && !matches!(*s, "mic" | "system"))
                .map(|s| format!("{s}: "))
                .unwrap_or_default();
            format!("{} {speaker}{}", time(row), row.transcript)
        })
        .collect::<Vec<_>>()
        .join("\n");
    (text, rows.iter().map(|r| r.transcript.clone()).collect())
}

/// The check before a run (`handleGenerateSummary`): the text to show, or
/// `None` when the model can be used. `ollama` is the number of models
/// Ollama lists and `builtin_ready` whether the built-in model is installed;
/// each is `Err` when the check itself failed.
pub fn preflight_message(
    provider: &str,
    model: &str,
    ollama: Option<Result<usize, String>>,
    builtin_ready: Option<Result<bool, String>>,
) -> Option<&'static str> {
    match provider {
        "ollama" => match ollama {
            Some(Ok(0)) => Some(text::SUMMARY_OLLAMA_NONE),
            Some(Err(_)) => Some(text::SUMMARY_MODEL_CHECK_FAILED),
            _ => None,
        },
        "builtin-ai" => {
            if model.is_empty() {
                return Some(text::SUMMARY_BUILTIN_NO_MODEL);
            }
            match builtin_ready {
                Some(Ok(false)) => Some(text::SUMMARY_BUILTIN_NOT_READY),
                Some(Err(_)) => Some(text::SUMMARY_MODEL_CHECK_FAILED),
                _ => None,
            }
        }
        _ if model.trim().is_empty() => Some(text::SUMMARY_NO_MODEL),
        _ => None,
    }
}

/// A message to show next to a started run (a Tauri warning toast).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub title: &'static str,
    pub help: &'static str,
}

/// A run that has started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Started {
    /// The start time of the run; names it for [`cancel`].
    pub process_id: String,
    pub provider: String,
    pub model: String,
    /// Warnings found while choosing the language.
    pub notices: Vec<Notice>,
}

/// The language a run writes in (`resolveSummaryLanguage`): the one saved
/// for the meeting, else the detected one stored earlier, else a detection
/// from the transcript, which is then stored. `session` is the choice of a
/// meeting that cannot store one.
async fn resolve_language(
    state: &AppState,
    meeting_id: &str,
    texts: &[String],
    session: Option<&str>,
    notices: &mut Vec<Notice>,
) -> Option<String> {
    match commands::api_get_meeting_summary_language(state, meeting_id.to_string()).await {
        Ok(saved) => {
            if let Some(language) = saved.language.as_deref().and_then(normalise_language) {
                return Some(language);
            }
            if saved.storage == commands::SummaryLanguageStorage::LocalFallback {
                if let Some(language) = session.and_then(normalise_language) {
                    return Some(language);
                }
            }
        }
        Err(error) => {
            log::warn!("Could not read the summary language of the meeting: {error}");
            notices.push(Notice {
                title: text::SUMMARY_LANGUAGE_UNREADABLE,
                help: text::SUMMARY_LANGUAGE_UNREADABLE_HELP,
            });
        }
    }
    match commands::api_get_meeting_detected_summary_language(state, meeting_id.to_string()).await {
        Ok(cached) => {
            if let Some(language) = cached.language.as_deref().and_then(normalise_language) {
                return Some(language);
            }
        }
        Err(error) => log::warn!("Could not read the detected summary language: {error}"),
    }
    let detection = detect_summary_language(texts);
    if detection.reason == SummaryLanguageDetectionReason::Tie {
        notices.push(Notice {
            title: text::SUMMARY_BILINGUAL,
            help: text::SUMMARY_BILINGUAL_HELP,
        });
    }
    if let Some(language) = &detection.language {
        if let Err(error) = commands::api_save_meeting_detected_summary_language(
            state,
            meeting_id.to_string(),
            Some(language.clone()),
        )
        .await
        {
            log::warn!("Could not store the detected summary language: {error}");
        }
    }
    detection.language
}

/// Start the summary of a meeting in the background (`handleGenerateSummary`
/// and `processSummary`). Returns when the run is registered in the database;
/// the model works on in the background. `Err` is a Danish text to show.
pub async fn start(
    host: HostHandle,
    db: &DatabaseManager,
    meeting_id: &str,
    template_id: &str,
    session_language: Option<&str>,
) -> Result<Started, String> {
    let pool = db.pool();
    let model = effective_model(pool).await.map_err(|error| {
        log::warn!("Could not read the summary model: {error}");
        text::SUMMARY_SETTINGS_UNREADABLE.to_string()
    })?;
    let (rows, _) =
        MeetingsRepository::get_meeting_transcripts_paginated(pool, meeting_id, i64::MAX, 0)
            .await
            .map_err(|error| {
                log::warn!("Could not read the transcript for the summary: {error}");
                text::SUMMARY_NO_TRANSCRIPT.to_string()
            })?;
    if rows.is_empty() {
        return Err(text::SUMMARY_NO_TRANSCRIPT.to_string());
    }
    if let Some(message) = check_model(host.clone(), &model).await {
        return Err(message.to_string());
    }
    let (transcript, texts) = transcript_payload(&rows);
    if transcript.trim().is_empty() {
        return Err(text::SUMMARY_NO_TRANSCRIPT.to_string());
    }
    let state = AppState {
        db_manager: db.clone(),
    };
    let mut notices = Vec::new();
    let language =
        resolve_language(&state, meeting_id, &texts, session_language, &mut notices).await;
    let started = commands::api_process_transcript(
        host,
        &state,
        transcript,
        model.provider.clone(),
        model.model.clone(),
        Some(meeting_id.to_string()),
        Some(CHUNK_SIZE),
        Some(OVERLAP),
        Some(String::new()),
        Some(template_id.to_string()),
        language,
        None,
    )
    .await?;
    Ok(Started {
        process_id: started.process_id,
        provider: model.provider,
        model: model.model,
        notices,
    })
}

/// Ask the provider whether the model can be used right now.
async fn check_model(host: HostHandle, model: &ModelChoice) -> Option<&'static str> {
    let ollama = if model.provider == "ollama" {
        Some(
            crate::ollama::get_ollama_models(model.ollama_endpoint.clone())
                .await
                .map(|models| models.len()),
        )
    } else {
        None
    };
    let builtin = if model.provider == "builtin-ai" && !model.model.is_empty() {
        Some(
            crate::summary::summary_engine::commands::builtin_ai_is_model_ready(
                host,
                &BUILTIN_MODELS,
                model.model.clone(),
                Some(true),
            )
            .await,
        )
    } else {
        None
    };
    preflight_message(&model.provider, &model.model, ollama, builtin)
}

/// What stopping a run came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stopped {
    /// The run is cancelled and the previous summary is back.
    Cancelled,
    /// It was too late: the run was finishing.
    AlmostDone,
    /// There was no run to stop.
    NothingRunning,
}

/// Stop the running summary of a meeting (`api_cancel_summary`). The run is
/// named by the start time stored with it. Only a run of this app session can
/// be cancelled (the registry of running tasks is in memory): a row still
/// "running" from before a restart answers [`Stopped::AlmostDone`], and is
/// cleaned up the next time the app starts.
pub async fn stop(db: &DatabaseManager, meeting_id: &str) -> Result<Stopped, String> {
    let pool = db.pool();
    let process = SummaryProcessesRepository::get_summary_data(pool, meeting_id)
        .await
        .map_err(|error| error.to_string())?;
    let Some(started_at) = process.and_then(|p| p.start_time) else {
        return Ok(Stopped::NothingRunning);
    };
    let state = AppState {
        db_manager: db.clone(),
    };
    let answer =
        commands::api_cancel_summary(&state, meeting_id.to_string(), process_id(started_at))
            .await?;
    Ok(if answer["cancelled"].as_bool() == Some(true) {
        Stopped::Cancelled
    } else {
        Stopped::AlmostDone
    })
}

/// The id [`start`] returns for a run.
pub fn process_id(started_at: DateTime<Utc>) -> String {
    started_at.to_rfc3339_opts(SecondsFormat::Nanos, true)
}

/// Save a summary the user edited (`api_save_meeting_summary` as the
/// BlockNote editor calls it). The new text replaces the Markdown; the
/// block copy and the English cache of the old text are dropped, because
/// they would show the old text, and what else the result holds (the
/// meeting name, metadata) is kept.
pub async fn save_edit(
    db: &DatabaseManager,
    meeting_id: &str,
    markdown: &str,
) -> Result<(), String> {
    let markdown = markdown.trim_end();
    if markdown.trim().is_empty() {
        return Err(text::SUMMARY_EDIT_EMPTY.to_string());
    }
    let pool = db.pool();
    let process = SummaryProcessesRepository::get_summary_data(pool, meeting_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| text::SUMMARY_SAVE_FAILED.to_string())?;
    if crate::meeting_document::summary_status(&process.status)
        == crate::meeting_document::SummaryStatus::Running
    {
        return Err(text::SUMMARY_EDIT_RUNNING.to_string());
    }
    let mut result = process
        .result
        .as_deref()
        .and_then(|stored| serde_json::from_str::<serde_json::Value>(stored).ok())
        .and_then(|value| match value {
            serde_json::Value::Object(object) => Some(object),
            _ => None,
        })
        .unwrap_or_default();
    result.remove("summary_json");
    result.remove("english_cache");
    result.remove("reasoning");
    result.remove("reasoning_stripped");
    result.insert(
        "markdown".into(),
        serde_json::Value::String(markdown.to_string()),
    );
    let state = AppState {
        db_manager: db.clone(),
    };
    commands::api_save_meeting_summary(
        &state,
        meeting_id.to_string(),
        serde_json::Value::Object(result),
        None,
    )
    .await
    .map(|_| ())
}

/// Whether the meeting has a summary process at all (a first run, not a new
/// one over an old summary).
pub async fn has_summary_process(pool: &SqlitePool, meeting_id: &str) -> Result<bool, String> {
    Ok(
        SummaryProcessesRepository::get_summary_data(pool, meeting_id)
            .await
            .map_err(|error| error.to_string())?
            .is_some(),
    )
}

/// Meetings whose automatic summary is being started right now.
static AUTO_IN_FLIGHT: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

struct AutoClaim(String);

impl AutoClaim {
    fn take(meeting_id: &str) -> Option<Self> {
        let mut claimed = AUTO_IN_FLIGHT.lock().unwrap_or_else(|e| e.into_inner());
        if claimed.iter().any(|id| id == meeting_id) {
            return None;
        }
        claimed.push(meeting_id.to_string());
        Some(Self(meeting_id.to_string()))
    }
}

impl Drop for AutoClaim {
    fn drop(&mut self) {
        let mut claimed = AUTO_IN_FLIGHT.lock().unwrap_or_else(|e| e.into_inner());
        claimed.retain(|id| id != &self.0);
    }
}

/// What the automatic summary did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Auto {
    Started(Started),
    /// Off, or the meeting already has a summary, or nothing to summarise.
    Skipped,
    /// It should have run and could not: the text to show.
    Failed(String),
}

/// "Automatisk opsummering": after a recording has been transcribed, make
/// the summary when the setting is on and the meeting has none yet
/// (`page.tsx` `setupAutoGeneration`). The pinned language is applied to the
/// meeting first, and with no model saved but gemma3:1b in Ollama, that is
/// saved as the model, as Tauri does.
pub async fn auto_start(host: HostHandle, db: &DatabaseManager, meeting_id: &str) -> Auto {
    // Two "completed" events for one meeting close together must not both
    // pass the "no summary yet" check below.
    let Some(_claim) = AutoClaim::take(meeting_id) else {
        return Auto::Skipped;
    };
    let pool = db.pool();
    let Ok(dir) = host.app_data_dir() else {
        return Auto::Skipped;
    };
    let preferences = match Preferences::load_async(dir).await {
        Ok(preferences) => preferences,
        Err(error) => {
            log::warn!("Automatic summary skipped, the settings cannot be read: {error}");
            return Auto::Skipped;
        }
    };
    if !preferences.auto_summary {
        return Auto::Skipped;
    }
    match has_summary_process(pool, meeting_id).await {
        Ok(false) => {}
        Ok(true) => return Auto::Skipped,
        Err(error) => return Auto::Failed(error),
    }
    if let Some(pinned) = &preferences.language_default {
        if let Err(error) = save_language(db, meeting_id, Some(pinned)).await {
            log::warn!("Could not apply the pinned summary language: {error}");
        }
    }
    match read_model(pool).await {
        Ok(Some(_)) => {}
        Ok(None) => {
            let has_gemma = crate::ollama::get_ollama_models(None)
                .await
                .is_ok_and(|models| models.iter().any(|m| m.name == GEMMA));
            if !has_gemma {
                return Auto::Skipped;
            }
            let choice = ModelChoice {
                provider: DEFAULT_PROVIDER.into(),
                model: GEMMA.into(),
                ollama_endpoint: None,
            };
            if let Err(error) = save_model(pool, &choice).await {
                return Auto::Failed(error);
            }
        }
        Err(error) => return Auto::Failed(error),
    }
    let templates = templates().await.unwrap_or_default();
    let template = pick_template(&templates, DEFAULT_TEMPLATE);
    match start(
        host,
        db,
        meeting_id,
        &template,
        preferences.language_default.as_deref(),
    )
    .await
    {
        Ok(started) => Auto::Started(started),
        Err(message) => Auto::Failed(message),
    }
}

#[cfg(test)]
#[allow(clippy::disallowed_methods)]
mod tests;
