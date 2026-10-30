//! Glue between `tingskriver_core` and Tauri: core reaches the UI through
//! [`AppHost`], which is implemented here on top of the `AppHandle`.

use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Listener, Manager, Runtime};
use tauri_plugin_store::StoreExt;
use tingskriver_core::database::manager::DatabaseManager;
use tingskriver_core::host::{AppHost, HostHandle, ListenerId};
use tingskriver_core::state::AppState;

struct TauriHost<R: Runtime>(AppHandle<R>);

impl<R: Runtime> AppHost for TauriHost<R> {
    fn emit_value(&self, event: &str, payload: serde_json::Value) -> Result<(), String> {
        self.0.emit(event, payload).map_err(|e| e.to_string())
    }

    fn database(&self) -> Option<DatabaseManager> {
        self.0
            .try_state::<AppState>()
            .map(|state| state.db_manager.clone())
    }

    fn app_data_dir(&self) -> Result<PathBuf, String> {
        self.0.path().app_data_dir().map_err(|e| e.to_string())
    }

    fn listen(&self, event: &str, handler: Box<dyn Fn(&str) + Send + Sync>) -> ListenerId {
        self.0
            .listen(event.to_string(), move |event: tauri::Event| handler(event.payload()))
    }

    fn unlisten(&self, id: ListenerId) {
        self.0.unlisten(id);
    }

    fn read_setting(&self, store: &str, key: &str) -> Result<Option<serde_json::Value>, String> {
        let store = self.0.store(store).map_err(|e| e.to_string())?;
        Ok(store.get(key))
    }

    fn write_setting(&self, store: &str, key: &str, value: serde_json::Value) -> Result<(), String> {
        let store = self
            .0
            .store(store)
            .map_err(|e| format!("Failed to access store: {}", e))?;
        store.set(key, value);
        store
            .save()
            .map_err(|e| format!("Failed to save store to disk: {}", e))
    }

    fn refresh_status_ui(&self) {
        crate::tray::update_tray_menu(&self.0);
    }
}

/// Host handle that forwards core events to every Tauri window and reads
/// state managed by Tauri.
pub fn host<R: Runtime>(app: &AppHandle<R>) -> HostHandle {
    Arc::new(TauriHost(app.clone()))
}

/// The app data folder, panicking like the database code did before it
/// moved to core.
pub fn app_data_dir<R: Runtime>(app: &AppHandle<R>) -> PathBuf {
    app.path()
        .app_data_dir()
        .expect("failed to get app data dir")
}
