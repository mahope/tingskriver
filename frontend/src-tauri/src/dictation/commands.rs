//! Tauri commands for the dictation UI (settings, dictionary, snippets, history).

use serde::Serialize;

use super::logic::cleanup::Style;
use super::secrets;
use super::service;
use super::settings::DictationSettings;
use super::store::{DictionaryWord, HistoryEntry, SnippetRow};
use super::syv;

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DictationStatus {
    /// Global shortcut and text insertion only work on Windows.
    pub platform_supported: bool,
    pub shortcut_active: bool,
    pub has_syv_key: bool,
    pub local_model_available: bool,
    pub last_error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyvTestResult {
    pub latency_ms: u64,
}

#[tauri::command]
pub async fn dictation_get_settings() -> Result<DictationSettings, String> {
    Ok(service::get()?.settings())
}

#[tauri::command]
pub async fn dictation_save_settings(settings: DictationSettings) -> Result<DictationSettings, String> {
    service::get()?.save_settings(settings).await
}

#[tauri::command]
pub async fn dictation_get_status() -> Result<DictationStatus, String> {
    let service = service::get()?;
    let local_model_available = match crate::parakeet_engine::commands::parakeet_has_available_models().await {
        Ok(available) => available,
        Err(_) => false,
    };
    Ok(DictationStatus {
        platform_supported: cfg!(windows),
        shortcut_active: service.shortcut_active(),
        has_syv_key: secrets::syv_key().is_some(),
        local_model_available,
        last_error: service.last_error(),
    })
}

#[tauri::command]
pub async fn dictation_set_syv_key(key: String) -> Result<(), String> {
    let key = key.trim();
    if key.is_empty() {
        return Err("Nøglen er tom".to_string());
    }
    secrets::write(secrets::SYV_TARGET, key)
}

#[tauri::command]
pub async fn dictation_delete_syv_key() -> Result<(), String> {
    secrets::delete(secrets::SYV_TARGET)
}

#[tauri::command]
pub async fn dictation_test_syv() -> Result<SyvTestResult, String> {
    let service = service::get()?;
    let key = secrets::syv_key().ok_or_else(|| "Der er ingen Syv.ai-nøgle".to_string())?;
    let endpoint = service.settings().syv_endpoint;
    let latency_ms = syv::test_key(&service.http, &endpoint, &key).await?;
    Ok(SyvTestResult { latency_ms })
}

#[tauri::command]
pub async fn dictation_preview_cleanup(text: String, style: String) -> Result<String, String> {
    Ok(service::get()?.preview(&text, Style::parse(&style)))
}

#[tauri::command]
pub async fn dictation_list_dictionary() -> Result<Vec<DictionaryWord>, String> {
    service::get()?.store.dictionary().await
}

#[tauri::command]
pub async fn dictation_save_dictionary_word(id: Option<i64>, word: String, aliases: String) -> Result<i64, String> {
    let service = service::get()?;
    let id = service.store.upsert_word(id, &word, &aliases, now_ms()).await?;
    service.reload_rules().await?;
    Ok(id)
}

#[tauri::command]
pub async fn dictation_delete_dictionary_word(id: i64) -> Result<(), String> {
    let service = service::get()?;
    service.store.delete_word(id).await?;
    service.reload_rules().await
}

#[tauri::command]
pub async fn dictation_list_snippets() -> Result<Vec<SnippetRow>, String> {
    service::get()?.store.snippets().await
}

#[tauri::command]
pub async fn dictation_save_snippet(id: Option<i64>, phrase: String, text: String) -> Result<i64, String> {
    let service = service::get()?;
    let id = service.store.upsert_snippet(id, &phrase, &text, now_ms()).await?;
    service.reload_rules().await?;
    Ok(id)
}

#[tauri::command]
pub async fn dictation_delete_snippet(id: i64) -> Result<(), String> {
    let service = service::get()?;
    service.store.delete_snippet(id).await?;
    service.reload_rules().await
}

#[tauri::command]
pub async fn dictation_list_history(limit: Option<i64>) -> Result<Vec<HistoryEntry>, String> {
    service::get()?.store.history(limit.unwrap_or(500)).await
}

#[tauri::command]
pub async fn dictation_delete_history(id: i64) -> Result<(), String> {
    let service = service::get()?;
    if let Some(path) = service.store.delete_history(id).await? {
        service.delete_files(vec![path]);
    }
    service.notify_history_changed();
    Ok(())
}

#[tauri::command]
pub async fn dictation_clear_history() -> Result<(), String> {
    let service = service::get()?;
    let paths = service.store.clear_history().await?;
    service.delete_files(paths);
    service.notify_history_changed();
    Ok(())
}
