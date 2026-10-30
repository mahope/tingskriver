//! Tauri commands for the Whisper engine; the engine and its state live in
//! `tingskriver_core::whisper_engine`.

pub use tingskriver_core::whisper_engine::commands::*;

use tauri::{AppHandle, Runtime};
use tingskriver_core::whisper_engine::{CancelDownloadOutcome, ModelInfo};

/// Initialize the models directory path using app_data_dir
/// This should be called during app setup before whisper_init
pub fn set_models_directory<R: Runtime>(app: &AppHandle<R>) {
    tingskriver_core::whisper_engine::commands::set_models_directory(&crate::core_bridge::host(app))
}

/// Internal version of whisper_validate_model_ready that respects user's transcript config
pub async fn whisper_validate_model_ready_with_config<R: Runtime>(
    app: &AppHandle<R>,
) -> Result<String, String> {
    tingskriver_core::whisper_engine::commands::whisper_validate_model_ready_with_config(&crate::core_bridge::host(app)).await
}

#[tauri::command]
pub async fn whisper_init() -> Result<(), String> {
    tingskriver_core::whisper_engine::commands::whisper_init().await
}

#[tauri::command]
pub async fn whisper_get_available_models() -> Result<Vec<ModelInfo>, String> {
    tingskriver_core::whisper_engine::commands::whisper_get_available_models().await
}

#[tauri::command]
pub async fn whisper_load_model(
    app_handle: tauri::AppHandle,
    model_name: String
) -> Result<(), String> {
    tingskriver_core::whisper_engine::commands::whisper_load_model(crate::core_bridge::host(&app_handle), model_name).await
}

#[tauri::command]
pub async fn whisper_get_current_model() -> Result<Option<String>, String> {
    tingskriver_core::whisper_engine::commands::whisper_get_current_model().await
}

#[tauri::command]
pub async fn whisper_is_model_loaded() -> Result<bool, String> {
    tingskriver_core::whisper_engine::commands::whisper_is_model_loaded().await
}

#[tauri::command]
pub async fn whisper_has_available_models() -> Result<bool, String> {
    tingskriver_core::whisper_engine::commands::whisper_has_available_models().await
}

#[tauri::command]
pub async fn whisper_validate_model_ready() -> Result<String, String> {
    tingskriver_core::whisper_engine::commands::whisper_validate_model_ready().await
}

#[tauri::command]
pub async fn whisper_transcribe_audio(audio_data: Vec<f32>) -> Result<String, String> {
    tingskriver_core::whisper_engine::commands::whisper_transcribe_audio(audio_data).await
}

#[tauri::command]
pub async fn whisper_get_models_directory() -> Result<String, String> {
    tingskriver_core::whisper_engine::commands::whisper_get_models_directory().await
}

#[tauri::command]
pub async fn whisper_download_model(
    app_handle: tauri::AppHandle,
    model_name: String,
) -> Result<(), String> {
    tingskriver_core::whisper_engine::commands::whisper_download_model(crate::core_bridge::host(&app_handle), model_name).await
}

#[tauri::command]
pub async fn whisper_cancel_download(model_name: String) -> Result<CancelDownloadOutcome, String> {
    tingskriver_core::whisper_engine::commands::whisper_cancel_download(model_name).await
}

#[tauri::command]
pub async fn whisper_delete_corrupted_model(model_name: String) -> Result<String, String> {
    tingskriver_core::whisper_engine::commands::whisper_delete_corrupted_model(model_name).await
}

#[tauri::command]
pub async fn open_models_folder() -> Result<(), String> {
    tingskriver_core::whisper_engine::commands::open_models_folder().await
}

