//! Tauri commands for summary templates; the logic lives in
//! `tingskriver_core::summary::template_commands`.

pub use tingskriver_core::summary::template_commands::*;

use tauri::Runtime;

#[tauri::command]
pub async fn api_list_templates<R: Runtime>(
    _app: tauri::AppHandle<R>,
) -> Result<Vec<TemplateInfo>, String> {
    tingskriver_core::summary::template_commands::api_list_templates().await
}

#[tauri::command]
pub async fn api_get_template_details<R: Runtime>(
    _app: tauri::AppHandle<R>,
    template_id: String,
) -> Result<TemplateDetails, String> {
    tingskriver_core::summary::template_commands::api_get_template_details(template_id).await
}

#[tauri::command]
pub async fn api_validate_template<R: Runtime>(
    _app: tauri::AppHandle<R>,
    template_json: String,
) -> Result<String, String> {
    tingskriver_core::summary::template_commands::api_validate_template(template_json).await
}

