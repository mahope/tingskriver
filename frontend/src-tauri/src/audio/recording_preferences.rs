//! Tauri commands for `tingskriver_core::audio::recording_preferences`.

pub use tingskriver_core::audio::recording_preferences::*;

use tauri::{AppHandle, Runtime};

#[tauri::command]
pub async fn get_recording_preferences<R: Runtime>(
    app: AppHandle<R>,
) -> Result<RecordingPreferences, String> {
    tingskriver_core::audio::recording_preferences::get_recording_preferences(crate::core_bridge::host(&app)).await
}

#[tauri::command]
pub async fn set_recording_preferences<R: Runtime>(
    app: AppHandle<R>,
    preferences: RecordingPreferences,
) -> Result<(), String> {
    tingskriver_core::audio::recording_preferences::set_recording_preferences(crate::core_bridge::host(&app), preferences).await
}

#[tauri::command]
pub async fn get_default_recordings_folder_path() -> Result<String, String> {
    tingskriver_core::audio::recording_preferences::get_default_recordings_folder_path().await
}

#[tauri::command]
pub async fn open_recordings_folder<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    tingskriver_core::audio::recording_preferences::open_recordings_folder(crate::core_bridge::host(&app)).await
}

#[tauri::command]
pub async fn select_recording_folder<R: Runtime>(
    _app: AppHandle<R>,
) -> Result<Option<String>, String> {
    tingskriver_core::audio::recording_preferences::select_recording_folder(crate::core_bridge::host(&_app)).await
}

#[tauri::command]
pub async fn get_available_audio_backends() -> Result<Vec<String>, String> {
    tingskriver_core::audio::recording_preferences::get_available_audio_backends().await
}

#[tauri::command]
pub async fn get_current_audio_backend() -> Result<String, String> {
    tingskriver_core::audio::recording_preferences::get_current_audio_backend().await
}

#[tauri::command]
pub async fn set_audio_backend(backend: String) -> Result<(), String> {
    tingskriver_core::audio::recording_preferences::set_audio_backend(backend).await
}

#[tauri::command]
pub async fn get_audio_backend_info() -> Result<Vec<BackendInfo>, String> {
    tingskriver_core::audio::recording_preferences::get_audio_backend_info().await
}

