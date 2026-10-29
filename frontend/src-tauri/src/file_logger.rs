//! Application log: stdout (env_logger) plus a size-rotated file in the app
//! log directory, so release builds (no console) leave something to read.
//!
//! Files: `tingskriver.log` (current), `tingskriver.log.1`, `tingskriver.log.2`;
//! the current file rotates at `MAX_FILE_BYTES`.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use log::{Log, Metadata, Record};

const APP_ID: &str = "dk.mahoje.tingskriver";
const LOG_NAME: &str = "tingskriver.log";
pub const MAX_FILE_BYTES: u64 = 5 * 1024 * 1024;
/// Current file plus this many rotated ones.
pub const KEPT_FILES: usize = 3;

/// `%LOCALAPPDATA%\dk.mahoje.tingskriver\logs` on Windows, the platform's
/// local data directory elsewhere.
pub fn log_dir() -> Option<PathBuf> {
    dirs::data_local_dir().map(|d| d.join(APP_ID).join("logs"))
}

/// Append-only file that rotates when it would grow beyond `max_bytes`.
pub struct RotatingFile {
    dir: PathBuf,
    max_bytes: u64,
    file: File,
    size: u64,
}

impl RotatingFile {
    pub fn open(dir: &Path, max_bytes: u64) -> std::io::Result<Self> {
        fs::create_dir_all(dir)?;
        let path = dir.join(LOG_NAME);
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        let size = file.metadata()?.len();
        Ok(Self {
            dir: dir.to_path_buf(),
            max_bytes,
            file,
            size,
        })
    }

    fn rotated_path(&self, index: usize) -> PathBuf {
        if index == 0 {
            self.dir.join(LOG_NAME)
        } else {
            self.dir.join(format!("{}.{}", LOG_NAME, index))
        }
    }

    fn rotate(&mut self) -> std::io::Result<()> {
        // Oldest first: .2 is dropped, .1 -> .2, current -> .1.
        let _ = fs::remove_file(self.rotated_path(KEPT_FILES - 1));
        for i in (0..KEPT_FILES - 1).rev() {
            let from = self.rotated_path(i);
            if from.exists() {
                fs::rename(&from, self.rotated_path(i + 1))?;
            }
        }
        self.file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.rotated_path(0))?;
        self.size = 0;
        Ok(())
    }

    pub fn write_line(&mut self, line: &str) -> std::io::Result<()> {
        let bytes = line.len() as u64 + 1;
        if self.size > 0 && self.size + bytes > self.max_bytes {
            if self.rotate().is_err() {
                // Rotation can fail (antivirus, a reader holding the backup
                // open). Keep writing to the current file and try again after
                // another max_bytes instead of losing all further logging.
                self.size = 0;
            }
        }
        writeln!(self.file, "{}", line)?;
        self.size += bytes;
        Ok(())
    }
}

struct TeeLogger {
    console: env_logger::Logger,
    file: Mutex<Option<RotatingFile>>,
}

impl Log for TeeLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        self.console.enabled(metadata)
    }

    fn log(&self, record: &Record) {
        if !self.console.matches(record) {
            return;
        }
        self.console.log(record);
        if let Ok(mut guard) = self.file.lock() {
            if let Some(file) = guard.as_mut() {
                let line = format!(
                    "{} {:<5} {} {}",
                    chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f"),
                    record.level(),
                    record.target(),
                    record.args()
                );
                // A full disk must never take the app down.
                let _ = file.write_line(&line);
            }
        }
    }

    fn flush(&self) {
        self.console.flush();
        if let Ok(mut guard) = self.file.lock() {
            if let Some(file) = guard.as_mut() {
                let _ = file.file.flush();
            }
        }
    }
}

/// Install the logger. Falls back to console-only if the file cannot be opened.
pub fn init() {
    let console = env_logger::Builder::from_default_env().build();
    let max_level = console.filter();
    let file = log_dir().and_then(|dir| match RotatingFile::open(&dir, MAX_FILE_BYTES) {
        Ok(f) => Some(f),
        Err(e) => {
            eprintln!("Could not open log file in {}: {}", dir.display(), e);
            None
        }
    });
    let logger = TeeLogger {
        console,
        file: Mutex::new(file),
    };
    if log::set_boxed_logger(Box::new(logger)).is_ok() {
        log::set_max_level(max_level);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tingskriver-log-test-{}-{}", name, std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn rotates_and_keeps_three_files() {
        let dir = temp_dir("rotate");
        let mut log = RotatingFile::open(&dir, 100).unwrap();
        let line = "x".repeat(59); // 60 bytes with newline: two lines overflow 100
        for _ in 0..10 {
            log.write_line(&line).unwrap();
        }
        assert!(dir.join(LOG_NAME).exists());
        assert!(dir.join(format!("{}.1", LOG_NAME)).exists());
        assert!(dir.join(format!("{}.2", LOG_NAME)).exists());
        assert!(!dir.join(format!("{}.3", LOG_NAME)).exists());
        for entry in fs::read_dir(&dir).unwrap() {
            assert!(entry.unwrap().metadata().unwrap().len() <= 100);
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn keeps_logging_when_rotation_fails() {
        let dir = temp_dir("rotate-fails");
        let mut log = RotatingFile::open(&dir, 100).unwrap();
        // Non-empty directories in the way make every rename fail.
        for i in 1..KEPT_FILES {
            let blocker = dir.join(format!("{}.{}", LOG_NAME, i));
            fs::create_dir_all(&blocker).unwrap();
            fs::write(blocker.join("x"), "x").unwrap();
        }
        let line = "x".repeat(59);
        for _ in 0..6 {
            log.write_line(&line).unwrap();
        }
        let written = fs::read_to_string(dir.join(LOG_NAME)).unwrap();
        assert_eq!(written.lines().count(), 6);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn appends_to_existing_file_across_restarts() {
        let dir = temp_dir("append");
        RotatingFile::open(&dir, 1000).unwrap().write_line("first").unwrap();
        RotatingFile::open(&dir, 1000).unwrap().write_line("second").unwrap();
        let content = fs::read_to_string(dir.join(LOG_NAME)).unwrap();
        assert_eq!(content, "first\nsecond\n");
        let _ = fs::remove_dir_all(&dir);
    }
}
