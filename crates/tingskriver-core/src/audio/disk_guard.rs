//! Advisory disk-space monitoring, independent of capture and the writer.
#![deny(clippy::unwrap_used)]

use super::recording_state::RecordingState;
use crate::{
    database::init::available_space,
    host::{HostExt, HostHandle},
};
use std::{path::PathBuf, sync::Arc, time::Duration};

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
    fn poll(&mut self, available: Option<u64>) -> Option<DiskEvent> {
        let available = available?;
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
    spawn_with_measure(app, state, move || {
        let path = folder.clone();
        async move {
            tokio::task::spawn_blocking(move || available_space(&path))
                .await
                .ok()
                .flatten()
        }
    });
}

fn spawn_with_measure<F, Fut>(
    app: HostHandle,
    state: Arc<RecordingState>,
    mut measure: F,
) -> tokio::task::JoinHandle<()>
where
    F: FnMut() -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Option<u64>> + Send,
{
    tokio::spawn(async move {
        let mut guard = DiskGuard::default();
        while state.is_recording() {
            let available = measure().await;
            // A measurement may outlive stop. Never publish it into a later session.
            if !state.is_recording() {
                break;
            }
            if let Some(event) = guard.poll(available) {
                emit(&app, event);
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }
        if guard.low {
            emit(&app, DiskEvent::DiskOk);
        }
    })
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

    #[tokio::test]
    async fn disk_guard_spawn_measures_immediately_every_five_seconds_and_stops() {
        tokio::time::pause();
        let host = Arc::new(TestHost::default());
        let state = RecordingState::new();
        state.start_recording().expect("recording");
        let measurements = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let calls = measurements.clone();
        let task = spawn_with_measure(host.clone(), state.clone(), move || {
            calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            std::future::ready(Some(0))
        });
        tokio::task::yield_now().await;
        assert_eq!(measurements.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(host.0.lock().expect("events")[0].0, "recording-disk-low");
        tokio::time::advance(Duration::from_secs(4)).await;
        tokio::task::yield_now().await;
        assert_eq!(measurements.load(std::sync::atomic::Ordering::SeqCst), 1);
        tokio::time::sleep(Duration::from_secs(1)).await;
        tokio::task::yield_now().await;
        assert_eq!(measurements.load(std::sync::atomic::Ordering::SeqCst), 2);
        assert_eq!(host.0.lock().expect("events").len(), 1);
        state.stop_recording();
        tokio::time::advance(POLL_INTERVAL).await;
        task.await.expect("guard task");
        assert_eq!(measurements.load(std::sync::atomic::Ordering::SeqCst), 2);
        let events = host.0.lock().expect("events");
        assert_eq!(events.len(), 2);
        assert_eq!(events[1].0, "recording-disk-ok");
        assert_eq!(events[1].1["kind"], "DiskOk");
    }

    #[tokio::test]
    async fn disk_guard_discards_a_measurement_that_finishes_after_stop() {
        tokio::time::pause();
        let host = Arc::new(TestHost::default());
        let state = RecordingState::new();
        state.start_recording().expect("recording");
        let task = spawn_with_measure(host.clone(), state.clone(), || async {
            tokio::time::sleep(Duration::from_secs(1)).await;
            Some(0)
        });
        tokio::task::yield_now().await;
        state.stop_recording();
        tokio::time::advance(Duration::from_secs(1)).await;
        task.await.expect("guard task");
        assert!(host.0.lock().expect("events").is_empty());
    }

    #[test]
    fn disk_guard_routes_transitions_through_host_even_when_ui_fails() {
        let host = Arc::new(TestHost::default());
        let app: HostHandle = host.clone();
        let state = RecordingState::new();
        state.start_recording().expect("recording");
        let mut guard = DiskGuard::default();
        for available in [OK_BYTES, 0, 0, LOW_BYTES, OK_BYTES, OK_BYTES] {
            if let Some(event) = guard.poll(Some(available)) {
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
            if let Some(event) = guard.poll(bytes) {
                events.push(event);
            }
        }
        assert_eq!(events, [DiskEvent::DiskLow, DiskEvent::DiskOk]);
    }

    #[test]
    fn disk_guard_unknown_space_does_not_change_state() {
        let mut guard = DiskGuard::default();
        assert_eq!(guard.poll(None), None);
        assert_eq!(guard.poll(Some(0)), Some(DiskEvent::DiskLow));
        assert_eq!(guard.poll(None), None);
        assert_eq!(guard.poll(Some(OK_BYTES)), Some(DiskEvent::DiskOk));
        assert_eq!(guard.poll(Some(0)), Some(DiskEvent::DiskLow));
    }
}
