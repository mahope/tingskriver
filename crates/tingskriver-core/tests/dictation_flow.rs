use std::sync::{Arc, Mutex};
use tingskriver_core::dictation::service::{DictationBackend, DictationState, Service};
use tingskriver_core::dictation::{
    overlay::{Overlay, OverlayState},
    settings::DictationSettings,
};
use tingskriver_core::host::{AppHost, ListenerId};

struct Host(std::path::PathBuf);
impl AppHost for Host {
    fn emit_json(&self, _: &str, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn database(&self) -> Option<tingskriver_core::database::manager::DatabaseManager> {
        None
    }
    fn app_data_dir(&self) -> Result<std::path::PathBuf, String> {
        Ok(self.0.clone())
    }
    fn listen(&self, _: &str, _: Box<dyn Fn(&str) + Send + Sync>) -> ListenerId {
        0
    }
    fn unlisten(&self, _: ListenerId) {}
    fn read_setting(&self, _: &str, _: &str) -> Result<Option<serde_json::Value>, String> {
        Ok(None)
    }
    fn write_setting(&self, _: &str, _: &str, _: serde_json::Value) -> Result<(), String> {
        Ok(())
    }
}
struct Pill;
impl Overlay for Pill {
    fn ensure(&self) -> Result<(), String> {
        Ok(())
    }
    fn set(&self, _: OverlayState) {}
}
#[derive(Default)]
struct Fake {
    calls: Mutex<Vec<&'static str>>,
    inserted: Mutex<Vec<String>>,
    block: bool,
    fail_local: bool,
    fail_insert: bool,
    started: tokio::sync::Notify,
}
#[async_trait::async_trait]
impl DictationBackend for Fake {
    fn syv_key(&self) -> Option<String> {
        Some("invalid-test-key".into())
    }
    async fn local_available(&self) -> bool {
        true
    }
    async fn syv(
        &self,
        key: &str,
        _: &DictationSettings,
        _: Vec<u8>,
        _: String,
        _: String,
    ) -> Result<String, String> {
        assert_eq!(key, "invalid-test-key");
        self.calls.lock().unwrap().push("syv");
        Err("Syv.ai svarede 401 Unauthorized".into())
    }
    async fn local(&self, _: Vec<f32>) -> Result<String, String> {
        assert_eq!(std::thread::current().name(), Some("dictation-test"));
        self.calls.lock().unwrap().push("parakeet");
        if self.fail_local {
            return Err("fortrolig motorfejl zebra".into());
        }
        self.started.notify_one();
        if self.block {
            std::future::pending::<()>().await;
        }
        Ok("fortrolig diktering zebra".into())
    }
    async fn insert(&self, text: String) -> Result<(), String> {
        if self.fail_insert {
            return Err("fortrolig indsættelsesfejl zebra".into());
        }
        self.inserted.lock().unwrap().push(text);
        Ok(())
    }
}
struct Logs(Mutex<Vec<String>>);
static LOGS: Logs = Logs(Mutex::new(Vec::new()));
impl log::Log for Logs {
    fn enabled(&self, _: &log::Metadata) -> bool {
        true
    }
    fn log(&self, record: &log::Record) {
        self.0.lock().unwrap().push(record.args().to_string());
    }
    fn flush(&self) {}
}

#[tokio::test]
async fn dictation_flow_fallback_cancel_and_private_logs() {
    log::set_logger(&LOGS).unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    let dir = tempfile::tempdir().unwrap();
    let fake = Arc::new(Fake::default());
    let service = Service::open_for_test(
        Arc::new(Host(dir.path().into())),
        Arc::new(Pill),
        fake.clone(),
    )
    .await
    .unwrap();
    let mut run = service.test_trigger(vec![0.2; 16000]).unwrap();
    let mut states = Vec::new();
    while let Some(state) = run.events.recv().await {
        states.push(state);
    }
    run.join().unwrap();
    assert_eq!(
        states,
        vec![
            DictationState::Inactive,
            DictationState::Recording,
            DictationState::Processing,
            DictationState::Inserting,
            DictationState::Inactive
        ]
    );
    assert_eq!(*fake.calls.lock().unwrap(), vec!["syv", "parakeet"]);
    assert_eq!(
        *fake.inserted.lock().unwrap(),
        vec!["Fortrolig diktering zebra"]
    );

    let fake = Arc::new(Fake {
        block: true,
        ..Fake::default()
    });
    let service = Service::open_for_test(
        Arc::new(Host(dir.path().into())),
        Arc::new(Pill),
        fake.clone(),
    )
    .await
    .unwrap();
    let mut run = service.test_trigger(vec![0.2; 16000]).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), fake.started.notified())
        .await
        .unwrap();
    run.cancel();
    let mut states = Vec::new();
    while let Some(state) = run.events.recv().await {
        states.push(state);
    }
    run.join().unwrap();
    assert_eq!(
        states,
        vec![
            DictationState::Inactive,
            DictationState::Recording,
            DictationState::Processing,
            DictationState::Cancelled,
            DictationState::Inactive
        ]
    );
    assert!(fake.inserted.lock().unwrap().is_empty());
    for (fail_local, fail_insert, expected) in [
        (
            true,
            false,
            vec![
                DictationState::Inactive,
                DictationState::Recording,
                DictationState::Processing,
                DictationState::Error,
                DictationState::Inactive,
            ],
        ),
        (
            false,
            true,
            vec![
                DictationState::Inactive,
                DictationState::Recording,
                DictationState::Processing,
                DictationState::Inserting,
                DictationState::Error,
                DictationState::Inactive,
            ],
        ),
    ] {
        let fake = Arc::new(Fake {
            fail_local,
            fail_insert,
            ..Fake::default()
        });
        let service = Service::open_for_test(
            Arc::new(Host(dir.path().into())),
            Arc::new(Pill),
            fake.clone(),
        )
        .await
        .unwrap();
        let mut run = service.test_trigger(vec![0.2; 16000]).unwrap();
        let mut states = Vec::new();
        while let Some(state) = run.events.recv().await {
            states.push(state);
        }
        run.join().unwrap();
        assert_eq!(states, expected);
        assert!(fake.inserted.lock().unwrap().is_empty());
    }
    let logs = LOGS.0.lock().unwrap().join("\n");
    assert!(
        logs.contains("Dictation"),
        "capture must exercise real logs"
    );
    for private in [
        "fortrolig",
        "Fortrolig",
        "zebra",
        "invalid-test-key",
        "ChordSignal",
        "Pressed",
        "Released",
    ] {
        assert!(!logs.contains(private), "private data in logs");
    }
}
