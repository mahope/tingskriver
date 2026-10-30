//! Tauri commands for `tingskriver_core::audio::incremental_saver`.

pub use tingskriver_core::audio::incremental_saver::*;

#[tauri::command]
pub async fn recover_audio_from_checkpoints(
    meeting_folder: String,
    _sample_rate: u32
) -> Result<AudioRecoveryStatus, String> {
    tingskriver_core::audio::incremental_saver::recover_audio_from_checkpoints(meeting_folder, _sample_rate).await
}

#[tauri::command]
pub async fn cleanup_checkpoints(meeting_folder: String) -> Result<(), String> {
    tingskriver_core::audio::incremental_saver::cleanup_checkpoints(meeting_folder).await
}

#[tauri::command]
pub async fn has_audio_checkpoints(meeting_folder: String) -> Result<bool, String> {
    tingskriver_core::audio::incremental_saver::has_audio_checkpoints(meeting_folder).await
}

