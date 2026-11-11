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
#[derive(Default)]
struct Pill(Mutex<Vec<OverlayState>>);
impl Overlay for Pill {
    fn ensure(&self) -> Result<(), String> {
        Ok(())
    }
    fn set(&self, state: OverlayState) {
        self.0.lock().unwrap().push(state);
    }
}
#[derive(Default)]
struct Fake {
    calls: Mutex<Vec<&'static str>>,
    inserted: Mutex<Vec<String>>,
    block: bool,
    block_insert: bool,
    inserting: tokio::sync::Notify,
    release: tokio::sync::Notify,
    fail_local: bool,
    fail_insert: bool,
    started: tokio::sync::Notify,
    endpoint: Option<String>,
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
        wav: Vec<u8>,
        prompt: String,
        language: String,
    ) -> Result<String, String> {
        assert_eq!(key, "invalid-test-key");
        self.calls.lock().unwrap().push("syv");
        if let Some(endpoint) = &self.endpoint {
            return tingskriver_core::dictation::syv::transcribe_language(
                &reqwest::Client::new(),
                endpoint,
                key,
                "test",
                wav,
                Some(prompt),
                &language,
            )
            .await;
        }
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
        self.inserting.notify_one();
        if self.block_insert {
            self.release.notified().await;
        }
        self.inserted.lock().unwrap().push(text);
        Ok(())
    }
}
async fn drain(
    run: &mut tingskriver_core::dictation::service::DictationRun,
) -> Vec<DictationState> {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        let mut states = Vec::new();
        while let Some(state) = run.events.recv().await {
            states.push(state);
        }
        states
    })
    .await
    .expect("dictation worker did not finish")
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
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        loop {
            let mut buf = [0; 8192];
            let n = socket.read(&mut buf).await.unwrap();
            assert_ne!(n, 0);
            request.extend_from_slice(&buf[..n]);
            let text = String::from_utf8_lossy(&request);
            if let Some(end) = text.find("\r\n\r\n") {
                let length: usize = text[..end]
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length: ")
                            .map(|v| v.parse().unwrap())
                    })
                    .unwrap();
                if request.len() >= end + 4 + length {
                    break;
                }
            }
        }
        assert!(request.starts_with(b"POST /audio/transcriptions "));
        socket
            .write_all(
                b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            )
            .await
            .unwrap();
    });
    let fake = Arc::new(Fake {
        endpoint: Some(endpoint),
        ..Fake::default()
    });
    let service = Service::open_for_test(
        Arc::new(Host(dir.path().into())),
        Arc::new(Pill::default()),
        fake.clone(),
    )
    .await
    .unwrap();
    let mut broadcast = service.subscribe_states();
    let mut run = service.test_trigger(vec![0.2; 16000]).unwrap();
    let mut states = Vec::new();
    states.extend(drain(&mut run).await);
    run.join().unwrap();
    server.await.unwrap();
    let mut published = Vec::new();
    while let Ok(state) = broadcast.try_recv() {
        published.push(state);
    }
    assert_eq!(published, states);
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
        Arc::new(Pill::default()),
        fake.clone(),
    )
    .await
    .unwrap();
    let mut run = service.test_trigger(vec![0.2; 16000]).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), fake.started.notified())
        .await
        .unwrap();
    service.cancel_processing();
    let mut states = Vec::new();
    states.extend(drain(&mut run).await);
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
    let mut run = service.test_trigger(vec![0.2; 16000]).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), fake.started.notified())
        .await
        .unwrap();
    run.cancel();
    assert_eq!(drain(&mut run).await, states);
    run.join().unwrap();
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
            Arc::new(Pill::default()),
            fake.clone(),
        )
        .await
        .unwrap();
        let mut run = service.test_trigger(vec![0.2; 16000]).unwrap();
        let mut states = Vec::new();
        states.extend(drain(&mut run).await);
        run.join().unwrap();
        assert_eq!(states, expected);
        assert!(fake.inserted.lock().unwrap().is_empty());
    }
    let logs = LOGS.0.lock().unwrap().join("\n");
    assert!(
        logs.contains("Dictation"),
        "capture must exercise real logs"
    );
    assert!(logs.contains("Dictation Syv request failed: HTTP 401"));
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

#[tokio::test]
async fn cancellation_keeps_insertion_turn_until_insert_finishes() {
    let dir = tempfile::tempdir().unwrap();
    let fake = Arc::new(Fake {
        block_insert: true,
        ..Fake::default()
    });
    let service = Service::open_for_test(
        Arc::new(Host(dir.path().into())),
        Arc::new(Pill::default()),
        fake.clone(),
    )
    .await
    .unwrap();
    let mut first = service.test_trigger(vec![0.2; 16000]).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), fake.inserting.notified())
        .await
        .unwrap();
    first.cancel();
    let mut second = service.test_trigger(vec![0.2; 16000]).unwrap();
    assert!(tokio::time::timeout(
        std::time::Duration::from_millis(100),
        fake.inserting.notified()
    )
    .await
    .is_err());
    fake.release.notify_one();
    drain(&mut first).await;
    first.join().unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), fake.inserting.notified())
        .await
        .unwrap();
    fake.release.notify_one();
    drain(&mut second).await;
    second.join().unwrap();
    assert_eq!(fake.inserted.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn cancellation_hides_processing_overlay() {
    let dir = tempfile::tempdir().unwrap();
    let fake = Arc::new(Fake {
        block: true,
        ..Fake::default()
    });
    let pill = Arc::new(Pill::default());
    let service = Service::open_for_test(
        Arc::new(Host(dir.path().into())),
        pill.clone(),
        fake.clone(),
    )
    .await
    .unwrap();
    let mut run = service.test_trigger(vec![0.2; 16000]).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), fake.started.notified())
        .await
        .unwrap();
    run.cancel();
    drain(&mut run).await;
    run.join().unwrap();
    assert_eq!(pill.0.lock().unwrap().last(), Some(&OverlayState::Hidden));
    let mut run = service.test_trigger(vec![0.2; 16000]).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), fake.started.notified())
        .await
        .unwrap();
    service.cancel_processing();
    let recording = OverlayState::Recording {
        level: 0.5,
        locked: false,
    };
    pill.set(recording);
    drain(&mut run).await;
    run.join().unwrap();
    assert_eq!(pill.0.lock().unwrap().last(), Some(&recording));
}
