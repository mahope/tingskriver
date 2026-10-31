//! Dictation (Wispr Flow style): hold a shortcut, speak, release, and the
//! cleaned-up text is inserted where the cursor is.
//!
//! - `logic`: platform independent decisions, unit tested on any OS
//! - `win`: keyboard hook, clipboard/SendInput, Credential Manager (Windows)
//! - `service`: orchestration; `overlay`: the pill the UI layer draws
//!
//! The Tauri commands and the webview overlay live in the Tauri crate.

pub mod capture;
pub mod logic;
pub mod overlay;
pub mod secrets;
pub mod service;
pub mod settings;
pub mod store;
pub mod syv;
#[cfg(windows)]
pub mod target_win;
#[cfg(windows)]
pub mod win;

pub use service::{init, is_dictating, HISTORY_CHANGED_EVENT};
