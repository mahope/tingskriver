//! Tauri commands for `tingskriver_core::audio::import`.

pub use tingskriver_core::audio::import::*;

use std::path::Path;

use log::{error, info};
use tauri::{AppHandle, Runtime};
use tauri_plugin_dialog::DialogExt;
use tingskriver_core::audio::constants::AUDIO_EXTENSIONS;

/// Select an audio file and validate it
#[tauri::command]
pub async fn select_and_validate_audio_command<R: Runtime>(
    app: AppHandle<R>,
) -> Result<Option<AudioFileInfo>, String> {
    info!("Opening file dialog for audio import");

    // Use spawn_blocking to avoid blocking async runtime
    let app_clone = app.clone();
    let file_path = tokio::task::spawn_blocking(move || {
        app_clone
            .dialog()
            .file()
            .add_filter("Audio Files", &AUDIO_EXTENSIONS.iter().map(|s| *s).collect::<Vec<_>>())
            .blocking_pick_file()
    })
    .await
    .map_err(|e| format!("File dialog task failed: {}", e))?;

    match file_path {
        Some(path) => {
            let path_str = path.to_string();
            info!("User selected: {}", path_str);

            match validate_audio_file(Path::new(&path_str)) {
                Ok(info) => Ok(Some(info)),
                Err(e) => {
                    error!("Validation failed: {}", e);
                    Err(e.to_string())
                }
            }
        }
        None => {
            info!("User cancelled file selection");
            Ok(None)
        }
    }
}

#[tauri::command]
pub async fn validate_audio_file_command(path: String) -> Result<AudioFileInfo, String> {
    tingskriver_core::audio::import::validate_audio_file_command(path).await
}

#[tauri::command]
pub async fn start_import_audio_command<R: Runtime>(
    app: AppHandle<R>,
    source_path: String,
    title: String,
    language: Option<String>,
    model: Option<String>,
    provider: Option<String>,
) -> Result<ImportStarted, String> {
    tingskriver_core::audio::import::start_import_audio_command(crate::core_bridge::host(&app), source_path, title, language, model, provider).await
}

#[tauri::command]
pub async fn cancel_import_command() -> Result<(), String> {
    tingskriver_core::audio::import::cancel_import_command().await
}

#[tauri::command]
pub async fn is_import_in_progress_command() -> bool {
    tingskriver_core::audio::import::is_import_in_progress_command().await
}

/// Import a `.vtt`/`.srt` transcript as a meeting without audio and start the summary.
#[tauri::command]
pub async fn import_subtitle_file_command<R: Runtime>(
    app: AppHandle<R>,
    state: tauri::State<'_, crate::state::AppState>,
    path: String,
) -> Result<tingskriver_core::subtitle::import::SubtitleImportOutcome, String> {
    tingskriver_core::subtitle::import::import_subtitle_file(
        crate::core_bridge::host(&app),
        &state,
        &path,
    )
    .await
}

/// "Lav referat igen": start the summary again for an existing meeting.
#[tauri::command]
pub async fn regenerate_summary_command<R: Runtime>(
    app: AppHandle<R>,
    state: tauri::State<'_, crate::state::AppState>,
    meeting_id: String,
) -> Result<(), String> {
    tingskriver_core::subtitle::import::start_summary_for_meeting(
        crate::core_bridge::host(&app),
        &state,
        &meeting_id,
    )
    .await
}

/// Native file dialog for `.vtt`/`.srt` files. Returns `None` when cancelled.
#[tauri::command]
pub async fn select_subtitle_file_command<R: Runtime>(
    app: AppHandle<R>,
) -> Result<Option<String>, String> {
    let picked = tokio::task::spawn_blocking(move || {
        app.dialog()
            .file()
            .add_filter("Undertekster", &["vtt", "srt"])
            .blocking_pick_file()
    })
    .await
    .map_err(|e| format!("File dialog task failed: {}", e))?;
    Ok(picked.map(|p| p.to_string()))
}
