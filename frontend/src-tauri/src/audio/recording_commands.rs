//! Tauri commands for recording; the orchestration lives in
//! `tingskriver_core::audio::recording_commands`.

pub use tingskriver_core::audio::recording_commands::*;

use tauri::{AppHandle, Runtime};

#[tauri::command]
pub async fn pause_recording<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    tingskriver_core::audio::recording_commands::pause_recording(crate::core_bridge::host(&app)).await
}

#[tauri::command]
pub async fn resume_recording<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    tingskriver_core::audio::recording_commands::resume_recording(crate::core_bridge::host(&app)).await
}

#[tauri::command]
pub async fn is_recording_paused() -> bool {
    tingskriver_core::audio::recording_commands::is_recording_paused().await
}

#[tauri::command]
pub async fn get_recording_state() -> serde_json::Value {
    tingskriver_core::audio::recording_commands::get_recording_state().await
}

#[tauri::command]
pub async fn get_meeting_folder_path() -> Result<Option<String>, String> {
    tingskriver_core::audio::recording_commands::get_meeting_folder_path().await
}

#[tauri::command]
pub async fn get_transcript_history() -> Result<Vec<crate::audio::recording_saver::TranscriptSegment>, String> {
    tingskriver_core::audio::recording_commands::get_transcript_history().await
}

#[tauri::command]
pub async fn get_recording_meeting_name() -> Result<Option<String>, String> {
    tingskriver_core::audio::recording_commands::get_recording_meeting_name().await
}

#[tauri::command]
pub async fn get_active_audio_output() -> Result<super::playback_monitor::AudioOutputInfo, String> {
    tingskriver_core::audio::recording_commands::get_active_audio_output().await
}

