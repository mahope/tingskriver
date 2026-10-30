//! Tauri commands for the built-in AI models; the logic lives in
//! `tingskriver_core::summary::summary_engine::commands`.

pub use tingskriver_core::summary::summary_engine::commands::*;

use tauri::{AppHandle, Manager, Runtime, State};
use tingskriver_core::summary::summary_engine::ModelInfo;

/// Initialize the model manager at app startup.
pub async fn init_model_manager_at_startup<R: Runtime>(
    app: &AppHandle<R>,
) -> Result<(), String> {
    tingskriver_core::summary::summary_engine::commands::init_model_manager_at_startup(
        &crate::core_bridge::host(app),
        &app.state::<ModelManagerState>(),
    )
    .await
}

#[tauri::command]
pub async fn builtin_ai_list_models<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, ModelManagerState>,
) -> Result<Vec<ModelInfo>, String> {
    tingskriver_core::summary::summary_engine::commands::builtin_ai_list_models(crate::core_bridge::host(&app), &state).await
}

#[tauri::command]
pub async fn builtin_ai_get_model_info<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, ModelManagerState>,
    model_name: String,
) -> Result<Option<ModelInfo>, String> {
    tingskriver_core::summary::summary_engine::commands::builtin_ai_get_model_info(crate::core_bridge::host(&app), &state, model_name).await
}

#[tauri::command]
pub async fn builtin_ai_download_model<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, ModelManagerState>,
    model_name: String,
) -> Result<(), String> {
    tingskriver_core::summary::summary_engine::commands::builtin_ai_download_model(crate::core_bridge::host(&app), &state, model_name).await
}

#[tauri::command]
pub async fn builtin_ai_cancel_download<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, ModelManagerState>,
    model_name: String,
) -> Result<(), String> {
    tingskriver_core::summary::summary_engine::commands::builtin_ai_cancel_download(crate::core_bridge::host(&app), &state, model_name).await
}

#[tauri::command]
pub async fn builtin_ai_delete_model(
    state: State<'_, ModelManagerState>,
    model_name: String,
) -> Result<(), String> {
    tingskriver_core::summary::summary_engine::commands::builtin_ai_delete_model(&state, model_name).await
}

#[tauri::command]
pub async fn builtin_ai_is_model_ready<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, ModelManagerState>,
    model_name: String,
    refresh: Option<bool>,  // NEW: Optional refresh parameter
) -> Result<bool, String> {
    tingskriver_core::summary::summary_engine::commands::builtin_ai_is_model_ready(crate::core_bridge::host(&app), &state, model_name, refresh).await
}

#[tauri::command]
pub async fn builtin_ai_get_available_summary_model<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, ModelManagerState>,
) -> Result<Option<String>, String> {
    tingskriver_core::summary::summary_engine::commands::builtin_ai_get_available_summary_model(crate::core_bridge::host(&app), &state).await
}

#[tauri::command]
pub async fn builtin_ai_get_recommended_model() -> Result<String, String> {
    tingskriver_core::summary::summary_engine::commands::builtin_ai_get_recommended_model().await
}

