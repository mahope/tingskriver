//! Tauri commands for analytics; the client lives in `tingskriver_core::analytics`.

use std::collections::HashMap;

#[tauri::command]
pub async fn init_analytics() -> Result<(), String> {
    tingskriver_core::analytics::commands::init_analytics().await
}

#[tauri::command]
pub async fn disable_analytics() -> Result<(), String> {
    tingskriver_core::analytics::commands::disable_analytics().await
}

#[tauri::command]
pub async fn track_event(event_name: String, properties: Option<HashMap<String, String>>) -> Result<(), String> {
    tingskriver_core::analytics::commands::track_event(event_name, properties).await
}

#[tauri::command]
pub async fn identify_user(user_id: String, properties: Option<HashMap<String, String>>) -> Result<(), String> {
    tingskriver_core::analytics::commands::identify_user(user_id, properties).await
}

#[tauri::command]
pub async fn track_meeting_started(meeting_id: String) -> Result<(), String> {
    tingskriver_core::analytics::commands::track_meeting_started(meeting_id).await
}

#[tauri::command]
pub async fn track_recording_started(meeting_id: String) -> Result<(), String> {
    tingskriver_core::analytics::commands::track_recording_started(meeting_id).await
}

#[tauri::command]
pub async fn track_recording_stopped(meeting_id: String, duration_seconds: Option<u64>) -> Result<(), String> {
    tingskriver_core::analytics::commands::track_recording_stopped(meeting_id, duration_seconds).await
}

#[tauri::command]
pub async fn track_meeting_deleted(meeting_id: String) -> Result<(), String> {
    tingskriver_core::analytics::commands::track_meeting_deleted(meeting_id).await
}

#[tauri::command]
pub async fn track_settings_changed(setting_type: String, new_value: String) -> Result<(), String> {
    tingskriver_core::analytics::commands::track_settings_changed(setting_type, new_value).await
}

#[tauri::command]
pub async fn track_feature_used(feature_name: String) -> Result<(), String> {
    tingskriver_core::analytics::commands::track_feature_used(feature_name).await
}

#[tauri::command]
pub async fn is_analytics_enabled() -> bool {
    tingskriver_core::analytics::commands::is_analytics_enabled().await
}

#[tauri::command]
pub async fn start_analytics_session(user_id: String) -> Result<String, String> {
    tingskriver_core::analytics::commands::start_analytics_session(user_id).await
}

#[tauri::command]
pub async fn end_analytics_session() -> Result<(), String> {
    tingskriver_core::analytics::commands::end_analytics_session().await
}

#[tauri::command]
pub async fn track_daily_active_user() -> Result<(), String> {
    tingskriver_core::analytics::commands::track_daily_active_user().await
}

#[tauri::command]
pub async fn track_user_first_launch() -> Result<(), String> {
    tingskriver_core::analytics::commands::track_user_first_launch().await
}

#[tauri::command]
pub async fn track_summary_generation_started(model_provider: String, model_name: String, transcript_length: usize) -> Result<(), String> {
    tingskriver_core::analytics::commands::track_summary_generation_started(model_provider, model_name, transcript_length).await
}

#[tauri::command]
pub async fn track_summary_generation_completed(model_provider: String, model_name: String, success: bool, duration_seconds: Option<u64>, error_message: Option<String>) -> Result<(), String> {
    tingskriver_core::analytics::commands::track_summary_generation_completed(model_provider, model_name, success, duration_seconds, error_message).await
}

#[tauri::command]
pub async fn track_summary_regenerated(model_provider: String, model_name: String) -> Result<(), String> {
    tingskriver_core::analytics::commands::track_summary_regenerated(model_provider, model_name).await
}

#[tauri::command]
pub async fn track_model_changed(old_provider: String, old_model: String, new_provider: String, new_model: String) -> Result<(), String> {
    tingskriver_core::analytics::commands::track_model_changed(old_provider, old_model, new_provider, new_model).await
}

#[tauri::command]
pub async fn track_custom_prompt_used(prompt_length: usize) -> Result<(), String> {
    tingskriver_core::analytics::commands::track_custom_prompt_used(prompt_length).await
}

#[tauri::command]
pub async fn track_meeting_ended(
    transcription_provider: String,
    transcription_model: String,
    summary_provider: String,
    summary_model: String,
    total_duration_seconds: Option<f64>,
    active_duration_seconds: f64,
    pause_duration_seconds: f64,
    microphone_device_type: String,
    system_audio_device_type: String,
    chunks_processed: u64,
    transcript_segments_count: u64,
    had_fatal_error: bool,
) -> Result<(), String> {
    tingskriver_core::analytics::commands::track_meeting_ended(transcription_provider, transcription_model, summary_provider, summary_model, total_duration_seconds, active_duration_seconds, pause_duration_seconds, microphone_device_type, system_audio_device_type, chunks_processed, transcript_segments_count, had_fatal_error).await
}

#[tauri::command]
pub async fn track_analytics_enabled() -> Result<(), String> {
    tingskriver_core::analytics::commands::track_analytics_enabled().await
}

#[tauri::command]
pub async fn track_analytics_disabled() -> Result<(), String> {
    tingskriver_core::analytics::commands::track_analytics_disabled().await
}

#[tauri::command]
pub async fn track_analytics_transparency_viewed() -> Result<(), String> {
    tingskriver_core::analytics::commands::track_analytics_transparency_viewed().await
}

#[tauri::command]
pub async fn is_analytics_session_active() -> bool {
    tingskriver_core::analytics::commands::is_analytics_session_active().await
}

