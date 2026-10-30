//! Tauri command for the Anthropic model list; the client lives in
//! `tingskriver_core::anthropic`.

pub mod anthropic {
    pub use tingskriver_core::anthropic::anthropic::*;

    #[tauri::command]
    pub async fn get_anthropic_models(api_key: Option<String>) -> Result<Vec<AnthropicModel>, String> {
        tingskriver_core::anthropic::anthropic::get_anthropic_models(api_key).await
    }
}
