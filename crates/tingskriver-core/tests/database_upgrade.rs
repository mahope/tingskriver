use tingskriver_core::briefing::{BriefCache, SqliteCache};
use tingskriver_core::database::repositories::meeting::MeetingsRepository;

#[tokio::test]
async fn upgrade_and_delete_remove_all_cache_derived_from_meeting() {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    let mut migrations = sqlx::migrate!("./migrations");
    let briefing_version = 20261105120000;
    let briefing = migrations
        .iter()
        .find(|m| m.version == briefing_version)
        .unwrap()
        .clone();
    migrations.migrations = migrations
        .iter()
        .filter(|m| m.version < briefing_version)
        .cloned()
        .collect::<Vec<_>>()
        .into();
    migrations.run(&pool).await.unwrap();
    for id in ["first", "second", "unrelated"] {
        sqlx::query(
            "INSERT INTO meetings (id, title, created_at, updated_at) VALUES (?, 'Test', '', '')",
        )
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
    }
    sqlx::raw_sql(&briefing.sql).execute(&pool).await.unwrap();
    let cache = SqliteCache(pool.clone());
    cache
        .put("points:first", "private first", &["first"])
        .await
        .unwrap();
    cache
        .put("points:second", "private second", &["second"])
        .await
        .unwrap();
    cache
        .put("overview", "private joint", &["first", "second"])
        .await
        .unwrap();
    cache
        .put("other", "unrelated", &["unrelated"])
        .await
        .unwrap();
    assert!(MeetingsRepository::delete_meeting(&pool, "second")
        .await
        .unwrap());
    assert_eq!(cache.get("overview").await.unwrap(), None);
    assert_eq!(cache.get("points:second").await.unwrap(), None);
    assert_eq!(
        cache.get("points:first").await.unwrap().as_deref(),
        Some("private first")
    );
    assert_eq!(
        cache.get("other").await.unwrap().as_deref(),
        Some("unrelated")
    );
    // Direct deletion also clears derived data with foreign keys disabled.
    sqlx::query("PRAGMA foreign_keys = OFF")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("PRAGMA foreign_keys")
            .fetch_one(&pool)
            .await
            .unwrap(),
        0
    );
    sqlx::query("DELETE FROM meetings WHERE id = 'first'")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(cache.get("points:first").await.unwrap(), None);
}
