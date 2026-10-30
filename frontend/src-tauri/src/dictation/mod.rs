//! Dictation (Wispr Flow style): hold a shortcut, speak, release, and the
//! cleaned-up text is inserted where the cursor is.
//!
//! - `logic`: platform independent decisions, unit tested on any OS
//! - `win`: keyboard hook, clipboard/SendInput, Credential Manager (Windows)
//! - `service`: orchestration; `commands`: Tauri commands for the UI

pub mod capture;
pub mod commands;
pub mod logic;
mod overlay;
pub mod secrets;
mod service;
pub mod settings;
pub mod store;
pub mod syv;
#[cfg(windows)]
pub mod win;

pub use service::{init, is_dictating, HISTORY_CHANGED_EVENT};
