//! Application log for UIs without a console: stderr plus a size-rotated file
//! in [`crate::paths::log_dir`], a panic hook that writes panics with a
//! backtrace to it, and timestamps for the phases of startup.
//!
//! Files: `<name>` (current), `<name>.1`, `<name>.2`; the current file
//! rotates at [`MAX_FILE_BYTES`]. Each UI picks its own file name, so the
//! Tauri app and the GPUI app can run side by side during the migration
//! without rotating each other's files.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use log::{LevelFilter, Log, Metadata, Record};

pub const MAX_FILE_BYTES: u64 = 5 * 1024 * 1024;
/// Current file plus this many rotated ones.
pub const KEPT_FILES: usize = 3;

/// Append-only file that rotates when it would grow beyond `max_bytes`.
pub struct RotatingFile {
    dir: PathBuf,
    name: String,
    max_bytes: u64,
    file: File,
    size: u64,
}

impl RotatingFile {
    pub fn open(dir: &Path, name: &str, max_bytes: u64) -> std::io::Result<Self> {
        fs::create_dir_all(dir)?;
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join(name))?;
        let size = file.metadata()?.len();
        Ok(Self {
            dir: dir.to_path_buf(),
            name: name.to_string(),
            max_bytes,
            file,
            size,
        })
    }

    /// The current log file.
    pub fn path(&self) -> PathBuf {
        self.rotated_path(0)
    }

    fn rotated_path(&self, index: usize) -> PathBuf {
        if index == 0 {
            self.dir.join(&self.name)
        } else {
            self.dir.join(format!("{}.{}", self.name, index))
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

    pub fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}

/// The level in `RUST_LOG` when it is a plain level (`debug`, `warn`, ...).
/// Per-module filters are not supported here.
pub fn level_from_env(value: Option<&str>) -> Option<LevelFilter> {
    value.and_then(|v| v.trim().parse::<LevelFilter>().ok())
}

/// Targets logged at info without `RUST_LOG`: Tingskriver itself and the
/// UI framework (adapter choice, GPU recovery). Everything else (zbus,
/// sqlx, hyper, ...) only from warn.
const INFO_TARGETS: &[&str] = &["tingskriver", "gpui"];

/// Whether a record passes: the `RUST_LOG` level for everything when set,
/// otherwise info for [`INFO_TARGETS`] and warn for the rest.
pub fn passes(target: &str, level: log::Level, configured: Option<LevelFilter>) -> bool {
    match configured {
        Some(max) => level <= max,
        None => {
            let max = if INFO_TARGETS.iter().any(|t| target.starts_with(t)) {
                LevelFilter::Info
            } else {
                LevelFilter::Warn
            };
            level <= max
        }
    }
}

struct FileLogger {
    configured: Option<LevelFilter>,
    file: Mutex<Option<RotatingFile>>,
}

impl Log for FileLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        passes(metadata.target(), metadata.level(), self.configured)
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let line = format!(
            "{} {:<5} {} {}",
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f"),
            record.level(),
            record.target(),
            record.args()
        );
        eprintln!("{line}");
        if let Ok(mut guard) = self.file.lock() {
            if let Some(file) = guard.as_mut() {
                // A full disk must never take the app down.
                let _ = file.write_line(&line);
            }
        }
    }

    fn flush(&self) {
        if let Ok(mut guard) = self.file.lock() {
            if let Some(file) = guard.as_mut() {
                let _ = file.flush();
            }
        }
    }
}

/// The log file [`init`] opened, for error pages ("send the log file").
static LOG_FILE: OnceLock<PathBuf> = OnceLock::new();

/// When [`init`] (or [`mark_process_start`]) ran: the start of the process
/// for the startup markers.
static PROCESS_START: OnceLock<Instant> = OnceLock::new();

/// The log file the logger writes to, `None` before [`init`] or when the
/// file could not be opened.
pub fn log_file() -> Option<PathBuf> {
    LOG_FILE.get().cloned()
}

/// Install a logger that writes to stderr and to `file_name` in
/// [`crate::paths::log_dir`], and the panic hook. Returns the log file's
/// path, or `None` when it could not be opened (the logger then writes to
/// stderr only).
pub fn init(file_name: &str) -> Option<PathBuf> {
    init_in(crate::paths::log_dir().as_deref(), file_name)
}

/// [`init`] with the log folder given (tests, other data folders).
pub fn init_in(dir: Option<&Path>, file_name: &str) -> Option<PathBuf> {
    mark_process_start();
    let rust_log = std::env::var("RUST_LOG").ok();
    let configured = level_from_env(rust_log.as_deref());
    let file = dir.and_then(
        |dir| match RotatingFile::open(dir, file_name, MAX_FILE_BYTES) {
            Ok(file) => Some(file),
            Err(e) => {
                eprintln!("Could not open log file in {}: {}", dir.display(), e);
                None
            }
        },
    );
    let path = file.as_ref().map(RotatingFile::path);
    let logger = FileLogger {
        configured,
        file: Mutex::new(file),
    };
    if log::set_logger(Box::leak(Box::new(logger))).is_ok() {
        log::set_max_level(configured.unwrap_or(LevelFilter::Info));
        if let Some(path) = &path {
            let _ = LOG_FILE.set(path.clone());
        }
        if let Some(value) = rust_log.filter(|v| !v.trim().is_empty() && configured.is_none()) {
            log::warn!(
                "RUST_LOG={value:?} is not a plain level (debug, info, ...); per-module filters are not supported, using the default levels"
            );
        }
    }
    install_panic_hook();
    log::info!(
        "Tingskriver {} starting (pid {}, {} {})",
        env!("CARGO_PKG_VERSION"),
        std::process::id(),
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    startup_phase("process start");
    path
}

/// Remember now as the start of the process, unless already done. Call it
/// first thing in `main` when the logger starts later.
pub fn mark_process_start() {
    PROCESS_START.get_or_init(Instant::now);
}

/// The line for a startup phase that was reached `elapsed` after the
/// process started.
pub fn startup_phase_line(phase: &str, elapsed: Duration) -> String {
    format!("Startup: {phase} after {} ms", elapsed.as_millis())
}

/// Log that startup reached `phase` (DB ready, tray ready, window ready,
/// ...), with the time since the process started. Each line starts with
/// "Startup:", so `grep Startup:` gives the timeline of one start.
pub fn startup_phase(phase: &str) {
    let start = *PROCESS_START.get_or_init(Instant::now);
    log::info!("{}", startup_phase_line(phase, start.elapsed()));
}

/// The log entry for a panic: thread, message, location and backtrace.
pub fn panic_report(
    thread: Option<&str>,
    message: &str,
    location: Option<String>,
    backtrace: &std::backtrace::Backtrace,
) -> String {
    format!(
        "PANIC in thread '{}' at {}: {}\nbacktrace:\n{}",
        thread.unwrap_or("<unnamed>"),
        location.as_deref().unwrap_or("<unknown location>"),
        message,
        backtrace
    )
}

/// The message of a panic payload (`panic!("...")` gives `&str` or `String`).
pub fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "<non-string panic payload>".to_string()
    }
}

/// Write every panic, from any thread, to the log with a backtrace, then
/// run the previous hook (stderr in debug builds). Release builds on
/// Windows have no console, so without this a panic leaves no trace.
/// Installed once; later calls do nothing.
pub fn install_panic_hook() {
    static INSTALLED: OnceLock<()> = OnceLock::new();
    INSTALLED.get_or_init(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let backtrace = std::backtrace::Backtrace::force_capture();
            let thread = std::thread::current();
            let report = panic_report(
                thread.name(),
                &panic_message(info.payload()),
                info.location().map(|l| l.to_string()),
                &backtrace,
            );
            log::error!("{report}");
            log::logger().flush();
            previous(info);
        }));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panic_report_has_thread_message_location_and_backtrace() {
        let backtrace = std::backtrace::Backtrace::force_capture();
        let report = panic_report(
            Some("recording"),
            "boom",
            Some("src/x.rs:1:2".to_string()),
            &backtrace,
        );
        assert!(
            report.starts_with("PANIC in thread 'recording' at src/x.rs:1:2: boom"),
            "{report}"
        );
        assert!(report.contains("backtrace:"), "{report}");
        let unnamed = panic_report(None, "x", None, &backtrace);
        assert!(
            unnamed.contains("'<unnamed>' at <unknown location>"),
            "{unnamed}"
        );
    }

    #[test]
    fn panic_messages_from_str_and_string() {
        let s: Box<dyn std::any::Any + Send> = Box::new("static");
        assert_eq!(panic_message(s.as_ref()), "static");
        let s: Box<dyn std::any::Any + Send> = Box::new(String::from("owned"));
        assert_eq!(panic_message(s.as_ref()), "owned");
        let s: Box<dyn std::any::Any + Send> = Box::new(42);
        assert_eq!(panic_message(s.as_ref()), "<non-string panic payload>");
    }

    #[test]
    fn startup_lines_are_greppable_with_milliseconds() {
        assert_eq!(
            startup_phase_line("DB ready", Duration::from_millis(1234)),
            "Startup: DB ready after 1234 ms"
        );
    }

    const NAME: &str = "test.log";

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "tingskriver-core-log-test-{}-{}",
            name,
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn rotates_and_keeps_three_files() {
        let dir = temp_dir("rotate");
        let mut log = RotatingFile::open(&dir, NAME, 100).unwrap();
        let line = "x".repeat(59); // 60 bytes with newline: two lines overflow 100
        for _ in 0..10 {
            log.write_line(&line).unwrap();
        }
        assert_eq!(log.path(), dir.join(NAME));
        assert!(dir.join(NAME).exists());
        assert!(dir.join(format!("{}.1", NAME)).exists());
        assert!(dir.join(format!("{}.2", NAME)).exists());
        assert!(!dir.join(format!("{}.3", NAME)).exists());
        for entry in fs::read_dir(&dir).unwrap() {
            assert!(entry.unwrap().metadata().unwrap().len() <= 100);
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn appends_to_existing_file_across_restarts() {
        let dir = temp_dir("append");
        RotatingFile::open(&dir, NAME, 1000)
            .unwrap()
            .write_line("første")
            .unwrap();
        RotatingFile::open(&dir, NAME, 1000)
            .unwrap()
            .write_line("anden")
            .unwrap();
        let content = fs::read_to_string(dir.join(NAME)).unwrap();
        assert_eq!(content, "første\nanden\n");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn reads_plain_levels_from_rust_log() {
        assert_eq!(level_from_env(Some("debug")), Some(LevelFilter::Debug));
        assert_eq!(level_from_env(Some(" WARN ")), Some(LevelFilter::Warn));
        assert_eq!(level_from_env(None), None);
        assert_eq!(level_from_env(Some("tingskriver_core=debug")), None);
    }

    #[test]
    fn without_rust_log_only_own_targets_log_info() {
        use log::Level;
        assert!(passes("tingskriver_app::ui", Level::Info, None));
        assert!(passes(
            "tingskriver_core::database::manager",
            Level::Info,
            None
        ));
        assert!(passes("gpui_wgpu::wgpu_context", Level::Info, None));
        assert!(!passes(
            "zbus::connection::handshake::common",
            Level::Info,
            None
        ));
        assert!(passes("zbus::connection", Level::Warn, None));
        assert!(!passes("tingskriver_app::ui", Level::Debug, None));
        assert!(passes(
            "sqlx::query",
            Level::Debug,
            Some(LevelFilter::Debug)
        ));
        assert!(!passes(
            "tingskriver_app",
            Level::Info,
            Some(LevelFilter::Warn)
        ));
    }
}
