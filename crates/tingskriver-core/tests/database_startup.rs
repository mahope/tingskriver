//! Opening the database at startup: errors instead of crashes (#5) and
//! explicit SQLite settings, no deleted WAL files, the transcript index and
//! keyset pages (#38).

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use sqlx::{Connection, Row, SqliteConnection};
use tingskriver_core::database::init::{database_files, DbInitError, BACKUP_DIR};
use tingskriver_core::database::manager::DatabaseManager;
use tingskriver_core::database::repositories::meeting::{MeetingsRepository, TRANSCRIPT_PAGE_SQL};
use tingskriver_core::paths::DATABASE_FILE;

const SHORT_BUSY: Duration = Duration::from_millis(200);
const LOCK_HELPER_ENV: &str = "TINGSKRIVER_TEST_LOCK_DB";

/// The log file shared by all tests in this binary.
fn log_file() -> &'static Path {
    static LOG: OnceLock<PathBuf> = OnceLock::new();
    LOG.get_or_init(|| {
        let dir =
            std::env::temp_dir().join(format!("tingskriver-db-test-log-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        tingskriver_core::logging::init_in(Some(&dir), "test.log").expect("log file")
    })
}

fn legacy(dir: &Path) -> PathBuf {
    dir.join("meeting_minutes.db")
}

async fn create_database(dir: &Path) -> DatabaseManager {
    DatabaseManager::open_at(&dir.join(DATABASE_FILE), &legacy(dir), SHORT_BUSY)
        .await
        .expect("new database opens")
}

async fn insert_meeting(pool: &sqlx::SqlitePool, id: &str) {
    sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES (?, ?, datetime('now'), datetime('now'))")
        .bind(id)
        .bind(format!("Møde {id}"))
        .execute(pool)
        .await
        .unwrap();
}

fn backups(dir: &Path) -> Vec<PathBuf> {
    match std::fs::read_dir(dir.join(BACKUP_DIR)) {
        Ok(entries) => entries.map(|e| e.unwrap().path()).collect(),
        Err(_) => Vec::new(),
    }
}

#[tokio::test]
async fn corrupt_database_gives_corrupt_error_backup_and_log_line() {
    let log = log_file();
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join(DATABASE_FILE);
    std::fs::write(
        &db,
        b"this is not a sqlite database, just bytes".repeat(200),
    )
    .unwrap();

    let error = match DatabaseManager::open(dir.path()).await {
        Err(error) => error,
        Ok(_) => panic!("a corrupt file must not open"),
    };
    assert!(matches!(error, DbInitError::Corrupt(_)), "{error:?}");
    assert_eq!(error.database_path(), db);
    assert_eq!(error.log_path(), Some(log));

    let backup = error.backup_path().expect("backup taken").to_path_buf();
    assert_eq!(backups(dir.path()), vec![backup.clone()]);
    assert_eq!(
        std::fs::read(backup.join(DATABASE_FILE)).unwrap(),
        std::fs::read(&db).unwrap()
    );

    log::logger().flush();
    let logged = std::fs::read_to_string(log).unwrap();
    assert!(
        logged.contains("Database failed to open (Corrupt)")
            && logged.contains(&db.display().to_string()),
        "log:\n{logged}"
    );
}

#[tokio::test]
async fn damaged_pages_in_a_real_database_are_corrupt() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join(DATABASE_FILE);
    let db = create_database(dir.path()).await;
    db.cleanup().await.unwrap();
    // Keep the header, overwrite the schema page and everything after it.
    let mut bytes = std::fs::read(&db_path).unwrap();
    for b in bytes.iter_mut().skip(100) {
        *b = 0xA5;
    }
    std::fs::write(&db_path, bytes).unwrap();

    let error = DatabaseManager::open_at(&db_path, &legacy(dir.path()), SHORT_BUSY)
        .await
        .err()
        .expect("damaged file must not open");
    assert!(matches!(error, DbInitError::Corrupt(_)), "{error:?}");
}

#[tokio::test]
async fn unknown_migration_is_a_migration_error_with_backup() {
    let dir = tempfile::tempdir().unwrap();
    let db = create_database(dir.path()).await;
    // A database from a newer Tingskriver.
    sqlx::query("INSERT INTO _sqlx_migrations (version, description, installed_on, success, checksum, execution_time) VALUES (99990101000000, 'future', datetime('now'), 1, x'00', 0)")
        .execute(db.pool())
        .await
        .unwrap();
    db.cleanup().await.unwrap();

    let error = DatabaseManager::open(dir.path())
        .await
        .err()
        .expect("must fail");
    assert!(matches!(error, DbInitError::Migration(_)), "{error:?}");
    assert!(error.backup_path().is_some());
}

/// Hold an exclusive lock on a database that is not in WAL mode yet (a
/// database from before this version, still open in the old app).
async fn lock_exclusively(path: &Path) -> SqliteConnection {
    let mut conn = SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Delete),
    )
    .await
    .unwrap();
    sqlx::query("CREATE TABLE IF NOT EXISTS old_app (x)")
        .execute(&mut conn)
        .await
        .unwrap();
    sqlx::query("BEGIN EXCLUSIVE")
        .execute(&mut conn)
        .await
        .unwrap();
    sqlx::query("INSERT INTO old_app VALUES (1)")
        .execute(&mut conn)
        .await
        .unwrap();
    conn
}

#[tokio::test]
async fn locked_database_gives_locked_error() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join(DATABASE_FILE);
    let _holder = lock_exclusively(&db_path).await;

    let error = DatabaseManager::open_at(&db_path, &legacy(dir.path()), SHORT_BUSY)
        .await
        .err()
        .expect("locked database must not open");
    assert!(matches!(error, DbInitError::Locked(_)), "{error:?}");
    assert!(!error.wants_backup());
}

/// Not a test by itself: the other process for
/// `database_locked_by_another_process_gives_locked_error`.
#[tokio::test]
#[ignore]
async fn lock_helper_process() {
    let Ok(path) = std::env::var(LOCK_HELPER_ENV) else {
        return;
    };
    let _conn = lock_exclusively(Path::new(&path)).await;
    println!("LOCKED");
    std::io::stdout().flush().unwrap();
    // Hold the lock until the parent closes stdin.
    let mut line = String::new();
    let _ = std::io::stdin().read_line(&mut line);
}

#[tokio::test]
async fn database_locked_by_another_process_gives_locked_error() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join(DATABASE_FILE);
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "lock_helper_process",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(LOCK_HELPER_ENV, &db_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    loop {
        line.clear();
        assert!(
            stdout.read_line(&mut line).unwrap() > 0,
            "helper exited before locking"
        );
        if line.trim_end().ends_with("LOCKED") {
            break;
        }
    }

    let result = DatabaseManager::open_at(&db_path, &legacy(dir.path()), SHORT_BUSY).await;
    drop(child.stdin.take());
    child.wait().unwrap();

    let error = result.err().expect("locked database must not open");
    assert!(matches!(error, DbInitError::Locked(_)), "{error:?}");
}

#[tokio::test]
async fn connections_use_wal_normal_sync_busy_timeout_and_foreign_keys() {
    let dir = tempfile::tempdir().unwrap();
    let db = DatabaseManager::open(dir.path()).await.unwrap();
    let mut conn = db.pool().acquire().await.unwrap();
    let journal: String = sqlx::query_scalar("PRAGMA journal_mode")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    let sync: i64 = sqlx::query_scalar("PRAGMA synchronous")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    let busy: i64 = sqlx::query_scalar("PRAGMA busy_timeout")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    let fk: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(journal, "wal");
    assert_eq!(sync, 1, "NORMAL");
    assert_eq!(busy, 5000);
    assert_eq!(fk, 1);
}

/// Copy the database files as they are on disk right now: what is left
/// after the process dies in the middle of a recording.
fn copy_crash_state(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for file in database_files(&from.join(DATABASE_FILE)) {
        if file.exists() {
            std::fs::copy(&file, to.join(file.file_name().unwrap())).unwrap();
        }
    }
}

#[tokio::test]
async fn committed_rows_in_the_wal_survive_a_crash() {
    let live = tempfile::tempdir().unwrap();
    let db = create_database(live.path()).await;
    insert_meeting(db.pool(), "efter-checkpoint").await;
    let wal = live.path().join(format!("{DATABASE_FILE}-wal"));
    assert!(
        wal.exists() && std::fs::metadata(&wal).unwrap().len() > 0,
        "row is in the WAL"
    );

    let crashed = tempfile::tempdir().unwrap();
    copy_crash_state(live.path(), crashed.path());
    let reopened = DatabaseManager::open(crashed.path()).await.unwrap();
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM meetings WHERE id = 'efter-checkpoint'")
            .fetch_one(reopened.pool())
            .await
            .unwrap();
    assert_eq!(count, 1, "the committed row from the WAL is there");
}

#[tokio::test]
async fn failed_open_never_deletes_wal_or_shm_and_backs_them_up() {
    let live = tempfile::tempdir().unwrap();
    let db = create_database(live.path()).await;
    // Move the schema into the main file, so the WAL holds only the new row
    // and not page 1 (SQLite would read the header from a WAL copy of it).
    sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
        .execute(db.pool())
        .await
        .unwrap();
    insert_meeting(db.pool(), "kun-i-wal").await;

    let crashed = tempfile::tempdir().unwrap();
    copy_crash_state(live.path(), crashed.path());
    let [main, wal, shm, _] = database_files(&crashed.path().join(DATABASE_FILE));
    let wal_bytes = std::fs::read(&wal).unwrap();
    assert!(shm.exists());
    // Destroy the header of the main file.
    let mut bytes = std::fs::read(&main).unwrap();
    bytes[..16].copy_from_slice(b"definitely-not!!");
    std::fs::write(&main, &bytes).unwrap();

    let error = DatabaseManager::open(crashed.path())
        .await
        .err()
        .expect("must fail");
    assert!(matches!(error, DbInitError::Corrupt(_)), "{error:?}");

    // The backup holds the files exactly as the crash left them, taken
    // before SQLite opened them.
    let backup = error.backup_path().unwrap();
    assert_eq!(std::fs::read(backup.join(DATABASE_FILE)).unwrap(), bytes);
    assert_eq!(
        std::fs::read(backup.join(format!("{DATABASE_FILE}-wal"))).unwrap(),
        wal_bytes
    );
    // Our code leaves the WAL alone. On Windows SQLite itself checkpoints
    // the WAL into the main file when the failed connection closes and then
    // removes it, so the frames move rather than disappear.
    #[cfg(not(windows))]
    {
        assert_eq!(std::fs::read(&wal).unwrap(), wal_bytes, "WAL untouched");
        assert!(shm.exists(), "SHM untouched");
    }
}

#[tokio::test]
async fn safety_copy_of_an_unclean_database_is_removed_when_it_opens() {
    let live = tempfile::tempdir().unwrap();
    let db = create_database(live.path()).await;
    insert_meeting(db.pool(), "m").await;
    let crashed = tempfile::tempdir().unwrap();
    copy_crash_state(live.path(), crashed.path());

    DatabaseManager::open(crashed.path()).await.unwrap();
    assert!(
        backups(crashed.path()).is_empty(),
        "{:?}",
        backups(crashed.path())
    );
}

#[test]
fn no_code_deletes_wal_or_shm_files() {
    // Every remove_file in the database module, for review: none of them may
    // be about -wal or -shm.
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/database");
    let mut stack = vec![src];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            assert!(
                !text.contains("wal_path") && !text.contains("shm_path"),
                "{} handles WAL/SHM paths",
                path.display()
            );
            for (n, line) in text.lines().enumerate() {
                let deletes = line.contains("remove_file") || line.contains("remove_dir");
                assert!(
                    !(deletes && (line.contains("wal") || line.contains("shm"))),
                    "{}:{} deletes a WAL/SHM file: {line}",
                    path.display(),
                    n + 1
                );
            }
        }
    }
}

#[tokio::test]
async fn writing_during_a_recording_does_not_block_the_meeting_list() {
    let dir = tempfile::tempdir().unwrap();
    let db = create_database(dir.path()).await;
    insert_meeting(db.pool(), "m1").await;

    // Reader: holds a read transaction on the meeting list.
    let mut reader = db.pool().acquire().await.unwrap();
    sqlx::query("BEGIN").execute(&mut *reader).await.unwrap();
    let before = sqlx::query("SELECT id FROM meetings")
        .fetch_all(&mut *reader)
        .await
        .unwrap();
    assert_eq!(before.len(), 1);

    // Writer: the recording saves transcripts and commits while the reader
    // is still inside its transaction (rollback journal: "database is locked").
    let mut writer = db.pool().acquire().await.unwrap();
    let started = Instant::now();
    sqlx::query("BEGIN IMMEDIATE")
        .execute(&mut *writer)
        .await
        .unwrap();
    sqlx::query("INSERT INTO transcripts (id, meeting_id, transcript, timestamp, audio_start_time) VALUES ('t1', 'm1', 'hej', '00:00', 0.0)")
        .execute(&mut *writer)
        .await
        .unwrap();

    // While the write transaction is open, a new reader still gets the list.
    let list = tokio::time::timeout(
        Duration::from_millis(500),
        MeetingsRepository::get_meetings(db.pool()),
    )
    .await
    .expect("meeting list not blocked by an open write")
    .unwrap();
    assert_eq!(list.len(), 1);

    sqlx::query("COMMIT").execute(&mut *writer).await.unwrap();
    assert!(
        started.elapsed() < Duration::from_millis(500),
        "commit waited for the reader"
    );
    let again = sqlx::query("SELECT id FROM meetings")
        .fetch_all(&mut *reader)
        .await
        .unwrap();
    assert_eq!(again.len(), 1);
    sqlx::query("COMMIT").execute(&mut *reader).await.unwrap();
}

async fn query_plan(pool: &sqlx::SqlitePool, sql: &str, binds: usize) -> String {
    let explain = format!("EXPLAIN QUERY PLAN {sql}");
    let mut q = sqlx::query(&explain);
    for _ in 0..binds {
        q = q.bind(1_i64);
    }
    q.fetch_all(pool)
        .await
        .unwrap()
        .iter()
        .map(|row| row.get::<String, _>("detail"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[tokio::test]
async fn transcript_queries_use_the_meeting_start_index() {
    let dir = tempfile::tempdir().unwrap();
    let db = create_database(dir.path()).await;
    let page = query_plan(db.pool(), TRANSCRIPT_PAGE_SQL, 4).await;
    assert!(
        page.contains("USING INDEX idx_transcripts_meeting_start"),
        "{page}"
    );
    assert!(!page.contains("TEMP B-TREE"), "no sort: {page}");
    let offset = query_plan(
        db.pool(),
        "SELECT * FROM transcripts WHERE meeting_id = ? ORDER BY audio_start_time ASC, rowid ASC LIMIT ? OFFSET ?",
        3,
    )
    .await;
    assert!(
        offset.contains("USING INDEX idx_transcripts_meeting_start"),
        "{offset}"
    );
    let count = query_plan(
        db.pool(),
        "SELECT COUNT(*) FROM transcripts WHERE meeting_id = ?",
        1,
    )
    .await;
    assert!(count.contains("idx_transcripts_meeting_start"), "{count}");
}

#[tokio::test]
async fn keyset_pages_return_every_segment_once_in_order() {
    let dir = tempfile::tempdir().unwrap();
    let db = create_database(dir.path()).await;
    insert_meeting(db.pool(), "m").await;
    // Old segments without times, ties on the start time, and a gap.
    let starts: [Option<f64>; 9] = [
        None,
        Some(3.0),
        None,
        Some(1.0),
        Some(1.0),
        Some(2.5),
        Some(1.0),
        Some(9.0),
        None,
    ];
    for (i, start) in starts.iter().enumerate() {
        sqlx::query("INSERT INTO transcripts (id, meeting_id, transcript, timestamp, audio_start_time) VALUES (?, 'm', ?, '00:00', ?)")
            .bind(format!("t{i}"))
            .bind(format!("segment {i}"))
            .bind(start)
            .execute(db.pool())
            .await
            .unwrap();
    }

    let mut cursor = None;
    let mut ids = Vec::new();
    loop {
        let (page, next) =
            MeetingsRepository::get_meeting_transcripts_page(db.pool(), "m", cursor.as_ref(), 2)
                .await
                .unwrap();
        ids.extend(page.into_iter().map(|t| t.id));
        match next {
            Some(next) => cursor = Some(next),
            None => break,
        }
    }
    assert_eq!(ids, ["t0", "t2", "t8", "t3", "t4", "t6", "t5", "t1", "t7"]);
}

#[tokio::test]
async fn three_hour_meeting_loads_in_under_200_ms_with_50_meetings() {
    let dir = tempfile::tempdir().unwrap();
    let db = create_database(dir.path()).await;
    // 50 meetings of 3 hours with a segment every 4 seconds: 2,700 segments
    // each, 135,000 in all.
    const SEGMENTS: i64 = 3 * 60 * 60 / 4;
    let started = Instant::now();
    sqlx::query(
        "WITH RECURSIVE m(n) AS (SELECT 0 UNION ALL SELECT n + 1 FROM m WHERE n < 49)
         INSERT INTO meetings (id, title, created_at, updated_at)
         SELECT 'meeting-' || n, 'Møde ' || n, datetime('now'), datetime('now') FROM m",
    )
    .execute(db.pool())
    .await
    .unwrap();
    sqlx::query(
        "WITH RECURSIVE s(n) AS (SELECT 0 UNION ALL SELECT n + 1 FROM s WHERE n < 50 * ?1 - 1)
         INSERT INTO transcripts (id, meeting_id, transcript, timestamp, audio_start_time, audio_end_time, duration, speaker)
         SELECT 'seg-' || n, 'meeting-' || (n % 50),
                'Det her er en sætning i et langt møde, nummer ' || n || ', med lidt mere tekst for at ligne virkeligheden.',
                '00:00:00', (n / 50) * 4.0, (n / 50) * 4.0 + 3.5, 3.5, 'mic'
         FROM s",
    )
    .bind(SEGMENTS)
    .execute(db.pool())
    .await
    .unwrap();
    println!(
        "Filled 50 meetings x {SEGMENTS} segments in {} ms",
        started.elapsed().as_millis()
    );

    let pool = db.pool();
    // Warm up one connection, as the app has after startup.
    MeetingsRepository::get_meeting_transcripts_page(pool, "meeting-7", None, 1)
        .await
        .unwrap();

    let started = Instant::now();
    let mut cursor = None;
    let mut loaded = 0;
    loop {
        let (page, next) = MeetingsRepository::get_meeting_transcripts_page(
            pool,
            "meeting-7",
            cursor.as_ref(),
            500,
        )
        .await
        .unwrap();
        loaded += page.len();
        match next {
            Some(next) => cursor = Some(next),
            None => break,
        }
    }
    let keyset = started.elapsed();

    let started = Instant::now();
    let (all, total) =
        MeetingsRepository::get_meeting_transcripts_paginated(pool, "meeting-7", SEGMENTS, 0)
            .await
            .unwrap();
    let single = started.elapsed();

    println!(
        "3-hour meeting ({loaded} segments): keyset pages of 500 in {} ms, one page in {} ms",
        keyset.as_millis(),
        single.as_millis()
    );
    assert_eq!(loaded as i64, SEGMENTS);
    assert_eq!(all.len() as i64, SEGMENTS);
    assert_eq!(total, SEGMENTS);
    assert!(
        keyset < Duration::from_millis(200),
        "keyset load took {keyset:?}"
    );
    assert!(
        single < Duration::from_millis(200),
        "single page took {single:?}"
    );
}
