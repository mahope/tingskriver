use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{Result, Sqlite, SqlitePool, Transaction};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::init::{
    backup_after_failure, backup_before_open, kind_from_io, kind_from_migrate, kind_from_sqlx,
    DbFailureInfo, DbFailureKind, DbInitError, BACKUP_DIR, BUSY_TIMEOUT,
};

/// The legacy backend's database, copied on first open when it exists.
const LEGACY_DATABASE_FILE: &str = "meeting_minutes.db";

#[derive(Clone)]
pub struct DatabaseManager {
    pool: SqlitePool,
}

/// Connection settings for every connection in the pool: WAL, so a write
/// during a recording never blocks reading the meeting list; NORMAL sync,
/// which is safe with WAL; a busy timeout instead of failing at once on a
/// lock; and foreign keys, so `ON DELETE CASCADE` works.
pub fn connect_options(database_path: &Path, busy_timeout: Duration) -> SqliteConnectOptions {
    SqliteConnectOptions::new()
        .filename(database_path)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .busy_timeout(busy_timeout)
        .foreign_keys(true)
}

impl DatabaseManager {
    /// Open (and migrate) the database at `database_path`, creating it, or
    /// copying `legacy_path` there first when that exists. Does not take a
    /// backup; [`DatabaseManager::open`] does.
    pub async fn open_at(
        database_path: &Path,
        legacy_path: &Path,
        busy_timeout: Duration,
    ) -> std::result::Result<Self, DbInitError> {
        let fail = |kind: DbFailureKind, detail: String| {
            DbInitError::new(
                kind,
                DbFailureInfo {
                    detail,
                    database_path: database_path.to_path_buf(),
                    backup_path: None,
                    log_path: crate::logging::log_file(),
                },
            )
        };

        if let Some(parent_dir) = database_path.parent() {
            fs::create_dir_all(parent_dir).map_err(|e| fail(kind_from_io(&e), e.to_string()))?;
        }

        let exists = database_path.exists();
        if !exists && legacy_path.exists() {
            log::info!(
                "Copying database from {} to {}",
                legacy_path.display(),
                database_path.display()
            );
            fs::copy(legacy_path, database_path)
                .map_err(|e| fail(kind_from_io(&e), e.to_string()))?;
        } else if !exists {
            log::info!("Creating database at {}", database_path.display());
        }

        let options = connect_options(database_path, busy_timeout).create_if_missing(true);
        let pool = SqlitePoolOptions::new()
            .connect_with(options)
            .await
            .map_err(|e| fail(kind_from_sqlx(&e), e.to_string()))?;

        if let Err(e) = sqlx::migrate!("./migrations").run(&pool).await {
            pool.close().await;
            return Err(fail(kind_from_migrate(&e), e.to_string()));
        }

        Ok(DatabaseManager { pool })
    }

    /// Open the database in `app_data_dir` at startup. A database that was
    /// not closed cleanly (a `-wal` or `-journal` left behind) is copied to
    /// `app_data_dir/backups` before it is opened, and the copy is removed
    /// again when it opens. On failure without such a copy, the database
    /// (with its `-wal` and `-shm` files) is copied when the kind of failure
    /// calls for it. The error is logged and says what the UI should show.
    /// Never deletes the database or its journals.
    pub async fn open(app_data_dir: &Path) -> std::result::Result<Self, DbInitError> {
        let started = Instant::now();
        let database_path = app_data_dir.join(crate::paths::DATABASE_FILE);
        let legacy_path = app_data_dir.join(LEGACY_DATABASE_FILE);
        log::info!("Database path: {}", database_path.display());
        let backup_dir = Self::backup_dir(app_data_dir);
        let safety_copy = backup_before_open(&database_path, &backup_dir);

        match Self::open_at(&database_path, &legacy_path, BUSY_TIMEOUT).await {
            Ok(db) => {
                if let Some(copy) = safety_copy {
                    copy.discard();
                }
                log::info!("Database opened in {} ms", started.elapsed().as_millis());
                Ok(db)
            }
            Err(mut error) => {
                log::error!("Database failed to open ({:?}): {}", error.kind(), error);
                match safety_copy {
                    // The files as they were before SQLite touched them.
                    Some(copy) => error.set_backup_path(Some(copy.folder().to_path_buf())),
                    None => backup_after_failure(&mut error, &backup_dir),
                }
                Err(error)
            }
        }
    }

    /// Where copies of databases that failed to open are kept.
    pub fn backup_dir(app_data_dir: &Path) -> PathBuf {
        app_data_dir.join(BACKUP_DIR)
    }

    /// The database in `app_data_dir`, see [`DatabaseManager::open`].
    ///
    /// For first-time users of the legacy backend: delete the new .sqlite
    /// file and copy the old .db file into the app data folder; the next
    /// start copies it to the new file and migrates it.
    pub async fn new_from_app_data_dir(
        app_data_dir: &Path,
    ) -> std::result::Result<Self, DbInitError> {
        Self::open(app_data_dir).await
    }

    /// Check if this is the first launch (sqlite database doesn't exist yet)
    pub async fn is_first_launch(app_data_dir: &Path) -> Result<bool> {
        let tauri_db_path = app_data_dir.join("meeting_minutes.sqlite");

        Ok(!tauri_db_path.exists())
    }

    /// Import a legacy database from the specified path and initialize
    pub async fn import_legacy_database(
        app_data_dir: &Path,
        legacy_db_path: &str,
    ) -> std::result::Result<Self, DbInitError> {
        let fail = |e: std::io::Error| {
            DbInitError::new(
                kind_from_io(&e),
                DbFailureInfo {
                    detail: e.to_string(),
                    database_path: app_data_dir.join(crate::paths::DATABASE_FILE),
                    backup_path: None,
                    log_path: crate::logging::log_file(),
                },
            )
        };
        fs::create_dir_all(app_data_dir).map_err(fail)?;

        // Copy legacy database to app data directory as meeting_minutes.db
        let target_legacy_path = app_data_dir.join("meeting_minutes.db");
        log::info!(
            "Copying legacy database from {} to {}",
            legacy_db_path,
            target_legacy_path.display()
        );

        fs::copy(legacy_db_path, &target_legacy_path).map_err(fail)?;

        // Now use the standard initialization which will detect and migrate the legacy db
        Self::new_from_app_data_dir(app_data_dir).await
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub async fn with_transaction<T, F, Fut>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&mut Transaction<'_, Sqlite>) -> Fut,
        Fut: std::future::Future<Output = Result<T>>,
    {
        let mut tx = self.pool.begin().await?;
        let result = f(&mut tx).await;

        match result {
            Ok(val) => {
                tx.commit().await?;
                Ok(val)
            }
            Err(err) => {
                tx.rollback().await?;
                Err(err)
            }
        }
    }

    /// Checkpoint the WAL and close the pool, on application shutdown:
    /// every committed change is then in the main database file. SQLite
    /// itself removes the `-wal` and `-shm` files when the last connection
    /// closes cleanly; this code never deletes them.
    pub async fn cleanup(&self) -> Result<()> {
        log::info!("Starting database cleanup...");

        // TRUNCATE: checkpoint every page and reset the WAL to zero bytes.
        match sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
            .execute(&self.pool)
            .await
        {
            Ok(_) => log::info!("WAL checkpoint completed successfully"),
            Err(e) => log::warn!("WAL checkpoint failed (non-fatal): {}", e),
        }

        // Close the connection pool gracefully
        self.pool.close().await;
        log::info!("Database connection pool closed");

        Ok(())
    }
}
