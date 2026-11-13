//! Keeping the user's data safe while a Tauri installation moves to the GPUI
//! app.
//!
//! Both apps use the same data folder (`%APPDATA%\dk.mahoje.tingskriver`: the
//! database, the settings stores, the models, the recordings), so the move
//! copies nothing. What can go wrong is the NSIS uninstaller (or a bug)
//! touching that folder. The move helper therefore
//!
//! 1. takes an [`Inventory`] of the folder and a database backup
//!    ([`prepare`]) before it runs the uninstaller, and
//! 2. checks the folder against the inventory afterwards ([`check`]) and puts
//!    the database back from the backup if it is gone ([`restore_database`]).
//!
//! The GPUI app upgrades the database schema itself on its first start
//! (`DatabaseManager::open`, with its own backup), see
//! `docs/database-migrationer.md`.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use crate::database::init::{backup_database, database_files, BACKUP_DIR};
use crate::paths::DATABASE_FILE;

/// Relative path (with `/`) to size in bytes of every file in the data folder.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Inventory {
    files: BTreeMap<String, u64>,
}

/// What the check found different from the inventory.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// In the inventory, gone now.
    pub missing: Vec<String>,
    /// In the inventory, now smaller (a file only ever grows or stays while
    /// nothing runs, so a smaller one is cut).
    pub shrunk: Vec<String>,
}

impl Report {
    pub fn is_intact(&self) -> bool {
        self.missing.is_empty() && self.shrunk.is_empty()
    }

    /// True when the database file itself is among the missing files.
    pub fn database_missing(&self) -> bool {
        self.missing.iter().any(|path| path == DATABASE_FILE)
    }
}

/// The inventory and, if there was a database, where its backup is.
#[derive(Clone, Debug)]
pub struct Prepared {
    pub inventory: Inventory,
    pub backup: Option<PathBuf>,
}

impl Inventory {
    pub fn len(&self) -> usize {
        self.files.len()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    pub fn contains(&self, relative: &str) -> bool {
        self.files.contains_key(relative)
    }

    /// Total size in bytes.
    pub fn bytes(&self) -> u64 {
        self.files.values().sum()
    }
}

/// Files that come and go while an app runs. They are neither promised nor
/// compared: SQLite journals, half-downloaded models and temporary files.
fn is_transient(relative: &str) -> bool {
    let name = relative.rsplit('/').next().unwrap_or(relative);
    name.ends_with("-wal")
        || name.ends_with("-shm")
        || name.ends_with("-journal")
        || name.ends_with(".part")
        || name.ends_with(".tmp")
        || name.ends_with(".partial")
}

/// Everything the user would miss: the whole data folder except the backups
/// folder and the transient files.
#[allow(clippy::disallowed_methods)] // Synchronous disk worker.
pub fn inventory(data_dir: &Path) -> io::Result<Inventory> {
    fn walk(dir: &Path, prefix: &str, files: &mut BTreeMap<String, u64>) -> io::Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let relative = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            let kind = entry.file_type()?;
            if kind.is_dir() {
                if prefix.is_empty() && name == BACKUP_DIR {
                    continue;
                }
                walk(&entry.path(), &relative, files)?;
            } else if kind.is_file() && !is_transient(&relative) {
                files.insert(relative, entry.metadata()?.len());
            }
        }
        Ok(())
    }
    let mut files = BTreeMap::new();
    match walk(data_dir, "", &mut files) {
        Ok(()) => {}
        // A fresh machine without a data folder has nothing to lose.
        Err(e) if e.kind() == io::ErrorKind::NotFound && !data_dir.exists() => {}
        Err(e) => return Err(e),
    }
    Ok(Inventory { files })
}

/// Back up the database into `backup_root` and take the inventory. Run it
/// after the Tauri app has exited (its write-ahead log is then complete) and
/// before the uninstaller starts. `backup_root` must lie outside `data_dir`
/// (the move helper uses a folder under the temp directory): an uninstaller
/// that wipes the whole data folder would otherwise take the backup with it.
/// The backup is taken first and does not depend on the inventory. A failing
/// backup or inventory is an error: the caller decides whether to go on.
pub fn prepare(data_dir: &Path, backup_root: &Path) -> io::Result<Prepared> {
    let backup = backup_database(&data_dir.join(DATABASE_FILE), backup_root)?;
    let inventory = inventory(data_dir)?;
    Ok(Prepared { inventory, backup })
}

/// Compare the data folder with the inventory.
pub fn check(data_dir: &Path, before: &Inventory) -> io::Result<Report> {
    let now = inventory(data_dir)?;
    let mut report = Report::default();
    for (path, size) in &before.files {
        match now.files.get(path) {
            None => report.missing.push(path.clone()),
            Some(now_size) if now_size < size => report.shrunk.push(path.clone()),
            Some(_) => {}
        }
    }
    Ok(report)
}

/// Copy the database (and its journals) back from `backup` when the database
/// file is gone. Never overwrites an existing database. Returns whether it
/// restored anything.
#[allow(clippy::disallowed_methods)] // Synchronous disk worker.
pub fn restore_database(data_dir: &Path, backup: &Path) -> io::Result<bool> {
    let target = data_dir.join(DATABASE_FILE);
    if target.exists() {
        return Ok(false);
    }
    let from = backup.join(DATABASE_FILE);
    if !from.is_file() {
        return Ok(false);
    }
    std::fs::create_dir_all(data_dir)?;
    for file in database_files(&target) {
        let Some(name) = file.file_name() else {
            continue;
        };
        let source = backup.join(name);
        if source.is_file() {
            std::fs::copy(&source, &file)?;
        }
    }
    Ok(true)
}

#[cfg(test)]
#[allow(clippy::disallowed_methods)] // Synchronous test fixtures.
mod tests {
    use super::*;

    fn write(dir: &Path, relative: &str, bytes: &[u8]) {
        let path = dir.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }

    fn seed(dir: &Path) {
        write(dir, DATABASE_FILE, b"sqlite");
        write(dir, "onboarding-status.json", b"{}");
        write(dir, "models/parakeet/model.onnx", &[7; 64]);
        write(dir, "meetings/m1/audio.mp4", &[1; 32]);
    }

    #[test]
    fn inventory_skips_backups_and_transient_files() {
        let dir = tempfile::tempdir().unwrap();
        seed(dir.path());
        write(dir.path(), "meeting_minutes.sqlite-wal", b"w");
        write(dir.path(), "models/big.bin.part", b"p");
        write(
            dir.path(),
            "backups/database-1/meeting_minutes.sqlite",
            b"b",
        );
        let inv = inventory(dir.path()).unwrap();
        assert_eq!(inv.len(), 4);
        assert!(inv.contains("models/parakeet/model.onnx"));
        assert!(!inv.contains("meeting_minutes.sqlite-wal"));
        assert!(!inv.contains("models/big.bin.part"));
        assert!(!inv.contains("backups/database-1/meeting_minutes.sqlite"));
        assert_eq!(inv.bytes(), 6 + 2 + 64 + 32);
    }

    #[test]
    fn a_missing_data_folder_is_an_empty_inventory() {
        let dir = tempfile::tempdir().unwrap();
        let inv = inventory(&dir.path().join("none")).unwrap();
        assert!(inv.is_empty());
        assert!(check(&dir.path().join("none"), &inv).unwrap().is_intact());
    }

    #[test]
    fn untouched_data_is_intact_and_new_files_are_fine() {
        let dir = tempfile::tempdir().unwrap();
        let backups = tempfile::tempdir().unwrap();
        seed(dir.path());
        let prepared = prepare(dir.path(), &backups.path().join("move")).unwrap();
        write(dir.path(), "gpui-window.json", b"{}");
        write(dir.path(), "meetings/m1/audio.mp4", &[1; 40]);
        assert!(check(dir.path(), &prepared.inventory).unwrap().is_intact());
    }

    #[test]
    fn lost_and_cut_files_are_reported() {
        let dir = tempfile::tempdir().unwrap();
        let backups = tempfile::tempdir().unwrap();
        seed(dir.path());
        let before = inventory(dir.path()).unwrap();
        std::fs::remove_dir_all(dir.path().join("models")).unwrap();
        write(dir.path(), "meetings/m1/audio.mp4", &[1; 8]);
        let report = check(dir.path(), &before).unwrap();
        assert_eq!(report.missing, vec!["models/parakeet/model.onnx"]);
        assert_eq!(report.shrunk, vec!["meetings/m1/audio.mp4"]);
        assert!(!report.is_intact());
        assert!(!report.database_missing());
    }

    #[test]
    fn a_deleted_database_comes_back_from_the_backup_but_is_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let backups = tempfile::tempdir().unwrap();
        seed(dir.path());
        write(dir.path(), "meeting_minutes.sqlite-wal", b"wal");
        let prepared = prepare(dir.path(), &backups.path().join("move")).unwrap();
        let backup = prepared.backup.clone().unwrap();
        assert!(backup.starts_with(backups.path()));

        // Present: nothing is touched.
        write(dir.path(), DATABASE_FILE, b"newer");
        assert!(!restore_database(dir.path(), &backup).unwrap());
        assert_eq!(
            std::fs::read(dir.path().join(DATABASE_FILE)).unwrap(),
            b"newer"
        );

        std::fs::remove_file(dir.path().join(DATABASE_FILE)).unwrap();
        std::fs::remove_file(dir.path().join("meeting_minutes.sqlite-wal")).unwrap();
        let report = check(dir.path(), &prepared.inventory).unwrap();
        assert!(report.database_missing());
        assert!(restore_database(dir.path(), &backup).unwrap());
        assert_eq!(
            std::fs::read(dir.path().join(DATABASE_FILE)).unwrap(),
            b"sqlite"
        );
        assert_eq!(
            std::fs::read(dir.path().join("meeting_minutes.sqlite-wal")).unwrap(),
            b"wal"
        );
        assert!(check(dir.path(), &prepared.inventory).unwrap().is_intact());
    }

    #[test]
    fn the_database_comes_back_when_the_whole_data_folder_is_wiped() {
        let dir = tempfile::tempdir().unwrap();
        let backups = tempfile::tempdir().unwrap();
        seed(dir.path());
        let prepared = prepare(dir.path(), &backups.path().join("move")).unwrap();
        let backup = prepared.backup.clone().unwrap();

        std::fs::remove_dir_all(dir.path()).unwrap();
        let report = check(dir.path(), &prepared.inventory).unwrap();
        assert!(report.database_missing());
        assert!(restore_database(dir.path(), &backup).unwrap());
        assert_eq!(
            std::fs::read(dir.path().join(DATABASE_FILE)).unwrap(),
            b"sqlite"
        );
    }

    #[test]
    fn no_database_means_no_backup() {
        let dir = tempfile::tempdir().unwrap();
        let backups = tempfile::tempdir().unwrap();
        write(dir.path(), "onboarding-status.json", b"{}");
        let prepared = prepare(dir.path(), &backups.path().join("move")).unwrap();
        assert!(prepared.backup.is_none());
        assert!(!restore_database(dir.path(), dir.path()).unwrap());
    }
}
