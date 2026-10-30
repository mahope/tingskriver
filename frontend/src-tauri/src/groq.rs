//! Tauri command for the Groq model list; the client lives in
//! `tingskriver_core::groq`.

pub mod groq {
    pub use tingskriver_core::groq::groq::*;

    #[tauri::command]
    pub async fn get_groq_models(api_key: Option<String>) -> Result<Vec<GroqModel>, String> {
        tingskriver_core::groq::groq::get_groq_models(api_key).await
    }
}
