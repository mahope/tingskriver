//! Dictation commands and the webview overlay; the dictation service lives
//! in `tingskriver_core::dictation`.

pub use tingskriver_core::dictation::*;

pub mod commands;
mod overlay;

use std::sync::Arc;
use tauri::AppHandle;

/// Start dictation in the background. Called once from app setup.
pub fn init(app: &AppHandle) {
    tingskriver_core::dictation::init(
        crate::core_bridge::host(app),
        Arc::new(overlay::TauriOverlay(app.clone())),
    );
}
