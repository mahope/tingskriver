use sqlx::SqlitePool;
use tingskriver_core::mcp::{Access, Reader};

async fn fixture() -> SqlitePool {
    let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    sqlx::query("UPDATE mcp_settings SET enabled=1,notes=1,summary=1,transcript=1,participants=1")
        .execute(&pool)
        .await
        .unwrap();
    for (id, shared) in [("visible", true), ("hidden", false)] {
        sqlx::query("INSERT INTO meetings(id,title,created_at,updated_at,ai_shared) VALUES (?,?,'2026-11-01','2026-11-01',?)")
            .bind(id).bind(id).bind(shared).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO transcripts(id,meeting_id,transcript,timestamp) VALUES (?,?,'SECRET transcript','00:01')")
            .bind(id).bind(id).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO summary_processes(meeting_id,status,created_at,updated_at,result) VALUES (?,'completed','','','{\"summary\":\"SECRET summary\"}')")
            .bind(id).execute(&pool).await.unwrap();
    }
    pool
}

#[tokio::test]
async fn data_switches_apply_to_every_read_surface_and_live_reader() {
    use tingskriver_core::mcp::{save_settings, Settings};
    let pool = fixture().await;
    sqlx::query("INSERT INTO meeting_notes(meeting_id,notes_markdown,created_at,updated_at) VALUES ('visible','SECRET note','','')").execute(&pool).await.unwrap();
    sqlx::query("UPDATE summary_processes SET result=?")
        .bind(r##"{"markdown":"# Opgaver\n- [ ] SECRET task"}"##)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE transcripts SET speaker='SECRET speaker'")
        .execute(&pool)
        .await
        .unwrap();
    let reader = Reader::new(pool.clone());
    let mut value = Settings {
        enabled: true,
        ..Settings::default()
    };
    save_settings(&pool, value).await.unwrap();
    let metadata = reader.get_meeting("visible", &[]).await.unwrap();
    assert!(metadata.get("transskript").is_none());
    assert!(metadata.get("referat").is_none());
    assert!(metadata.get("noter").is_none());
    assert!(reader.search("SECRET").await.unwrap().is_empty());
    assert!(reader.list_tasks(false).await.unwrap().is_empty());
    value.transcript = true;
    save_settings(&pool, value).await.unwrap();
    let transcript = reader.get_meeting("visible", &[]).await.unwrap();
    assert_eq!(transcript["transskript"][0]["text"], "SECRET transcript");
    assert!(transcript["transskript"][0]["speaker"].is_null());
    assert!(reader
        .search("SECRET")
        .await
        .unwrap()
        .iter()
        .all(|hit| hit["source"] == "transcript"));
    value.participants = true;
    value.summary = true;
    save_settings(&pool, value).await.unwrap();
    assert_eq!(
        reader.get_meeting("visible", &[]).await.unwrap()["transskript"][0]["speaker"],
        "SECRET speaker"
    );
    assert_eq!(reader.list_tasks(true).await.unwrap().len(), 1);
    assert!(reader
        .get_meeting("visible", &[])
        .await
        .unwrap()
        .get("noter")
        .is_none());
    assert!(reader
        .search("SECRET")
        .await
        .unwrap()
        .iter()
        .any(|hit| hit["source"] == "summary"));
    value.notes = true;
    value.summary = false;
    value.transcript = false;
    save_settings(&pool, value).await.unwrap();
    assert_eq!(
        reader.get_meeting("visible", &[]).await.unwrap()["noter"],
        "SECRET note"
    );
    assert!(reader.list_tasks(true).await.unwrap().is_empty());
    let hits = reader.search("SECRET").await.unwrap();
    assert!(!hits.is_empty());
    assert!(hits.iter().all(|hit| hit["source"] == "note"));
    assert!(reader.get_meeting("hidden", &[]).await.is_err());
    value.enabled = false;
    save_settings(&pool, value).await.unwrap();
    assert!(reader.get_meeting("visible", &[]).await.is_err());
    assert!(reader.search("SECRET").await.is_err());
    assert!(reader.list_tasks(false).await.is_err());
}

#[tokio::test]
async fn new_installation_only_exposes_explicitly_shared_meetings() {
    let pool = fixture().await;
    let reader = Reader::new(pool);
    assert_eq!(reader.access().await.unwrap(), Access::Marked);
    let meetings = reader.list_meetings(None, None, None).await.unwrap();
    assert_eq!(meetings.len(), 1);
    assert_eq!(meetings[0]["id"], "visible");
    assert!(reader.get_meeting("hidden", &[]).await.is_err());
    assert!(reader
        .get_meeting("visible", &[])
        .await
        .unwrap()
        .to_string()
        .contains("SECRET"));
    let hits = reader.search("SECRET").await.unwrap();
    assert!(!hits.is_empty());
    assert!(hits.iter().all(|hit| hit["id"] == "visible"));
}

#[tokio::test]
async fn metadata_and_off_never_return_private_content() {
    let pool = fixture().await;
    let reader = Reader::new(pool.clone());
    for access in [Access::Metadata, Access::Off] {
        tingskriver_core::mcp::set_access(&pool, access)
            .await
            .unwrap();
        let result = reader.get_meeting("visible", &[]).await;
        if access == Access::Off {
            assert!(result.is_err());
        } else {
            assert!(!result.unwrap().to_string().contains("SECRET"));
        }
        assert!(reader.search("SECRET").await.unwrap_or_default().is_empty());
    }
}

#[tokio::test]
async fn policy_changes_apply_to_an_existing_reader() {
    let pool = fixture().await;
    let reader = Reader::new(pool.clone());
    tingskriver_core::mcp::set_access(&pool, Access::All)
        .await
        .unwrap();
    assert_eq!(
        reader.list_meetings(None, None, None).await.unwrap().len(),
        2
    );
    tingskriver_core::mcp::set_access(&pool, Access::Off)
        .await
        .unwrap();
    assert!(reader.list_meetings(None, None, None).await.is_err());
}

#[tokio::test]
async fn tasks_are_filtered_and_never_include_the_surrounding_summary() {
    let pool = fixture().await;
    sqlx::query("UPDATE summary_processes SET result=?")
        .bind(
            r#"{"markdown":"SECRET surrounding summary\n- [ ] Call supplier\n- [x] Already done"}"#,
        )
        .execute(&pool)
        .await
        .unwrap();
    let reader = Reader::new(pool.clone());
    let tasks = reader.list_tasks(true).await.unwrap();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0]["text"], "Call supplier");
    assert!(!serde_json::to_string(&tasks).unwrap().contains("SECRET"));
    assert_eq!(reader.list_tasks(false).await.unwrap().len(), 2);
    tingskriver_core::mcp::set_access(&pool, Access::Metadata)
        .await
        .unwrap();
    assert_eq!(reader.list_tasks(true).await.unwrap().len(), 2);
    tingskriver_core::mcp::set_access(&pool, Access::Off)
        .await
        .unwrap();
    assert!(reader.list_tasks(true).await.is_err());
}

#[tokio::test]
async fn dictation_needs_all_access_and_reads_only_allowlisted_fields() {
    let pool = fixture().await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("dictation.sqlite");
    let store = SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&path)
            .create_if_missing(true),
    )
    .await
    .unwrap();
    sqlx::raw_sql("CREATE TABLE dictation_history(id INTEGER, text TEXT,app TEXT,created_at INTEGER,audio_path TEXT); INSERT INTO dictation_history VALUES (1,'dictated','Editor',123,'SECRET path');")
        .execute(&store).await.unwrap();
    let reader = Reader::new(pool.clone());
    for access in [Access::Marked, Access::Metadata, Access::Off] {
        tingskriver_core::mcp::set_access(&pool, access)
            .await
            .unwrap();
        assert!(reader.recent_dictations(&path, 10).await.is_err());
    }
    tingskriver_core::mcp::set_access(&pool, Access::All)
        .await
        .unwrap();
    let values = reader.recent_dictations(&path, 10).await.unwrap();
    assert_eq!(values[0]["text"], "dictated");
    assert!(!serde_json::to_string(&values).unwrap().contains("SECRET"));
    assert!(reader.recent_dictations(&path, 0).await.unwrap().is_empty());
}

#[tokio::test]
async fn tasks_include_plain_lists_only_under_task_headings() {
    let pool = fixture().await;
    sqlx::query("UPDATE summary_processes SET result=?")
        .bind(r###"{"markdown":"## Resumé\n- SECRET\n## Opgaver\n- Call supplier\n1. Book room\n## Beslutninger\n- SECRET decision"}"###)
        .execute(&pool).await.unwrap();
    let tasks = Reader::new(pool).list_tasks(true).await.unwrap();
    assert_eq!(tasks.len(), 2);
    assert_eq!(tasks[1]["text"], "Book room");
    assert!(!serde_json::to_string(&tasks).unwrap().contains("SECRET"));
}

#[tokio::test]
async fn missing_policy_fails_closed_for_reads_and_updates() {
    let pool = fixture().await;
    sqlx::query("DELETE FROM mcp_settings")
        .execute(&pool)
        .await
        .unwrap();
    let reader = Reader::new(pool.clone());
    assert!(reader.list_meetings(None, None, None).await.is_err());
    assert!(tingskriver_core::mcp::set_access(&pool, Access::All)
        .await
        .is_err());
}

#[tokio::test]
async fn local_sharing_controls_include_private_meetings_and_can_revoke_access() {
    let pool = fixture().await;
    let meetings = tingskriver_core::mcp::sharing_meetings(&pool)
        .await
        .unwrap();
    assert_eq!(meetings.len(), 2);
    assert!(!meetings.iter().find(|m| m.id == "hidden").unwrap().shared);
    let reader = Reader::new(pool.clone());
    for shared in [true, false] {
        tingskriver_core::mcp::set_shared(&pool, "hidden", shared)
            .await
            .unwrap();
        assert_eq!(reader.get_meeting("hidden", &[]).await.is_ok(), shared);
        let reloaded = tingskriver_core::mcp::sharing_meetings(&pool)
            .await
            .unwrap();
        assert_eq!(
            reloaded.iter().find(|m| m.id == "hidden").unwrap().shared,
            shared
        );
    }
    assert!(tingskriver_core::mcp::set_shared(&pool, "missing", true)
        .await
        .is_err());
}

#[tokio::test]
async fn meeting_notes_preserve_headings_lists_and_task_status_read_only() {
    let pool = fixture().await;
    let doc = tingskriver_core::notes::markdown::parse(
        "## Plan\n\n- [ ] Ring tilbage\n\n- [x] Send kontrakt\n\n- Punkt\n\n1. Første",
    )
    .unwrap();
    tingskriver_core::notes::NotesStore::new(pool.clone())
        .save("visible", &doc)
        .await
        .unwrap();
    let expected = tingskriver_core::notes::markdown::serialize(&doc);
    sqlx::query("PRAGMA query_only=ON")
        .execute(&pool)
        .await
        .unwrap();
    let result = Reader::new(pool)
        .get_meeting("visible", &["noter".into()])
        .await
        .unwrap();
    assert_eq!(result["noter"], expected);
}
