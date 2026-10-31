//! Where Tingskriver keeps its files, independent of the UI framework.
//!
//! The folders match what the Tauri app gets from its path resolver for the
//! identifier `dk.mahoje.tingskriver`, so every UI reads the same database,
//! models and logs.

use std::path::PathBuf;

/// The application identifier (Tauri's `identifier`).
pub const APP_ID: &str = "dk.mahoje.tingskriver";

/// The database file inside [`app_data_dir`].
pub const DATABASE_FILE: &str = "meeting_minutes.sqlite";

/// `%APPDATA%\dk.mahoje.tingskriver` on Windows, the platform's data
/// directory elsewhere (Tauri's `app_data_dir`).
pub fn app_data_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|dir| dir.join(APP_ID))
}

/// `%LOCALAPPDATA%\dk.mahoje.tingskriver` on Windows: files that belong to
/// this PC only (logs, crash dumps), not the roaming profile.
pub fn local_data_dir() -> Option<PathBuf> {
    dirs::data_local_dir().map(|dir| dir.join(APP_ID))
}

/// `%LOCALAPPDATA%\dk.mahoje.tingskriver\logs` on Windows, the platform's
/// local data directory elsewhere.
pub fn log_dir() -> Option<PathBuf> {
    local_data_dir().map(|dir| dir.join("logs"))
}

/// `%LOCALAPPDATA%\dk.mahoje.tingskriver\dumps`: crash dumps written by
/// Windows Error Reporting, see [`crate::crash_dumps`].
pub fn dump_dir() -> Option<PathBuf> {
    local_data_dir().map(|dir| dir.join("dumps"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folders_end_in_the_app_id() {
        if let Some(dir) = app_data_dir() {
            assert!(dir.ends_with(APP_ID));
        }
        if let Some(dir) = log_dir() {
            assert!(dir.ends_with(PathBuf::from(APP_ID).join("logs")));
        }
        if let Some(dir) = dump_dir() {
            assert!(dir.ends_with(PathBuf::from(APP_ID).join("dumps")));
        }
    }
}
