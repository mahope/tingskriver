//! Installer hooks and updates through Velopack (`installer/README.md`).
//!
//! [`handle_installer_events`] runs first thing at startup: the installer
//! starts the app with a hook argument after install, after update and
//! before uninstall, and the app does its part (`tingskriver_core::install`)
//! and exits. [`apply_pending_update`] runs once this process is the only
//! instance and applies an update that was downloaded earlier, so an update
//! takes effect at the next start without ever replacing the files of an
//! instance that is still running (and perhaps recording).
//!
//! [`start_background_checks`] looks for a newer release on GitHub shortly
//! after start and then every few hours, and downloads it in the background
//! (a delta package when one exists). Nothing happens outside an installed
//! copy: `cargo run` and portable copies never update themselves.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

/// Releases and the update feed (`releases.win.json` on each release).
pub const REPO_URL: &str = "https://github.com/mahope/tingskriver";

/// `TINGSKRIVER_UPDATE_FEED`: `off`, a folder with Velopack release files
/// or an `http(s)` URL to such a folder. For tests on a VM without a
/// GitHub release.
pub const FEED_ENV: &str = "TINGSKRIVER_UPDATE_FEED";

/// The first check waits until startup is long done.
pub const FIRST_CHECK_DELAY: Duration = Duration::from_secs(60);

/// Between checks while the app runs (it often runs for days in the tray).
pub const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);

/// After a failed check (offline, GitHub rate limit).
pub const RETRY_DELAY: Duration = Duration::from_secs(60 * 60);

/// Where updates come from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Feed {
    /// GitHub Releases of [`REPO_URL`]; pre-releases only for builds that
    /// are pre-releases themselves (`0.5.1-dev.1`).
    GitHub { prerelease: bool },
    Folder(PathBuf),
    Http(String),
    Off,
}

impl Feed {
    /// The feed for this build, from [`FEED_ENV`] when set.
    pub fn choose(env: Option<&str>, version: &str) -> Feed {
        match env.map(str::trim).filter(|value| !value.is_empty()) {
            None => Feed::GitHub {
                prerelease: is_prerelease(version),
            },
            Some(value) if value.eq_ignore_ascii_case("off") || value == "0" => Feed::Off,
            Some(value) if value.starts_with("http://") || value.starts_with("https://") => {
                Feed::Http(value.to_string())
            }
            Some(value) => Feed::Folder(PathBuf::from(value)),
        }
    }
}

/// `0.5.1-dev.1` is a pre-release, `0.5.1` is not (semver).
pub fn is_prerelease(version: &str) -> bool {
    version.split('+').next().unwrap_or(version).contains('-')
}

/// Is `exe` inside a Velopack installation (`<root>\current\<exe>` next
/// to `<root>\Update.exe`)?
pub fn is_installed_layout(exe: &Path, exists: impl Fn(&Path) -> bool) -> bool {
    let Some(current) = exe.parent() else {
        return false;
    };
    let named_current = current.file_name().is_some_and(|name| name.eq_ignore_ascii_case("current"));
    named_current && current.parent().is_some_and(|root| exists(&root.join("Update.exe")))
}

/// Where the updater is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateStatus {
    /// Not an installed copy, or updates are off.
    Inactive,
    Checking,
    UpToDate,
    /// The feed had no release of this app. On GitHub that also happens when
    /// more than ten newer releases of other kinds (Tauri, pre-releases for
    /// a release build) hide it (`installer/README.md`, "Updates").
    NoReleaseFound,
    Downloading { version: String },
    /// Downloaded; installs at the next start.
    ReadyAtRestart { version: String },
    Failed { detail: String },
}

impl UpdateStatus {
    /// Danish text for a settings or about page.
    pub fn label(&self) -> String {
        match self {
            UpdateStatus::Inactive => "Automatisk opdatering er slået fra for denne kopi.".to_string(),
            UpdateStatus::Checking => "Søger efter opdateringer …".to_string(),
            UpdateStatus::UpToDate => "Du har den nyeste version.".to_string(),
            UpdateStatus::NoReleaseFound => {
                "Fandt ingen udgivelser at opdatere fra. Tingskriver prøver igen senere.".to_string()
            }
            UpdateStatus::Downloading { version } => format!("Henter version {version} …"),
            UpdateStatus::ReadyAtRestart { version } => {
                format!("Version {version} er klar og installeres, næste gang Tingskriver starter.")
            }
            UpdateStatus::Failed { .. } => {
                "Kunne ikke søge efter opdateringer. Tingskriver prøver igen senere.".to_string()
            }
        }
    }

    /// How long until the next check after this result.
    pub fn next_check(&self) -> Option<Duration> {
        match self {
            UpdateStatus::Inactive | UpdateStatus::ReadyAtRestart { .. } => None,
            UpdateStatus::Failed { .. } => Some(RETRY_DELAY),
            _ => Some(CHECK_INTERVAL),
        }
    }
}

static STATUS: Mutex<UpdateStatus> = Mutex::new(UpdateStatus::Inactive);

/// The updater's state now.
pub fn status() -> UpdateStatus {
    STATUS.lock().map(|s| s.clone()).unwrap_or(UpdateStatus::Inactive)
}

#[cfg_attr(not(windows), allow(dead_code))]
fn set_status(status: UpdateStatus) {
    if let Ok(mut current) = STATUS.lock() {
        *current = status;
    }
}

fn installed_exe() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    is_installed_layout(&exe, Path::exists).then_some(exe)
}

/// Is this start a Velopack hook (`--veloapp-install 0.5.1` and so on)?
pub fn is_hook_invocation(args: &[String]) -> bool {
    args.first().is_some_and(|arg| arg.to_ascii_lowercase().starts_with("--veloapp-"))
}

/// Set when Velopack restarted the app right after applying an update.
static RESTARTED_AFTER_UPDATE: AtomicBool = AtomicBool::new(false);

/// Run the installer's hooks. Call first in `main`, after logging: a hook
/// argument makes the process exit here. A downloaded update is not applied
/// here (a second launch must not stop the running instance), but by
/// [`apply_pending_update`].
pub fn handle_installer_events() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let installed = installed_exe();
    // A hook must never fall through to a normal start with a window, even
    // if the layout check is wrong; Velopack itself handles a hook outside
    // an installation by logging and returning.
    if installed.is_none() && !is_hook_invocation(&args) {
        return;
    }
    let exe = installed
        .or_else(|| std::env::current_exe().ok())
        .unwrap_or_default();
    platform::run_velopack_app(exe);
    if is_hook_invocation(&args) {
        log::warn!("installer: hook {args:?} was not handled; exiting");
        std::process::exit(0);
    }
}

/// Should this start apply a downloaded update? Only an installed copy that
/// is the sole instance, and not right after Velopack restarted it, so a
/// failed apply cannot loop.
pub fn should_apply_pending(installed: bool, primary: bool, restarted: bool) -> bool {
    installed && primary && !restarted
}

/// Apply an update downloaded by an earlier run: Velopack's updater waits
/// for this process to exit, installs the package and starts the app again
/// with the same arguments (a `--hidden` start stays hidden). Call only
/// once this process holds the single-instance claim (`primary`), before
/// anything is recorded; returns when there is nothing to apply.
pub fn apply_pending_update(primary: bool) {
    let installed = installed_exe().is_some();
    let restarted = RESTARTED_AFTER_UPDATE.load(Ordering::Relaxed);
    if !should_apply_pending(installed, primary, restarted) {
        if installed && !primary {
            log::info!("update: not the only instance; a downloaded update waits");
        }
        return;
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    platform::apply_pending(&args);
}

/// Look for updates in the background while the app runs.
pub fn start_background_checks() {
    if installed_exe().is_none() {
        log::info!("update: not an installed copy; automatic updates are off");
        return;
    }
    let env = std::env::var(FEED_ENV).ok();
    let feed = Feed::choose(env.as_deref(), env!("CARGO_PKG_VERSION"));
    if feed == Feed::Off {
        log::info!("update: turned off by {FEED_ENV}");
        return;
    }
    log::info!("update: checking {feed:?}");
    let spawned = std::thread::Builder::new()
        .name("tingskriver-update".to_string())
        .spawn(move || {
            let mut delay = FIRST_CHECK_DELAY;
            loop {
                std::thread::sleep(delay);
                let status = platform::check_and_download(&feed);
                log::info!("update: {status:?}");
                let next = status.next_check();
                set_status(status);
                match next {
                    Some(next) => delay = next,
                    None => break,
                }
            }
        });
    if let Err(e) = spawned {
        log::warn!("update: could not start the update thread: {e}");
    }
}

#[cfg(windows)]
mod platform {
    use std::path::PathBuf;

    use std::sync::atomic::Ordering;

    use velopack::sources::{FileSource, GithubSource, HttpSource, NoneSource};
    use velopack::{UpdateCheck, UpdateManager, VelopackApp};

    use super::{set_status, Feed, UpdateStatus, REPO_URL, RESTARTED_AFTER_UPDATE};

    fn log_report(hook: &str, report: Vec<String>) {
        for line in report {
            log::info!("installer {hook}: {line}");
        }
    }

    pub fn run_velopack_app(exe: PathBuf) {
        let (install_exe, update_exe) = (exe.clone(), exe);
        VelopackApp::build()
            // Applied by `apply_pending` once this is the only instance.
            .set_auto_apply_on_startup(false)
            .on_after_install_fast_callback(move |version| {
                log::info!("installer: installed {version}");
                log_report("install", tingskriver_core::install::after_install(&install_exe));
            })
            .on_after_update_fast_callback(move |version| {
                log::info!("installer: updated to {version}");
                log_report("update", tingskriver_core::install::after_update(&update_exe));
            })
            .on_before_uninstall_fast_callback(|version| {
                log::info!("installer: uninstalling {version}");
                log_report("uninstall", tingskriver_core::install::before_uninstall());
            })
            .on_restarted(|version| {
                RESTARTED_AFTER_UPDATE.store(true, Ordering::Relaxed);
                log::info!("installer: restarted after updating to {version}");
            })
            .run();
    }

    pub fn apply_pending(args: &[String]) {
        let manager = match UpdateManager::new(NoneSource {}, None, None) {
            Ok(manager) => manager,
            Err(e) => {
                log::warn!("update: could not look for a downloaded update: {e}");
                return;
            }
        };
        let Some(pending) = manager.get_update_pending_restart() else {
            return;
        };
        log::info!("update: applying {} and restarting", pending.Version);
        // Exits the process when the updater starts.
        if let Err(e) = manager.apply_updates_and_restart_with_args(&pending, args) {
            log::warn!("update: could not apply {}: {e}", pending.Version);
        }
    }

    fn manager(feed: &Feed) -> Result<UpdateManager, velopack::Error> {
        match feed {
            Feed::GitHub { prerelease } => UpdateManager::new(GithubSource::new(REPO_URL, None, *prerelease), None, None),
            Feed::Folder(dir) => UpdateManager::new(FileSource::new(dir), None, None),
            Feed::Http(url) => UpdateManager::new(HttpSource::new(url), None, None),
            Feed::Off => Err(velopack::Error::Other("updates are off".to_string())),
        }
    }

    pub fn check_and_download(feed: &Feed) -> UpdateStatus {
        let failed = |e: velopack::Error| UpdateStatus::Failed { detail: e.to_string() };
        let manager = match manager(feed) {
            Ok(manager) => manager,
            Err(e) => return failed(e),
        };
        if let Some(pending) = manager.get_update_pending_restart() {
            return UpdateStatus::ReadyAtRestart {
                version: pending.Version,
            };
        }
        set_status(UpdateStatus::Checking);
        let update = match manager.check_for_updates() {
            Ok(UpdateCheck::UpdateAvailable(update)) => update,
            Ok(UpdateCheck::NoUpdateAvailable) => return UpdateStatus::UpToDate,
            Ok(UpdateCheck::RemoteIsEmpty) => {
                log::warn!("update: the feed has no release of this app; newer releases of other kinds may hide it");
                return UpdateStatus::NoReleaseFound;
            }
            Err(e) => return failed(e),
        };
        let version = update.TargetFullRelease.Version.clone();
        let kind = if update.DeltasToTarget.is_empty() { "full" } else { "delta" };
        log::info!("update: downloading {version} ({kind})");
        set_status(UpdateStatus::Downloading {
            version: version.clone(),
        });
        match manager.download_updates(&update, None) {
            Ok(()) => UpdateStatus::ReadyAtRestart { version },
            Err(e) => failed(e),
        }
    }
}

#[cfg(not(windows))]
mod platform {
    use super::{Feed, UpdateStatus};

    pub fn run_velopack_app(_exe: std::path::PathBuf) {}

    pub fn apply_pending(_args: &[String]) {}

    pub fn check_and_download(_feed: &Feed) -> UpdateStatus {
        UpdateStatus::Inactive
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dev_builds_follow_pre_releases_and_releases_do_not() {
        assert_eq!(
            Feed::choose(None, "0.5.1-dev.1"),
            Feed::GitHub { prerelease: true }
        );
        assert_eq!(Feed::choose(None, "0.5.1"), Feed::GitHub { prerelease: false });
        assert_eq!(Feed::choose(Some("  "), "0.5.1"), Feed::GitHub { prerelease: false });
        assert!(!is_prerelease("0.5.1+build-7"));
    }

    #[test]
    fn the_feed_can_be_overridden_for_tests() {
        assert_eq!(Feed::choose(Some("off"), "0.5.1"), Feed::Off);
        assert_eq!(Feed::choose(Some("0"), "0.5.1"), Feed::Off);
        assert_eq!(
            Feed::choose(Some(r"C:\feed"), "0.5.1"),
            Feed::Folder(PathBuf::from(r"C:\feed"))
        );
        assert_eq!(
            Feed::choose(Some("http://10.0.0.5:8000/"), "0.5.1"),
            Feed::Http("http://10.0.0.5:8000/".to_string())
        );
    }

    #[test]
    fn only_the_velopack_layout_counts_as_installed() {
        let root = PathBuf::from("root");
        let update_exe = root.join("Update.exe");
        let installed = root.join("current").join("tingskriver-app.exe");
        assert!(is_installed_layout(&installed, |p| p == update_exe));
        assert!(!is_installed_layout(&installed, |_| false));
        let dev = PathBuf::from("target").join("release").join("tingskriver-app.exe");
        assert!(!is_installed_layout(&dev, |_| true));
        assert!(!is_installed_layout(Path::new("tingskriver-app.exe"), |_| true));
    }

    #[test]
    fn hook_arguments_are_recognised() {
        let args = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(is_hook_invocation(&args(&["--veloapp-install", "0.5.1"])));
        assert!(is_hook_invocation(&args(&["--VELOAPP-UNINSTALL", "0.5.1"])));
        assert!(!is_hook_invocation(&args(&["--hidden"])));
        assert!(!is_hook_invocation(&[]));
    }

    #[test]
    fn only_the_sole_installed_instance_applies_an_update() {
        assert!(should_apply_pending(true, true, false));
        // A second launch (Start menu while recording) leaves it alone.
        assert!(!should_apply_pending(true, false, false));
        assert!(!should_apply_pending(false, true, false));
        assert!(!should_apply_pending(true, true, true));
    }

    #[test]
    fn checks_stop_once_an_update_waits_for_restart() {
        let ready = UpdateStatus::ReadyAtRestart {
            version: "0.5.2".to_string(),
        };
        assert_eq!(ready.next_check(), None);
        assert_eq!(UpdateStatus::Inactive.next_check(), None);
        assert_eq!(UpdateStatus::UpToDate.next_check(), Some(CHECK_INTERVAL));
        assert_eq!(UpdateStatus::NoReleaseFound.next_check(), Some(CHECK_INTERVAL));
        assert_ne!(UpdateStatus::NoReleaseFound.label(), UpdateStatus::UpToDate.label());
        assert_eq!(
            UpdateStatus::Failed { detail: "offline".into() }.next_check(),
            Some(RETRY_DELAY)
        );
        assert!(ready.label().contains("0.5.2"));
        assert!(UpdateStatus::Failed { detail: "x".into() }.label().starts_with("Kunne ikke"));
    }
}
