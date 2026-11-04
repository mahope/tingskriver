use sqlx::SqlitePool;

#[tokio::test]
async fn sqlx_sqlite_supports_fts5() {
    let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
    sqlx::query(
        "CREATE VIRTUAL TABLE probe USING fts5(body, tokenize='unicode61 remove_diacritics 0')",
    )
    .execute(&pool)
    .await
    .unwrap();
}

use sqlx::{migrate::Migrator, sqlite::SqlitePoolOptions};
use tingskriver_core::database::repositories::{
    meeting::MeetingsRepository,
    search::{rebuild, search, SearchFilters, SearchSource},
};

async fn database(before_search: bool) -> SqlitePool {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    let mut migrator = Migrator::new(std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/migrations"
    )))
    .await
    .unwrap();
    if before_search {
        migrator.migrations = migrator
            .iter()
            .filter(|m| m.version < 20261101120000)
            .cloned()
            .collect();
    }
    migrator.run(&pool).await.unwrap();
    pool
}

async fn seed(pool: &SqlitePool, id: &str, word: &str) {
    sqlx::query("INSERT INTO meetings(id,title,created_at,updated_at) VALUES (?, ?, '2026-11-01T12:00:00Z', '2026-11-01T12:00:00Z')")
        .bind(id).bind(format!("{} titel", word.to_uppercase())).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO transcripts(id,meeting_id,transcript,timestamp,audio_start_time) VALUES (?, ?, ?, '2026-11-01T12:00:00Z', 123.5)")
        .bind(format!("t-{id}")).bind(id).bind(format!("{word} samtale")).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO meeting_notes(meeting_id,notes_markdown,created_at,updated_at) VALUES (?, ?, '', '')")
        .bind(id).bind(format!("{word} note")).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO summary_processes(meeting_id,status,created_at,updated_at,result) VALUES (?, 'completed', '', '', ?)")
        .bind(id).bind(serde_json::json!({"markdown": format!("{word} referat\nny linje")}).to_string()).execute(pool).await.unwrap();
}

#[tokio::test]
async fn danish_letters_prefix_snippets_and_safe_input() {
    let pool = database(false).await;
    for word in ["øl", "ol", "æble", "able", "å", "a"] {
        seed(&pool, word, word).await;
    }
    for (query, id) in [
        ("øl", "øl"),
        ("ØL", "øl"),
        ("ol", "ol"),
        ("OL", "ol"),
        ("æble", "æble"),
        ("ÆBLE", "æble"),
        ("å", "å"),
        ("Å", "å"),
        ("a", "a"),
        ("A", "a"),
        ("æbl*", "æble"),
    ] {
        let hits = search(&pool, query, &SearchFilters::default())
            .await
            .unwrap();
        assert_eq!(hits.len(), 4, "{query}: {hits:?}");
        assert!(hits.iter().all(|h| h.meeting_id == id
            && h.snippet.contains('\u{1e}')
            && h.snippet.contains('\u{1f}')));
        let transcript = hits
            .iter()
            .find(|h| h.source == SearchSource::Transcript)
            .unwrap();
        assert_eq!(transcript.time_seconds, Some(123.5));
        assert!(hits
            .iter()
            .filter(|h| h.source != SearchSource::Transcript)
            .all(|h| h.time_seconds.is_none()));
    }
    for query in [
        "",
        "  ",
        "\"",
        "*",
        "NEAR",
        "-",
        "øl\" OR *",
        "(øl)",
        "øl -ol",
        "\"øl\"*",
    ] {
        search(&pool, query, &SearchFilters::default())
            .await
            .unwrap();
    }
    assert!(search(&pool, "øl OR ol", &SearchFilters::default())
        .await
        .unwrap()
        .is_empty());
    seed(&pool, "literal", "NEAR").await;
    assert_eq!(
        search(&pool, "NEAR", &SearchFilters::default())
            .await
            .unwrap()
            .len(),
        4
    );
}

async fn source_snapshot(pool: &SqlitePool) -> Vec<Vec<String>> {
    let mut snapshot = Vec::new();
    for table in [
        "meetings",
        "transcripts",
        "meeting_notes",
        "summary_processes",
    ] {
        let columns: Vec<String> =
            sqlx::query_scalar(&format!("SELECT name FROM pragma_table_info('{table}')"))
                .fetch_all(pool)
                .await
                .unwrap();
        let fields = columns
            .iter()
            .map(|column| format!("'{column}', \"{column}\""))
            .collect::<Vec<_>>()
            .join(", ");
        snapshot.push(
            sqlx::query_scalar::<_, String>(&format!(
                "SELECT json_object({fields}) FROM {table} ORDER BY rowid"
            ))
            .fetch_all(pool)
            .await
            .unwrap(),
        );
    }
    snapshot
}

#[tokio::test]
async fn migration_backfills_without_changing_data_and_rebuild_is_idempotent() {
    let pool = database(true).await;
    seed(&pool, "old", "bevaring").await;
    seed(&pool, "older", "bevaring").await;
    let before = source_snapshot(&pool).await;
    let mut migrator = Migrator::new(std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/migrations"
    )))
    .await
    .unwrap();
    // This snapshot verifies the FTS migration, not later additive schema changes.
    migrator.migrations = migrator
        .iter()
        .filter(|m| m.version <= 20261101120000)
        .cloned()
        .collect();
    migrator.run(&pool).await.unwrap();
    for _ in 0..2 {
        let hits = search(&pool, "bevaring", &SearchFilters::default())
            .await
            .unwrap();
        assert_eq!(hits.len(), 8);
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM search_documents")
                .fetch_one(&pool)
                .await
                .unwrap(),
            8
        );
        sqlx::query("DELETE FROM search_documents WHERE source='note'")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(
            search(&pool, "bevaring", &SearchFilters::default())
                .await
                .unwrap()
                .len(),
            6
        );
        rebuild(&pool).await.unwrap();
        sqlx::query("INSERT INTO search_fts(search_fts, rank) VALUES ('integrity-check', 1)")
            .execute(&pool)
            .await
            .unwrap();
    }
    assert_eq!(before, source_snapshot(&pool).await);
}

#[tokio::test]
async fn updates_deletes_and_app_meeting_deletion_keep_index_current() {
    let pool = database(false).await;
    seed(&pool, "app", "gammelt").await;
    for statement in [
        "UPDATE meetings SET title='nyt' WHERE id='app'",
        "UPDATE transcripts SET transcript='nyt', audio_start_time=42 WHERE meeting_id='app'",
        "UPDATE meeting_notes SET notes_markdown='nyt' WHERE meeting_id='app'",
        "UPDATE summary_processes SET result='{\"markdown\":\"nyt\"}' WHERE meeting_id='app'",
    ] {
        sqlx::query(statement).execute(&pool).await.unwrap();
    }
    assert!(search(&pool, "gammelt", &SearchFilters::default())
        .await
        .unwrap()
        .is_empty());
    assert_eq!(
        search(&pool, "nyt", &SearchFilters::default())
            .await
            .unwrap()
            .len(),
        4
    );
    assert!(MeetingsRepository::delete_meeting(&pool, "app")
        .await
        .unwrap());
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM search_fts")
            .fetch_one(&pool)
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM search_fts WHERE search_fts MATCH 'nyt'"
        )
        .fetch_one(&pool)
        .await
        .unwrap(),
        0
    );
    sqlx::query("INSERT INTO search_fts(search_fts, rank) VALUES ('integrity-check', 1)")
        .execute(&pool)
        .await
        .unwrap();
    seed(&pool, "cascade", "kaskade").await;
    sqlx::query("DELETE FROM meetings WHERE id='cascade'")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM search_fts WHERE search_fts MATCH 'kaskade'"
        )
        .fetch_one(&pool)
        .await
        .unwrap(),
        0
    );
    sqlx::query("INSERT INTO search_fts(search_fts, rank) VALUES ('integrity-check', 1)")
        .execute(&pool)
        .await
        .unwrap();
    for table in [
        "transcripts",
        "meeting_notes",
        "summary_processes",
        "search_fts",
    ] {
        assert_eq!(
            sqlx::query_scalar::<_, i64>(&format!("SELECT count(*) FROM {table}"))
                .fetch_one(&pool)
                .await
                .unwrap(),
            0,
            "{table}"
        );
    }
}

#[tokio::test]
async fn filters_ranking_and_ties_are_deterministic() {
    let pool = database(false).await;
    seed(&pool, "b", "budget").await;
    seed(&pool, "a", "budget").await;
    let filters = SearchFilters {
        source: Some(SearchSource::Transcript),
        ..Default::default()
    };
    let included = SearchFilters {
        source: Some(SearchSource::Note),
        created_from: Some("2026-11-01T12:00:00Z".parse().unwrap()),
        created_before: Some("2026-11-02T00:00:00Z".parse().unwrap()),
        limit: 1,
    };
    let included_hits = search(&pool, "budget", &included).await.unwrap();
    assert_eq!(included_hits.len(), 1);
    assert_eq!(included_hits[0].source, SearchSource::Note);
    assert_eq!(included_hits[0].meeting_id, "a");
    let zero = SearchFilters {
        limit: 0,
        ..Default::default()
    };
    assert!(search(&pool, "budget", &zero).await.unwrap().is_empty());
    let hits = search(&pool, "budget", &filters).await.unwrap();
    assert_eq!(
        hits.iter()
            .map(|h| h.meeting_id.as_str())
            .collect::<Vec<_>>(),
        ["a", "b"]
    );
    sqlx::query("UPDATE transcripts SET transcript='budget budget budget' WHERE meeting_id='b'")
        .execute(&pool)
        .await
        .unwrap();
    let hits = search(&pool, "budget", &filters).await.unwrap();
    assert_eq!(hits[0].meeting_id, "b");
    assert!(hits[0].rank < hits[1].rank);
    let excluded = SearchFilters {
        created_from: Some("2026-11-02T00:00:00Z".parse().unwrap()),
        ..Default::default()
    };
    assert!(search(&pool, "budget", &excluded).await.unwrap().is_empty());
    let excluded = SearchFilters {
        created_before: Some("2026-11-01T12:00:00Z".parse().unwrap()),
        ..Default::default()
    };
    assert!(search(&pool, "budget", &excluded).await.unwrap().is_empty());
}

#[tokio::test]
async fn hundred_hours_search_latency() {
    use std::time::{Duration, Instant};
    let pool = database(false).await;
    let mut transaction = pool.begin().await.unwrap();
    let words = [
        "budget",
        "planlægning",
        "udvikling",
        "økonomi",
        "aftale",
        "årsplan",
        "æbler",
        "samarbejde",
    ];
    let mut state = 31_u64;
    for meeting in 0..100 {
        sqlx::query("INSERT INTO meetings(id,title,created_at,updated_at) VALUES (?, 'Møde', '2026-11-01', '2026-11-01')")
            .bind(format!("m{meeting:03}")).execute(&mut *transaction).await.unwrap();
        for segment in 0..800 {
            let mut text = String::new();
            for _ in 0..12 {
                state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
                text.push_str(words[((state >> 32) as usize) % words.len()]);
                text.push(' ');
            }
            sqlx::query("INSERT INTO transcripts(id,meeting_id,transcript,timestamp,audio_start_time,audio_end_time) VALUES (?, ?, ?, '', ?, ?)")
                .bind(format!("{meeting}-{segment}")).bind(format!("m{meeting:03}")).bind(text)
                .bind(segment as f64 * 4.5).bind((segment+1) as f64 * 4.5).execute(&mut *transaction).await.unwrap();
        }
    }
    transaction.commit().await.unwrap();
    for query in words {
        search(&pool, query, &SearchFilters::default())
            .await
            .unwrap();
    }
    let mut times = Vec::new();
    for _ in 0..3 {
        for query in [
            "budget",
            "økonomi",
            "årsplan",
            "planl*",
            "aftale samarbejde",
        ] {
            let start = Instant::now();
            assert!(!search(&pool, query, &SearchFilters::default())
                .await
                .unwrap()
                .is_empty());
            times.push(start.elapsed());
        }
    }
    times.sort();
    let median = times[times.len() / 2];
    eprintln!("80,000 segments / 100 hours, warmed search median: {median:?}");
    // Unoptimized SQLite and concurrent CI jobs make the 100 ms debug bound
    // unstable. Release retains the product target on the same dense corpus.
    let threshold_ms = if cfg!(debug_assertions) { 500 } else { 100 };
    assert!(
        median < Duration::from_millis(threshold_ms),
        "median {median:?}"
    );
}

#[tokio::test]
async fn json_formats_null_timing_and_vacuum_remain_searchable() {
    let pool = database(false).await;
    seed(&pool, "one", "første").await;
    seed(&pool, "two", "anden").await;
    sqlx::query("DELETE FROM meetings WHERE id='one'")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("VACUUM").execute(&pool).await.unwrap();
    sqlx::query("UPDATE meeting_notes SET notes_markdown='', notes_json='[{\"content\":[{\"text\":\"bloknotat\"}]}]' WHERE meeting_id='two'").execute(&pool).await.unwrap();
    sqlx::query("UPDATE summary_processes SET result='{\"Aftaler\":[{\"content\":\"ældreformat\"}]}' WHERE meeting_id='two'").execute(&pool).await.unwrap();
    sqlx::query("UPDATE transcripts SET transcript='udenlydtid', audio_start_time=NULL WHERE meeting_id='two'").execute(&pool).await.unwrap();
    for (query, source) in [
        ("bloknotat", SearchSource::Note),
        ("ældreformat", SearchSource::Summary),
        ("udenlydtid", SearchSource::Transcript),
    ] {
        let hits = search(&pool, query, &SearchFilters::default())
            .await
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].source, source);
        assert_eq!(hits[0].meeting_id, "two");
        assert_eq!(hits[0].time_seconds, None);
    }
    sqlx::query("UPDATE summary_processes SET result='gammelt råformat' WHERE meeting_id='two'")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        search(&pool, "råformat", &SearchFilters::default())
            .await
            .unwrap()
            .len(),
        1
    );
    for table in ["transcripts", "meeting_notes", "summary_processes"] {
        sqlx::query(&format!("DELETE FROM {table}"))
            .execute(&pool)
            .await
            .unwrap();
    }
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM search_documents")
            .fetch_one(&pool)
            .await
            .unwrap(),
        1
    );
    sqlx::query("INSERT INTO search_fts(search_fts, rank) VALUES ('integrity-check', 1)")
        .execute(&pool)
        .await
        .unwrap();
}
