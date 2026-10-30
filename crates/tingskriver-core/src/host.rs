//! The connection from core to the UI layer.
//!
//! Core never talks to a UI framework directly. Code that used to take a
//! Tauri `AppHandle` takes a [`HostHandle`] instead: it emits events to the
//! UI, reads the database once it is open and finds the app data folder.
//! The Tauri crate implements [`AppHost`] on top of its `AppHandle`; the GPUI
//! app will implement it on top of its own event channel.

use crate::database::manager::DatabaseManager;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;

/// Identifies a listener registered with [`AppHost::listen`].
pub type ListenerId = u32;

/// What core needs from the application around it.
pub trait AppHost: Send + Sync + 'static {
    /// Deliver an event with an already serialized payload to every listener.
    fn emit_value(&self, event: &str, payload: serde_json::Value) -> Result<(), String>;

    /// The open database, or `None` until it has been initialised (first
    /// launch before onboarding).
    fn database(&self) -> Option<DatabaseManager>;

    /// The per-user application data folder.
    fn app_data_dir(&self) -> Result<PathBuf, String>;

    /// Call `handler` with the JSON payload of every `event`, whether core
    /// or the UI emitted it, until [`AppHost::unlisten`].
    fn listen(&self, event: &str, handler: Box<dyn Fn(&str) + Send + Sync>) -> ListenerId;

    /// Remove a listener registered with [`AppHost::listen`].
    fn unlisten(&self, id: ListenerId);

    /// Read `key` from the settings file `store` (for example
    /// `recording_preferences.json`). `Ok(None)` when the key is not set.
    fn read_setting(&self, store: &str, key: &str) -> Result<Option<serde_json::Value>, String>;

    /// Write `key` in the settings file `store` and persist it to disk.
    fn write_setting(&self, store: &str, key: &str, value: serde_json::Value) -> Result<(), String>;

    /// Recording or model availability changed: refresh status UI such as
    /// the tray menu.
    fn refresh_status_ui(&self) {}
}

/// Shared handle to the application host.
pub type HostHandle = Arc<dyn AppHost>;

/// `emit` with any serializable payload, mirroring `tauri::Emitter::emit`.
pub trait HostExt {
    fn emit<S: Serialize>(&self, event: &str, payload: S) -> Result<(), String>;

    /// The open database. Panics when it is not initialised yet, like
    /// Tauri's `state::<AppState>()` did.
    fn expect_database(&self) -> DatabaseManager;
}

impl<T: AppHost + ?Sized> HostExt for T {
    fn emit<S: Serialize>(&self, event: &str, payload: S) -> Result<(), String> {
        let value = serde_json::to_value(payload).map_err(|e| e.to_string())?;
        self.emit_value(event, value)
    }

    fn expect_database(&self) -> DatabaseManager {
        self.database()
            .expect("state() called before manage() for AppState")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Recorder(Mutex<Vec<(String, serde_json::Value)>>);

    impl AppHost for Recorder {
        fn emit_value(&self, event: &str, payload: serde_json::Value) -> Result<(), String> {
            self.0.lock().unwrap().push((event.to_string(), payload));
            Ok(())
        }

        fn database(&self) -> Option<DatabaseManager> {
            None
        }

        fn app_data_dir(&self) -> Result<PathBuf, String> {
            Err("no app data dir in tests".to_string())
        }

        fn listen(&self, _event: &str, _handler: Box<dyn Fn(&str) + Send + Sync>) -> ListenerId {
            0
        }

        fn unlisten(&self, _id: ListenerId) {}

        fn read_setting(&self, _store: &str, _key: &str) -> Result<Option<serde_json::Value>, String> {
            Ok(None)
        }

        fn write_setting(&self, _store: &str, _key: &str, _value: serde_json::Value) -> Result<(), String> {
            Ok(())
        }
    }

    #[derive(Serialize)]
    struct Progress {
        percent: u8,
    }

    #[test]
    fn emit_serializes_payload_through_handle() {
        let recorder = Arc::new(Recorder::default());
        let handle: HostHandle = recorder.clone();
        handle.emit("progress", Progress { percent: 42 }).unwrap();
        handle.emit("done", ()).unwrap();
        let events = recorder.0.lock().unwrap();
        assert_eq!(events[0], ("progress".to_string(), serde_json::json!({ "percent": 42 })));
        assert_eq!(events[1], ("done".to_string(), serde_json::Value::Null));
    }
}
