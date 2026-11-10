//! Advisory disk-space monitoring, independent of capture and the writer.
#![deny(clippy::unwrap_used)]

use super::recording_state::RecordingState;
use crate::{
    database::init::available_space,
    host::{HostExt, HostHandle},
};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

const POLL_INTERVAL: Duration = Duration::from_secs(5);
const LOW_BYTES: u64 = 256 * 1024 * 1024;
const OK_BYTES: u64 = 320 * 1024 * 1024;

#[derive(Debug, PartialEq, Eq, serde::Serialize)]
enum DiskEvent {
    DiskLow,
    DiskOk,
}

#[derive(Default)]
struct DiskGuard {
    low: bool,
}

impl DiskGuard {
    fn poll(
        &mut self,
        path: &Path,
        measure: impl FnOnce(&Path) -> Option<u64>,
    ) -> Option<DiskEvent> {
        let available = measure(path)?;
        if !self.low && available < LOW_BYTES {
            self.low = true;
            Some(DiskEvent::DiskLow)
        } else if self.low && available >= OK_BYTES {
            self.low = false;
            Some(DiskEvent::DiskOk)
        } else {
            None
        }
    }
}

fn emit(app: &HostHandle, event: DiskEvent) {
    let (name, message) = match event {
        DiskEvent::DiskLow => (
            "recording-disk-low",
            "Disken er næsten fuld. Frigør plads, så optagelsen fortsat kan gemmes.",
        ),
        DiskEvent::DiskOk => ("recording-disk-ok", "Der er igen plads på disken."),
    };
    if let Err(error) = app.emit(
        name,
        serde_json::json!({ "kind": event, "message": message }),
    ) {
        log::warn!("Failed to emit {name}: {error}");
    }
}

pub(super) fn spawn(app: HostHandle, state: Arc<RecordingState>, folder: PathBuf) {
    tokio::spawn(async move {
        let mut guard = DiskGuard::default();
        while state.is_recording() {
            let path = folder.clone();
            let measured = tokio::task::spawn_blocking(move || available_space(&path)).await;
            // A measurement may outlive stop. Never publish it into a later session.
            if !state.is_recording() {
                break;
            }
            if let Ok(available) = measured {
                if let Some(event) = guard.poll(&folder, |_| available) {
                    emit(&app, event);
                }
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct TestHost(std::sync::Mutex<Vec<(String, serde_json::Value)>>);

    impl crate::host::AppHost for TestHost {
        fn emit_json(&self, event: &str, json: &str) -> Result<(), String> {
            self.0
                .lock()
                .expect("events")
                .push((event.into(), serde_json::from_str(json).expect("payload")));
            Err("UI unavailable".into())
        }
        fn database(&self) -> Option<crate::database::manager::DatabaseManager> {
            None
        }
        fn app_data_dir(&self) -> Result<PathBuf, String> {
            Err("unused".into())
        }
        fn listen(&self, _: &str, _: Box<dyn Fn(&str) + Send + Sync>) -> crate::host::ListenerId {
            0
        }
        fn unlisten(&self, _: crate::host::ListenerId) {}
        fn read_setting(&self, _: &str, _: &str) -> Result<Option<serde_json::Value>, String> {
            Ok(None)
        }
        fn write_setting(&self, _: &str, _: &str, _: serde_json::Value) -> Result<(), String> {
            Ok(())
        }
    }

    #[test]
    fn disk_guard_routes_transitions_through_host_even_when_ui_fails() {
        let host = Arc::new(TestHost::default());
        let app: HostHandle = host.clone();
        let state = RecordingState::new();
        state.start_recording().expect("recording");
        let mut guard = DiskGuard::default();
        for available in [OK_BYTES, 0, 0, LOW_BYTES, OK_BYTES, OK_BYTES] {
            if let Some(event) = guard.poll(Path::new("recording"), |_| Some(available)) {
                emit(&app, event);
            }
        }
        let events = host.0.lock().expect("events");
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].0, "recording-disk-low");
        assert_eq!(events[0].1["kind"], "DiskLow");
        assert_eq!(events[1].0, "recording-disk-ok");
        assert_eq!(events[1].1["kind"], "DiskOk");
        assert!(state.is_recording());
    }

    #[test]
    fn disk_guard_emits_once_per_transition_with_hysteresis() {
        let mut guard = DiskGuard::default();
        let mut events = Vec::new();
        for bytes in [
            Some(OK_BYTES),
            Some(LOW_BYTES),
            Some(LOW_BYTES - 1),
            Some(0),
            None,
            Some(LOW_BYTES),
            Some(OK_BYTES - 1),
            Some(OK_BYTES),
            Some(OK_BYTES + 1),
        ] {
            if let Some(event) = guard.poll(Path::new("recording"), |_| bytes) {
                events.push(event);
            }
        }
        assert_eq!(events, [DiskEvent::DiskLow, DiskEvent::DiskOk]);
    }

    #[test]
    fn disk_guard_unknown_space_does_not_change_state() {
        let mut guard = DiskGuard::default();
        let path = Path::new("recording");
        assert_eq!(
            guard.poll(path, |p| {
                assert_eq!(p, path);
                None
            }),
            None
        );
        assert_eq!(guard.poll(path, |_| Some(0)), Some(DiskEvent::DiskLow));
        assert_eq!(guard.poll(path, |_| None), None);
        assert_eq!(
            guard.poll(path, |_| Some(OK_BYTES)),
            Some(DiskEvent::DiskOk)
        );
        assert_eq!(guard.poll(path, |_| Some(0)), Some(DiskEvent::DiskLow));
    }
}
