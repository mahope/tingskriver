//! What uninstalling does with the user's data.
//!
//! By default nothing: meetings, recordings, settings, models, logs and the
//! API keys in Credential Manager stay, so a reinstall picks them up. The
//! user can ask for everything to go; the choice is a marker file in the
//! data folder, which the uninstall hook reads (the uninstaller itself has
//! no dialog).

use std::path::{Path, PathBuf};

/// Marker file in the data folder: present means "delete my data when
/// Tingskriver is uninstalled".
pub const REMOVE_ON_UNINSTALL_MARKER: &str = "slet-data-ved-afinstallation";

/// Credential Manager targets of Tingskriver start with this.
pub const CREDENTIAL_PREFIX: &str = "Tingskriver/";

/// Remember whether uninstalling should delete the user's data.
pub fn set_remove_on_uninstall(app_data_dir: &Path, remove: bool) -> std::io::Result<()> {
    let marker = app_data_dir.join(REMOVE_ON_UNINSTALL_MARKER);
    if remove {
        std::fs::create_dir_all(app_data_dir)?;
        std::fs::write(
            marker,
            "Tingskriver sletter dine data, når programmet afinstalleres. Slet filen for at beholde dem.\n",
        )
    } else {
        match std::fs::remove_file(marker) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }
}

pub fn remove_on_uninstall_requested(app_data_dir: &Path) -> bool {
    app_data_dir.join(REMOVE_ON_UNINSTALL_MARKER).is_file()
}

/// The folders deleted on request: the data folder and the local folder
/// with the logs. Only folders named after the app id qualify, so a wrong
/// path can never delete anything else.
pub fn removal_targets(app_data_dir: Option<PathBuf>, log_dir: Option<PathBuf>) -> Vec<PathBuf> {
    let local_dir = log_dir.and_then(|logs| logs.parent().map(Path::to_path_buf));
    let mut targets: Vec<PathBuf> = Vec::new();
    for dir in [app_data_dir, local_dir].into_iter().flatten() {
        let named_after_app = dir.file_name().is_some_and(|name| name == crate::paths::APP_ID);
        if named_after_app && !targets.contains(&dir) {
            targets.push(dir);
        }
    }
    targets
}

/// Delete the user's data if they asked for it. Returns what happened, for
/// the log.
pub fn remove_if_requested() -> Vec<String> {
    let Some(app_data_dir) = crate::paths::app_data_dir() else {
        return vec!["no data folder; nothing to remove".to_string()];
    };
    if !remove_on_uninstall_requested(&app_data_dir) {
        return vec![format!("keeping user data in {}", app_data_dir.display())];
    }
    let mut report = Vec::new();
    for dir in removal_targets(Some(app_data_dir), crate::paths::log_dir()) {
        match std::fs::remove_dir_all(&dir) {
            Ok(()) => report.push(format!("removed {}", dir.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => report.push(format!("could not remove {}: {e}", dir.display())),
        }
    }
    report.extend(remove_credentials());
    report
}

#[cfg(windows)]
fn remove_credentials() -> Vec<String> {
    match super::win::delete_credentials_with_prefix(CREDENTIAL_PREFIX) {
        Ok(count) => vec![format!("removed {count} Credential Manager entries")],
        Err(e) => vec![format!("could not remove Credential Manager entries: {e}")],
    }
}

#[cfg(not(windows))]
fn remove_credentials() -> Vec<String> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::APP_ID;

    #[test]
    fn data_stays_unless_the_user_asks() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join(APP_ID);
        assert!(!remove_on_uninstall_requested(&data));
        set_remove_on_uninstall(&data, true).unwrap();
        assert!(remove_on_uninstall_requested(&data));
        set_remove_on_uninstall(&data, false).unwrap();
        assert!(!remove_on_uninstall_requested(&data));
        // Turning it off twice is fine.
        set_remove_on_uninstall(&data, false).unwrap();
    }

    #[test]
    fn only_folders_named_after_the_app_are_removed() {
        let data = PathBuf::from("roaming").join(APP_ID);
        let logs = PathBuf::from("local").join(APP_ID).join("logs");
        assert_eq!(
            removal_targets(Some(data.clone()), Some(logs)),
            vec![data.clone(), PathBuf::from("local").join(APP_ID)]
        );
        // TINGSKRIVER_DATA_DIR style overrides or odd log paths are never
        // targets.
        assert!(removal_targets(Some(PathBuf::from("C:/Users/x")), Some(PathBuf::from("logs"))).is_empty());
        assert_eq!(removal_targets(Some(data.clone()), Some(data.join("logs"))), vec![data]);
    }

    #[test]
    fn the_credential_prefix_matches_the_dictation_keys() {
        assert!(crate::dictation::secrets::SYV_TARGET.starts_with(CREDENTIAL_PREFIX));
    }
}
