//! Opening the database at startup without taking the app down.
//!
//! [`DbInitError`] says why the database did not open, in a form both UIs
//! can show on an error page: the kind (locked, corrupt, disk full, failed
//! migration), the technical detail, the database file, the backup copy and
//! the log file. Before anything else happens to a database that failed to
//! open, [`backup_database`] copies it (with its `-wal` and `-shm` files) to
//! a backup folder. Nothing here ever deletes the `-wal` or `-shm` files: a
//! WAL holds committed transactions that are not in the main file yet.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::Serialize;

/// The backup folder inside the app data folder.
pub const BACKUP_DIR: &str = "backups";

/// How long a connection waits for another connection's lock before it
/// gives up with "database is locked".
pub const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// What the error page needs besides the kind of failure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DbFailureInfo {
    /// The technical error (English, from SQLite or the OS).
    pub detail: String,
    /// The database file that failed to open.
    pub database_path: PathBuf,
    /// The folder with the copy taken before anything else happened, when
    /// one was taken.
    pub backup_path: Option<PathBuf>,
    /// The application log, when logging to a file works.
    pub log_path: Option<PathBuf>,
}

/// Why the database did not open.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize)]
#[serde(tag = "kind", content = "info", rename_all = "snake_case")]
pub enum DbInitError {
    /// Another process (or connection) holds a lock on the file.
    #[error("database is locked by another program: {}", .0.detail)]
    Locked(DbFailureInfo),
    /// The file is not a database or is damaged.
    #[error("database file is corrupt: {}", .0.detail)]
    Corrupt(DbFailureInfo),
    /// No room left on the disk.
    #[error("disk is full: {}", .0.detail)]
    DiskFull(DbFailureInfo),
    /// The schema could not be brought up to date.
    #[error("database migration failed: {}", .0.detail)]
    Migration(DbFailureInfo),
    /// Anything else: no access to the folder, file cannot be created, ...
    #[error("database could not be opened: {}", .0.detail)]
    Other(DbFailureInfo),
}

/// The kind of failure without its details.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DbFailureKind {
    Locked,
    Corrupt,
    DiskFull,
    Migration,
    Other,
}

impl DbInitError {
    pub fn new(kind: DbFailureKind, info: DbFailureInfo) -> Self {
        match kind {
            DbFailureKind::Locked => Self::Locked(info),
            DbFailureKind::Corrupt => Self::Corrupt(info),
            DbFailureKind::DiskFull => Self::DiskFull(info),
            DbFailureKind::Migration => Self::Migration(info),
            DbFailureKind::Other => Self::Other(info),
        }
    }

    pub fn kind(&self) -> DbFailureKind {
        match self {
            Self::Locked(_) => DbFailureKind::Locked,
            Self::Corrupt(_) => DbFailureKind::Corrupt,
            Self::DiskFull(_) => DbFailureKind::DiskFull,
            Self::Migration(_) => DbFailureKind::Migration,
            Self::Other(_) => DbFailureKind::Other,
        }
    }

    pub fn info(&self) -> &DbFailureInfo {
        match self {
            Self::Locked(info)
            | Self::Corrupt(info)
            | Self::DiskFull(info)
            | Self::Migration(info)
            | Self::Other(info) => info,
        }
    }

    fn info_mut(&mut self) -> &mut DbFailureInfo {
        match self {
            Self::Locked(info)
            | Self::Corrupt(info)
            | Self::DiskFull(info)
            | Self::Migration(info)
            | Self::Other(info) => info,
        }
    }

    pub fn detail(&self) -> &str {
        &self.info().detail
    }

    pub fn database_path(&self) -> &Path {
        &self.info().database_path
    }

    pub fn backup_path(&self) -> Option<&Path> {
        self.info().backup_path.as_deref()
    }

    pub fn log_path(&self) -> Option<&Path> {
        self.info().log_path.as_deref()
    }

    pub(crate) fn set_backup_path(&mut self, path: Option<PathBuf>) {
        self.info_mut().backup_path = path;
    }

    /// Plain Danish explanation for the error page.
    pub fn summary_da(&self) -> &'static str {
        match self.kind() {
            DbFailureKind::Locked => "Databasen er låst af et andet program.",
            DbFailureKind::Corrupt => "Databasefilen er beskadiget.",
            DbFailureKind::DiskFull => "Disken er fuld.",
            DbFailureKind::Migration => "Databasen kunne ikke opdateres til den nye version.",
            DbFailureKind::Other => "Databasen kunne ikke åbnes.",
        }
    }

    /// What the user can do about it, in Danish.
    pub fn advice_da(&self) -> &'static str {
        match self.kind() {
            DbFailureKind::Locked => {
                "Luk andre vinduer med Tingskriver og programmer, der har databasen åben, og start Tingskriver igen."
            }
            DbFailureKind::Corrupt => {
                "Dine optagelser ligger stadig som lydfiler, og der er taget en kopi af databasen. Send logfilen til udvikleren, før du sletter noget."
            }
            DbFailureKind::DiskFull => "Frigør plads på disken, og start Tingskriver igen.",
            DbFailureKind::Migration => {
                "Der er taget en kopi af databasen. Send logfilen til udvikleren."
            }
            DbFailureKind::Other => {
                "Tjek at mappen ikke er skrivebeskyttet, og start Tingskriver igen. Sker det igen, så send logfilen til udvikleren."
            }
        }
    }

    /// Whether a copy of the database is taken before the error is shown.
    /// Not for a lock (the file is fine, and Windows may refuse to read it)
    /// or a full disk (the copy would make it worse).
    pub fn wants_backup(&self) -> bool {
        matches!(
            self.kind(),
            DbFailureKind::Corrupt | DbFailureKind::Migration | DbFailureKind::Other
        )
    }
}

// SQLite primary result codes (the low byte of an extended code).
const SQLITE_PERM: i64 = 3;
const SQLITE_BUSY: i64 = 5;
const SQLITE_LOCKED: i64 = 6;
const SQLITE_CORRUPT: i64 = 11;
const SQLITE_FULL: i64 = 13;
const SQLITE_NOTADB: i64 = 26;

/// The kind of failure for a SQLite result code (primary or extended).
pub fn kind_from_sqlite_code(code: i64) -> Option<DbFailureKind> {
    match code & 0xff {
        SQLITE_BUSY | SQLITE_LOCKED => Some(DbFailureKind::Locked),
        SQLITE_CORRUPT | SQLITE_NOTADB => Some(DbFailureKind::Corrupt),
        SQLITE_FULL => Some(DbFailureKind::DiskFull),
        SQLITE_PERM => Some(DbFailureKind::Other),
        _ => None,
    }
}

/// The kind of failure for an OS error while reading or writing files.
pub fn kind_from_io(error: &io::Error) -> DbFailureKind {
    match error.raw_os_error() {
        // ENOSPC (Linux/macOS), ERROR_HANDLE_DISK_FULL, ERROR_DISK_FULL (Windows)
        #[cfg(unix)]
        Some(28) => DbFailureKind::DiskFull,
        #[cfg(windows)]
        Some(39) | Some(112) => DbFailureKind::DiskFull,
        // ERROR_SHARING_VIOLATION, ERROR_LOCK_VIOLATION (Windows)
        #[cfg(windows)]
        Some(32) | Some(33) => DbFailureKind::Locked,
        _ => DbFailureKind::Other,
    }
}

/// The kind of failure for an error from sqlx while connecting or querying.
pub fn kind_from_sqlx(error: &sqlx::Error) -> DbFailureKind {
    match error {
        sqlx::Error::Database(db) => db
            .code()
            .and_then(|code| code.parse::<i64>().ok())
            .and_then(kind_from_sqlite_code)
            .unwrap_or_else(|| kind_from_message(db.message())),
        sqlx::Error::Io(io) => kind_from_io(io),
        other => kind_from_message(&other.to_string()),
    }
}

/// The kind of failure for an error while running migrations. A migration
/// that fails because the file is locked, damaged or the disk is full is
/// that failure, not a migration failure.
pub fn kind_from_migrate(error: &sqlx::migrate::MigrateError) -> DbFailureKind {
    use sqlx::migrate::MigrateError;
    match error {
        MigrateError::Execute(inner) | MigrateError::ExecuteMigration(inner, _) => {
            match kind_from_sqlx(inner) {
                DbFailureKind::Other => DbFailureKind::Migration,
                kind => kind,
            }
        }
        _ => DbFailureKind::Migration,
    }
}

/// Last resort when an error carries no code: SQLite's own messages.
fn kind_from_message(message: &str) -> DbFailureKind {
    let lower = message.to_lowercase();
    if lower.contains("database is locked") || lower.contains("database table is locked") {
        DbFailureKind::Locked
    } else if lower.contains("not a database")
        || lower.contains("malformed")
        || lower.contains("corrupt")
    {
        DbFailureKind::Corrupt
    } else if lower.contains("disk is full") {
        DbFailureKind::DiskFull
    } else {
        DbFailureKind::Other
    }
}

/// The database file, the two files SQLite keeps next to it in WAL mode
/// and the rollback journal.
pub fn database_files(database_path: &Path) -> [PathBuf; 4] {
    let with_suffix = |suffix: &str| {
        let mut path = database_path.as_os_str().to_owned();
        path.push(suffix);
        PathBuf::from(path)
    };
    [
        database_path.to_path_buf(),
        with_suffix("-wal"),
        with_suffix("-shm"),
        with_suffix("-journal"),
    ]
}

/// Whether the last session left a `-wal` or `-journal` with content behind,
/// i.e. ended without closing the database (a crash). A clean shutdown
/// checkpoints the WAL and SQLite removes it.
pub fn has_leftover_journal(database_path: &Path) -> bool {
    let [_, wal, _, journal] = database_files(database_path);
    [wal, journal]
        .iter()
        .any(|file| fs::metadata(file).map(|m| m.len() > 0).unwrap_or(false))
}

/// Folder name for a backup of `database_path`: the file name, its
/// modification time and its size. Starting a broken database again gives
/// the same name, so repeated failures do not fill the disk with copies.
fn backup_folder_name(database_path: &Path, modified: SystemTime, len: u64) -> String {
    let stem = database_path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "database".to_string());
    let modified: chrono::DateTime<chrono::Local> = modified.into();
    format!("{stem}-{}-{len}", modified.format("%Y%m%d-%H%M%S"))
}

/// Copy the database file and its `-wal`, `-shm` and `-journal` files to a
/// new folder in `backup_root`. Returns the folder, or `None` when there is
/// no database file to copy. A backup of the same file state (modification
/// time and size) is reused instead of copied again.
pub fn backup_database(database_path: &Path, backup_root: &Path) -> io::Result<Option<PathBuf>> {
    Ok(copy_database(database_path, backup_root)?.map(|(folder, _)| folder))
}

/// [`backup_database`], and whether this call made the copy (`false` when an
/// existing backup of the same file state was reused).
fn copy_database(database_path: &Path, backup_root: &Path) -> io::Result<Option<(PathBuf, bool)>> {
    let metadata = match fs::metadata(database_path) {
        Ok(metadata) => metadata,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
    let folder = backup_root.join(backup_folder_name(database_path, modified, metadata.len()));
    let main_copy = folder.join(database_path.file_name().unwrap_or_default());
    if main_copy.exists() {
        return Ok(Some((folder, false)));
    }

    // Copy into a temporary folder and rename it at the end, so a copy that
    // fails halfway (full disk) is never mistaken for a complete backup.
    let partial = backup_root.join(format!(
        "{}.partial",
        folder.file_name().unwrap_or_default().to_string_lossy()
    ));
    if partial.exists() {
        fs::remove_dir_all(&partial)?;
    }
    fs::create_dir_all(&partial)?;
    for file in database_files(database_path) {
        if !file.exists() {
            continue;
        }
        if let Some(name) = file.file_name() {
            fs::copy(&file, partial.join(name))?;
        }
    }
    fs::rename(&partial, &folder)?;
    Ok(Some((folder, true)))
}

/// A copy taken before opening a database that was not closed cleanly.
pub(crate) struct SafetyCopy {
    folder: PathBuf,
    created: bool,
}

impl SafetyCopy {
    pub(crate) fn folder(&self) -> &Path {
        &self.folder
    }

    /// The database opened: the copy is not needed. Only a copy made by
    /// this start is removed, never an earlier backup.
    pub(crate) fn discard(self) {
        if self.created {
            if let Err(e) = fs::remove_dir_all(&self.folder) {
                log::warn!("Could not remove {}: {}", self.folder.display(), e);
            }
        }
    }
}

/// Copy the database before opening it when the last session left a
/// journal behind. SQLite replays a rollback journal, and checkpoints a WAL
/// into the main file when the connection closes, even when the open then
/// fails, so a copy taken after the failure would no longer show the files
/// as they were.
pub(crate) fn backup_before_open(database_path: &Path, backup_root: &Path) -> Option<SafetyCopy> {
    if !has_leftover_journal(database_path) {
        return None;
    }
    match copy_database(database_path, backup_root) {
        Ok(Some((folder, created))) => {
            log::info!(
                "The database was not closed cleanly; copied it to {} before opening",
                folder.display()
            );
            Some(SafetyCopy { folder, created })
        }
        Ok(None) => None,
        Err(e) => {
            log::warn!(
                "Could not copy {} before opening: {}",
                database_path.display(),
                e
            );
            None
        }
    }
}

/// Take the backup an error asks for, log the outcome and record the
/// folder in the error. A failing backup is logged and never hides the
/// original error.
pub(crate) fn backup_after_failure(error: &mut DbInitError, backup_root: &Path) {
    if !error.wants_backup() {
        return;
    }
    match backup_database(error.database_path(), backup_root) {
        Ok(Some(folder)) => {
            log::warn!(
                "Copied the database that failed to open to {}",
                folder.display()
            );
            error.set_backup_path(Some(folder));
        }
        Ok(None) => {}
        Err(e) => log::error!(
            "Could not copy {} to {}: {}",
            error.database_path().display(),
            backup_root.display(),
            e
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(detail: &str) -> DbFailureInfo {
        DbFailureInfo {
            detail: detail.to_string(),
            database_path: PathBuf::from("meeting_minutes.sqlite"),
            backup_path: None,
            log_path: None,
        }
    }

    #[test]
    fn sqlite_codes_map_to_kinds() {
        assert_eq!(kind_from_sqlite_code(5), Some(DbFailureKind::Locked));
        assert_eq!(kind_from_sqlite_code(6), Some(DbFailureKind::Locked));
        // SQLITE_BUSY_SNAPSHOT is an extended code of SQLITE_BUSY.
        assert_eq!(
            kind_from_sqlite_code(5 | (2 << 8)),
            Some(DbFailureKind::Locked)
        );
        assert_eq!(kind_from_sqlite_code(11), Some(DbFailureKind::Corrupt));
        assert_eq!(kind_from_sqlite_code(26), Some(DbFailureKind::Corrupt));
        assert_eq!(kind_from_sqlite_code(13), Some(DbFailureKind::DiskFull));
        assert_eq!(kind_from_sqlite_code(1), None);
    }

    #[test]
    fn messages_without_codes_are_recognised() {
        assert_eq!(
            kind_from_message("database disk image is malformed"),
            DbFailureKind::Corrupt
        );
        assert_eq!(
            kind_from_message("file is not a database"),
            DbFailureKind::Corrupt
        );
        assert_eq!(
            kind_from_message("database is locked"),
            DbFailureKind::Locked
        );
        assert_eq!(
            kind_from_message("database or disk is full"),
            DbFailureKind::DiskFull
        );
        assert_eq!(kind_from_message("something else"), DbFailureKind::Other);
    }

    #[cfg(unix)]
    #[test]
    fn no_space_is_a_full_disk() {
        let error = io::Error::from_raw_os_error(28);
        assert_eq!(kind_from_io(&error), DbFailureKind::DiskFull);
        let error = io::Error::from_raw_os_error(13);
        assert_eq!(kind_from_io(&error), DbFailureKind::Other);
    }

    #[test]
    fn messages_keep_the_words_the_gpui_error_page_looks_for() {
        // tingskriver-app classifies by these words in the Display text.
        let cases = [
            (DbFailureKind::Locked, "locked"),
            (DbFailureKind::Corrupt, "corrupt"),
            (DbFailureKind::DiskFull, "disk is full"),
            (DbFailureKind::Migration, "migrat"),
        ];
        for (kind, word) in cases {
            let error = DbInitError::new(kind, info("x"));
            assert!(error.to_string().contains(word), "{error}");
            assert_eq!(error.kind(), kind);
        }
    }

    #[test]
    fn locks_and_full_disks_are_not_copied() {
        assert!(!DbInitError::new(DbFailureKind::Locked, info("x")).wants_backup());
        assert!(!DbInitError::new(DbFailureKind::DiskFull, info("x")).wants_backup());
        assert!(DbInitError::new(DbFailureKind::Corrupt, info("x")).wants_backup());
        assert!(DbInitError::new(DbFailureKind::Migration, info("x")).wants_backup());
    }

    #[test]
    fn serializes_with_kind_for_the_ui() {
        let json = serde_json::to_value(DbInitError::Corrupt(info("bad"))).unwrap();
        assert_eq!(json["kind"], "corrupt");
        assert_eq!(json["info"]["detail"], "bad");
    }

    #[test]
    fn database_files_include_wal_and_shm() {
        let [db, wal, shm, journal] = database_files(Path::new("/x/meeting_minutes.sqlite"));
        assert_eq!(db, PathBuf::from("/x/meeting_minutes.sqlite"));
        assert_eq!(wal, PathBuf::from("/x/meeting_minutes.sqlite-wal"));
        assert_eq!(shm, PathBuf::from("/x/meeting_minutes.sqlite-shm"));
        assert_eq!(journal, PathBuf::from("/x/meeting_minutes.sqlite-journal"));
    }

    #[test]
    fn backup_copies_all_files_once_and_leaves_the_originals() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("meeting_minutes.sqlite");
        fs::write(&db, b"main").unwrap();
        fs::write(dir.path().join("meeting_minutes.sqlite-wal"), b"wal").unwrap();
        let root = dir.path().join(BACKUP_DIR);

        let folder = backup_database(&db, &root).unwrap().unwrap();
        assert_eq!(
            fs::read(folder.join("meeting_minutes.sqlite")).unwrap(),
            b"main"
        );
        assert_eq!(
            fs::read(folder.join("meeting_minutes.sqlite-wal")).unwrap(),
            b"wal"
        );
        assert!(!folder.join("meeting_minutes.sqlite-shm").exists());
        // The originals are untouched.
        assert!(db.exists());
        assert!(dir.path().join("meeting_minutes.sqlite-wal").exists());

        // The same file state reuses the backup.
        let again = backup_database(&db, &root).unwrap().unwrap();
        assert_eq!(again, folder);
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
    }

    #[test]
    fn leftover_journals_are_detected_and_safety_copies_only_remove_their_own() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("meeting_minutes.sqlite");
        let wal = dir.path().join("meeting_minutes.sqlite-wal");
        let root = dir.path().join(BACKUP_DIR);
        fs::write(&db, b"main").unwrap();
        assert!(!has_leftover_journal(&db));
        assert!(backup_before_open(&db, &root).is_none());
        fs::write(&wal, b"").unwrap();
        assert!(!has_leftover_journal(&db), "an empty WAL is a clean state");
        fs::write(&wal, b"frames").unwrap();
        assert!(has_leftover_journal(&db));

        let copy = backup_before_open(&db, &root).unwrap();
        let folder = copy.folder().to_path_buf();
        assert_eq!(
            fs::read(folder.join("meeting_minutes.sqlite-wal")).unwrap(),
            b"frames"
        );
        copy.discard();
        assert!(!folder.exists());

        // A backup from an earlier failure of the same file state stays.
        let earlier = backup_database(&db, &root).unwrap().unwrap();
        let copy = backup_before_open(&db, &root).unwrap();
        assert_eq!(copy.folder(), earlier);
        copy.discard();
        assert!(earlier.exists());
        assert!(wal.exists(), "the WAL itself is never touched");
    }

    #[test]
    fn backup_without_a_database_is_none() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("meeting_minutes.sqlite");
        assert_eq!(
            backup_database(&db, &dir.path().join(BACKUP_DIR)).unwrap(),
            None
        );
    }
}
