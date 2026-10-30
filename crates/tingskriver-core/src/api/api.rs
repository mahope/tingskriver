//! Types shared between the UI and core, and settings lookups that used to
//! live inside the Tauri commands of the same name.

use log::{error as log_error, info as log_info, warn as log_warn};
use serde::{Deserialize, Serialize};

use crate::database::{manager::DatabaseManager, repositories::setting::SettingsRepository};

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    pub data: Option<T>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Meeting {
    pub id: String,
    pub title: String,
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SearchRequest {
    pub query: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TranscriptSearchResult {
    pub id: String,
    pub title: String,
    #[serde(rename = "matchContext")]
    pub match_context: String,
    pub timestamp: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ModelConfig {
    pub provider: String,
    pub model: String,
    #[serde(rename = "whisperModel")]
    pub whisper_model: String,
    #[serde(rename = "apiKey")]
    pub api_key: Option<String>,
    #[serde(rename = "ollamaEndpoint")]
    pub ollama_endpoint: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SaveModelConfigRequest {
    pub provider: String,
    pub model: String,
    #[serde(rename = "whisperModel")]
    pub whisper_model: String,
    #[serde(rename = "apiKey")]
    pub api_key: Option<String>,
    #[serde(rename = "ollamaEndpoint")]
    pub ollama_endpoint: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GetApiKeyRequest {
    pub provider: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TranscriptConfig {
    pub provider: String,
    pub model: String,
    #[serde(rename = "apiKey")]
    pub api_key: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SaveTranscriptConfigRequest {
    pub provider: String,
    pub model: String,
    #[serde(rename = "apiKey")]
    pub api_key: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DeleteMeetingRequest {
    pub meeting_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MeetingDetails {
    pub id: String,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
    pub transcripts: Vec<MeetingTranscript>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MeetingTranscript {
    pub id: String,
    pub text: String,
    pub timestamp: String,
    // Recording-relative timestamps for audio-transcript synchronization
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_start_time: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_end_time: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<f64>,
}

/// Meeting metadata without transcripts (for pagination)
#[derive(Debug, Serialize, Deserialize)]
pub struct MeetingMetadata {
    pub id: String,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder_path: Option<String>,
}

/// Paginated transcripts response with total count
#[derive(Debug, Serialize, Deserialize)]
pub struct PaginatedTranscriptsResponse {
    pub transcripts: Vec<MeetingTranscript>,
    pub total_count: i64,
    pub has_more: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SaveMeetingTitleRequest {
    pub meeting_id: String,
    pub title: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SaveMeetingSummaryRequest {
    pub meeting_id: String,
    pub summary: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SaveTranscriptRequest {
    pub meeting_title: String,
    pub transcripts: Vec<TranscriptSegment>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TranscriptSegment {
    pub id: String,
    pub text: String,
    pub timestamp: String,
    // NEW: Recording-relative timestamps for playback synchronization
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_start_time: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_end_time: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<f64>,
}

/// The configured summary model and its API key (`api_get_model_config`).
pub async fn get_model_config(db: &DatabaseManager) -> Result<Option<ModelConfig>, String> {
    log_info!("api_get_model_config called (native)");
    let pool = db.pool();

    match SettingsRepository::get_model_config(pool).await {
        Ok(Some(config)) => {
            log_info!(
                "✅ Found model config in database: provider={}, model={}, whisperModel={}, ollamaEndpoint={:?}",
                &config.provider,
                &config.model,
                &config.whisper_model,
                &config.ollama_endpoint
            );
            match SettingsRepository::get_api_key(pool, &config.provider).await {
                Ok(api_key) => {
                    log_info!("Successfully retrieved model config and API key.");
                    Ok(Some(ModelConfig {
                        provider: config.provider,
                        model: config.model,
                        whisper_model: config.whisper_model,
                        api_key,
                        ollama_endpoint: config.ollama_endpoint,
                    }))
                }
                Err(e) => {
                    log_error!(
                        "Failed to get API key for provider {}: {}",
                        &config.provider,
                        e
                    );
                    Err(e.to_string())
                }
            }
        }
        Ok(None) => {
            log_warn!("⚠️ No model config found in database - database may be empty or settings table not initialized");
            Ok(None)
        }
        Err(e) => {
            log_error!("❌ Failed to get model config from database: {}", e);
            Err(e.to_string())
        }
    }
}

/// The configured transcription provider and model, defaulting to local
/// Parakeet (`api_get_transcript_config`).
pub async fn get_transcript_config(db: &DatabaseManager) -> Result<Option<TranscriptConfig>, String> {
    log_info!("api_get_transcript_config called (native)");
    let pool = db.pool();

    match SettingsRepository::get_transcript_config(pool).await {
        Ok(Some(config)) => {
            log_info!(
                "Found transcript config: provider={}, model={}",
                &config.provider,
                &config.model
            );
            match SettingsRepository::get_transcript_api_key(pool, &config.provider).await {
                Ok(api_key) => {
                    log_info!("Successfully retrieved transcript config and API key.");
                    Ok(Some(TranscriptConfig {
                        provider: config.provider,
                        model: config.model,
                        api_key,
                    }))
                }
                Err(e) => {
                    log_error!(
                        "Failed to get transcript API key for provider {}: {}",
                        &config.provider,
                        e
                    );
                    Err(e.to_string())
                }
            }
        }
        Ok(None) => {
            log_info!("No transcript config found, returning default.");
            Ok(Some(TranscriptConfig {
                provider: "parakeet".to_string(),
                model: crate::config::DEFAULT_PARAKEET_MODEL.to_string(),
                api_key: None,
            }))
        }
        Err(e) => {
            log_error!("Failed to get transcript config: {}", e);
            Err(e.to_string())
        }
    }
}
