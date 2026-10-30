//! Tauri commands for Ollama; the client lives in `tingskriver_core::ollama`.

pub use tingskriver_core::ollama::*;

use tauri::{AppHandle, Runtime};

#[tauri::command]
pub async fn get_ollama_models(endpoint: Option<String>) -> Result<Vec<OllamaModel>, String> {
    tingskriver_core::ollama::get_ollama_models(endpoint).await
}

#[tauri::command]
pub async fn pull_ollama_model<R: Runtime>(
    app_handle: AppHandle<R>,
    model_name: String,
    endpoint: Option<String>,
) -> Result<(), String> {
    tingskriver_core::ollama::pull_ollama_model(
        crate::core_bridge::events(&app_handle),
        model_name,
        endpoint,
    )
    .await
}

#[tauri::command]
pub async fn delete_ollama_model(
    model_name: String,
    endpoint: Option<String>,
) -> Result<(), String> {
    tingskriver_core::ollama::delete_ollama_model(model_name, endpoint).await
}

#[tauri::command]
pub async fn get_ollama_model_context(
    model_name: String,
    endpoint: Option<String>,
) -> Result<usize, String> {
    tingskriver_core::ollama::get_ollama_model_context(model_name, endpoint).await
}
