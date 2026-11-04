use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
use tingskriver_core::{
    database::manager::DatabaseManager,
    delivery::Delivery,
    host::{AppHost, ListenerId},
    mcp::{self, Access},
    mcp_ipc::{self, Action},
    providers::secrets::MemoryKeyStore,
};

struct Host {
    db: DatabaseManager,
    dir: PathBuf,
    events: Mutex<Vec<String>>,
}
impl AppHost for Host {
    fn database(&self) -> Option<DatabaseManager> {
        Some(self.db.clone())
    }
    fn app_data_dir(&self) -> Result<PathBuf, String> {
        Ok(self.dir.clone())
    }
    fn emit_json(&self, event: &str, payload: &str) -> Result<(), String> {
        self.events
            .lock()
            .unwrap()
            .push(format!("{event}:{payload}"));
        Ok(())
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

#[tokio::test]
async fn app_checks_policy_and_notifies_without_logging_note_content() {
    let dir = tempfile::tempdir().unwrap();
    let db = DatabaseManager::open_at(
        &dir.path().join("meeting.sqlite"),
        &dir.path().join("none"),
        Duration::from_secs(1),
    )
    .await
    .unwrap();
    sqlx::query("INSERT INTO meetings(id,title,created_at,updated_at) VALUES ('one','Fixture','2026-11-01','2026-11-01')").execute(db.pool()).await.unwrap();
    let host = Arc::new(Host {
        db,
        dir: dir.path().into(),
        events: Mutex::new(vec![]),
    });
    let delivery = Delivery::open(dir.path(), Arc::new(MemoryKeyStore::default()))
        .await
        .unwrap();
    let action = Action::AddNote {
        id: "one".into(),
        text: "PRIVATE note".into(),
    };
    assert!(mcp_ipc::execute(host.clone(), &delivery, action.clone())
        .await
        .is_err());
    assert!(host.events.lock().unwrap().is_empty());
    mcp::set_shared(host.db.pool(), "one", true).await.unwrap();
    mcp_ipc::execute(host.clone(), &delivery, action.clone())
        .await
        .unwrap();
    let notes = tingskriver_core::notes::NotesStore::new(host.db.pool().clone())
        .load("one")
        .await
        .unwrap();
    assert!(tingskriver_core::notes::markdown::serialize(&notes).contains("PRIVATE"));
    let events = host.events.lock().unwrap().join("\n");
    assert!(events.contains("started") && events.contains("completed"));
    assert!(!events.contains("PRIVATE"));
    for access in [Access::Metadata, Access::Off] {
        mcp::set_access(host.db.pool(), access).await.unwrap();
        assert!(mcp_ipc::execute(host.clone(), &delivery, action.clone())
            .await
            .is_err());
    }
}

#[cfg(unix)]
#[tokio::test]
async fn local_channel_is_private_and_missing_app_has_a_clear_error() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let action = Action::AddNote {
        id: "one".into(),
        text: "IPC note".into(),
    };
    assert!(mcp_ipc::call(dir.path(), &action)
        .await
        .unwrap_err()
        .contains("Åbn Tingskriver"));
    let db = DatabaseManager::open_at(
        &dir.path().join("meeting.sqlite"),
        &dir.path().join("none"),
        Duration::from_secs(1),
    )
    .await
    .unwrap();
    sqlx::query("INSERT INTO meetings(id,title,created_at,updated_at,ai_shared) VALUES ('one','Fixture','','',1)").execute(db.pool()).await.unwrap();
    let host = Arc::new(Host {
        db,
        dir: dir.path().into(),
        events: Mutex::new(vec![]),
    });
    let delivery = Delivery::open(dir.path(), Arc::new(MemoryKeyStore::default()))
        .await
        .unwrap();
    let server = tokio::spawn(mcp_ipc::serve(host, delivery));
    tokio::time::timeout(Duration::from_secs(5), async {
        while !mcp_ipc::socket_path(dir.path()).exists() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        std::fs::metadata(dir.path().join("mcp-ipc"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert_eq!(
        mcp_ipc::call(dir.path(), &action).await.unwrap()["saved"],
        true
    );
    server.abort();
    let _ = server.await;
}
