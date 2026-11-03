//! Move from the Tauri app to the GPUI app (`installer/README.md`, "Move
//! from Tauri", step 1). Windows release builds only.
//!
//! At startup the Tauri app installed by NSIS in `%LOCALAPPDATA%\Tingskriver`
//! downloads `Tingskriver-Setup.exe` from the newest `app-v*` release, runs
//! it with `--silent` and waits for the new app's uninstall entry
//! `TingskriverApp`. The NSIS uninstaller kills every running
//! `tingskriver.exe`, so the rest is done by a copy of this exe under another
//! name ([`HELPER_EXE`], started with [`HELPER_ARG`]) once the Tauri app has
//! exited: it runs `uninstall.exe /S`, installs the GPUI app once more if the
//! uninstaller took the `Tingskriver.lnk` shortcuts, puts back the
//! start-with-Windows value (the NSIS uninstaller deletes `Run\Tingskriver`,
//! which the GPUI app uses too) and starts the GPUI app.
//!
//! On any failure the Tauri app keeps running as before and tries again at
//! the next start. A recording is never cut off: the move only starts when
//! the app is idle (no recording, no audio being encoded, no retranscription
//! or import) and waits until it is idle again before the app exits. User
//! data is shared by the two apps and is not touched.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

/// Set (to anything) to keep the Tauri app.
pub const NO_MIGRATE_ENV: &str = "TINGSKRIVER_NO_MIGRATE";

/// The newest releases of the repo, pre-releases included.
pub const RELEASES_API: &str =
    "https://api.github.com/repos/mahope/tingskriver/releases?per_page=30";

/// Release assets must come from the repo's own releases.
pub const DOWNLOAD_PREFIX: &str = "https://github.com/mahope/tingskriver/releases/download/";

/// Tags of the GPUI app's Velopack releases (`app-v0.5.0-dev.3`).
pub const APP_TAG_PREFIX: &str = "app-v";

/// Velopack's installer on each GPUI release.
pub const SETUP_ASSET: &str = "Tingskriver-Setup.exe";

/// The GPUI app's uninstall entry under `HKCU\...\Uninstall`.
pub const GPUI_UNINSTALL_ENTRY: &str = "TingskriverApp";

/// The argument that makes the exe finish the move instead of starting the app.
pub const HELPER_ARG: &str = "--finish-move-to-gpui";

/// The copy of this exe that finishes the move. Any name but `tingskriver.exe`.
pub const HELPER_EXE: &str = "tingskriver-move.exe";

/// Name of the shortcuts both installers create.
pub const SHORTCUT_NAME: &str = "Tingskriver.lnk";

/// Let startup finish (and an update restart settle) before the move starts.
pub const START_DELAY: Duration = Duration::from_secs(30);

/// How long the installer and the uninstaller may take.
pub const INSTALL_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// How often to look for the uninstall entry while the installer runs.
pub const POLL_INTERVAL: Duration = Duration::from_secs(2);

/// How often to look again while the app is busy.
pub const BUSY_POLL_INTERVAL: Duration = Duration::from_secs(30);

/// Where the two apps live for the current user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MovePaths {
    /// The NSIS installation of the Tauri app.
    pub tauri_dir: PathBuf,
    /// The NSIS uninstaller.
    pub uninstaller: PathBuf,
    /// The installed GPUI app.
    pub gpui_exe: PathBuf,
    /// `Tingskriver.lnk` in the Start menu and on the Desktop.
    pub shortcuts: Vec<PathBuf>,
}

impl MovePaths {
    /// `local` is `%LOCALAPPDATA%`, `roaming` `%APPDATA%`.
    pub fn from_roots(local: &Path, roaming: &Path, desktop: &Path) -> Self {
        let tauri_dir = local.join("Tingskriver");
        let start_menu = roaming
            .join("Microsoft")
            .join("Windows")
            .join("Start Menu")
            .join("Programs");
        MovePaths {
            uninstaller: tauri_dir.join("uninstall.exe"),
            tauri_dir,
            gpui_exe: local
                .join("TingskriverApp")
                .join("current")
                .join("tingskriver-app.exe"),
            shortcuts: vec![start_menu.join(SHORTCUT_NAME), desktop.join(SHORTCUT_NAME)],
        }
    }

    pub fn for_current_user() -> Option<Self> {
        Some(Self::from_roots(
            &dirs::data_local_dir()?,
            &dirs::data_dir()?,
            &dirs::desktop_dir()?,
        ))
    }

    /// The shortcuts that are not there.
    pub fn missing_shortcuts(&self, exists: impl Fn(&Path) -> bool) -> Vec<PathBuf> {
        self.shortcuts
            .iter()
            .filter(|path| !exists(path))
            .cloned()
            .collect()
    }
}

/// Windows paths compare without regard to case or slash direction.
fn normalized(path: &Path) -> String {
    path.to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_lowercase()
}

/// Is `exe` inside `dir` (any depth)?
pub fn is_inside(exe: &Path, dir: &Path) -> bool {
    let (exe, dir) = (normalized(exe), normalized(dir));
    !dir.is_empty()
        && exe.len() > dir.len() + 1
        && exe.starts_with(&dir)
        && exe[dir.len()..].starts_with('\\')
}

/// Why the move does not run now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Skip {
    /// Not the NSIS installation (a dev build, a portable copy, the GPUI app).
    NotInstalledByNsis,
    /// [`NO_MIGRATE_ENV`] is set.
    TurnedOff,
    /// A recording or its processing runs; the move waits for the next start.
    Busy,
}

/// Should this start move the installation? `exe` is the running exe.
pub fn should_move(exe: &Path, tauri_dir: &Path, turned_off: bool, busy: bool) -> Result<(), Skip> {
    if !is_inside(exe, tauri_dir) {
        return Err(Skip::NotInstalledByNsis);
    }
    if turned_off {
        return Err(Skip::TurnedOff);
    }
    if busy {
        return Err(Skip::Busy);
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
struct Release {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    assets: Vec<Asset>,
}

#[derive(Debug, Deserialize)]
struct Asset {
    name: String,
    size: u64,
    browser_download_url: String,
}

/// The installer to download.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetupDownload {
    pub tag: String,
    pub url: String,
    /// Bytes, as GitHub reports them.
    pub size: u64,
}

/// The `Tingskriver-Setup.exe` of the highest `app-v*` version in a GitHub
/// `releases` response. Drafts, other tags and releases without a usable
/// installer are skipped; pre-releases count.
pub fn newest_setup(releases_json: &str) -> Result<SetupDownload, String> {
    let releases: Vec<Release> = serde_json::from_str(releases_json)
        .map_err(|e| format!("unexpected releases response: {e}"))?;
    releases
        .into_iter()
        .filter(|release| !release.draft)
        .filter_map(|release| {
            let version =
                semver::Version::parse(release.tag_name.strip_prefix(APP_TAG_PREFIX)?).ok()?;
            let asset = release.assets.into_iter().find(|asset| {
                asset.name == SETUP_ASSET
                    && asset.size > 0
                    && asset.browser_download_url.starts_with(DOWNLOAD_PREFIX)
            })?;
            Some((version, release.tag_name, asset))
        })
        .max_by(|a, b| a.0.cmp(&b.0))
        .map(|(_, tag, asset)| SetupDownload {
            tag,
            url: asset.browser_download_url,
            size: asset.size,
        })
        .ok_or_else(|| {
            format!("no {APP_TAG_PREFIX}* release with {SETUP_ASSET} among the newest releases")
        })
}

/// Is a downloaded file of `actual` bytes the asset of `expected` bytes?
pub fn download_is_complete(actual: u64, expected: u64) -> bool {
    actual > 0 && actual == expected
}

/// The parent process id after [`HELPER_ARG`], when this process is the helper.
pub fn helper_parent_pid(args: &[String]) -> Option<Option<u32>> {
    let position = args.iter().position(|arg| arg == HELPER_ARG)?;
    Some(args.get(position + 1).and_then(|pid| pid.parse().ok()))
}

/// Run the helper when the exe was started as one. Returns `true` when it
/// did, and the process should exit instead of starting the app.
pub fn run_helper_if_requested() -> bool {
    let args: Vec<String> = std::env::args().collect();
    let Some(parent) = helper_parent_pid(&args) else {
        return false;
    };
    #[cfg(windows)]
    platform::finish(parent);
    #[cfg(not(windows))]
    let _ = parent;
    true
}

/// Start the move in the background, if this start should move. `exit`
/// quits the app once the helper runs.
#[cfg(windows)]
pub fn start(is_busy: fn() -> bool, exit: impl FnOnce() + Send + 'static) {
    let spawned = std::thread::Builder::new().name("move-to-gpui".into()).spawn(move || {
        std::thread::sleep(START_DELAY);
        match platform::prepare(is_busy) {
            Ok(()) => {
                log::info!("move to GPUI: the helper takes over, exiting the Tauri app");
                exit();
            }
            Err(Some(e)) => log::error!("move to GPUI failed, keeping the Tauri app and trying again at next start: {e}"),
            Err(None) => {}
        }
    });
    if let Err(e) = spawned {
        log::error!("move to GPUI: could not start the thread: {e}");
    }
}

#[cfg(windows)]
mod platform {
    use std::io::Write;
    use std::os::windows::process::CommandExt;
    use std::path::PathBuf;
    use std::process::{Child, Command};
    use std::time::{Duration, Instant};

    use tingskriver_core::install::{autostart, uninstall_entry_exists};

    use super::*;

    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    /// How long the helper waits for the Tauri app to exit.
    const PARENT_EXIT_TIMEOUT: Duration = Duration::from_secs(60);

    fn work_dir() -> PathBuf {
        std::env::temp_dir().join("tingskriver-move")
    }

    fn setup_path() -> PathBuf {
        work_dir().join(SETUP_ASSET)
    }

    /// In the Tauri app: install the GPUI app and start the helper.
    /// `Err(None)`: nothing to do at this start.
    pub fn prepare(is_busy: fn() -> bool) -> Result<(), Option<String>> {
        let exe = std::env::current_exe().map_err(|e| Some(format!("no exe path: {e}")))?;
        let paths =
            MovePaths::for_current_user().ok_or_else(|| Some("no user folders".to_string()))?;
        let turned_off = std::env::var_os(NO_MIGRATE_ENV).is_some();
        match should_move(&exe, &paths.tauri_dir, turned_off, is_busy()) {
            Ok(()) => {}
            Err(Skip::NotInstalledByNsis) => return Err(None),
            Err(skip) => {
                log::info!("move to GPUI: not now ({skip:?})");
                return Err(None);
            }
        }
        log::info!("move to GPUI: starting ({})", exe.display());

        download_setup().map_err(Some)?;
        if uninstall_entry_exists(GPUI_UNINSTALL_ENTRY) {
            log::info!("move to GPUI: the GPUI app is already installed");
        } else {
            install_gpui().map_err(Some)?;
        }
        if !paths.gpui_exe.exists() {
            return Err(Some(format!(
                "{} is missing after install",
                paths.gpui_exe.display()
            )));
        }

        let helper = work_dir().join(HELPER_EXE);
        std::fs::copy(&exe, &helper).map_err(|e| {
            Some(format!(
                "copy {} to {}: {e}",
                exe.display(),
                helper.display()
            ))
        })?;

        // Last, so the gap between this check and the exit stays short.
        let mut waited = false;
        while is_busy() {
            if !waited {
                log::info!("move to GPUI: waiting for the recording and its processing to finish");
                waited = true;
            }
            std::thread::sleep(BUSY_POLL_INTERVAL);
        }
        Command::new(&helper)
            .arg(HELPER_ARG)
            .arg(std::process::id().to_string())
            .current_dir(work_dir())
            .creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP)
            .spawn()
            .map_err(|e| Some(format!("start {}: {e}", helper.display())))?;
        Ok(())
    }

    fn download_setup() -> Result<(), String> {
        let client = reqwest::blocking::Client::builder()
            .user_agent(concat!("Tingskriver/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(30))
            .timeout(Duration::from_secs(15 * 60))
            .build()
            .map_err(|e| format!("http client: {e}"))?;
        let releases = client
            .get(RELEASES_API)
            .header("Accept", "application/vnd.github+json")
            .send()
            .and_then(|response| response.error_for_status())
            .and_then(|response| response.text())
            .map_err(|e| format!("GitHub releases: {e}"))?;
        let setup = newest_setup(&releases)?;
        log::info!(
            "move to GPUI: downloading {} ({} bytes) from {}",
            SETUP_ASSET,
            setup.size,
            setup.tag
        );

        std::fs::create_dir_all(work_dir())
            .map_err(|e| format!("create {}: {e}", work_dir().display()))?;
        let path = setup_path();
        let mut response = client
            .get(&setup.url)
            .send()
            .and_then(|response| response.error_for_status())
            .map_err(|e| format!("download {}: {e}", setup.url))?;
        let mut file =
            std::fs::File::create(&path).map_err(|e| format!("create {}: {e}", path.display()))?;
        let written = response
            .copy_to(&mut file)
            .map_err(|e| format!("download {}: {e}", setup.url))?;
        file.flush()
            .and_then(|()| file.sync_all())
            .map_err(|e| format!("write {}: {e}", path.display()))?;
        drop(file);
        let on_disk = std::fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
        if !download_is_complete(written, setup.size) || !download_is_complete(on_disk, setup.size)
        {
            let _ = std::fs::remove_file(&path);
            return Err(format!(
                "{SETUP_ASSET} has {on_disk} bytes ({written} received), expected {}",
                setup.size
            ));
        }
        Ok(())
    }

    /// Wait for `child`; kill it after `timeout`.
    fn wait(
        mut child: Child,
        what: &str,
        timeout: Duration,
    ) -> Result<std::process::ExitStatus, String> {
        let deadline = Instant::now() + timeout;
        loop {
            match child.try_wait() {
                Ok(Some(status)) => return Ok(status),
                Ok(None) if Instant::now() >= deadline => {
                    let _ = child.kill();
                    return Err(format!(
                        "{what} did not finish within {} s",
                        timeout.as_secs()
                    ));
                }
                Ok(None) => std::thread::sleep(POLL_INTERVAL),
                Err(e) => return Err(format!("{what}: {e}")),
            }
        }
    }

    /// Run `Tingskriver-Setup.exe --silent` and wait for the uninstall
    /// entry, which Velopack writes last.
    fn install_gpui() -> Result<(), String> {
        let setup = setup_path();
        log::info!("move to GPUI: running {} --silent", setup.display());
        let mut child = Command::new(&setup)
            .arg("--silent")
            .spawn()
            .map_err(|e| format!("start {}: {e}", setup.display()))?;
        let deadline = Instant::now() + INSTALL_TIMEOUT;
        loop {
            if uninstall_entry_exists(GPUI_UNINSTALL_ENTRY) {
                // Let the installer finish before anything else runs.
                let status = wait(child, SETUP_ASSET, INSTALL_TIMEOUT)?;
                log::info!("move to GPUI: GPUI app installed ({status})");
                return Ok(());
            }
            if let Ok(Some(status)) = child.try_wait() {
                if !uninstall_entry_exists(GPUI_UNINSTALL_ENTRY) {
                    return Err(format!(
                        "{SETUP_ASSET} exited ({status}) without installing the GPUI app"
                    ));
                }
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                return Err(format!(
                    "no {GPUI_UNINSTALL_ENTRY} uninstall entry after {} s",
                    INSTALL_TIMEOUT.as_secs()
                ));
            }
            std::thread::sleep(POLL_INTERVAL);
        }
    }

    /// Wait for the Tauri app to exit, so the uninstaller does not have to
    /// kill it.
    fn wait_for_exit(pid: u32) {
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Threading::{
            OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE,
        };
        unsafe {
            if let Ok(process) = OpenProcess(PROCESS_SYNCHRONIZE, false, pid) {
                WaitForSingleObject(process, PARENT_EXIT_TIMEOUT.as_millis() as u32);
                let _ = CloseHandle(process);
            }
        }
    }

    /// In the helper: uninstall the Tauri app, repair what its uninstaller
    /// removed and start the GPUI app.
    pub fn finish(parent: Option<u32>) {
        if let Some(pid) = parent {
            wait_for_exit(pid);
        }
        crate::file_logger::init();
        log::info!("move to GPUI: helper running");
        let Some(paths) = MovePaths::for_current_user() else {
            log::error!("move to GPUI: no user folders, the Tauri app stays");
            return;
        };

        match uninstall_tauri(&paths) {
            Ok(()) => log::info!("move to GPUI: Tauri app uninstalled"),
            Err(e) => log::error!("move to GPUI: {e}; the Tauri app may still be installed and retries at its next start"),
        }

        let missing = paths.missing_shortcuts(|path| path.exists());
        if !missing.is_empty() {
            log::warn!("move to GPUI: shortcuts gone after uninstall: {missing:?}; installing the GPUI app once more");
            match Command::new(setup_path()).arg("--silent").spawn() {
                Ok(child) => match wait(child, SETUP_ASSET, INSTALL_TIMEOUT) {
                    Ok(status) => log::info!(
                        "move to GPUI: reinstalled ({status}), still missing: {:?}",
                        paths.missing_shortcuts(|path| path.exists())
                    ),
                    Err(e) => log::error!("move to GPUI: reinstall: {e}"),
                },
                Err(e) => log::error!("move to GPUI: start {}: {e}", setup_path().display()),
            }
        }

        // The NSIS uninstaller removes `Run\Tingskriver`, which the GPUI
        // installer had just pointed at the GPUI app.
        if autostart::current().is_none() {
            match autostart::enable(&paths.gpui_exe) {
                Ok(()) => log::info!(
                    "move to GPUI: start with Windows: {}",
                    autostart::run_command(&paths.gpui_exe)
                ),
                Err(e) => log::error!("move to GPUI: could not restore start with Windows: {e}"),
            }
        }

        match Command::new(&paths.gpui_exe)
            .creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP)
            .spawn()
        {
            Ok(_) => log::info!("move to GPUI: started {}", paths.gpui_exe.display()),
            Err(e) => log::error!("move to GPUI: start {}: {e}", paths.gpui_exe.display()),
        }

        remove_work_dir_later();
    }

    fn uninstall_tauri(paths: &MovePaths) -> Result<(), String> {
        if !paths.uninstaller.exists() {
            return Err(format!("{} is missing", paths.uninstaller.display()));
        }
        // `_?=` runs the uninstaller in place, so it can be waited for
        // (without it, NSIS copies itself to %TEMP% and returns at once).
        let child = Command::new(&paths.uninstaller)
            .arg("/S")
            .raw_arg(format!("_?={}", paths.tauri_dir.display()))
            .spawn()
            .map_err(|e| format!("start {}: {e}", paths.uninstaller.display()))?;
        let status = wait(child, "the NSIS uninstaller", INSTALL_TIMEOUT)?;
        if !status.success() {
            return Err(format!("the NSIS uninstaller failed ({status})"));
        }
        // In place, it cannot delete itself.
        let _ = std::fs::remove_file(&paths.uninstaller);
        if let Err(e) = std::fs::remove_dir(&paths.tauri_dir) {
            log::info!(
                "move to GPUI: {} not removed: {e}",
                paths.tauri_dir.display()
            );
        }
        Ok(())
    }

    /// The helper cannot delete its own exe; a short-lived `cmd` does it
    /// after the helper has exited.
    fn remove_work_dir_later() {
        let dir = work_dir();
        let spawned = Command::new("cmd.exe")
            .raw_arg(format!(
                "/d /c ping -n 6 127.0.0.1 >nul & rmdir /s /q \"{}\"",
                dir.display()
            ))
            .creation_flags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP)
            .spawn();
        if let Err(e) = spawned {
            log::info!("move to GPUI: {} stays: {e}", dir.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths() -> MovePaths {
        MovePaths::from_roots(
            Path::new(r"C:\Users\Anne Sofie\AppData\Local"),
            Path::new(r"C:\Users\Anne Sofie\AppData\Roaming"),
            Path::new(r"C:\Users\Anne Sofie\Desktop"),
        )
    }

    #[test]
    fn paths_follow_the_two_installers() {
        let paths = paths();
        let shown = |path: &Path| normalized(path);
        assert_eq!(
            shown(&paths.tauri_dir),
            r"c:\users\anne sofie\appdata\local\tingskriver"
        );
        assert_eq!(
            shown(&paths.uninstaller),
            r"c:\users\anne sofie\appdata\local\tingskriver\uninstall.exe"
        );
        assert_eq!(
            shown(&paths.gpui_exe),
            r"c:\users\anne sofie\appdata\local\tingskriverapp\current\tingskriver-app.exe"
        );
        let shortcuts: Vec<String> = paths.shortcuts.iter().map(|p| shown(p)).collect();
        assert_eq!(
            shortcuts,
            [
                r"c:\users\anne sofie\appdata\roaming\microsoft\windows\start menu\programs\tingskriver.lnk",
                r"c:\users\anne sofie\desktop\tingskriver.lnk",
            ]
        );
    }

    #[test]
    fn only_the_nsis_installation_moves() {
        let dir = Path::new(r"C:\Users\a\AppData\Local\Tingskriver");
        assert!(is_inside(
            Path::new(r"C:\Users\a\AppData\Local\Tingskriver\tingskriver.exe"),
            dir
        ));
        assert!(is_inside(
            Path::new(r"c:\users\A\appdata\local\TINGSKRIVER\tingskriver.exe"),
            dir
        ));
        assert!(is_inside(
            Path::new("C:/Users/a/AppData/Local/Tingskriver/tingskriver.exe"),
            dir
        ));
        assert!(is_inside(
            Path::new(r"C:\Users\a\AppData\Local\Tingskriver\tingskriver.exe"),
            Path::new(r"C:\Users\a\AppData\Local\Tingskriver\")
        ));
        // The GPUI app's folder starts with the same letters.
        assert!(!is_inside(
            Path::new(r"C:\Users\a\AppData\Local\TingskriverApp\current\tingskriver-app.exe"),
            dir
        ));
        assert!(!is_inside(
            Path::new(r"C:\Projects\tingskriver\target\release\tingskriver.exe"),
            dir
        ));
        assert!(!is_inside(dir, dir));
        assert!(!is_inside(Path::new(r"C:\x.exe"), Path::new("")));
    }

    #[test]
    fn the_move_is_skipped_when_off_busy_or_elsewhere() {
        let dir = Path::new(r"C:\Users\a\AppData\Local\Tingskriver");
        let exe = Path::new(r"C:\Users\a\AppData\Local\Tingskriver\tingskriver.exe");
        assert_eq!(should_move(exe, dir, false, false), Ok(()));
        assert_eq!(should_move(exe, dir, true, false), Err(Skip::TurnedOff));
        assert_eq!(should_move(exe, dir, false, true), Err(Skip::Busy));
        let dev = Path::new(r"C:\src\tingskriver\target\release\tingskriver.exe");
        assert_eq!(
            should_move(dev, dir, false, false),
            Err(Skip::NotInstalledByNsis)
        );
        // Outside the installation nothing is logged, so that check comes first.
        assert_eq!(
            should_move(dev, dir, true, true),
            Err(Skip::NotInstalledByNsis)
        );
    }

    fn release(
        tag: &str,
        draft: bool,
        prerelease: bool,
        assets: &[(&str, u64)],
    ) -> serde_json::Value {
        serde_json::json!({
            "tag_name": tag,
            "draft": draft,
            "prerelease": prerelease,
            "assets": assets.iter().map(|(name, size)| serde_json::json!({
                "name": name,
                "size": size,
                "browser_download_url": format!("{DOWNLOAD_PREFIX}{tag}/{name}"),
            })).collect::<Vec<_>>(),
        })
    }

    #[test]
    fn the_newest_app_release_with_an_installer_wins() {
        let json = serde_json::json!([
            release(
                "v0.5.0-dev.3",
                false,
                false,
                &[("Tingskriver_0.5.0-dev.3_x64-setup.exe", 9)]
            ),
            release("app-v0.5.0-dev.4", true, true, &[(SETUP_ASSET, 4)]),
            release(
                "app-v0.5.0-dev.3",
                false,
                true,
                &[("releases.win.json", 1), (SETUP_ASSET, 3)]
            ),
            release(
                "app-v0.5.0-dev.10",
                false,
                true,
                &[("releases.win.json", 1)]
            ),
            release("app-v0.5.0-dev.2", false, true, &[(SETUP_ASSET, 2)]),
            release("app-vnext", false, true, &[(SETUP_ASSET, 5)]),
        ])
        .to_string();
        assert_eq!(
            newest_setup(&json),
            Ok(SetupDownload {
                tag: "app-v0.5.0-dev.3".into(),
                url: format!("{DOWNLOAD_PREFIX}app-v0.5.0-dev.3/{SETUP_ASSET}"),
                size: 3,
            })
        );
    }

    #[test]
    fn versions_compare_as_semver_not_as_api_order() {
        let json = serde_json::json!([
            release("app-v0.5.0-dev.9", false, true, &[(SETUP_ASSET, 9)]),
            release("app-v0.5.1", false, false, &[(SETUP_ASSET, 51)]),
            release("app-v0.5.0-dev.10", false, true, &[(SETUP_ASSET, 10)]),
        ])
        .to_string();
        assert_eq!(newest_setup(&json).unwrap().size, 51);
    }

    #[test]
    fn empty_foreign_or_odd_installers_are_not_used() {
        let mut foreign = release("app-v0.6.0", false, false, &[(SETUP_ASSET, 6)]);
        foreign["assets"][0]["browser_download_url"] =
            "https://example.com/Tingskriver-Setup.exe".into();
        let json = serde_json::json!([
            foreign,
            release("app-v0.5.9", false, false, &[(SETUP_ASSET, 0)])
        ])
        .to_string();
        assert!(newest_setup(&json).is_err());
        assert!(newest_setup("[]").is_err());
        assert!(newest_setup(r#"{"message":"API rate limit exceeded"}"#).is_err());
    }

    #[test]
    fn a_download_must_match_the_reported_size() {
        assert!(download_is_complete(1234, 1234));
        assert!(!download_is_complete(0, 0));
        assert!(!download_is_complete(1000, 1234));
        assert!(!download_is_complete(1300, 1234));
    }

    #[test]
    fn missing_shortcuts_are_found() {
        let paths = paths();
        assert!(paths.missing_shortcuts(|_| true).is_empty());
        let desktop_only = |path: &Path| normalized(path).contains("desktop");
        assert_eq!(
            paths.missing_shortcuts(desktop_only),
            vec![paths.shortcuts[0].clone()]
        );
        assert_eq!(paths.missing_shortcuts(|_| false).len(), 2);
    }

    #[test]
    fn the_helper_is_recognised_by_its_argument() {
        let args = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(helper_parent_pid(&args(&["tingskriver.exe"])), None);
        assert_eq!(
            helper_parent_pid(&args(&["tingskriver.exe", "--hidden"])),
            None
        );
        assert_eq!(
            helper_parent_pid(&args(&["tingskriver-move.exe", HELPER_ARG, "4242"])),
            Some(Some(4242))
        );
        assert_eq!(
            helper_parent_pid(&args(&["tingskriver-move.exe", HELPER_ARG])),
            Some(None)
        );
    }
}
