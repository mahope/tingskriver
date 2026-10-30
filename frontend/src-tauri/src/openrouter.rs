//! Tauri command for the OpenRouter model list; the client lives in
//! `tingskriver_core::openrouter`.

pub use tingskriver_core::openrouter::*;

#[tauri::command]
pub fn get_openrouter_models() -> Result<Vec<OpenRouterModel>, String> {
    tingskriver_core::openrouter::get_openrouter_models()
}
