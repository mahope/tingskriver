use crate::host::HostHandle;
use log::{info, warn};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use anyhow::Result;
#[cfg(target_os = "macos")]
use log::error;

#[cfg(target_os = "macos")]
use crate::audio::capture::AudioCaptureBackend;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RecordingPreferences {
    pub save_folder: PathBuf,
    pub auto_save: bool,
    pub file_format: String,
    #[serde(default)]
    pub preferred_mic_device: Option<String>,
    #[serde(default)]
    pub preferred_system_device: Option<String>,
    #[cfg(target_os = "macos")]
    #[serde(default)]
    pub system_audio_backend: Option<String>,
}

impl Default for RecordingPreferences {
    fn default() -> Self {
        Self {
            save_folder: get_default_recordings_folder(),
            auto_save: true,
            file_format: "mp4".to_string(),
            preferred_mic_device: None,
            preferred_system_device: None,
            #[cfg(target_os = "macos")]
            system_audio_backend: Some("coreaudio".to_string()),
        }
    }
}

/// Get the default recordings folder based on platform
pub fn get_default_recordings_folder() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        // Windows: %USERPROFILE%\Music\tingskriver-recordings
        if let Some(music_dir) = dirs::audio_dir() {
            music_dir.join("tingskriver-recordings")
        } else {
            // Fallback to Documents if Music folder is not available
            dirs::document_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("tingskriver-recordings")
        }
    }

    #[cfg(target_os = "macos")]
    {
        // macOS: ~/Movies/tingskriver-recordings
        if let Some(movies_dir) = dirs::video_dir() {
            movies_dir.join("tingskriver-recordings")
        } else {
            // Fallback to Documents if Movies folder is not available
            dirs::document_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("tingskriver-recordings")
        }
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        // Linux/Others: ~/Documents/tingskriver-recordings
        dirs::document_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("tingskriver-recordings")
    }
}

/// Ensure the recordings directory exists
#[allow(clippy::disallowed_methods)] // Synchronous disk worker.
pub fn ensure_recordings_directory(path: &PathBuf) -> Result<()> {
    if !path.exists() {
        std::fs::create_dir_all(path)?;
        info!("Created recordings directory: {:?}", path);
    }
    Ok(())
}

/// Generate a unique filename for a recording
pub fn generate_recording_filename(format: &str) -> String {
    let now = chrono::Utc::now();
    let timestamp = now.format("%Y%m%d_%H%M%S");
    format!("recording_{}.{}", timestamp, format)
}

/// Load recording preferences from store
pub async fn load_recording_preferences(app: &HostHandle) -> Result<RecordingPreferences> {
    // Try to load from the host's settings store
    let stored = match app.read_setting("recording_preferences.json", "preferences") {
        Ok(stored) => stored,
        Err(e) => {
            warn!("Failed to access store: {}, using defaults", e);
            return Ok(RecordingPreferences::default());
        }
    };

    // Try to get the preferences from store
    let prefs = if let Some(value) = stored {
        match serde_json::from_value::<RecordingPreferences>(value) {
            Ok(mut p) => {
                info!("Loaded recording preferences from store");
                // Update macOS backend to current value if needed
                #[cfg(target_os = "macos")]
                {
                    let backend = crate::audio::capture::get_current_backend();
                    p.system_audio_backend = Some(backend.to_string());
                }
                p
            }
            Err(e) => {
                warn!("Failed to deserialize preferences: {}, using defaults", e);
                RecordingPreferences::default()
            }
        }
    } else {
        info!("No stored preferences found, using defaults");
        RecordingPreferences::default()
    };

    info!("Loaded recording preferences: save_folder={:?}, auto_save={}, format={}, mic={:?}, system={:?}",
          prefs.save_folder, prefs.auto_save, prefs.file_format,
          prefs.preferred_mic_device, prefs.preferred_system_device);
    Ok(prefs)
}

/// Save recording preferences to store
pub async fn save_recording_preferences(
    app: &HostHandle,
    preferences: &RecordingPreferences,
) -> Result<()> {
    info!("Saving recording preferences: save_folder={:?}, auto_save={}, format={}, mic={:?}, system={:?}",
          preferences.save_folder, preferences.auto_save, preferences.file_format,
          preferences.preferred_mic_device, preferences.preferred_system_device);

    // Serialize preferences to JSON value
    let prefs_value = serde_json::to_value(preferences)
        .map_err(|e| anyhow::anyhow!("Failed to serialize preferences: {}", e))?;

    // Save to the host's settings store and persist to disk
    app.write_setting("recording_preferences.json", "preferences", prefs_value)
        .map_err(|e| anyhow::anyhow!(e))?;

    info!("Successfully persisted recording preferences to disk");

    // Save backend preference to global config
    #[cfg(target_os = "macos")]
    if let Some(backend_str) = &preferences.system_audio_backend {
        if let Some(backend) = AudioCaptureBackend::from_string(backend_str) {
            info!("Setting audio capture backend to: {:?}", backend);
            crate::audio::capture::set_current_backend(backend);
        }
    }

    // Ensure the directory exists
    ensure_recordings_directory(&preferences.save_folder)?;

    Ok(())
}

/// Recording preferences behind the Tauri commands of the same name
pub async fn get_recording_preferences(app: HostHandle) -> Result<RecordingPreferences, String> {
    load_recording_preferences(&app)
        .await
        .map_err(|e| format!("Failed to load recording preferences: {}", e))
}

pub async fn set_recording_preferences(
    app: HostHandle,
    preferences: RecordingPreferences,
) -> Result<(), String> {
    save_recording_preferences(&app, &preferences)
        .await
        .map_err(|e| format!("Failed to save recording preferences: {}", e))
}

pub async fn get_default_recordings_folder_path() -> Result<String, String> {
    let path = get_default_recordings_folder();
    Ok(path.to_string_lossy().to_string())
}

pub async fn open_recordings_folder(app: HostHandle) -> Result<(), String> {
    let preferences = load_recording_preferences(&app)
        .await
        .map_err(|e| format!("Failed to load preferences: {}", e))?;

    // Ensure directory exists before trying to open it
    ensure_recordings_directory(&preferences.save_folder)
        .map_err(|e| format!("Failed to create directory: {}", e))?;

    let folder_path = preferences.save_folder.to_string_lossy().to_string();

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(&folder_path)
            .spawn()
            .map_err(|e| format!("Failed to open folder: {}", e))?;
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&folder_path)
            .spawn()
            .map_err(|e| format!("Failed to open folder: {}", e))?;
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        std::process::Command::new("xdg-open")
            .arg(&folder_path)
            .spawn()
            .map_err(|e| format!("Failed to open folder: {}", e))?;
    }

    info!("Opened recordings folder");
    Ok(())
}

pub async fn select_recording_folder(_app: HostHandle) -> Result<Option<String>, String> {
    // Use Tauri's dialog to select folder
    // For now, return None - this would need to be implemented with tauri-plugin-dialog
    // when it's available in the Cargo.toml
    warn!("Folder selection not yet implemented - using dialog plugin");
    Ok(None)
}

// Backend selection commands

/// Get available audio capture backends for the current platform
pub async fn get_available_audio_backends() -> Result<Vec<String>, String> {
    #[cfg(target_os = "macos")]
    {
        let backends = crate::audio::capture::get_available_backends();
        Ok(backends.iter().map(|b| b.to_string()).collect())
    }

    #[cfg(not(target_os = "macos"))]
    {
        // Only ScreenCaptureKit available on non-macOS
        Ok(vec!["screencapturekit".to_string()])
    }
}

/// Get current audio capture backend
pub async fn get_current_audio_backend() -> Result<String, String> {
    #[cfg(target_os = "macos")]
    {
        let backend = crate::audio::capture::get_current_backend();
        Ok(backend.to_string())
    }

    #[cfg(not(target_os = "macos"))]
    {
        Ok("screencapturekit".to_string())
    }
}

/// Set audio capture backend
pub async fn set_audio_backend(backend: String) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        use crate::audio::capture::AudioCaptureBackend;
        use crate::audio::permissions::{
            check_screen_recording_permission, request_screen_recording_permission,
        };

        let backend_enum = AudioCaptureBackend::from_string(&backend)
            .ok_or_else(|| format!("Invalid backend: {}", backend))?;

        // If switching to Core Audio, log information about Audio Capture permission
        if backend_enum == AudioCaptureBackend::CoreAudio {
            info!("🔐 Core Audio backend requires Audio Capture permission (macOS 14.4+)");
            info!("📍 Permission dialog will appear automatically when recording starts");

            // Check if permission is already granted (this is informational only)
            if !check_screen_recording_permission() {
                warn!("⚠️  Audio Capture permission may not be granted");

                // Attempt to open System Settings (opens System Settings)
                if let Err(e) = request_screen_recording_permission() {
                    error!("Failed to open System Settings: {}", e);
                }

                return Err(
                    "Core Audio requires Audio Capture permission. \
                    The permission dialog will appear when you start recording. \
                    If already denied, enable it in System Settings → Privacy & Security → Audio Capture, \
                    then restart the app.".to_string()
                );
            }

            info!(
                "✅ Core Audio backend selected - permission check will occur at recording start"
            );
        }

        info!("Setting audio backend to: {:?}", backend_enum);
        crate::audio::capture::set_current_backend(backend_enum);
        Ok(())
    }

    #[cfg(not(target_os = "macos"))]
    {
        if backend != "screencapturekit" {
            return Err(format!(
                "Backend {} not available on this platform",
                backend
            ));
        }
        Ok(())
    }
}

/// Get backend information (name and description)
#[derive(Serialize)]
pub struct BackendInfo {
    pub id: String,
    pub name: String,
    pub description: String,
}

pub async fn get_audio_backend_info() -> Result<Vec<BackendInfo>, String> {
    #[cfg(target_os = "macos")]
    {
        use crate::audio::capture::AudioCaptureBackend;

        let backends = vec![
            BackendInfo {
                id: AudioCaptureBackend::ScreenCaptureKit.to_string(),
                name: AudioCaptureBackend::ScreenCaptureKit.name().to_string(),
                description: AudioCaptureBackend::ScreenCaptureKit
                    .description()
                    .to_string(),
            },
            BackendInfo {
                id: AudioCaptureBackend::CoreAudio.to_string(),
                name: AudioCaptureBackend::CoreAudio.name().to_string(),
                description: AudioCaptureBackend::CoreAudio.description().to_string(),
            },
        ];
        Ok(backends)
    }

    #[cfg(not(target_os = "macos"))]
    {
        Ok(vec![BackendInfo {
            id: "screencapturekit".to_string(),
            name: "ScreenCaptureKit".to_string(),
            description: "Default system audio capture".to_string(),
        }])
    }
}

/// The settings file the Tauri app keeps the start reminder in
/// (`tauri-plugin-store`, `preferences.json` in the app data folder).
pub const PREFERENCES_STORE: &str = "preferences.json";
/// Key of the start reminder in [`PREFERENCES_STORE`].
pub const RECORDING_NOTICE_KEY: &str = "show_recording_notification";

/// Whether the reminder to tell the participants is shown when a recording
/// starts. On unless the user switched it off (Tauri: `?? true`), also when
/// the saved value is not a switch.
pub fn recording_notice_enabled(app: &HostHandle) -> Result<bool, String> {
    let stored = app.read_setting(PREFERENCES_STORE, RECORDING_NOTICE_KEY)?;
    Ok(stored.and_then(|v| v.as_bool()).unwrap_or(true))
}

/// Save the start reminder where the Tauri app keeps it.
pub fn set_recording_notice_enabled(app: &HostHandle, enabled: bool) -> Result<(), String> {
    app.write_setting(
        PREFERENCES_STORE,
        RECORDING_NOTICE_KEY,
        serde_json::Value::Bool(enabled),
    )
}

#[cfg(test)]
mod notice_tests {
    use super::*;
    use crate::database::manager::DatabaseManager;
    use crate::host::{AppHost, ListenerId};
    use std::sync::{Arc, Mutex};

    fn block_on<T>(future: impl std::future::Future<Output = T>) -> T {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(future)
    }

    #[derive(Default)]
    struct Store {
        values: Mutex<std::collections::HashMap<(String, String), serde_json::Value>>,
        /// Every write fails, like a full or read-only disk.
        fail_writes: std::sync::atomic::AtomicBool,
    }

    impl AppHost for Store {
        fn emit_json(&self, _: &str, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn database(&self) -> Option<DatabaseManager> {
            None
        }
        fn app_data_dir(&self) -> Result<PathBuf, String> {
            Err("none".into())
        }
        fn listen(&self, _: &str, _: Box<dyn Fn(&str) + Send + Sync>) -> ListenerId {
            0
        }
        fn unlisten(&self, _: ListenerId) {}
        fn read_setting(
            &self,
            store: &str,
            key: &str,
        ) -> Result<Option<serde_json::Value>, String> {
            Ok(self
                .values
                .lock()
                .unwrap()
                .get(&(store.into(), key.into()))
                .cloned())
        }
        fn write_setting(
            &self,
            store: &str,
            key: &str,
            value: serde_json::Value,
        ) -> Result<(), String> {
            if self.fail_writes.load(std::sync::atomic::Ordering::SeqCst) {
                return Err("disk full".to_string());
            }
            self.values
                .lock()
                .unwrap()
                .insert((store.into(), key.into()), value);
            Ok(())
        }
    }

    #[test]
    fn the_reminder_is_on_until_switched_off() {
        let host: HostHandle = Arc::new(Store::default());
        assert!(recording_notice_enabled(&host).unwrap());
        set_recording_notice_enabled(&host, false).unwrap();
        assert!(!recording_notice_enabled(&host).unwrap());
        set_recording_notice_enabled(&host, true).unwrap();
        assert!(recording_notice_enabled(&host).unwrap());
    }

    #[test]
    fn the_reminder_uses_the_tauri_store_and_key() {
        let store = Arc::new(Store::default());
        let host: HostHandle = store.clone();
        set_recording_notice_enabled(&host, false).unwrap();
        let saved = store.values.lock().unwrap();
        assert_eq!(
            saved.get(&(
                "preferences.json".into(),
                "show_recording_notification".into()
            )),
            Some(&serde_json::Value::Bool(false))
        );
    }

    #[test]
    fn a_saved_value_that_is_not_a_switch_means_on() {
        let store = Arc::new(Store::default());
        let host: HostHandle = store.clone();
        store
            .write_setting(
                PREFERENCES_STORE,
                RECORDING_NOTICE_KEY,
                serde_json::json!("nej"),
            )
            .unwrap();
        assert!(recording_notice_enabled(&host).unwrap());
    }

    #[test]
    fn a_failed_save_of_the_reminder_is_an_error_and_keeps_the_old_value() {
        let store = Arc::new(Store::default());
        let host: HostHandle = store.clone();
        set_recording_notice_enabled(&host, false).unwrap();
        store
            .fail_writes
            .store(true, std::sync::atomic::Ordering::SeqCst);
        assert_eq!(
            set_recording_notice_enabled(&host, true),
            Err("disk full".to_string())
        );
        assert!(!recording_notice_enabled(&host).unwrap());
    }

    #[test]
    fn chosen_devices_are_what_the_next_recording_loads() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::default());
        let host: HostHandle = store.clone();
        let chosen = RecordingPreferences {
            save_folder: dir.path().join("recordings"),
            preferred_mic_device: Some("Headset (input)".to_string()),
            preferred_system_device: Some("Højttalere (output)".to_string()),
            ..RecordingPreferences::default()
        };
        block_on(save_recording_preferences(&host, &chosen)).unwrap();
        // `start_recording` reads them with this function.
        let loaded = block_on(load_recording_preferences(&host)).unwrap();
        assert_eq!(
            loaded.preferred_mic_device.as_deref(),
            Some("Headset (input)")
        );
        assert_eq!(
            loaded.preferred_system_device.as_deref(),
            Some("Højttalere (output)")
        );
        assert_eq!(loaded.save_folder, chosen.save_folder);
        // The Tauri app reads the same store and key.
        assert!(store
            .values
            .lock()
            .unwrap()
            .contains_key(&("recording_preferences.json".into(), "preferences".into())));
        // Back to the defaults.
        let reset = RecordingPreferences {
            preferred_mic_device: None,
            preferred_system_device: None,
            ..loaded
        };
        block_on(save_recording_preferences(&host, &reset)).unwrap();
        let loaded = block_on(load_recording_preferences(&host)).unwrap();
        assert_eq!(loaded.preferred_mic_device, None);
        assert_eq!(loaded.preferred_system_device, None);
    }

    #[test]
    fn a_failed_save_of_the_devices_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::default());
        let host: HostHandle = store.clone();
        store
            .fail_writes
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let chosen = RecordingPreferences {
            save_folder: dir.path().to_path_buf(),
            preferred_mic_device: Some("Headset (input)".to_string()),
            ..RecordingPreferences::default()
        };
        assert!(block_on(save_recording_preferences(&host, &chosen)).is_err());
        let loaded = block_on(load_recording_preferences(&host)).unwrap();
        assert_eq!(loaded.preferred_mic_device, None);
    }
}
