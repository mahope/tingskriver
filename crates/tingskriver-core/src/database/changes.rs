//! Tells the UI which tables changed, once a write has been committed.
//!
//! SQLite's update hook reports every changed row on the connection that
//! wrote it, the commit hook says the transaction is final, and the rollback
//! hook says it is not. Each pooled connection collects table names from its
//! update hook, sends them when it commits and throws them away on rollback.
//! Subscribers (the meeting list, the search) get the set of tables, never
//! row data, and merge bursts with [`ChangeReceiver::next_batch`].
//!
//! Where to hook in: the meeting list (#56) and the search (#68) call
//! [`keep_fresh`] with `db.changes().subscribe()` and the tables they read
//! when they are created. Nothing subscribes yet because neither list exists
//! on this branch.
//!
//! Limits of SQLite's hooks, all of which only cause an extra reload and
//! never a missed one:
//! - Subscribe to base tables (`meetings`, `transcripts`, ...), not to the
//!   FTS5 table `search_fts`: it is filled by triggers on the base tables,
//!   so those are reported, while the virtual table and its shadow tables
//!   are not a reliable name to listen on. DDL and `WITHOUT ROWID` tables
//!   are not reported by the update hook at all.
//! - The rollback hook does not run for `ROLLBACK TO savepoint` or for a
//!   statement that fails in autocommit, so those tables can stay pending
//!   and be announced with the connection's next commit.
//!
//! The hooks only push a small value into a channel: they never wait, never
//! touch the database and never block the writer. If a subscriber is too slow
//! the channel drops the oldest messages and the receiver reports "everything
//! may have changed", which costs one extra reload and nothing else.

use std::collections::BTreeSet;
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use sqlx::sqlite::SqliteConnection;
use tokio::sync::broadcast;
use tokio::time::Instant;

/// Messages kept for a slow subscriber before it falls behind.
const CHANNEL_CAPACITY: usize = 256;

/// Tables changed by one committed transaction, or everything after a lag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    Tables(BTreeSet<String>),
    /// The subscriber missed messages; reload whatever it shows.
    Everything,
}

impl Change {
    /// Whether a list that reads `tables` has to reload.
    pub fn touches(&self, tables: &[&str]) -> bool {
        match self {
            Change::Everything => true,
            Change::Tables(changed) => tables.iter().any(|t| changed.contains(*t)),
        }
    }

    fn merge(&mut self, other: Change) {
        match (&mut *self, other) {
            (Change::Tables(mine), Change::Tables(theirs)) => mine.extend(theirs),
            (_, Change::Everything) => *self = Change::Everything,
            (Change::Everything, Change::Tables(_)) => {}
        }
    }
}

#[derive(Debug)]
struct Shared {
    sender: broadcast::Sender<Arc<BTreeSet<String>>>,
    /// Counts commits that changed something. Cheap fallback for readers
    /// that only need "has anything changed since I looked".
    generation: AtomicU64,
}

/// Cloneable handle to the change feed of one database.
#[derive(Debug, Clone)]
pub struct ChangeNotifier {
    shared: Arc<Shared>,
}

impl Default for ChangeNotifier {
    fn default() -> Self {
        Self::new()
    }
}

impl ChangeNotifier {
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(CHANNEL_CAPACITY);
        Self {
            shared: Arc::new(Shared {
                sender,
                generation: AtomicU64::new(0),
            }),
        }
    }

    pub fn subscribe(&self) -> ChangeReceiver {
        ChangeReceiver {
            receiver: self.shared.sender.subscribe(),
        }
    }

    /// Number of committed transactions that changed rows so far.
    pub fn generation(&self) -> u64 {
        self.shared.generation.load(Ordering::Acquire)
    }

    /// Publish a committed set of tables. Never blocks.
    pub fn publish(&self, tables: BTreeSet<String>) {
        if tables.is_empty() {
            return;
        }
        self.shared.generation.fetch_add(1, Ordering::AcqRel);
        // No subscriber is not an error.
        let _ = self.shared.sender.send(Arc::new(tables));
    }

    /// Install the hooks on one connection. Call from the pool's
    /// `after_connect`, once per connection.
    pub async fn attach(&self, connection: &mut SqliteConnection) -> sqlx::Result<()> {
        let pending = Arc::new(Mutex::new(BTreeSet::<String>::new()));
        let mut handle = connection.lock_handle().await?;

        let on_update = Arc::clone(&pending);
        handle.set_update_hook(move |change| {
            // Only the main database; SQLite's own bookkeeping is not data.
            if change.database == "main" && !change.table.starts_with("sqlite_") {
                on_update
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .insert(change.table.to_owned());
            }
        });

        let on_commit = Arc::clone(&pending);
        let notifier = self.clone();
        handle.set_commit_hook(move || {
            let tables =
                std::mem::take(&mut *on_commit.lock().unwrap_or_else(PoisonError::into_inner));
            notifier.publish(tables);
            true // true lets the commit go ahead; false would roll it back
        });

        let on_rollback = pending;
        handle.set_rollback_hook(move || {
            on_rollback
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clear();
        });
        Ok(())
    }
}

/// One subscriber's view of the feed.
#[derive(Debug)]
pub struct ChangeReceiver {
    receiver: broadcast::Receiver<Arc<BTreeSet<String>>>,
}

impl ChangeReceiver {
    /// The next committed change. `None` when the database is gone.
    pub async fn next(&mut self) -> Option<Change> {
        match self.receiver.recv().await {
            Ok(tables) => Some(Change::Tables((*tables).clone())),
            Err(broadcast::error::RecvError::Lagged(_)) => Some(Change::Everything),
            Err(broadcast::error::RecvError::Closed) => None,
        }
    }

    /// Wait for a change, then keep collecting until `quiet` passes without
    /// another one (or `max_wait` has passed since the first), and return the
    /// union. A burst of writes thus gives one batch, so one reload.
    pub async fn next_batch(&mut self, quiet: Duration, max_wait: Duration) -> Option<Change> {
        let mut batch = self.next().await?;
        let deadline = Instant::now() + max_wait;
        loop {
            let wait = quiet.min(deadline.saturating_duration_since(Instant::now()));
            if wait.is_zero() {
                return Some(batch);
            }
            match tokio::time::timeout(wait, self.next()).await {
                Ok(Some(more)) => batch.merge(more),
                Ok(None) => return Some(batch),
                Err(_) => return Some(batch),
            }
        }
    }
}

/// Reloads a list when a table it reads changes, never two reloads at once
/// and one reload per burst. `reload` runs off the writer's thread; a failed
/// reload is logged and the next change tries again. Returns when the
/// database is closed.
pub async fn keep_fresh<F, Fut>(
    mut receiver: ChangeReceiver,
    tables: &[&str],
    quiet: Duration,
    max_wait: Duration,
    mut reload: F,
) where
    F: FnMut() -> Fut,
    Fut: Future<Output = ()>,
{
    while let Some(change) = receiver.next_batch(quiet, max_wait).await {
        if change.touches(tables) {
            reload().await;
        }
    }
}
