use tingskriver_core::mcp_audit::{clear, recent, record};

#[tokio::test]
async fn journal_is_bounded_and_clearable_and_rejects_unknown_tool_text() {
    let dir = tempfile::tempdir().unwrap();
    assert!(recent(dir.path()).await.unwrap().is_empty());
    record(dir.path(), "PRIVATE text", None, false)
        .await
        .unwrap();
    assert!(recent(dir.path()).await.unwrap().is_empty());
    for i in 0..103 {
        record(
            dir.path(),
            "get_meeting",
            Some(&format!("fixture-{i}")),
            i % 2 == 0,
        )
        .await
        .unwrap();
    }
    let entries = recent(dir.path()).await.unwrap();
    assert_eq!(entries.len(), 100);
    assert_eq!(entries[0].meeting_id.as_deref(), Some("fixture-102"));
    assert_eq!(entries[99].meeting_id.as_deref(), Some("fixture-3"));
    assert!(entries[0].success);
    assert!(!entries[1].success);
    clear(dir.path()).await.unwrap();
    assert!(recent(dir.path()).await.unwrap().is_empty());
}
