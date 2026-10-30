//! Tauri commands for summaries; the logic lives in `tingskriver_core::summary::commands`.

pub use tingskriver_core::summary::commands::*;

use crate::state::AppState;
use tauri::{AppHandle, Runtime};
use tingskriver_core::summary::language_detection::SummaryLanguageDetection;

#[tauri::command]
pub async fn api_save_meeting_summary<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    meeting_id: String,
    summary: serde_json::Value,
    _auth_token: Option<String>,
) -> Result<serde_json::Value, String> {
    tingskriver_core::summary::commands::api_save_meeting_summary(&state, meeting_id, summary, _auth_token).await
}

#[tauri::command]
pub async fn api_get_meeting_summary_language<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    meeting_id: String,
) -> Result<MeetingSummaryLanguagePreference, String> {
    tingskriver_core::summary::commands::api_get_meeting_summary_language(&state, meeting_id).await
}

#[tauri::command]
pub async fn api_save_meeting_summary_language<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    meeting_id: String,
    summary_language: Option<String>,
) -> Result<MeetingSummaryLanguagePreference, String> {
    tingskriver_core::summary::commands::api_save_meeting_summary_language(&state, meeting_id, summary_language).await
}

#[tauri::command]
pub async fn api_get_meeting_detected_summary_language<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    meeting_id: String,
) -> Result<MeetingSummaryLanguagePreference, String> {
    tingskriver_core::summary::commands::api_get_meeting_detected_summary_language(&state, meeting_id).await
}

#[tauri::command]
pub async fn api_save_meeting_detected_summary_language<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    meeting_id: String,
    detected_summary_language: Option<String>,
) -> Result<MeetingSummaryLanguagePreference, String> {
    tingskriver_core::summary::commands::api_save_meeting_detected_summary_language(&state, meeting_id, detected_summary_language).await
}

#[tauri::command]
pub async fn api_detect_transcript_summary_language(
    transcript_texts: Vec<String>,
) -> Result<SummaryLanguageDetection, String> {
    tingskriver_core::summary::commands::api_detect_transcript_summary_language(transcript_texts).await
}

#[tauri::command]
pub async fn api_get_summary<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    meeting_id: String,
    _auth_token: Option<String>,
) -> Result<SummaryResponse, String> {
    tingskriver_core::summary::commands::api_get_summary(&state, meeting_id, _auth_token).await
}

#[tauri::command]
pub async fn api_process_transcript<R: Runtime>(
    app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    text: String,
    model: String,
    model_name: String,
    meeting_id: Option<String>,
    _chunk_size: Option<i32>,
    _overlap: Option<i32>,
    custom_prompt: Option<String>,
    template_id: Option<String>,
    summary_language: Option<String>,
    _auth_token: Option<String>,
) -> Result<ProcessTranscriptResponse, String> {
    tingskriver_core::summary::commands::api_process_transcript(crate::core_bridge::host(&app), &state, text, model, model_name, meeting_id, _chunk_size, _overlap, custom_prompt, template_id, summary_language, _auth_token).await
}

#[tauri::command]
pub async fn api_cancel_summary<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    meeting_id: String,
    process_id: String,
) -> Result<serde_json::Value, String> {
    tingskriver_core::summary::commands::api_cancel_summary(&state, meeting_id, process_id).await
}

