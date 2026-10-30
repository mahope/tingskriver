//! Startup: timing against the first-frame budget and the database, which
//! opens in the background while the window already shows.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use tingskriver_core::database::manager::DatabaseManager;

/// The first frame must be on screen within this time after process start
/// on the reference machine (issue #25).
pub const FIRST_FRAME_BUDGET: Duration = Duration::from_millis(500);

/// Measures from process start. `main` creates it first thing; on Windows
/// it also counts the time the loader spent before `main`.
#[derive(Debug, Clone, Copy)]
pub struct StartupClock {
    created: Instant,
    before_main: Duration,
}

impl StartupClock {
    pub fn start() -> Self {
        StartupClock {
            created: Instant::now(),
            before_main: platform::time_since_process_creation().unwrap_or_default(),
        }
    }

    /// Time since the process was created.
    pub fn elapsed(&self) -> Duration {
        self.before_main + self.created.elapsed()
    }

    pub fn before_main(&self) -> Duration {
        self.before_main
    }
}

/// The log line for the first frame; warns when over budget.
pub fn first_frame_report(elapsed: Duration, before_main: Duration) -> (log::Level, String) {
    let level = if elapsed > FIRST_FRAME_BUDGET {
        log::Level::Warn
    } else {
        log::Level::Info
    };
    (
        level,
        format!(
            "startup: first frame {} ms after process start ({} ms before main, budget {} ms)",
            elapsed.as_millis(),
            before_main.as_millis(),
            FIRST_FRAME_BUDGET.as_millis()
        ),
    )
}

#[cfg(windows)]
mod platform {
    use std::time::Duration;
    use windows::Win32::Foundation::FILETIME;
    use windows::Win32::System::SystemInformation::GetSystemTimeAsFileTime;
    use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};

    fn ticks(t: FILETIME) -> u64 {
        (u64::from(t.dwHighDateTime) << 32) | u64::from(t.dwLowDateTime)
    }

    pub fn time_since_process_creation() -> Option<Duration> {
        let (mut created, mut exit, mut kernel, mut user) = Default::default();
        unsafe {
            GetProcessTimes(
                GetCurrentProcess(),
                &mut created,
                &mut exit,
                &mut kernel,
                &mut user,
            )
            .ok()?;
            let now = GetSystemTimeAsFileTime();
            super::filetime_delta(ticks(created), ticks(now))
        }
    }
}

#[cfg(not(windows))]
mod platform {
    pub fn time_since_process_creation() -> Option<std::time::Duration> {
        None
    }
}

/// Difference of two FILETIMEs (100 ns ticks); `None` when the clock went
/// backwards or the result is implausible (over a minute before `main`).
#[cfg_attr(not(windows), allow(dead_code))]
fn filetime_delta(created: u64, now: u64) -> Option<Duration> {
    let delta = Duration::from_nanos(now.checked_sub(created)?.saturating_mul(100));
    (delta < Duration::from_secs(60)).then_some(delta)
}

/// Where the database is in its life.
#[derive(Clone)]
pub enum DbState {
    /// Opening in the background.
    Loading,
    /// No database yet: first launch, before onboarding creates it.
    NotCreated,
    Ready(DatabaseManager),
    Failed(DbFailure),
}

impl DbState {
    pub fn is_loading(&self) -> bool {
        matches!(self, DbState::Loading)
    }
}

/// Why the database did not open, for the error page.
#[derive(Debug, Clone, PartialEq)]
pub struct DbFailure {
    /// Plain Danish explanation.
    pub summary: String,
    /// What to do about it.
    pub advice: String,
    /// The technical error, shown small and in the log.
    pub detail: String,
    pub database_path: PathBuf,
}

impl DbFailure {
    pub fn from_error(detail: &str, database_path: PathBuf) -> Self {
        let lower = detail.to_lowercase();
        let (summary, advice) = if lower.contains("locked") || lower.contains("busy") {
            (
                "Databasen er låst af et andet program.",
                "Luk andre vinduer med Tingskriver (også den gamle app), og prøv igen.",
            )
        } else if lower.contains("not a database")
            || lower.contains("malformed")
            || lower.contains("corrupt")
        {
            (
                "Databasefilen er beskadiget.",
                "Dine optagelser ligger stadig som lydfiler. Send logfilen til udvikleren, før du sletter noget.",
            )
        } else if lower.contains("disk is full")
            || lower.contains("no space")
            || lower.contains("os error 112")
        {
            ("Disken er fuld.", "Frigør plads på disken, og prøv igen.")
        } else if lower.contains("migrat") {
            (
                "Databasen kunne ikke opdateres til den nye version.",
                "Send logfilen til udvikleren. Databasen er ikke ændret.",
            )
        } else if lower.contains("permission")
            || lower.contains("access is denied")
            || lower.contains("os error 5")
        {
            (
                "Tingskriver har ikke adgang til databasefilen.",
                "Tjek at mappen ikke er skrivebeskyttet, og prøv igen.",
            )
        } else {
            (
                "Databasen kunne ikke åbnes.",
                "Prøv igen. Sker det igen, så send logfilen til udvikleren.",
            )
        };
        DbFailure {
            summary: summary.to_string(),
            advice: advice.to_string(),
            detail: detail.to_string(),
            database_path,
        }
    }
}

/// Open the database in `app_data_dir` the way the Tauri app does: no file
/// means first launch (onboarding creates it), otherwise open and migrate.
/// Never panics; errors become [`DbState::Failed`]. Runs on the tokio
/// runtime, never on the UI thread.
pub async fn open_database(app_data_dir: PathBuf) -> DbState {
    let database_path = app_data_dir.join(tingskriver_core::paths::DATABASE_FILE);
    match DatabaseManager::is_first_launch(&app_data_dir).await {
        Ok(true) => return DbState::NotCreated,
        Ok(false) => {}
        Err(error) => {
            return DbState::Failed(DbFailure::from_error(&error.to_string(), database_path))
        }
    }
    match DatabaseManager::new_from_app_data_dir(&app_data_dir).await {
        Ok(db) => DbState::Ready(db),
        Err(error) => {
            log::error!(
                "Database at {} failed to open: {error}",
                database_path.display()
            );
            DbState::Failed(DbFailure::from_error(&error.to_string(), database_path))
        }
    }
}

/// Open a folder in the system file manager.
pub fn open_folder(path: &Path) {
    let program = if cfg!(windows) {
        "explorer.exe"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    if let Err(error) = std::process::Command::new(program).arg(path).spawn() {
        log::warn!("Could not open {}: {error}", path.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "tingskriver-app-startup-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn first_frame_over_budget_is_a_warning() {
        let (level, line) =
            first_frame_report(Duration::from_millis(320), Duration::from_millis(40));
        assert_eq!(level, log::Level::Info);
        assert!(line.contains("first frame 320 ms"), "{line}");
        assert!(line.contains("40 ms before main"), "{line}");
        let (level, _) = first_frame_report(Duration::from_millis(501), Duration::ZERO);
        assert_eq!(level, log::Level::Warn);
    }

    #[test]
    fn filetime_delta_rejects_nonsense() {
        assert_eq!(
            filetime_delta(1_000, 1_000 + 10_000 * 250),
            Some(Duration::from_millis(250))
        );
        assert_eq!(filetime_delta(2_000, 1_000), None);
        assert_eq!(filetime_delta(0, 10_000_000 * 120), None);
    }

    #[test]
    fn failures_are_explained_in_danish() {
        let path = PathBuf::from("meeting_minutes.sqlite");
        let cases = [
            (
                "error returned from database: (code: 5) database is locked",
                "låst",
            ),
            (
                "error returned from database: (code: 26) file is not a database",
                "beskadiget",
            ),
            ("database disk image is malformed", "beskadiget"),
            (
                "error returned from database: (code: 13) database or disk is full",
                "fuld",
            ),
            (
                "while executing migrations: error returned from database",
                "opdateres",
            ),
            ("Permission denied (os error 13)", "adgang"),
            ("something else", "kunne ikke åbnes"),
        ];
        for (detail, word) in cases {
            let failure = DbFailure::from_error(detail, path.clone());
            assert!(
                failure.summary.contains(word),
                "{detail} -> {}",
                failure.summary
            );
            assert_eq!(failure.detail, detail);
        }
    }

    #[tokio::test]
    async fn missing_database_is_first_launch_and_is_not_created() {
        let dir = temp_dir("missing");
        assert!(matches!(
            open_database(dir.clone()).await,
            DbState::NotCreated
        ));
        assert!(!dir.join(tingskriver_core::paths::DATABASE_FILE).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn broken_database_gives_the_error_page_instead_of_a_crash() {
        let dir = temp_dir("broken");
        let db = dir.join(tingskriver_core::paths::DATABASE_FILE);
        std::fs::write(
            &db,
            b"dette er ikke en SQLite-database, men det ligner en ".repeat(200),
        )
        .unwrap();
        let DbState::Failed(failure) = open_database(dir.clone()).await else {
            panic!("a broken file must fail");
        };
        assert_eq!(failure.database_path, db);
        assert!(failure.summary.contains("beskadiget"), "{failure:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn existing_database_opens_and_migrates() {
        let dir = temp_dir("ok");
        std::fs::write(dir.join(tingskriver_core::paths::DATABASE_FILE), b"").unwrap();
        assert!(matches!(
            open_database(dir.clone()).await,
            DbState::Ready(_)
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
