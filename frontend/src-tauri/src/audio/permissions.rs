//! Tauri commands for `tingskriver_core::audio::permissions`.

pub use tingskriver_core::audio::permissions::*;

#[tauri::command]
pub async fn check_screen_recording_permission_command() -> bool {
    tingskriver_core::audio::permissions::check_screen_recording_permission_command().await
}

#[tauri::command]
pub async fn request_screen_recording_permission_command() -> Result<(), String> {
    tingskriver_core::audio::permissions::request_screen_recording_permission_command().await
}

#[tauri::command]
pub async fn trigger_system_audio_permission_command() -> Result<bool, String> {
    tingskriver_core::audio::permissions::trigger_system_audio_permission_command().await
}

