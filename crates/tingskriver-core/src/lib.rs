//! UI-free core of Tingskriver: audio, transcription, database, AI providers,
//! dictation and summaries. The UI layer (the Tauri app today, the GPUI app
//! later) calls into this crate and receives events through
//! [`events::EventSink`].

pub mod anthropic;
pub mod config;
pub mod events;
pub mod groq;
pub mod ollama;
pub mod openai;
pub mod openrouter;
pub mod runtime;
