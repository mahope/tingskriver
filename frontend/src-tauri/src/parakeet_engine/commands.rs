//! Tauri commands for the Parakeet engine; the engine and its state live in
//! `tingskriver_core::parakeet_engine`.

pub use tingskriver_core::parakeet_engine::commands::*;

use tauri::{AppHandle, Runtime};
use tingskriver_core::parakeet_engine::{parakeet_engine::CancelDownloadOutcome, ModelInfo};

/// Initialize the models directory path using app_data_dir
/// This should be called during app setup before parakeet_init
pub fn set_models_directory<R: Runtime>(app: &AppHandle<R>) {
    tingskriver_core::parakeet_engine::commands::set_models_directory(&crate::core_bridge::host(app))
}

/// Internal version of parakeet_validate_model_ready that respects user's transcript config
pub async fn parakeet_validate_model_ready_with_config<R: Runtime>(
    app: &AppHandle<R>,
) -> Result<String, String> {
    tingskriver_core::parakeet_engine::commands::parakeet_validate_model_ready_with_config(&crate::core_bridge::host(app)).await
}

#[tauri::command]
pub async fn parakeet_init() -> Result<(), String> {
    tingskriver_core::parakeet_engine::commands::parakeet_init().await
}

#[tauri::command]
pub async fn parakeet_get_available_models() -> Result<Vec<ModelInfo>, String> {
    tingskriver_core::parakeet_engine::commands::parakeet_get_available_models().await
}

#[tauri::command]
pub async fn parakeet_load_model<R: Runtime>(
    app_handle: AppHandle<R>,
    model_name: String
) -> Result<(), String> {
    tingskriver_core::parakeet_engine::commands::parakeet_load_model(crate::core_bridge::host(&app_handle), model_name).await
}

#[tauri::command]
pub async fn parakeet_get_current_model() -> Result<Option<String>, String> {
    tingskriver_core::parakeet_engine::commands::parakeet_get_current_model().await
}

#[tauri::command]
pub async fn parakeet_is_model_loaded() -> Result<bool, String> {
    tingskriver_core::parakeet_engine::commands::parakeet_is_model_loaded().await
}

#[tauri::command]
pub async fn parakeet_has_available_models() -> Result<bool, String> {
    tingskriver_core::parakeet_engine::commands::parakeet_has_available_models().await
}

#[tauri::command]
pub async fn parakeet_validate_model_ready() -> Result<String, String> {
    tingskriver_core::parakeet_engine::commands::parakeet_validate_model_ready().await
}

#[tauri::command]
pub async fn parakeet_transcribe_audio(audio_data: Vec<f32>) -> Result<String, String> {
    tingskriver_core::parakeet_engine::commands::parakeet_transcribe_audio(audio_data).await
}

#[tauri::command]
pub async fn parakeet_get_models_directory() -> Result<String, String> {
    tingskriver_core::parakeet_engine::commands::parakeet_get_models_directory().await
}

#[tauri::command]
pub async fn parakeet_download_model<R: Runtime>(
    app_handle: AppHandle<R>,
    model_name: String,
) -> Result<(), String> {
    tingskriver_core::parakeet_engine::commands::parakeet_download_model(crate::core_bridge::host(&app_handle), model_name).await
}

#[tauri::command]
pub async fn parakeet_cancel_download(
    model_name: String,
) -> Result<CancelDownloadOutcome, String> {
    tingskriver_core::parakeet_engine::commands::parakeet_cancel_download(model_name).await
}

#[tauri::command]
pub async fn parakeet_retry_download<R: Runtime>(
    app_handle: AppHandle<R>,
    model_name: String,
) -> Result<(), String> {
    tingskriver_core::parakeet_engine::commands::parakeet_retry_download(crate::core_bridge::host(&app_handle), model_name).await
}

#[tauri::command]
pub async fn parakeet_delete_corrupted_model(model_name: String) -> Result<String, String> {
    tingskriver_core::parakeet_engine::commands::parakeet_delete_corrupted_model(model_name).await
}

#[tauri::command]
pub async fn open_parakeet_models_folder() -> Result<(), String> {
    tingskriver_core::parakeet_engine::commands::open_parakeet_models_folder().await
}

