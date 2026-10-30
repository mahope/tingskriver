//! Events from core to the UI.
//!
//! Core never talks to a UI framework directly. Code that used to call
//! `AppHandle::emit` takes an [`EventHandle`] instead; the Tauri layer
//! implements [`EventSink`] on top of its `AppHandle`, and the GPUI app will
//! implement it on top of its own channel.

use serde::Serialize;
use std::sync::Arc;

/// Receives events emitted by core.
pub trait EventSink: Send + Sync + 'static {
    /// Deliver an event with an already serialized payload to every listener.
    fn emit_value(&self, event: &str, payload: serde_json::Value) -> Result<(), String>;
}

/// Shared handle to the UI's event sink.
pub type EventHandle = Arc<dyn EventSink>;

/// `emit` with any serializable payload, mirroring `tauri::Emitter::emit`.
pub trait EventSinkExt {
    fn emit<S: Serialize>(&self, event: &str, payload: S) -> Result<(), String>;
}

impl<T: EventSink + ?Sized> EventSinkExt for T {
    fn emit<S: Serialize>(&self, event: &str, payload: S) -> Result<(), String> {
        let value = serde_json::to_value(payload).map_err(|e| e.to_string())?;
        self.emit_value(event, value)
    }
}

/// An event sink that drops everything (tests and headless use).
pub struct NoopEventSink;

impl EventSink for NoopEventSink {
    fn emit_value(&self, _event: &str, _payload: serde_json::Value) -> Result<(), String> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Recorder(Mutex<Vec<(String, serde_json::Value)>>);

    impl EventSink for Recorder {
        fn emit_value(&self, event: &str, payload: serde_json::Value) -> Result<(), String> {
            self.0.lock().unwrap().push((event.to_string(), payload));
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
        let handle: EventHandle = recorder.clone();
        handle.emit("progress", Progress { percent: 42 }).unwrap();
        handle.emit("done", ()).unwrap();
        let events = recorder.0.lock().unwrap();
        assert_eq!(events[0], ("progress".to_string(), serde_json::json!({ "percent": 42 })));
        assert_eq!(events[1], ("done".to_string(), serde_json::Value::Null));
    }
}
