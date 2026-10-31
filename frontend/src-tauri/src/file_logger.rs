//! Application log: stderr plus a size-rotated file in the app log
//! directory, so release builds (no console) leave something to read. The
//! logger, the panic hook and the startup markers live in
//! `tingskriver_core::logging`, shared with the GPUI app.
//!
//! Files: `tingskriver.log` (current), `tingskriver.log.1`, `tingskriver.log.2`.

use std::path::PathBuf;

const LOG_NAME: &str = "tingskriver.log";

/// `%LOCALAPPDATA%\dk.mahoje.tingskriver\logs` on Windows, the platform's
/// local data directory elsewhere.
pub fn log_dir() -> Option<PathBuf> {
    tingskriver_core::paths::log_dir()
}

/// Install the logger and the panic hook. Falls back to stderr only if the
/// file cannot be opened.
pub fn init() {
    tingskriver_core::logging::init(LOG_NAME);
}
