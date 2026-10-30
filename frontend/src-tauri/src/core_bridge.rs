//! Glue between `tingskriver_core` and Tauri: core emits events through
//! [`EventSink`], which is implemented here on top of the `AppHandle`.

use std::sync::Arc;
use tauri::{AppHandle, Emitter, Runtime};
use tingskriver_core::events::{EventHandle, EventSink};

struct TauriEvents<R: Runtime>(AppHandle<R>);

impl<R: Runtime> EventSink for TauriEvents<R> {
    fn emit_value(&self, event: &str, payload: serde_json::Value) -> Result<(), String> {
        self.0.emit(event, payload).map_err(|e| e.to_string())
    }
}

/// Event handle that forwards core events to every Tauri window.
pub fn events<R: Runtime>(app: &AppHandle<R>) -> EventHandle {
    Arc::new(TauriEvents(app.clone()))
}
