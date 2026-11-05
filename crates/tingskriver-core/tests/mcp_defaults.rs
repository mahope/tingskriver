use tingskriver_core::mcp::{Access, Reader};

#[tokio::test]
async fn fresh_installation_disables_mcp_even_for_marked_meetings() {
    let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    sqlx::query("INSERT INTO meetings(id,title,created_at,updated_at,ai_shared) VALUES ('marked','Fixture','','',1)").execute(&pool).await.unwrap();
    let controls = tingskriver_core::mcp::settings(&pool).await.unwrap();
    assert_eq!(controls, Default::default());
    let reader = Reader::new(pool);
    assert_eq!(reader.selected_access().await.unwrap(), Access::Marked);
    assert_eq!(reader.access().await.unwrap(), Access::Off);
    assert!(reader.list_meetings(None, None, None).await.is_err());
    assert!(reader.get_meeting("marked", &[]).await.is_err());
}
