//! The "Notifikationer om start og slut" switch of Indstillinger > Generelt.
//!
//! The Tauri app keeps its notification settings in
//! `<config dir>/meetily/notifications.json`
//! (`frontend/src-tauri/src/notifications/settings.rs`) and the switch means
//! that "recording started" and "recording stopped" are both on. This module
//! reads and writes the same file and the same two fields, so a user who
//! switches between the apps keeps the choice. Every other field in the file
//! is left as it is.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const STARTED: &str = "show_recording_started";
const STOPPED: &str = "show_recording_stopped";

/// Where the Tauri app saves its notification settings.
pub fn settings_path() -> Option<PathBuf> {
    dirs::config_dir().map(|dir| dir.join("meetily").join("notifications.json"))
}

/// The file Tauri writes on first launch (its `NotificationSettings::default`).
fn default_settings() -> Value {
    json!({
        "recording_notifications": true,
        "time_based_reminders": true,
        "meeting_reminders": true,
        "respect_do_not_disturb": true,
        "notification_sound": true,
        "system_permission_granted": false,
        "consent_given": false,
        "manual_dnd_mode": false,
        "notification_preferences": {
            "show_recording_started": false,
            "show_recording_stopped": false,
            "show_recording_paused": true,
            "show_recording_resumed": true,
            "show_transcription_complete": true,
            "show_meeting_reminders": true,
            "show_system_errors": true,
            "meeting_reminder_minutes": [15, 5]
        }
    })
}

#[allow(clippy::disallowed_methods)] // Small synchronous settings file, read on a worker.
fn read(path: &Path) -> Result<Option<Value>, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text)
            .map(Some)
            .map_err(|e| format!("{} is not valid JSON: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("could not read {}: {e}", path.display())),
    }
}

/// Whether notifications about the start and the end of a recording are on.
/// Off without a file, as in Tauri.
pub fn recording_notifications_enabled(path: &Path) -> Result<bool, String> {
    let Some(settings) = read(path)? else {
        return Ok(false);
    };
    let flag = |key: &str| {
        settings
            .pointer(&format!("/notification_preferences/{key}"))
            .and_then(Value::as_bool)
            .unwrap_or(false)
    };
    Ok(flag(STARTED) && flag(STOPPED))
}

/// Whether a start/stop notification is actually shown now: the switch is on
/// and "Forstyr ikke" (`manual_dnd_mode`, set by the Tauri app) is off.
///
/// Deviation from Tauri (`should_show_notifications`): `consent_given` and
/// `system_permission_granted` are not required. They belong to Tauri's
/// operating-system permission flow, which the GPUI app does not have; with
/// them required the switch would do nothing for a user who never ran Tauri.
/// Choosing the switch is the consent.
pub fn recording_notifications_to_show(path: &Path) -> Result<bool, String> {
    if !recording_notifications_enabled(path)? {
        return Ok(false);
    }
    let dnd = read(path)?
        .and_then(|s| s.get("manual_dnd_mode").and_then(Value::as_bool))
        .unwrap_or(false);
    Ok(!dnd)
}

/// Switch the start and end notifications on or off together.
#[allow(clippy::disallowed_methods)] // Small synchronous settings file, written on a worker.
pub fn set_recording_notifications(path: &Path, enabled: bool) -> Result<(), String> {
    let mut settings = read(path)?.unwrap_or_else(default_settings);
    let preferences = settings
        .as_object_mut()
        .ok_or_else(|| format!("{} is not a JSON object", path.display()))?
        .entry("notification_preferences")
        .or_insert_with(|| default_settings()["notification_preferences"].clone());
    let preferences = preferences.as_object_mut().ok_or_else(|| {
        format!(
            "{}: notification_preferences is not an object",
            path.display()
        )
    })?;
    preferences.insert(STARTED.to_string(), Value::Bool(enabled));
    preferences.insert(STOPPED.to_string(), Value::Bool(enabled));
    let text = serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    }
    // Write next to the file and rename, so a crash never leaves half a file.
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text).map_err(|e| format!("could not write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("could not replace {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("meetily").join("notifications.json");
        (dir, path)
    }

    #[test]
    fn do_not_disturb_silences_the_notifications() {
        let (_dir, path) = file();
        assert!(!recording_notifications_to_show(&path).unwrap());
        set_recording_notifications(&path, true).unwrap();
        assert!(recording_notifications_to_show(&path).unwrap());
        let mut saved: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        saved["manual_dnd_mode"] = json!(true);
        std::fs::write(&path, saved.to_string()).unwrap();
        // The switch still reads as on; only the delivery is held back.
        assert!(recording_notifications_enabled(&path).unwrap());
        assert!(!recording_notifications_to_show(&path).unwrap());
    }

    #[test]
    fn off_without_a_file() {
        let (_dir, path) = file();
        assert!(!recording_notifications_enabled(&path).unwrap());
    }

    #[test]
    fn switching_on_writes_the_full_tauri_file() {
        let (_dir, path) = file();
        set_recording_notifications(&path, true).unwrap();
        assert!(recording_notifications_enabled(&path).unwrap());
        let saved: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        // Tauri's struct has no field defaults: a missing one would make it
        // fail to read the file.
        assert_eq!(saved["consent_given"], false);
        assert_eq!(
            saved["notification_preferences"]["show_recording_paused"],
            true
        );
        assert_eq!(
            saved["notification_preferences"]["meeting_reminder_minutes"],
            json!([15, 5])
        );
    }

    #[test]
    fn other_choices_in_the_file_are_kept() {
        let (_dir, path) = file();
        let mut settings = default_settings();
        settings["notification_sound"] = json!(false);
        settings["notification_preferences"]["show_recording_paused"] = json!(false);
        settings["notification_preferences"]["meeting_reminder_minutes"] = json!([30]);
        settings["added_by_a_newer_version"] = json!(1);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, settings.to_string()).unwrap();
        set_recording_notifications(&path, true).unwrap();
        set_recording_notifications(&path, false).unwrap();
        let saved: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(saved["notification_sound"], false);
        assert_eq!(
            saved["notification_preferences"]["show_recording_paused"],
            false
        );
        assert_eq!(
            saved["notification_preferences"]["meeting_reminder_minutes"],
            json!([30])
        );
        assert_eq!(saved["added_by_a_newer_version"], 1);
        assert!(!recording_notifications_enabled(&path).unwrap());
    }

    #[test]
    fn only_both_flags_together_mean_on() {
        let (_dir, path) = file();
        let mut settings = default_settings();
        settings["notification_preferences"]["show_recording_started"] = json!(true);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, settings.to_string()).unwrap();
        assert!(!recording_notifications_enabled(&path).unwrap());
    }

    #[test]
    fn a_damaged_file_is_an_error_and_is_not_overwritten() {
        let (_dir, path) = file();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{ not json").unwrap();
        assert!(recording_notifications_enabled(&path).is_err());
        assert!(set_recording_notifications(&path, true).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ not json");
    }
}
