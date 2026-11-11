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
    sqlx::query("UPDATE mcp_settings SET enabled=1,notes=1,summary=1,transcript=1,participants=1")
        .execute(db.pool())
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
    sqlx::query("UPDATE mcp_settings SET enabled=1,notes=1,summary=1,transcript=1,participants=1")
        .execute(db.pool())
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

#[tokio::test]
async fn handoff_uses_the_app_queue_and_obeys_delivery_privacy() {
    use tingskriver_core::delivery::{Destination, Policy, Target};
    let dir = tempfile::tempdir().unwrap();
    let db = DatabaseManager::open_at(
        &dir.path().join("meeting.sqlite"),
        &dir.path().join("none"),
        Duration::from_secs(1),
    )
    .await
    .unwrap();
    sqlx::query("UPDATE mcp_settings SET enabled=1,notes=1,summary=1,transcript=1,participants=1")
        .execute(db.pool())
        .await
        .unwrap();
    sqlx::query("INSERT INTO meetings(id,title,created_at,updated_at,ai_shared) VALUES ('one','Fixture','2026-11-01T10:00:00Z','2026-11-01T10:00:00Z',1)").execute(db.pool()).await.unwrap();
    sqlx::query("INSERT INTO summary_processes(meeting_id,status,created_at,updated_at,result) VALUES ('one','completed','2026-11-01T10:00:00Z','2026-11-01T10:00:00Z','{\"markdown\":\"Approved summary\"}')").execute(db.pool()).await.unwrap();
    let host = Arc::new(Host {
        db,
        dir: dir.path().into(),
        events: Mutex::new(vec![]),
    });
    let delivery = Delivery::open(dir.path(), Arc::new(MemoryKeyStore::default()))
        .await
        .unwrap();
    let out = dir.path().join("out");
    delivery
        .save_target(
            Target {
                id: "archive".into(),
                name: "Archive".into(),
                destination: Destination::Folder { path: out.clone() },
            },
            None,
        )
        .await
        .unwrap();
    let action = Action::Handoff {
        id: "one".into(),
        target: "archive".into(),
    };
    assert!(mcp_ipc::execute(host.clone(), &delivery, action.clone())
        .await
        .is_err());
    assert!(delivery.jobs().await.unwrap().is_empty());
    delivery
        .set_policy(Policy {
            local_only: false,
            private: true,
        })
        .await
        .unwrap();
    assert!(mcp_ipc::execute(host.clone(), &delivery, action.clone())
        .await
        .is_err());
    delivery
        .set_policy(Policy {
            local_only: false,
            private: false,
        })
        .await
        .unwrap();
    let mut controls = mcp::settings(host.db.pool()).await.unwrap();
    controls.transcript = false;
    mcp::save_settings(host.db.pool(), controls).await.unwrap();
    assert!(mcp_ipc::execute(host.clone(), &delivery, action.clone())
        .await
        .unwrap_err()
        .contains("alle mødele"));
    assert!(delivery.jobs().await.unwrap().is_empty());
    controls.transcript = true;
    mcp::save_settings(host.db.pool(), controls).await.unwrap();
    let result = mcp_ipc::execute(host.clone(), &delivery, action)
        .await
        .unwrap();
    let job = result["job"].as_str().unwrap();
    assert_eq!(result["status"], "awaiting_approval");
    assert!(!out.exists());
    drop(delivery);
    let delivery = Delivery::open(dir.path(), Arc::new(MemoryKeyStore::default()))
        .await
        .unwrap();
    delivery.run_due().await.unwrap();
    assert!(!out.exists());
    assert_eq!(
        delivery.jobs().await.unwrap()[0].status,
        "awaiting_approval"
    );
    assert!(delivery.retry(job).await.is_err());
    let bundle = tingskriver_core::delivery::meeting::snapshot(
        host.db.pool(),
        "one",
        "User-reviewed summary".into(),
    )
    .await
    .unwrap();
    assert!(delivery
        .approve_request(job, "one", "archive", bundle.clone(), true)
        .await
        .is_err());
    assert!(delivery
        .approve_request(job, "another-meeting", "archive", bundle.clone(), false)
        .await
        .is_err());
    delivery
        .approve_request(job, "one", "archive", bundle, false)
        .await
        .unwrap();
    delivery.run_due().await.unwrap();
    assert_eq!(
        std::fs::read_to_string(out.join(job).join("summary.md")).unwrap(),
        "User-reviewed summary"
    );
    assert_eq!(delivery.jobs().await.unwrap()[0].status, "completed");
}

#[tokio::test]
async fn mcp_write_actions_require_the_corresponding_data_switch() {
    let dir = tempfile::tempdir().unwrap();
    let db = DatabaseManager::open_at(
        &dir.path().join("meeting.sqlite"),
        &dir.path().join("none"),
        Duration::from_secs(1),
    )
    .await
    .unwrap();
    sqlx::query("UPDATE mcp_settings SET enabled=1,notes=0,summary=0")
        .execute(db.pool())
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
    for (action, expected) in [
        (
            Action::AddNote {
                id: "one".into(),
                text: "note".into(),
            },
            "noter",
        ),
        (
            Action::RegenerateSummary {
                id: "one".into(),
                template: "daily_standup".into(),
            },
            "referat",
        ),
    ] {
        assert!(mcp_ipc::execute(host.clone(), &delivery, action)
            .await
            .unwrap_err()
            .contains(expected));
    }
    assert!(host.events.lock().unwrap().is_empty());
}

#[test]
fn mcp_pipe_names_are_stable_and_isolated_by_data_directory() {
    let first = PathBuf::from("user-one/data");
    assert_eq!(mcp_ipc::pipe_name(&first), mcp_ipc::pipe_name(&first));
    assert_ne!(
        mcp_ipc::pipe_name(&first),
        mcp_ipc::pipe_name(&PathBuf::from("user-two/data"))
    );
}

#[tokio::test]
async fn mcp_regeneration_retains_user_edits_after_completion() {
    use tingskriver_core::database::repositories::summary::SummaryProcessesRepository as Summaries;
    let dir = tempfile::tempdir().unwrap();
    let db = DatabaseManager::open_at(
        &dir.path().join("meeting.sqlite"),
        &dir.path().join("none"),
        Duration::from_secs(1),
    )
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO meetings(id,title,created_at,updated_at) VALUES ('one','Fixture','','')",
    )
    .execute(db.pool())
    .await
    .unwrap();
    let now = chrono::Utc::now();
    Summaries::create_or_reset_process(db.pool(), "one", now)
        .await
        .unwrap();
    for version in 0..2 {
        let edited = serde_json::json!({"markdown":format!("User edits {version}")});
        Summaries::update_meeting_summary(db.pool(), "one", &edited)
            .await
            .unwrap();
        let start = chrono::Utc::now();
        Summaries::create_or_reset_process(db.pool(), "one", start)
            .await
            .unwrap();
        Summaries::update_process_completed(
            db.pool(),
            "one",
            start,
            serde_json::json!({"markdown":"Generated"}),
            1,
            1.0,
        )
        .await
        .unwrap();
        let result = Summaries::get_summary_data(db.pool(), "one")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&result.result_backup.unwrap()).unwrap(),
            edited
        );
    }
}
