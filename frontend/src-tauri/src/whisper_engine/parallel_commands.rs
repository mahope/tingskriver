//! Tauri commands for parallel Whisper processing; the logic lives in
//! `tingskriver_core::whisper_engine::parallel_commands`.

pub use tingskriver_core::whisper_engine::parallel_commands::*;

use tauri::State;
use tingskriver_core::whisper_engine::*;

#[tauri::command]
pub async fn initialize_parallel_processor(
    state: State<'_, ParallelProcessorState>,
    max_workers: Option<usize>,
    memory_budget_mb: Option<u64>,
) -> Result<String, String> {
    tingskriver_core::whisper_engine::parallel_commands::initialize_parallel_processor(&state, max_workers, memory_budget_mb).await
}

#[tauri::command]
pub async fn start_parallel_processing(
    state: State<'_, ParallelProcessorState>,
    audio_chunks: Vec<serde_json::Value>, // JSON representation of AudioChunk
    model_name: String,
) -> Result<String, String> {
    tingskriver_core::whisper_engine::parallel_commands::start_parallel_processing(&state, audio_chunks, model_name).await
}

#[tauri::command]
pub async fn pause_parallel_processing(
    state: State<'_, ParallelProcessorState>,
) -> Result<String, String> {
    tingskriver_core::whisper_engine::parallel_commands::pause_parallel_processing(&state).await
}

#[tauri::command]
pub async fn resume_parallel_processing(
    state: State<'_, ParallelProcessorState>,
) -> Result<String, String> {
    tingskriver_core::whisper_engine::parallel_commands::resume_parallel_processing(&state).await
}

#[tauri::command]
pub async fn stop_parallel_processing(
    state: State<'_, ParallelProcessorState>,
) -> Result<String, String> {
    tingskriver_core::whisper_engine::parallel_commands::stop_parallel_processing(&state).await
}

#[tauri::command]
pub async fn get_parallel_processing_status(
    state: State<'_, ParallelProcessorState>,
) -> Result<ProcessingStatus, String> {
    tingskriver_core::whisper_engine::parallel_commands::get_parallel_processing_status(&state).await
}

#[tauri::command]
pub async fn get_system_resources(
    state: State<'_, ParallelProcessorState>,
) -> Result<serde_json::Value, String> {
    tingskriver_core::whisper_engine::parallel_commands::get_system_resources(&state).await
}

#[tauri::command]
pub async fn check_resource_constraints(
    state: State<'_, ParallelProcessorState>,
) -> Result<serde_json::Value, String> {
    tingskriver_core::whisper_engine::parallel_commands::check_resource_constraints(&state).await
}

#[tauri::command]
pub async fn calculate_optimal_workers(
    state: State<'_, ParallelProcessorState>,
) -> Result<usize, String> {
    tingskriver_core::whisper_engine::parallel_commands::calculate_optimal_workers(&state).await
}

#[tauri::command]
pub async fn prepare_audio_chunks(
    audio_data: Vec<f32>,
    sample_rate: u32,
    chunk_duration_ms: Option<f64>,
) -> Result<Vec<AudioChunk>, String> {
    tingskriver_core::whisper_engine::parallel_commands::prepare_audio_chunks(audio_data, sample_rate, chunk_duration_ms).await
}

#[tauri::command]
pub async fn test_parallel_processing_setup(
    state: State<'_, ParallelProcessorState>,
) -> Result<String, String> {
    tingskriver_core::whisper_engine::parallel_commands::test_parallel_processing_setup(&state).await
}

