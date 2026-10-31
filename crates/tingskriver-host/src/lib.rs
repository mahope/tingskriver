//! The host around core for the GPUI app: keyboard hook, global shortcuts,
//! tray icon and dictation pill, outside GPUI's event loop.
//!
//! GPUI has no tray and no global shortcuts, and the dictation hook must
//! answer within 1 ms whatever the UI is doing. So [`Host::start`] runs:
//!
//! - core's dictation service, whose `WH_KEYBOARD_LL` hook has its own thread
//!   and message loop (`tingskriver_core::dictation::win::KeyboardHook`), with
//!   the pill from `tingskriver-overlay` (its own thread too);
//! - one host thread with a Win32 message loop (`win`): the tray icon and
//!   menu (`tray-icon`) and the `RegisterHotKey` shortcuts;
//! - two tasks on the core runtime (`watch`) that keep the tray in step with
//!   the recording and re-register shortcuts when the settings change.
//!
//! The app gets [`Request`]s for what needs the UI (show the window, quit)
//! and talks to core through [`Host::handle`], an `AppHost` ([`bridge`]).
//! Nothing here sees or logs which keys are typed: the hook only tracks the
//! configured shortcut, and `WM_HOTKEY` only says which shortcut fired.
//!
//! On other systems the host runs dictation's bridge and watchers without a
//! tray or shortcuts, so the app builds and its tests run on Linux.

pub mod bridge;
pub mod hotkeys;
pub mod icon;
pub mod status;
pub mod watch;
#[cfg(windows)]
mod win;

use std::path::PathBuf;
use std::sync::Arc;

use tingskriver_core::dictation;
use tingskriver_core::host::HostHandle;
use tingskriver_core::shortcuts::{Conflict, ShortcutAction, ShortcutSettings, KEY, STORE};
use tokio::runtime::Handle;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use tokio::task::JoinHandle;

pub use bridge::{Bridge, Event};
pub use status::{Dictation, MenuEntry, Recording, TrayStatus};
use watch::{Probe, Shared, Wanted};

/// What the app should do; the host does the rest itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Request {
    /// Show and focus the main window (tray click, "Åbn", shortcut).
    ShowWindow,
    /// Start a meeting recording ("Ny optagelse").
    NewRecording,
    /// Open the assistant ("Spørg").
    Ask,
    /// Quit the app ("Afslut").
    Quit,
}

/// Something the user did in the tray or with a registered shortcut.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
// Only the Windows host thread produces them.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) enum Trigger {
    Menu(MenuEntry),
    TrayClicked,
    Shortcut(ShortcutAction),
}

pub struct Host {
    bridge: Arc<Bridge>,
    shared: Arc<Shared>,
    runtime: Handle,
    tasks: Vec<JoinHandle<()>>,
    /// The Tauri app runs, so this one does not dictate.
    dictation_blocked: bool,
    #[cfg(windows)]
    thread: Option<Arc<win::HostThread>>,
}

impl Host {
    /// Start the host. `runtime` is the core runtime (already registered
    /// with `tingskriver_core::runtime::set_handle`).
    pub fn start(app_data_dir: PathBuf, runtime: Handle) -> (Host, UnboundedReceiver<Request>) {
        let bridge = Arc::new(Bridge::new(app_data_dir));
        let elevated = runs_elevated();
        if elevated {
            log::warn!(
                "Tingskriver runs as administrator. Run it as a normal user: the keyboard hook and \
                 text insertion should not run elevated"
            );
        }
        let shared = Arc::new(Shared::new(elevated));
        {
            let shared = shared.clone();
            bridge.on_status_change(Box::new(move || shared.wake.notify_one()));
        }
        let (requests, receiver) = unbounded_channel();

        let mut host = Host {
            bridge: bridge.clone(),
            shared: shared.clone(),
            runtime: runtime.clone(),
            tasks: Vec::new(),
            dictation_blocked: false,
            #[cfg(windows)]
            thread: None,
        };

        let handle: HostHandle = bridge.clone();
        let on_trigger = {
            let handle = handle.clone();
            let runtime = runtime.clone();
            move |trigger: Trigger| handle_trigger(trigger, &handle, &runtime, &requests)
        };

        #[cfg(windows)]
        {
            match win::HostThread::start(Arc::new(on_trigger)) {
                Ok(thread) => host.thread = Some(Arc::new(thread)),
                Err(e) => log::error!("Tray and shortcuts could not start: {e}"),
            }
        }
        #[cfg(not(windows))]
        drop(on_trigger);

        // Two hooks on Ctrl+Win would insert every dictation twice.
        if old_app_running(process_names()) {
            log::warn!(
                "The Tauri Tingskriver ({OLD_APP_EXE}) runs; dictation stays off here until it is closed and this app restarted"
            );
            host.dictation_blocked = true;
        } else {
            // The pill is drawn by the overlay crate; core's service decides what it shows.
            dictation::init(handle.clone(), overlay());
        }

        ensure_shortcut_file(&bridge);
        host.start_watchers();
        (host, receiver)
    }

    fn start_watchers(&mut self) {
        let shared = self.shared.clone();
        #[cfg(windows)]
        let thread = self.thread.clone();
        self.tasks.push(self.runtime.spawn(watch::watch_status(
            CoreProbe {
                dictation_blocked: self.dictation_blocked,
            },
            shared.clone(),
            move |status| {
                #[cfg(windows)]
                if let Some(thread) = &thread {
                    thread.set_status(status);
                }
                #[cfg(not(windows))]
                let _ = status;
            },
        )));

        let bridge = self.bridge.clone();
        #[cfg(windows)]
        let thread = self.thread.clone();
        self.tasks.push(self.runtime.spawn(watch::watch_shortcuts(
            shared,
            move || read_wanted(&bridge),
            move |wanted: Wanted| {
                #[cfg(windows)]
                let thread = thread.clone();
                async move {
                    #[cfg(windows)]
                    if let Some(thread) = thread {
                        return thread.apply_shortcuts(wanted).await;
                    }
                    #[cfg(not(windows))]
                    let _ = wanted;
                    Vec::new()
                }
            },
        )));
    }

    /// A tray icon is showing (Windows), so closing the window should hide
    /// the app instead of quitting.
    pub fn has_tray(&self) -> bool {
        #[cfg(windows)]
        return self.thread.as_ref().is_some_and(|t| t.has_tray());
        #[cfg(not(windows))]
        false
    }

    /// The `AppHost` core functions take.
    pub fn handle(&self) -> HostHandle {
        self.bridge.clone()
    }

    pub fn bridge(&self) -> &Arc<Bridge> {
        &self.bridge
    }

    /// Save new shortcuts. They are registered within a second, without a
    /// restart; the returned list is the conflict check of the new set
    /// (`TakenByOtherProgram` shows up in the tray once registered).
    pub fn save_shortcuts(&self, settings: &ShortcutSettings) -> Result<Vec<Conflict>, String> {
        let problems =
            tingskriver_core::shortcuts::check(dictation_shortcut().as_deref(), settings);
        settings.save(self.bridge.as_ref())?;
        Ok(problems)
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        for task in self.tasks.drain(..) {
            task.abort();
        }
        #[cfg(windows)]
        self.thread.take();
    }
}

fn handle_trigger(
    trigger: Trigger,
    handle: &HostHandle,
    runtime: &Handle,
    requests: &UnboundedSender<Request>,
) {
    let request = match trigger {
        Trigger::TrayClicked
        | Trigger::Menu(MenuEntry::Open)
        | Trigger::Shortcut(ShortcutAction::OpenWindow) => Some(Request::ShowWindow),
        Trigger::Menu(MenuEntry::NewRecording)
        | Trigger::Shortcut(ShortcutAction::NewRecording) => {
            // The same entry stops a running recording. Stopping needs no UI.
            let handle = handle.clone();
            let requests = requests.clone();
            runtime.spawn(async move {
                use tingskriver_core::audio::recording_commands as rec;
                if rec::is_recording().await {
                    let args = rec::RecordingArgs {
                        save_path: String::new(),
                    };
                    if let Err(e) = rec::stop_recording(handle, args).await {
                        log::error!("Could not stop the recording from the tray: {e}");
                    }
                } else {
                    let _ = requests.send(Request::NewRecording);
                }
            });
            None
        }
        Trigger::Menu(MenuEntry::Dictate) | Trigger::Shortcut(ShortcutAction::Dictate) => {
            match dictation::service::get().and_then(|s| s.toggle_hands_free()) {
                Ok(()) => {}
                Err(e) => log::warn!("Dictation from the tray failed: {e}"),
            }
            None
        }
        Trigger::Menu(MenuEntry::Ask) | Trigger::Shortcut(ShortcutAction::Ask) => {
            Some(Request::Ask)
        }
        Trigger::Menu(MenuEntry::Quit) => Some(Request::Quit),
    };
    if let Some(request) = request {
        let _ = requests.send(request);
    }
}

/// The dictation shortcut, `None` while dictation is off or not started.
fn dictation_shortcut() -> Option<String> {
    let service = dictation::service::get().ok()?;
    let settings = service.settings();
    settings.enabled.then(|| {
        settings
            .chord()
            .map(|c| c.label())
            .unwrap_or(settings.shortcut)
    })
}

fn read_wanted(bridge: &Bridge) -> Result<Wanted, String> {
    use tingskriver_core::host::AppHost;
    let settings = match bridge.read_setting(STORE, KEY)? {
        Some(value) => serde_json::from_value(value).map_err(|e| format!("{STORE}: {e}"))?,
        None => ShortcutSettings::default(),
    };
    Ok(Wanted {
        dictation: dictation_shortcut(),
        settings,
    })
}

/// Write the default (empty) shortcuts once, so the file exists to edit.
fn ensure_shortcut_file(bridge: &Bridge) {
    use tingskriver_core::host::AppHost;
    if let Ok(None) = bridge.read_setting(STORE, KEY) {
        if let Err(e) = ShortcutSettings::default().save(bridge) {
            log::warn!("Could not write the default shortcuts: {e}");
        }
    }
}

/// The Tauri app's executable. It has its own dictation hook.
pub const OLD_APP_EXE: &str = "tingskriver.exe";

fn old_app_running(process_names: Vec<String>) -> bool {
    process_names
        .iter()
        .any(|name| name.eq_ignore_ascii_case(OLD_APP_EXE))
}

#[cfg(windows)]
fn process_names() -> Vec<String> {
    win::process_names()
}

#[cfg(not(windows))]
fn process_names() -> Vec<String> {
    Vec::new()
}

/// Status from core: the meeting recording and the dictation service.
struct CoreProbe {
    dictation_blocked: bool,
}

impl Probe for CoreProbe {
    async fn recording(&self) -> Recording {
        use tingskriver_core::audio::recording_commands as rec;
        if !rec::is_recording().await {
            Recording::Idle
        } else if rec::is_recording_paused().await {
            Recording::Paused
        } else {
            Recording::Recording
        }
    }

    fn dictation(&self) -> Dictation {
        if self.dictation_blocked {
            return Dictation::Failed(
                "Den gamle Tingskriver dikterer allerede. Luk den, og start denne igen".into(),
            );
        }
        let Ok(service) = dictation::service::get() else {
            return Dictation::Starting;
        };
        let settings = service.settings();
        if !settings.enabled {
            return Dictation::Off;
        }
        let label = settings
            .chord()
            .map(|c| c.label())
            .unwrap_or(settings.shortcut);
        if service.shortcut_active() {
            if dictation::is_dictating() {
                Dictation::Listening(label)
            } else {
                Dictation::Ready(label)
            }
        } else if let Some(error) = service.last_error() {
            Dictation::Failed(error)
        } else if cfg!(windows) {
            Dictation::Starting
        } else {
            Dictation::Off
        }
    }
}

#[cfg(windows)]
fn overlay() -> Arc<dyn dictation::overlay::Overlay> {
    Arc::new(tingskriver_overlay::windows::CoreOverlay::new())
}

#[cfg(not(windows))]
fn overlay() -> Arc<dyn dictation::overlay::Overlay> {
    struct NoPill;
    impl dictation::overlay::Overlay for NoPill {
        fn ensure(&self) -> Result<(), String> {
            Ok(())
        }
        fn set(&self, _state: dictation::overlay::OverlayState) {}
    }
    Arc::new(NoPill)
}

#[cfg(windows)]
fn runs_elevated() -> bool {
    use tingskriver_core::dictation::logic::paste::INTEGRITY_HIGH;
    tingskriver_core::dictation::win::own_integrity_level()
        .is_some_and(|level| level >= INTEGRITY_HIGH)
}

#[cfg(not(windows))]
fn runs_elevated() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(trigger: Trigger) -> Option<Request> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let dir =
            std::env::temp_dir().join(format!("tingskriver-host-trigger-{}", std::process::id()));
        let handle: HostHandle = Arc::new(Bridge::new(dir));
        let (tx, mut rx) = unbounded_channel();
        handle_trigger(trigger, &handle, runtime.handle(), &tx);
        runtime.block_on(async {
            tokio::time::timeout(std::time::Duration::from_millis(200), rx.recv())
                .await
                .ok()
                .flatten()
        })
    }

    #[test]
    fn triggers_map_to_requests() {
        assert_eq!(run(Trigger::TrayClicked), Some(Request::ShowWindow));
        assert_eq!(
            run(Trigger::Menu(MenuEntry::Open)),
            Some(Request::ShowWindow)
        );
        assert_eq!(
            run(Trigger::Shortcut(ShortcutAction::OpenWindow)),
            Some(Request::ShowWindow)
        );
        assert_eq!(run(Trigger::Menu(MenuEntry::Quit)), Some(Request::Quit));
        assert_eq!(run(Trigger::Menu(MenuEntry::Ask)), Some(Request::Ask));
        assert_eq!(
            run(Trigger::Shortcut(ShortcutAction::Ask)),
            Some(Request::Ask)
        );
        // Not recording: the app is asked to start one.
        assert_eq!(
            run(Trigger::Menu(MenuEntry::NewRecording)),
            Some(Request::NewRecording)
        );
        assert_eq!(
            run(Trigger::Shortcut(ShortcutAction::NewRecording)),
            Some(Request::NewRecording)
        );
        // Dictation is handled by the host (and fails quietly without the service).
        assert_eq!(run(Trigger::Menu(MenuEntry::Dictate)), None);
    }

    #[test]
    fn the_tauri_app_is_recognised_by_its_exe() {
        let names = |list: &[&str]| list.iter().map(|n| n.to_string()).collect::<Vec<_>>();
        assert!(old_app_running(names(&["explorer.exe", "Tingskriver.exe"])));
        assert!(!old_app_running(names(&[
            "tingskriver-app.exe",
            "explorer.exe"
        ])));
        assert!(!old_app_running(Vec::new()));
        let probe = CoreProbe {
            dictation_blocked: true,
        };
        assert!(matches!(probe.dictation(), Dictation::Failed(m) if m.contains("Luk den")));
    }

    #[test]
    fn shortcut_file_is_created_and_read() {
        use tingskriver_core::host::AppHost;
        let dir =
            std::env::temp_dir().join(format!("tingskriver-host-shortcuts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let bridge = Bridge::new(dir.clone());
        ensure_shortcut_file(&bridge);
        let text = std::fs::read_to_string(dir.join(STORE)).unwrap();
        assert!(text.contains("\"newRecording\": null"), "{text}");
        assert_eq!(
            read_wanted(&bridge).unwrap().settings,
            ShortcutSettings::default()
        );

        // An edited file is read as is.
        std::fs::write(
            dir.join(STORE),
            r#"{"shortcuts": {"newRecording": "Ctrl+Alt+N", "ask": null, "openWindow": "Ctrl+Alt+T"}}"#,
        )
        .unwrap();
        let wanted = read_wanted(&bridge).unwrap();
        assert_eq!(wanted.settings.new_recording.as_deref(), Some("Ctrl+Alt+N"));
        assert_eq!(wanted.settings.open_window.as_deref(), Some("Ctrl+Alt+T"));
        // ensure_shortcut_file leaves an existing file alone.
        ensure_shortcut_file(&bridge);
        assert!(bridge.read_setting(STORE, KEY).unwrap().unwrap()["newRecording"] == "Ctrl+Alt+N");

        std::fs::write(dir.join(STORE), r#"{"shortcuts": {"newRecording": 5}}"#).unwrap();
        assert!(read_wanted(&bridge).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
