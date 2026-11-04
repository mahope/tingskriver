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
//! after start according to persisted user preferences. Downloads require consent
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

/// Where updates come from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Feed {
    /// GitHub Releases of [`REPO_URL`]; pre-releases only for builds that
    /// are pre-releases themselves (`0.5.1-dev.1`).
    GitHub {
        prerelease: bool,
    },
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
    let named_current = current
        .file_name()
        .is_some_and(|name| name.eq_ignore_ascii_case("current"));
    named_current
        && current
            .parent()
            .is_some_and(|root| exists(&root.join("Update.exe")))
}

use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, OnceLock};
use tingskriver_core::host::AppHost;
pub use tingskriver_core::updates::State as UpdateStatus;
use tingskriver_core::updates::{Effect, Event, Preferences};
use tingskriver_host::Bridge;

static STATUS: Mutex<UpdateStatus> = Mutex::new(UpdateStatus::Inactive);
static BUSY: AtomicBool = AtomicBool::new(false);

pub fn busy() -> bool {
    BUSY.load(Ordering::Relaxed)
}

static BRIDGE: OnceLock<Arc<Bridge>> = OnceLock::new();
static COMMANDS: OnceLock<Sender<Event>> = OnceLock::new();
const STORE: &str = "updates.json";
const KEY: &str = "preferences";

pub fn status() -> UpdateStatus {
    #[cfg(debug_assertions)]
    if let Ok(name) = std::env::var("TINGSKRIVER_MOCK_UPDATE") {
        if let Some(state) = mock_state(&name) {
            return state;
        }
    }
    STATUS.lock().map(|s| s.clone()).unwrap_or_default()
}

fn set_status(status: UpdateStatus) {
    tingskriver_host::status::set_update_available(matches!(
        status,
        UpdateStatus::Available(_) | UpdateStatus::Ready(_)
    ));
    if let Ok(mut current) = STATUS.lock() {
        *current = status;
    }
}

pub fn preferences() -> Result<Preferences, String> {
    let bridge = BRIDGE
        .get()
        .ok_or(tingskriver_core::strings::UPDATE_SETTINGS_UNAVAILABLE)?;
    match bridge.read_setting(STORE, KEY)? {
        Some(value) => serde_json::from_value(value)
            .map_err(|_| tingskriver_core::strings::UPDATE_SETTINGS_INVALID.into()),
        None => Ok(Preferences::default()),
    }
}

static SETTINGS_LOCK: Mutex<()> = Mutex::new(());

pub fn save_preferences(prefs: &Preferences) {
    let _guard = SETTINGS_LOCK.lock().unwrap();
    let result = preferences().and_then(|current| {
        write_preferences(&Preferences {
            last_check: current.last_check,
            ..prefs.clone()
        })
    });
    if result.is_err() {
        set_status(UpdateStatus::Error(
            tingskriver_core::strings::UPDATE_SETTINGS_FAILED.into(),
        ));
    }
}

fn write_preferences(prefs: &Preferences) -> Result<(), String> {
    BRIDGE
        .get()
        .ok_or(tingskriver_core::strings::UPDATE_SETTINGS_MISSING.to_string())?
        .write_setting(
            STORE,
            KEY,
            serde_json::to_value(prefs).map_err(|e| e.to_string())?,
        )
}

fn record_check() -> bool {
    let _guard = SETTINGS_LOCK.lock().unwrap();
    let result = preferences().and_then(|mut prefs| {
        if !prefs.enabled {
            return Ok(false);
        }
        prefs.last_check = Some(now());
        write_preferences(&prefs)?;
        Ok(true)
    });
    match result {
        Ok(allowed) => allowed,
        Err(_) => {
            set_status(UpdateStatus::Error(
                tingskriver_core::strings::UPDATE_TIMESTAMP_FAILED.into(),
            ));
            false
        }
    }
}

pub fn request(event: Event) {
    if let Some(tx) = COMMANDS.get() {
        let _ = tx.send(event);
    }
}

pub fn mock_state(name: &str) -> Option<UpdateStatus> {
    use tingskriver_core::updates::Release;
    let release = Release {
        version: "0.5.1".into(),
        bytes: 41900000,
        notes: "Bedre lyd og tydeligere opdateringer.".into(),
        published: Some("2026-11-04".into()),
    };
    Some(match name {
        "idle" => UpdateStatus::Idle,
        "checking" => UpdateStatus::Checking,
        "available" => UpdateStatus::Available(release),
        "downloading" => UpdateStatus::Downloading(release, 42),
        "ready" => UpdateStatus::Ready(release),
        "error" => {
            UpdateStatus::Error("Ingen forbindelse. Kontrollér netværket, og prøv igen.".into())
        }
        "blocked" => UpdateStatus::Blocked(release, false),
        _ => return None,
    })
}

fn installed_exe() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    is_installed_layout(&exe, Path::exists).then_some(exe)
}

/// Is this start a Velopack hook (`--veloapp-install 0.5.1` and so on)?
pub fn is_hook_invocation(args: &[String]) -> bool {
    args.first()
        .is_some_and(|arg| arg.to_ascii_lowercase().starts_with("--veloapp-"))
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
pub fn start_background_checks(bridge: Arc<Bridge>, runtime: tokio::runtime::Handle) {
    let _ = BRIDGE.set(bridge);
    runtime.spawn(async {
        loop {
            BUSY.store(
                tingskriver_core::audio::recording_commands::is_recording().await
                    || tingskriver_core::dictation::is_dictating(),
                Ordering::Relaxed,
            );
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    });
    if installed_exe().is_none() || !cfg!(windows) {
        return;
    }
    let feed = Feed::choose(
        std::env::var(FEED_ENV).ok().as_deref(),
        crate::about::VERSION,
    );
    if feed == Feed::Off {
        return;
    }
    set_status(UpdateStatus::Idle);
    let (tx, rx) = mpsc::channel();
    let _ = COMMANDS.set(tx);
    let spawned = std::thread::Builder::new()
        .name("tingskriver-update".into())
        .spawn(move || {
            let mut first = true;
            loop {
                let event = match rx.recv_timeout(if first {
                    FIRST_CHECK_DELAY
                } else {
                    Duration::from_secs(60)
                }) {
                    Ok(event) => event,
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        first = false;
                        match preferences() {
                            Ok(prefs) if prefs.due(now()) => Event::Check,
                            Ok(_) => continue,
                            Err(message) => {
                                set_status(UpdateStatus::Error(message));
                                continue;
                            }
                        }
                    }
                    Err(_) => break,
                };
                let prefs = match preferences() {
                    Ok(prefs) => prefs,
                    Err(message) => {
                        set_status(UpdateStatus::Error(message));
                        continue;
                    }
                };
                let busy = runtime
                    .block_on(tingskriver_core::audio::recording_commands::is_recording())
                    || tingskriver_core::dictation::is_dictating();
                let (next, effect) =
                    tingskriver_core::updates::transition(&status(), event, prefs.enabled, busy);
                set_status(next);
                match effect {
                    Effect::Check => {
                        if record_check() {
                            set_status(platform::check(&feed));
                        }
                    }
                    Effect::Download => set_status(platform::download(&feed)),
                    Effect::Restart => {
                        // Block new meeting starts under core's lifecycle lock.
                        if runtime.block_on(
                            tingskriver_core::audio::recording_commands::block_recordings_if_idle(),
                        ) {
                            if let Some(_guard) =
                                tingskriver_core::dictation::service::block_for_update()
                            {
                                platform::apply_pending(
                                    &std::env::args().skip(1).collect::<Vec<_>>(),
                                );
                            } else {
                                if let Some(r) = status().release() {
                                    set_status(UpdateStatus::Blocked(r.clone(), true));
                                }
                            }
                            tingskriver_core::audio::recording_commands::unblock_recordings();
                        } else {
                            if let Some(r) = status().release() {
                                set_status(UpdateStatus::Blocked(r.clone(), true));
                            }
                        }
                    }
                    Effect::None => {}
                }
            }
        });
    if spawned.is_err() {
        set_status(UpdateStatus::Error(
            tingskriver_core::strings::UPDATE_START_FAILED.into(),
        ));
    }
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(windows)]
mod platform {
    use std::path::PathBuf;

    use std::sync::atomic::Ordering;

    use velopack::sources::{FileSource, GithubSource, HttpSource, NoneSource, UpdateSource};
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
                log_report(
                    "install",
                    tingskriver_core::install::after_install(&install_exe),
                );
            })
            .on_after_update_fast_callback(move |version| {
                log::info!("installer: updated to {version}");
                log_report(
                    "update",
                    tingskriver_core::install::after_update(&update_exe),
                );
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
            set_status(failed(e));
        }
    }

    #[derive(Clone)]
    struct PrivateHttpSource {
        inner: HttpSource,
        url: String,
    }

    impl UpdateSource for PrivateHttpSource {
        fn get_release_feed(
            &self,
            channel: &str,
            _app: &velopack::bundle::Manifest,
            _staged_user_id: &str,
        ) -> Result<velopack::VelopackAssetFeed, velopack::Error> {
            // The standard HTTP source adds a persistent installation identifier.
            let url = format!("{}/releases.{channel}.json", self.url.trim_end_matches('/'));
            let json = velopack::download::download_url_as_string(&url, None)?;
            Ok(serde_json::from_str(&json)?)
        }

        fn download_release_entry(
            &self,
            asset: &velopack::VelopackAsset,
            path: &std::path::Path,
            progress: Option<std::sync::mpsc::Sender<i16>>,
        ) -> Result<(), velopack::Error> {
            self.inner.download_release_entry(asset, path, progress)
        }
    }

    fn manager(feed: &Feed) -> Result<UpdateManager, velopack::Error> {
        match feed {
            Feed::GitHub { prerelease } => {
                UpdateManager::new(GithubSource::new(REPO_URL, None, *prerelease), None, None)
            }
            Feed::Folder(dir) => UpdateManager::new(FileSource::new(dir), None, None),
            Feed::Http(url) => UpdateManager::new(
                PrivateHttpSource {
                    inner: HttpSource::new(url),
                    url: url.clone(),
                },
                None,
                None,
            ),
            Feed::Off => Err(velopack::Error::Other("updates are off".to_string())),
        }
    }

    static PENDING: std::sync::Mutex<Option<velopack::UpdateInfo>> = std::sync::Mutex::new(None);

    fn failed(error: velopack::Error) -> UpdateStatus {
        // Do not put feed URLs, paths or response bodies into the UI.
        let detail = match &error {
            velopack::Error::Network(_) => tingskriver_core::strings::UPDATE_NETWORK_FAILED,
            velopack::Error::Io(e) if matches!(e.raw_os_error(), Some(112 | 28)) => {
                tingskriver_core::strings::UPDATE_DISK_FULL
            }
            velopack::Error::Io(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                tingskriver_core::strings::UPDATE_ACCESS_DENIED
            }
            velopack::Error::SizeInvalid(..) | velopack::Error::ChecksumInvalid(..) => {
                tingskriver_core::strings::UPDATE_VERIFY_FAILED
            }
            _ => tingskriver_core::strings::UPDATE_FAILED,
        };
        UpdateStatus::Error(detail.into())
    }

    fn release(asset: &velopack::VelopackAsset) -> tingskriver_core::updates::Release {
        let (notes, published) = tingskriver_core::updates::release_notes(&asset.NotesMarkdown);
        tingskriver_core::updates::Release {
            version: asset.Version.clone(),
            bytes: asset.Size,
            notes,
            published,
        }
    }

    pub fn check(feed: &Feed) -> UpdateStatus {
        let manager = match manager(feed) {
            Ok(m) => m,
            Err(e) => return failed(e),
        };
        if let Some(pending) = manager.get_update_pending_restart() {
            return UpdateStatus::Ready(release(&pending));
        }
        match manager.check_for_updates() {
            Ok(UpdateCheck::UpdateAvailable(update)) => {
                let state = UpdateStatus::Available(release(&update.TargetFullRelease));
                *PENDING.lock().unwrap() = Some(update);
                state
            }
            Ok(UpdateCheck::NoUpdateAvailable) => UpdateStatus::Idle,
            Ok(UpdateCheck::RemoteIsEmpty) => {
                UpdateStatus::Error(tingskriver_core::strings::UPDATE_FEED_EMPTY.into())
            }
            Err(e) => failed(e),
        }
    }

    pub fn download(feed: &Feed) -> UpdateStatus {
        let manager = match manager(feed) {
            Ok(m) => m,
            Err(e) => return failed(e),
        };
        let pending = PENDING.lock().unwrap().clone();
        let Some(update) = pending else {
            return UpdateStatus::Error(tingskriver_core::strings::UPDATE_RECHECK.into());
        };
        let info = release(&update.TargetFullRelease);
        let (tx, rx) = std::sync::mpsc::channel();
        // Velopack sends download progress from its synchronous operation.
        std::thread::scope(|scope| {
            let progress_info = info.clone();
            scope.spawn(move || {
                for percent in rx {
                    set_status(UpdateStatus::Downloading(
                        progress_info.clone(),
                        percent.clamp(0i16, 100) as u8,
                    ));
                }
            });
            match manager.download_updates(&update, Some(tx)) {
                Ok(()) => UpdateStatus::Ready(info),
                Err(e) => failed(e),
            }
        })
    }
}

#[cfg(not(windows))]
mod platform {
    use super::{Feed, UpdateStatus};

    pub fn run_velopack_app(_exe: std::path::PathBuf) {}

    pub fn apply_pending(_args: &[String]) {}

    pub fn check(_feed: &Feed) -> UpdateStatus {
        UpdateStatus::Inactive
    }
    pub fn download(feed: &Feed) -> UpdateStatus {
        check(feed)
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
        assert_eq!(
            Feed::choose(None, "0.5.1"),
            Feed::GitHub { prerelease: false }
        );
        assert_eq!(
            Feed::choose(Some("  "), "0.5.1"),
            Feed::GitHub { prerelease: false }
        );
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
        let dev = PathBuf::from("target")
            .join("release")
            .join("tingskriver-app.exe");
        assert!(!is_installed_layout(&dev, |_| true));
        assert!(!is_installed_layout(
            Path::new("tingskriver-app.exe"),
            |_| true
        ));
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
}
