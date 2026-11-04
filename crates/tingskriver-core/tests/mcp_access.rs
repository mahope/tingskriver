use sqlx::SqlitePool;
use tingskriver_core::mcp::{Access, Reader};

async fn fixture() -> SqlitePool {
    let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
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
