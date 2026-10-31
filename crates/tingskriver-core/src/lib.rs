//! UI-free core of Tingskriver: audio, transcription, database, AI providers,
//! dictation and summaries. The UI layer (the Tauri app today, the GPUI app
//! later) calls into this crate and receives events through
//! [`events::EventSink`].

// Performance optimization: Conditional logging macros for hot paths
#[cfg(debug_assertions)]
macro_rules! perf_debug {
    ($($arg:tt)*) => {
        log::debug!($($arg)*)
    };
}

#[cfg(not(debug_assertions))]
macro_rules! perf_debug {
    ($($arg:tt)*) => {};
}

#[cfg(debug_assertions)]
macro_rules! perf_trace {
    ($($arg:tt)*) => {
        log::trace!($($arg)*)
    };
}

#[cfg(not(debug_assertions))]
macro_rules! perf_trace {
    ($($arg:tt)*) => {};
}

#[allow(unused_imports)]
pub(crate) use perf_debug;
#[allow(unused_imports)]
pub(crate) use perf_trace;

pub mod analytics;
pub mod anthropic;
pub mod api;
pub mod audio;
pub mod config;
pub mod database;
pub mod dictation;
pub mod host;
pub mod groq;
pub mod logging;
pub mod ollama;
pub mod openai;
pub mod openrouter;
pub mod parakeet_engine;
pub mod paths;
pub mod runtime;
pub mod state;
pub mod summary;
pub mod whisper_engine;

#[cfg(target_os = "windows")]
static ONNX_RUNTIME_INIT_ERROR: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// Fails when the bundled ONNX Runtime could not be initialised at startup.
pub fn ensure_onnx_runtime_available() -> anyhow::Result<()> {
    #[cfg(target_os = "windows")]
    if let Some(error) = ONNX_RUNTIME_INIT_ERROR.get() {
        anyhow::bail!("{error}");
    }

    Ok(())
}

/// Remember why the ONNX Runtime failed to initialise, so Parakeet and VAD
/// report it instead of crashing.
#[cfg(target_os = "windows")]
pub fn record_onnx_runtime_failure(error: String) {
    log::error!("{error}");
    let _ = ONNX_RUNTIME_INIT_ERROR.set(error);
}

// Global language preference storage (default to "auto-translate" for automatic translation to English)
static LANGUAGE_PREFERENCE: std::sync::LazyLock<std::sync::Mutex<String>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new("auto-translate".to_string()));

/// Set the transcription language (the `set_language_preference` command).
pub fn set_language_preference(language: String) -> Result<(), String> {
    let mut lang_pref = LANGUAGE_PREFERENCE
        .lock()
        .map_err(|e| format!("Failed to set language preference: {}", e))?;
    log::info!("Setting language preference to: {}", language);
    *lang_pref = language;
    Ok(())
}

// Internal helper function to get language preference (for use within Rust code)
pub fn get_language_preference_internal() -> Option<String> {
    LANGUAGE_PREFERENCE.lock().ok().map(|lang| lang.clone())
}
