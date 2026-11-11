//! Upgrades and downgrades of the database (#183): a database from a newer
//! Tingskriver is refused without being touched, and a database from an
//! older schema version keeps its rows when the newest migrations run.

use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::time::Duration;

use sqlx::migrate::Migrator;
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{Connection, SqliteConnection};
use tingskriver_core::briefing::{BriefCache, SqliteCache};
use tingskriver_core::database::init::{DbInitError, BACKUP_DIR};
use tingskriver_core::database::manager::DatabaseManager;
use tingskriver_core::database::repositories::meeting::MeetingsRepository;
use tingskriver_core::paths::DATABASE_FILE;

const SHORT_BUSY: Duration = Duration::from_millis(200);

/// The migrations of this build, as a migrator limited to the first `count`.
fn migrator_with(count: usize) -> Migrator {
    let mut migrator = sqlx::migrate!("./migrations");
    let all: Vec<_> = migrator.migrations.iter().cloned().collect();
    migrator.migrations = Cow::Owned(all.into_iter().take(count).collect());
    migrator
}

fn migration_count() -> usize {
    sqlx::migrate!("./migrations").migrations.len()
}

/// Build the database of an older version: only the first `count` migrations
/// applied, as that version would have left it. Returns it closed cleanly.
async fn old_database(path: &Path, count: usize) {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .foreign_keys(true);
    let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
    migrator_with(count).run(&mut connection).await.unwrap();
    connection.close().await.unwrap();
}

fn tree(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .map(|p| {
            let bytes = std::fs::read(&p).unwrap_or_default();
            (p, bytes)
        })
        .collect();
    files.sort();
    files
}

#[tokio::test]
async fn database_with_a_newer_migration_is_refused_and_left_byte_identical() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(DATABASE_FILE);
    old_database(&path, migration_count()).await;
    // What a newer Tingskriver leaves: one more applied migration, and data.
    let options = SqliteConnectOptions::new().filename(&path);
    let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
    sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES ('m1', 'Budgetmøde', datetime('now'), datetime('now'))")
        .execute(&mut connection).await.unwrap();
    sqlx::query("INSERT INTO _sqlx_migrations (version, description, installed_on, success, checksum, execution_time) VALUES (99990101000000, 'future', datetime('now'), 1, x'00', 0)")
        .execute(&mut connection).await.unwrap();
    connection.close().await.unwrap();
    let before = tree(dir.path());

    let error = DatabaseManager::open(dir.path())
        .await
        .err()
        .expect("a newer database must not open");

    assert!(matches!(error, DbInitError::NewerDatabase(_)), "{error:?}");
    assert_eq!(
        error.summary_da(),
        "Databasen er oprettet af en nyere Tingskriver. Opdatér Tingskriver."
    );
    assert!(
        error.detail().contains("99990101000000"),
        "{}",
        error.detail()
    );
    assert!(!error.wants_backup());
    assert_eq!(tree(dir.path()), before, "files must be byte-identical");
    assert!(!dir.path().join(BACKUP_DIR).exists());
}

#[tokio::test]
async fn newer_database_in_wal_mode_is_refused_without_changing_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(DATABASE_FILE);
    old_database(&path, migration_count()).await;
    let options = SqliteConnectOptions::new()
        .filename(&path)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal);
    // Keep the writer open so its committed migration remains in the WAL.
    let mut writer = SqliteConnection::connect_with(&options).await.unwrap();
    sqlx::query("PRAGMA wal_autocheckpoint = 0")
        .execute(&mut writer)
        .await
        .unwrap();
    sqlx::query("INSERT INTO _sqlx_migrations (version, description, installed_on, success, checksum, execution_time) VALUES (99990101000000, 'future', datetime('now'), 1, x'00', 0)")
        .execute(&mut writer).await.unwrap();
    let wal = dir.path().join(format!("{DATABASE_FILE}-wal"));
    assert!(std::fs::metadata(&wal).unwrap().len() > 32);
    // Read the bytes in-process: SQLite opens its files with shared read
    // access, so this works on Windows while the writer is still open.
    let bytes = |path: &Path| std::fs::read(path).unwrap();
    let before = (bytes(&path), bytes(&wal));
    let backup_dir = dir.path().join(BACKUP_DIR);
    std::fs::create_dir(&backup_dir).unwrap();
    std::fs::write(backup_dir.join("existing-backup"), b"keep").unwrap();
    let backups_before = tree(&backup_dir);

    for _ in 0..2 {
        let error = DatabaseManager::open(dir.path())
            .await
            .err()
            .expect("refused");
        assert!(matches!(error, DbInitError::NewerDatabase(_)), "{error:?}");
        assert!(error.backup_path().is_none());
        assert_eq!((bytes(&path), bytes(&wal)), before);
        assert_eq!(tree(&backup_dir), backups_before);
    }
    writer.close().await.unwrap();
}

#[tokio::test]
async fn every_older_schema_version_upgrades_without_losing_rows() {
    let total = migration_count();
    for old in 1..total {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(DATABASE_FILE);
        old_database(&path, old).await;
        let options = SqliteConnectOptions::new().filename(&path);
        let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
        // The initial schema is in every version.
        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES ('m1', 'Ugemøde', datetime('now'), datetime('now'))")
            .execute(&mut connection).await.unwrap();
        sqlx::query("INSERT INTO transcripts (id, meeting_id, transcript, timestamp) VALUES ('t1', 'm1', 'Vi aftalte nye åbningstider', datetime('now'))")
            .execute(&mut connection).await.unwrap();
        connection.close().await.unwrap();

        let db =
            DatabaseManager::open_at(&path, &dir.path().join("meeting_minutes.db"), SHORT_BUSY)
                .await
                .unwrap_or_else(|e| panic!("upgrade from {old} of {total} migrations: {e}"));

        let title: String = sqlx::query_scalar("SELECT title FROM meetings WHERE id = 'm1'")
            .fetch_one(db.pool())
            .await
            .unwrap();
        let text: String = sqlx::query_scalar("SELECT transcript FROM transcripts WHERE id = 't1'")
            .fetch_one(db.pool())
            .await
            .unwrap();
        assert_eq!(
            (title.as_str(), text.as_str()),
            ("Ugemøde", "Vi aftalte nye åbningstider"),
            "from {old}"
        );
        let applied: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM _sqlx_migrations WHERE success = 1")
                .fetch_one(db.pool())
                .await
                .unwrap();
        assert_eq!(applied as usize, total, "from {old}");
    }
}

/// The newest migrations (search index, note blocks) on a database with
/// realistic rows from just before them.
#[tokio::test]
async fn seeded_database_before_search_and_note_blocks_keeps_everything() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(DATABASE_FILE);
    // Everything before the search index (20261101120000), so the search
    // index and the note blocks are both applied by the upgrade. Newer
    // migrations (the briefing cache) come after them.
    let before_search = sqlx::migrate!("./migrations")
        .migrations
        .iter()
        .filter(|m| m.version < 20261101120000)
        .count();
    old_database(&path, before_search).await;
    let options = SqliteConnectOptions::new()
        .filename(&path)
        .foreign_keys(true);
    let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
    for (id, title) in [("a", "Projektstart"), ("b", "Kvartalsstatus")] {
        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES (?, ?, datetime('now'), datetime('now'))")
            .bind(id).bind(title).execute(&mut connection).await.unwrap();
        for n in 0..3 {
            sqlx::query("INSERT INTO transcripts (id, meeting_id, transcript, timestamp, audio_start_time) VALUES (?, ?, ?, datetime('now'), ?)")
                .bind(format!("{id}{n}")).bind(id)
                .bind(format!("Linje {n} om budgettet"))
                .bind(n as f64 * 4.0)
                .execute(&mut connection).await.unwrap();
        }
    }
    sqlx::query("INSERT INTO meeting_notes (meeting_id, notes_markdown, notes_json, created_at, updated_at) VALUES ('a', 'Husk **budget**', '{}', datetime('now'), datetime('now'))")
        .execute(&mut connection).await.unwrap();
    connection.close().await.unwrap();

    let db = DatabaseManager::open_at(&path, &dir.path().join("meeting_minutes.db"), SHORT_BUSY)
        .await
        .unwrap();

    let meetings: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM meetings")
        .fetch_one(db.pool())
        .await
        .unwrap();
    let transcripts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM transcripts")
        .fetch_one(db.pool())
        .await
        .unwrap();
    let notes: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM meeting_notes")
        .fetch_one(db.pool())
        .await
        .unwrap();
    assert_eq!((meetings, transcripts, notes), (2, 6, 1));
    // The new tables exist and the new migration is recorded.
    sqlx::query("SELECT COUNT(*) FROM meeting_note_blocks")
        .fetch_one(db.pool())
        .await
        .unwrap();
    let last: i64 = sqlx::query_scalar("SELECT MAX(version) FROM _sqlx_migrations")
        .fetch_one(db.pool())
        .await
        .unwrap();
    assert_eq!(
        last,
        sqlx::migrate!("./migrations")
            .migrations
            .last()
            .unwrap()
            .version
    );
}

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
