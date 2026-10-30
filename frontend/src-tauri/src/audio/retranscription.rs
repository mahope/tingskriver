//! Tauri commands for `tingskriver_core::audio::retranscription`.

pub use tingskriver_core::audio::retranscription::*;

use tauri::{AppHandle, Runtime};

#[tauri::command]
pub async fn start_retranscription_command<R: Runtime>(
    app: AppHandle<R>,
    meeting_id: String,
    meeting_folder_path: String,
    language: Option<String>,
    model: Option<String>,
    provider: Option<String>,
) -> Result<RetranscriptionStarted, String> {
    tingskriver_core::audio::retranscription::start_retranscription_command(crate::core_bridge::host(&app), meeting_id, meeting_folder_path, language, model, provider).await
}

#[tauri::command]
pub async fn cancel_retranscription_command() -> Result<(), String> {
    tingskriver_core::audio::retranscription::cancel_retranscription_command().await
}

#[tauri::command]
pub async fn is_retranscription_in_progress_command() -> bool {
    tingskriver_core::audio::retranscription::is_retranscription_in_progress_command().await
}

