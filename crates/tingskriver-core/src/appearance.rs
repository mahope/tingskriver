//! The user's choice of light or dark appearance (Indstillinger > Generelt >
//! Udseende): follow the system, or always light or dark. Stored as a small
//! JSON file in the app data folder, so the GPUI app applies it before the
//! database is open.

use std::path::Path;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

/// The file in the app data folder.
pub const FILE: &str = "appearance.json";

/// Follow the system, or pin the appearance.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Appearance {
    #[default]
    System,
    Light,
    Dark,
}

/// One save at a time in this process: every save writes the same temp file.
static SAVING: Mutex<()> = Mutex::new(());

#[derive(Serialize, Deserialize)]
struct Stored {
    appearance: Appearance,
}

impl Appearance {
    /// In the order of the segmented control (Følg systemet, Lyst, Mørkt).
    pub const ALL: [Appearance; 3] = [Appearance::System, Appearance::Light, Appearance::Dark];

    /// The saved choice; [`Appearance::System`] when nothing is saved yet.
    #[allow(clippy::disallowed_methods)] // Read once at startup, before any window or async work.
    pub fn load(data_dir: &Path) -> Result<Self, String> {
        let text = match std::fs::read_to_string(data_dir.join(FILE)) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(crate::strings::appearance_file_read_failed(&e.to_string())),
        };
        serde_json::from_str::<Stored>(&text)
            .map(|stored| stored.appearance)
            .map_err(|e| crate::strings::appearance_file_invalid(&e.to_string()))
    }

    /// Writes the choice (temp file, then rename). Saves in the same process
    /// wait for each other, so two quick choices never share the temp file.
    #[allow(clippy::disallowed_methods)] // Called on a blocking worker.
    pub fn save(self, data_dir: &Path) -> Result<(), String> {
        let _saving = SAVING.lock().unwrap_or_else(|e| e.into_inner());
        let fail = |e: std::io::Error| crate::strings::appearance_file_write_failed(&e.to_string());
        let json = serde_json::to_string_pretty(&Stored { appearance: self })
            .map_err(|e| e.to_string())?;
        std::fs::create_dir_all(data_dir).map_err(fail)?;
        let temp = data_dir.join(format!("{FILE}.tmp"));
        let write = |temp: &Path| -> std::io::Result<()> {
            use std::io::Write;
            let mut file = std::fs::File::create(temp)?;
            file.write_all(json.as_bytes())?;
            file.sync_all()
        };
        write(&temp).map_err(fail)?;
        std::fs::rename(&temp, data_dir.join(FILE)).map_err(fail)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_follows_the_system() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(Appearance::load(dir.path()), Ok(Appearance::System));
    }

    #[test]
    fn a_saved_choice_is_read_back() {
        let dir = tempfile::tempdir().unwrap();
        for choice in Appearance::ALL {
            choice.save(dir.path()).unwrap();
            assert_eq!(Appearance::load(dir.path()), Ok(choice));
        }
        assert!(!dir.path().join(format!("{FILE}.tmp")).exists());
    }

    #[test]
    fn saves_from_many_threads_never_collide() {
        let dir = tempfile::tempdir().unwrap();
        let threads: Vec<_> = (0..16)
            .map(|i| {
                let dir = dir.path().to_path_buf();
                std::thread::spawn(move || Appearance::ALL[i % 3].save(&dir))
            })
            .collect();
        for thread in threads {
            assert_eq!(thread.join().unwrap(), Ok(()));
        }
        assert!(Appearance::load(dir.path()).is_ok());
        assert!(!dir.path().join(format!("{FILE}.tmp")).exists());
    }

    #[test]
    #[allow(clippy::disallowed_methods)] // Test setup on the test thread.
    fn a_broken_file_is_an_error_not_a_silent_default() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(FILE), "{\"appearance\": \"purple\"}").unwrap();
        assert!(Appearance::load(dir.path()).is_err());
    }
}
