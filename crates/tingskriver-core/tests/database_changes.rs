//! Change hints from SQLite hooks (#208): explicit rollbacks are discarded,
//! bursts are merged, and subscribers wait before reloading across connections.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tingskriver_core::database::changes::{keep_fresh, Change, ChangeNotifier};
use tingskriver_core::database::manager::DatabaseManager;
use tingskriver_core::paths::DATABASE_FILE;

const QUIET: Duration = Duration::from_millis(60);
const MAX_WAIT: Duration = Duration::from_secs(2);

async fn open(dir: &Path) -> DatabaseManager {
    DatabaseManager::open_at(
        &dir.join(DATABASE_FILE),
        &dir.join("meeting_minutes.db"),
        Duration::from_millis(500),
    )
    .await
    .expect("database opens")
}

async fn insert_meeting(pool: &sqlx::SqlitePool, id: &str) {
    sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES (?, ?, datetime('now'), datetime('now'))")
        .bind(id)
        .bind(format!("Møde {id}"))
        .execute(pool)
        .await
        .unwrap();
}

fn tables(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|n| n.to_string()).collect()
}

#[tokio::test]
async fn commit_reports_the_changed_table() {
    let dir = tempfile::tempdir().unwrap();
    let db = open(dir.path()).await;
    let mut rx = db.changes().subscribe();

    insert_meeting(db.pool(), "a").await;

    let change = tokio::time::timeout(Duration::from_secs(2), rx.next())
        .await
        .expect("a commit is announced")
        .unwrap();
    assert!(change.touches(&["meetings"]), "{change:?}");
    assert!(!change.touches(&["settings"]));
}

#[tokio::test]
async fn rollback_reports_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let db = open(dir.path()).await;
    let mut rx = db.changes().subscribe();

    let mut tx = db.pool().begin().await.unwrap();
    sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES ('r', 'x', datetime('now'), datetime('now'))")
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.rollback().await.unwrap();

    assert!(
        tokio::time::timeout(Duration::from_millis(300), rx.next())
            .await
            .is_err(),
        "a rollback must not be announced"
    );
}

#[tokio::test]
async fn rolled_back_tables_do_not_leak_into_the_next_commit() {
    let dir = tempfile::tempdir().unwrap();
    let db = open(dir.path()).await;
    // One connection, so the same one serves both transactions.
    let mut conn = db.pool().acquire().await.unwrap();
    let mut rx = db.changes().subscribe();

    let mut tx = sqlx::Connection::begin(&mut *conn).await.unwrap();
    sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES ('r', 'x', datetime('now'), datetime('now'))")
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.rollback().await.unwrap();

    sqlx::query("INSERT INTO settings (id, provider, model, whisperModel) VALUES ('2', 'p', 'm', 'w') ON CONFLICT(id) DO UPDATE SET model = 'm2'")
        .execute(&mut *conn)
        .await
        .unwrap();

    let change = tokio::time::timeout(Duration::from_secs(2), rx.next())
        .await
        .unwrap()
        .unwrap();
    assert!(change.touches(&["settings"]), "{change:?}");
    assert!(
        !change.touches(&["meetings"]),
        "rolled-back table leaked: {change:?}"
    );
}

#[tokio::test]
async fn many_quick_writes_give_one_batch() {
    let dir = tempfile::tempdir().unwrap();
    let db = open(dir.path()).await;
    let mut rx = db.changes().subscribe();

    for i in 0..100 {
        insert_meeting(db.pool(), &format!("m{i}")).await;
    }

    let batch = rx.next_batch(QUIET, MAX_WAIT).await.unwrap();
    assert!(batch.touches(&["meetings"]));
    // Everything was merged: nothing is left to deliver.
    assert!(
        tokio::time::timeout(Duration::from_millis(200), rx.next())
            .await
            .is_err(),
        "a burst of 100 commits produced more than one batch"
    );
}

#[tokio::test]
async fn keep_fresh_reloads_once_per_burst_and_only_for_its_tables() {
    let notifier = ChangeNotifier::new();
    let reloads = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&reloads);
    let task = tokio::spawn(keep_fresh(
        notifier.subscribe(),
        &["meetings"],
        QUIET,
        MAX_WAIT,
        move || {
            let counter = Arc::clone(&counter);
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
            }
        },
    ));

    // Fake source: 5 000 commits as fast as possible, far beyond the
    // channel's capacity, so subscribers also lag.
    let started = Instant::now();
    for _ in 0..5_000 {
        notifier.publish(tables(&["meetings"]));
    }
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "publishing must not wait for the subscriber"
    );
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(
        reloads.load(Ordering::SeqCst),
        1,
        "one reload for the burst"
    );

    // A change in an unrelated table reloads nothing.
    notifier.publish(tables(&["settings"]));
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(reloads.load(Ordering::SeqCst), 1);

    // The next burst reloads once more.
    notifier.publish(tables(&["meetings", "transcripts"]));
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(reloads.load(Ordering::SeqCst), 2);

    drop(notifier);
    tokio::time::timeout(Duration::from_secs(1), task)
        .await
        .expect("keep_fresh ends when the database is gone")
        .unwrap();
}

#[tokio::test]
async fn a_slow_subscriber_is_told_to_reload_everything() {
    let notifier = ChangeNotifier::new();
    let mut rx = notifier.subscribe();
    for _ in 0..1_000 {
        notifier.publish(tables(&["meetings"]));
    }
    assert_eq!(rx.next().await, Some(Change::Everything));
}

#[tokio::test]
async fn a_continuous_stream_is_cut_at_max_wait() {
    let notifier = ChangeNotifier::new();
    let mut rx = notifier.subscribe();
    let feeder = notifier.clone();
    let task = tokio::spawn(async move {
        loop {
            feeder.publish(tables(&["meetings"]));
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    });
    let started = Instant::now();
    let batch = rx
        .next_batch(Duration::from_millis(100), Duration::from_millis(300))
        .await;
    task.abort();
    assert!(batch.is_some());
    assert!(
        started.elapsed() < Duration::from_millis(800),
        "list would starve"
    );
}

#[tokio::test]
async fn a_late_subscriber_receives_new_writes_after_the_channel_overflows() {
    let dir = tempfile::tempdir().unwrap();
    let db = open(dir.path()).await;
    // Subscribes and never reads: the channel fills and starts dropping.
    let _stuck = db.changes().subscribe();

    for i in 0..600 {
        insert_meeting(db.pool(), &format!("m{i}")).await;
    }
    let mut late = db.changes().subscribe();
    insert_meeting(db.pool(), "last").await;
    assert!(late.next().await.unwrap().touches(&["meetings"]));
}

#[tokio::test]
#[should_panic(expected = "quiet must be greater than zero")]
async fn next_batch_rejects_zero_quiet() {
    let notifier = ChangeNotifier::new();
    let mut rx = notifier.subscribe();
    notifier.publish(tables(&["meetings"]));
    rx.next_batch(Duration::ZERO, MAX_WAIT).await;
}

#[tokio::test]
#[should_panic(expected = "max_wait must be greater than zero")]
async fn next_batch_rejects_zero_max_wait() {
    let notifier = ChangeNotifier::new();
    let mut rx = notifier.subscribe();
    notifier.publish(tables(&["meetings"]));
    rx.next_batch(QUIET, Duration::ZERO).await;
}

#[tokio::test]
async fn an_immediate_reload_after_a_batch_sees_the_new_row() {
    let dir = tempfile::tempdir().unwrap();
    let db = open(dir.path()).await;
    let mut rx = db.changes().subscribe();
    // Hold a different connection before the writer starts.
    let mut reader = db.pool().acquire().await.unwrap();
    let writer = insert_meeting(db.pool(), "visible");
    let subscriber = async {
        let change = rx.next_batch(QUIET, MAX_WAIT).await.unwrap();
        assert!(change.touches(&["meetings"]));
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM meetings WHERE id = 'visible'")
            .fetch_one(&mut *reader)
            .await
            .unwrap();
        assert_eq!(count, 1);
    };
    tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(writer, subscriber);
    })
    .await
    .unwrap();
}
