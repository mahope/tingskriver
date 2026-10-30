//! Tauri command for the OpenAI model list; the client lives in
//! `tingskriver_core::openai`.

pub mod openai {
    pub use tingskriver_core::openai::openai::*;

    #[tauri::command]
    pub async fn get_openai_models(api_key: Option<String>) -> Result<Vec<OpenAIModel>, String> {
        tingskriver_core::openai::openai::get_openai_models(api_key).await
    }
}
