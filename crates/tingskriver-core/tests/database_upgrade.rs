//! Upgrades and downgrades of the database (#183): a database from a newer
//! Tingskriver is refused without being touched, and a database from an
//! older schema version keeps its rows when the newest migrations run.

use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::time::Duration;

use sqlx::migrate::Migrator;
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{Connection, SqliteConnection};
use tingskriver_core::database::init::{DbInitError, BACKUP_DIR};
use tingskriver_core::database::manager::DatabaseManager;
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
    // Created by the current code, so in WAL mode, and closed cleanly.
    let db = DatabaseManager::open_at(
        &dir.path().join(DATABASE_FILE),
        &dir.path().join("meeting_minutes.db"),
        SHORT_BUSY,
    )
    .await
    .unwrap();
    sqlx::query("INSERT INTO _sqlx_migrations (version, description, installed_on, success, checksum, execution_time) VALUES (99990101000000, 'future', datetime('now'), 1, x'00', 0)")
        .execute(db.pool()).await.unwrap();
    db.cleanup().await.unwrap();
    // SQLite removes `-wal` and `-shm` when the connection handle is dropped,
    // a moment after the pool reports closed.
    for _ in 0..100 {
        if !dir.path().join(format!("{DATABASE_FILE}-wal")).exists()
            && !dir.path().join(format!("{DATABASE_FILE}-shm")).exists()
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    // `-shm` is SQLite's volatile shared-memory index, rebuilt by any reader
    // of a WAL database; the database and its `-wal` must be untouched.
    let content = |dir: &Path| -> Vec<(PathBuf, Vec<u8>)> {
        tree(dir)
            .into_iter()
            .filter(|(p, _)| !p.to_string_lossy().ends_with("-shm"))
            .collect()
    };
    let before = content(dir.path());

    let error = DatabaseManager::open(dir.path())
        .await
        .err()
        .expect("refused");

    assert!(matches!(error, DbInitError::NewerDatabase(_)), "{error:?}");
    assert!(!dir.path().join(BACKUP_DIR).exists());
    assert!(content(dir.path()) == before, "database or -wal changed");
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
    let total = migration_count();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(DATABASE_FILE);
    // Everything except the last migration.
    old_database(&path, total - 1).await;
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
