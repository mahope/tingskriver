//! UI-free core of Tingskriver: audio, transcription, database, AI providers,
//! dictation and summaries. The UI layer (the Tauri app today, the GPUI app
//! later) calls into this crate and receives events through
//! [`events::EventSink`].

pub mod anthropic;
pub mod api;
pub mod config;
pub mod database;
pub mod host;
pub mod groq;
pub mod ollama;
pub mod openai;
pub mod openrouter;
pub mod runtime;
pub mod state;
pub mod summary;
