//! The main window: the shell (sidebar plus the current page), the shared
//! state and the page building blocks. The pages live in `crate::pages`.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use gpui::{
    div, prelude::*, relative, size, AnyElement, App, Bounds, Context, Entity, FontWeight, Global,
    Pixels, SharedString, Subscription, Task, TitlebarOptions, Window, WindowBounds, WindowHandle,
    WindowOptions,
};
use gpui_component::{
    h_flex,
    input::{InputEvent, InputState, TextareaState},
    spinner::Spinner,
    v_flex, ActiveTheme, Root, WindowExt,
};
use tingskriver_core::audio::power::InterruptNotice;
use tingskriver_core::strings;
use tokio::runtime::Handle;

use crate::controls::{self, Variant};
use crate::startup::{self, DbFailure, DbState, StartupClock};
use crate::theme::{self, layout, space, typography, Palette};
use crate::window_state;

/// State shared by every window: the database and where things live.
pub struct AppModel {
    pub db: DbState,
    pub app_data_dir: Option<PathBuf>,
    pub log_path: Option<PathBuf>,
    pub runtime: Handle,
    pub meeting_jobs: Vec<tingskriver_core::transcription_queue::QueueEvent>,
    pub clock: StartupClock,
    /// The page a new window opens on.
    pub start_page: Page,
    /// Whether the first-frame time is still to be reported. Off for
    /// `--hidden`, where the window opens long after process start.
    pub report_first_frame: bool,
    pub(crate) mcp_access: Option<tingskriver_core::mcp::Access>,
    pub(crate) mcp_message: String,
    pub(crate) mcp_controls: Option<tingskriver_core::mcp::Settings>,
    pub(crate) mcp_pending: bool,
    pub(crate) mcp_running: usize,
    pub(crate) mcp_log: Option<Result<Vec<tingskriver_core::mcp_audit::Entry>, String>>,
    pub(crate) sharing_meetings: Option<Result<Vec<tingskriver_core::mcp::SharingMeeting>, String>>,
    pub(crate) sharing_pending: bool,
    pub display_version: &'static str,
    /// Fixed About text for screenshot fixtures; `None` reads the real build.
    pub about_metadata: Option<String>,
    /// A meeting that sleep or logoff ended, until the user has seen it.
    pub interrupted: Option<InterruptNotice>,
    pub rerun_comparison: Option<tingskriver_core::dictation::rerun::Comparison>,
    /// Main language, extra spoken languages and the report language.
    pub languages: tingskriver_core::meeting_language::MeetingLanguages,
    /// Why the language settings could not be read or saved.
    pub language_error: Option<String>,
    /// Where the MCP server program should be, next to the app. Screenshot
    /// fixtures set a fixed path so the image is the same on every machine.
    pub mcp_server: Option<std::path::PathBuf>,
    /// Indstillinger: recordings folder, Udseende and crash dump state.
    pub(crate) settings: crate::pages::settings::SettingsModel,
    /// The meeting recorder of the host; `None` without one (no data folder,
    /// screenshot fixtures).
    pub recorder: Option<std::sync::Arc<tingskriver_host::Recorder>>,
}

/// The MCP server program ships next to the app executable.
fn mcp_server_path() -> Option<std::path::PathBuf> {
    std::env::current_exe().ok().and_then(|p| {
        p.parent().map(|p| {
            p.join(if cfg!(windows) {
                "tingskriver-mcp.exe"
            } else {
                "tingskriver-mcp"
            })
        })
    })
}

impl AppModel {
    pub fn new(runtime: Handle, clock: StartupClock, log_path: Option<PathBuf>) -> Self {
        let app_data_dir = startup::app_data_dir(std::env::var_os(startup::DATA_DIR_ENV));
        if std::env::var_os(startup::DATA_DIR_ENV).is_some_and(|v| !v.is_empty()) {
            log::warn!(
                "Data folder from {}: {:?}",
                startup::DATA_DIR_ENV,
                app_data_dir
            );
        }
        AppModel {
            db: DbState::Loading,
            app_data_dir,
            log_path,
            runtime,
            meeting_jobs: Vec::new(),
            clock,
            start_page: Page::Home,
            report_first_frame: true,
            mcp_access: None,
            mcp_message: String::new(),
            mcp_controls: None,
            mcp_pending: false,
            mcp_running: 0,
            mcp_log: None,
            sharing_meetings: None,
            sharing_pending: false,
            display_version: crate::about::VERSION,
            about_metadata: None,
            interrupted: None,
            rerun_comparison: None,
            mcp_server: mcp_server_path(),
            settings: crate::pages::settings::SettingsModel::load(),
            languages: Default::default(),
            language_error: None,
            recorder: None,
        }
        .with_languages_loaded()
    }

    /// Reads the saved language settings. A broken file is shown on the
    /// settings page and the defaults stay in use until the user saves.
    fn with_languages_loaded(mut self) -> Self {
        if let Some(dir) = &self.app_data_dir {
            match tingskriver_core::meeting_language::MeetingLanguages::load(dir) {
                Ok(languages) => self.languages = languages,
                Err(error) => self.language_error = Some(strings::language_load_failed(&error)),
            }
        }
        self
    }

    /// Applies a change to the language settings and saves it on the runtime,
    /// so the window thread never waits for the disk. A failed save is shown
    /// on the settings page.
    pub(crate) fn change_languages(
        &mut self,
        cx: &mut Context<Self>,
        change: impl FnOnce(
            tingskriver_core::meeting_language::MeetingLanguages,
        )
            -> Result<tingskriver_core::meeting_language::MeetingLanguages, String>,
    ) {
        let Some(dir) = self.app_data_dir.clone() else {
            self.language_error = Some(strings::language_save_failed(strings::UNKNOWN));
            return;
        };
        let languages = match change(self.languages.clone()).and_then(|l| l.normalized()) {
            Ok(languages) => languages,
            Err(error) => {
                self.language_error = Some(strings::language_save_failed(&error));
                return;
            }
        };
        self.languages = languages.clone();
        self.language_error = None;
        let task = self.runtime.spawn_blocking(move || languages.save(&dir));
        cx.spawn(async move |this, cx| {
            let failure = match task.await {
                Ok(Ok(_)) => None,
                Ok(Err(error)) => Some(error),
                Err(error) => Some(error.to_string()),
            };
            if let Some(error) = failure {
                let _ = this.update(cx, |m, cx| {
                    m.language_error = Some(strings::language_save_failed(&error));
                    cx.notify();
                });
            }
        })
        .detach();
    }

    pub(crate) fn log_dir(&self) -> Option<PathBuf> {
        self.log_path
            .as_ref()
            .and_then(|p| p.parent().map(PathBuf::from))
            .or_else(tingskriver_core::paths::log_dir)
    }
}

struct Globals {
    model: Entity<AppModel>,
    main_window: Option<WindowHandle<Root>>,
    shell: Option<gpui::WeakEntity<Shell>>,
}

impl Global for Globals {}

pub fn init(model: Entity<AppModel>, cx: &mut App) {
    crate::pages::settings::load_appearance(&model, cx);
    cx.set_global(Globals {
        model,
        main_window: None,
        shell: None,
    });
}

pub fn show_rerun_comparison(
    comparison: tingskriver_core::dictation::rerun::Comparison,
    cx: &mut App,
) {
    model(cx).update(cx, |model, cx| {
        model.rerun_comparison = Some(comparison);
        cx.notify();
    });
    show_main_window(cx);
}

/// Whether the main window shows Indstillinger on Generelt (job 5: the
/// delivery block reads the queue only then, as Tauri does).
pub(crate) fn settings_general_shown(cx: &App) -> bool {
    cx.try_global::<Globals>()
        .and_then(|globals| globals.shell.as_ref())
        .and_then(|shell| shell.upgrade())
        .is_some_and(|shell| {
            let shell = shell.read(cx);
            shell.page == Page::Settings && shell.settings.tab == SettingsTab::General
        })
}

fn model(cx: &App) -> Entity<AppModel> {
    cx.global::<Globals>().model.clone()
}

/// Open the database on the tokio runtime. The window keeps drawing; the
/// pages show a loading state until the result arrives.
pub fn load_database(cx: &mut App) {
    let model = model(cx);
    let (runtime, dir) = model.update(cx, |m, cx| {
        m.db = DbState::Loading;
        m.sharing_meetings = None;
        cx.notify();
        (m.runtime.clone(), m.app_data_dir.clone())
    });
    let Some(dir) = dir else {
        model.update(cx, |m, cx| {
            m.db = DbState::Failed(DbFailure::from_error(
                "no application data folder (dirs::data_dir returned nothing)",
                PathBuf::from(tingskriver_core::paths::DATABASE_FILE),
            ));
            cx.notify();
        });
        return;
    };
    let database_path = dir.join(tingskriver_core::paths::DATABASE_FILE);
    let started = Instant::now();
    let task = runtime.spawn(startup::open_database(dir));
    cx.spawn(async move |cx| {
        let state = match task.await {
            Ok(state) => state,
            Err(error) => DbState::Failed(DbFailure::from_error(
                &format!("database task stopped: {error}"),
                database_path,
            )),
        };
        let outcome = match &state {
            DbState::Ready(_) => "ready",
            DbState::NotCreated => "not created yet (first launch)",
            DbState::Failed(_) => "failed",
            DbState::Loading => "loading",
        };
        let mcp_access = if let DbState::Ready(db) = &state {
            let reader = tingskriver_core::mcp::Reader::new(db.pool().clone());
            runtime
                .spawn(async move { reader.selected_access().await })
                .await
                .ok()
                .and_then(Result::ok)
        } else {
            None
        };
        let mcp_controls = if let DbState::Ready(db) = &state {
            let pool = db.pool().clone();
            runtime
                .spawn(async move { tingskriver_core::mcp::settings(&pool).await })
                .await
                .ok()
                .and_then(Result::ok)
        } else {
            None
        };
        let mcp_log = if let Some(dir) = model.update(cx, |m, _| m.app_data_dir.clone()) {
            Some(
                runtime
                    .spawn(async move { tingskriver_core::mcp_audit::recent(&dir).await })
                    .await
                    .unwrap_or_else(|_| Err(strings::MCP_FAILED.into())),
            )
        } else {
            None
        };
        let mcp_running = runtime
            .spawn_blocking(|| {
                std::env::current_exe()
                    .ok()
                    .and_then(|p| {
                        p.parent().map(|p| {
                            p.join(if cfg!(windows) {
                                "tingskriver-mcp.exe"
                            } else {
                                "tingskriver-mcp"
                            })
                        })
                    })
                    .map(|exe| tingskriver_core::mcp_probe::running_servers(&exe))
                    .unwrap_or(0)
            })
            .await
            .unwrap_or(0);
        let sharing_meetings = if let DbState::Ready(db) = &state {
            let pool = db.pool().clone();
            Some(
                match runtime
                    .spawn(async move { tingskriver_core::mcp::sharing_meetings(&pool).await })
                    .await
                {
                    Ok(result) => result,
                    Err(_) => Err(strings::MCP_SAVE_FAILED.into()),
                },
            )
        } else {
            None
        };
        model.update(cx, |m, cx| {
            log::info!(
                "startup: database {outcome} after {} ms ({} ms after process start)",
                started.elapsed().as_millis(),
                m.clock.elapsed().as_millis()
            );
            m.db = state;
            m.mcp_access = mcp_access;
            m.mcp_controls = mcp_controls;
            m.mcp_log = mcp_log;
            m.mcp_running = mcp_running;
            m.sharing_meetings = sharing_meetings;
            cx.notify();
        });
    })
    .detach();
}

/// Show a meeting that sleep or logoff ended (event from the host).
pub fn set_interrupted(notice: InterruptNotice, cx: &mut App) {
    model(cx).update(cx, |m, cx| {
        m.interrupted = Some(notice);
        cx.notify();
    });
    // The window may be hidden in the tray: the user should see why.
    show_main_window(cx);
}

/// Read a notice left by the last run (logoff ends the process, so it is
/// shown at the next start).
pub fn load_interrupted(cx: &mut App) {
    let model = model(cx);
    let (runtime, dir) = model.read_with(cx, |m, _| (m.runtime.clone(), m.app_data_dir.clone()));
    let Some(dir) = dir else { return };
    let task = runtime.spawn_blocking(move || InterruptNotice::load(&dir));
    cx.spawn(async move |cx| {
        if let Ok(Some(notice)) = task.await {
            model.update(cx, |m, cx| {
                if m.interrupted.is_none() {
                    m.interrupted = Some(notice);
                    cx.notify();
                }
            });
        }
    })
    .detach();
}

pub(crate) fn dismiss_interrupted(cx: &mut App) {
    let model = model(cx);
    let (runtime, dir) = model.update(cx, |m, cx| {
        m.interrupted = None;
        cx.notify();
        (m.runtime.clone(), m.app_data_dir.clone())
    });
    if let Some(dir) = dir {
        runtime.spawn_blocking(move || InterruptNotice::dismiss(&dir));
    }
}

/// Dictation history changed in core: the Diktering page reads it again.
pub fn dictation_history_changed(cx: &mut App) {
    let shell = cx
        .global::<Globals>()
        .shell
        .as_ref()
        .and_then(|shell| shell.upgrade());
    if let Some(shell) = shell {
        shell.update(cx, |shell, cx| shell.dictation_changed(cx));
    }
}

/// Run `f` on the shell of the main window, opening the window if needed.
fn with_shell(cx: &mut App, f: impl FnOnce(&mut Shell, &mut Window, &mut Context<Shell>)) {
    show_main_window(cx);
    if let Some(handle) = cx.global::<Globals>().main_window {
        let shell = cx
            .global::<Globals>()
            .shell
            .as_ref()
            .and_then(|shell| shell.upgrade());
        let _ = handle.update(cx, |_, window, cx| {
            if let Some(shell) = shell {
                shell.update(cx, |shell, cx| f(shell, window, cx));
            }
        });
    }
}

/// Open "Om Tingskriver" over the screen of an existing or newly opened
/// window.
pub fn show_about(cx: &mut App) {
    with_shell(cx, |shell, window, cx| shell.open_about(window, cx));
}

/// "Opdatering klar" for an update the background updater downloaded.
pub(crate) fn show_update_ready(version: String, cx: &mut App) {
    let Some(handle) = cx.global::<Globals>().main_window else {
        return;
    };
    let current = model(cx).read(cx).display_version;
    let _ = handle.update(cx, |_, window, cx| {
        window.push_notification(
            crate::about_dialog::ready_notification(version, current),
            cx,
        );
    });
}

/// Show the main window, opening it if needed.
pub fn show_main_window(cx: &mut App) {
    if let Some(handle) = cx.global::<Globals>().main_window {
        let shown = handle.update(cx, |_, window, _| window.activate_window());
        if shown.is_ok() {
            return;
        }
    }
    match open_main_window(cx) {
        Ok(handle) => {
            cx.global_mut::<Globals>().main_window = Some(handle);
            cx.activate(true);
        }
        Err(error) => {
            let log_path = model(cx).read(cx).log_path.clone();
            crate::crash::window_failed(&error, log_path.as_deref())
        }
    }
}

/// A notification in the main window, if it is open.
pub(crate) fn notify_main_window(
    notification: gpui_component::notification::Notification,
    cx: &mut App,
) {
    if let Some(handle) = cx.global::<Globals>().main_window {
        let _ = handle.update(cx, |_, window, cx| {
            window.push_notification(notification, cx)
        });
    }
}

/// A meeting was saved elsewhere (a recording): read the list again.
pub(crate) fn meetings_changed(cx: &mut App) {
    let shell = cx
        .global::<Globals>()
        .shell
        .as_ref()
        .and_then(|s| s.upgrade());
    if let Some(shell) = shell {
        let meetings = shell.read(cx).meetings.clone();
        meetings.update(cx, |meetings, cx| meetings.reload(cx));
    }
}

/// Open a meeting in the main window, if it is open (a finished recording).
pub(crate) fn open_meeting(id: String, cx: &mut App) {
    let shell = cx
        .global::<Globals>()
        .shell
        .as_ref()
        .and_then(|s| s.upgrade());
    if let Some(shell) = shell {
        let meetings = shell.read(cx).meetings.clone();
        meetings.update(cx, |meetings, cx| meetings.open(id, cx));
    }
}

pub fn show_mcp_action(action: String, cx: &mut App) {
    show_main_window(cx);
    if let Some(handle) = cx.global::<Globals>().main_window {
        let _ = handle.update(cx, |_, window, cx| {
            window.push_notification(
                gpui_component::notification::Notification::info(strings::mcp_action(&action))
                    .in_app_and_system(),
                cx,
            );
        });
    }
}

fn open_main_window(cx: &mut App) -> anyhow::Result<WindowHandle<Root>> {
    let state_path = model(cx)
        .read(cx)
        .app_data_dir
        .as_deref()
        .map(window_state::path_in);
    let displays: Vec<Bounds<gpui::Pixels>> =
        cx.displays().iter().map(|d| d.visible_bounds()).collect();
    let saved = state_path.as_deref().and_then(window_state::load);
    let bounds = window_state::restore(saved, &displays).unwrap_or_else(|| {
        WindowBounds::centered(
            size(layout::DEFAULT_WINDOW_WIDTH, layout::DEFAULT_WINDOW_HEIGHT),
            cx,
        )
    });

    cx.open_window(
        WindowOptions {
            window_bounds: Some(bounds),
            titlebar: Some(TitlebarOptions {
                title: Some(strings::APP_NAME.into()),
                ..Default::default()
            }),
            app_id: Some(tingskriver_core::paths::APP_ID.to_string()),
            window_min_size: Some(size(layout::MIN_WINDOW_WIDTH, layout::MIN_WINDOW_HEIGHT)),
            ..Default::default()
        },
        move |window, cx| {
            theme::sync_with_window(window, cx);
            let shell = cx.new(|cx| Shell::new(state_path, window, cx));
            cx.global_mut::<Globals>().shell = Some(shell.downgrade());
            let model = model(cx);
            window.on_next_frame(move |_, cx| {
                model.update(cx, |m, _| {
                    if m.report_first_frame {
                        m.report_first_frame = false;
                        let (level, line) =
                            startup::first_frame_report(m.clock.elapsed(), m.clock.before_main());
                        log::log!(level, "{line}");
                    }
                });
            });
            cx.new(|cx| Root::new(shell, window, cx))
        },
    )
}

/// What the content area shows next to the sidebar. Meetings are not a
/// page: the list lives in the sidebar and an open meeting replaces Home.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    /// The idle home screen (Tauri `/`), or the open meeting.
    Home,
    Dictation,
    Settings,
    /// Om Tingskriver: not a page but the About dialog over the home
    /// screen (`--page om`, the tray). The shell opens the dialog and shows
    /// Home underneath.
    About,
    /// Not in the sidebar: a sub-page of Indstillinger > Transskription
    /// (docs/design-paritet.md, Job 6), also `--page måling`.
    Measurement,
}

impl Page {
    /// Every page, in sidebar order; the screenshot inventory too.
    pub const ALL: [Page; 5] = [
        Page::Home,
        Page::Dictation,
        Page::Measurement,
        Page::Settings,
        Page::About,
    ];

    /// The page for a `--page` argument (English or Danish name). The old
    /// Møder page opens the home screen, where the sidebar lists meetings.
    pub fn from_arg(name: &str) -> Option<Page> {
        match name.to_lowercase().as_str() {
            "home" | "hjem" | "meetings" | "møder" => Some(Page::Home),
            "dictation" | "diktering" => Some(Page::Dictation),
            "about" | "om" => Some(Page::About),
            "settings" | "indstillinger" => Some(Page::Settings),
            "measurement" | "måling" => Some(Page::Measurement),
            _ => None,
        }
    }
}

#[cfg(feature = "screenshot-tests")]
pub use crate::pages::meeting::FixtureMeeting;

/// Extra state of a screenshot fixture.
#[cfg(feature = "screenshot-tests")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixtureVariant {
    Plain,
    /// Synthetic meetings in the sidebar (four, over three days).
    Meetings,
    /// The 64 px sidebar.
    SidebarCollapsed,
    /// The settings page on a given tab.
    Settings(SettingsTab),
    /// Job 3: the recording screen (docs/screenshots/optagelse-*.png),
    /// with the meetings of `Meetings` in the sidebar.
    Recording,
    RecordingPaused,
    RecordingDeviceSwitched,
    /// "Stop og gem" was pressed: "Gemmer optagelsen…" in the pill.
    RecordingSaving,
    /// After saving: the overlay "Gør transskriptionen færdig…".
    RecordingFinishing,
    /// Starting failed: the alert under the pill on the idle screen.
    RecordingStartError,
    /// First start, step 1 (docs/screenshots/onboarding-*.png).
    OnboardingWelcome,
    /// First start, step 2: the transcription model has just started.
    OnboardingModel,
    /// An open meeting document (Job 4).
    Meeting(FixtureMeeting),
    /// Diktering without history (docs/screenshots/diktering-tom-*.png).
    DictationEmpty,
    /// Diktering with "Test tastatur" unfolded.
    DictationProbe,
    /// The update dialog with a downloaded update over the home screen.
    UpdateAvailable,
    /// "Gendan afbrudte møder" with two synthetic interrupted recordings.
    RecoveryDialog,
}

pub use crate::pages::settings::SettingsTab;

pub struct Shell {
    focus_handle: gpui::FocusHandle,
    /// "Om Tingskriver", shown as a dialog (`about_dialog`).
    about: Entity<crate::about_dialog::AboutView>,
    pub(crate) model: Entity<AppModel>,
    pub(crate) page: Page,
    pub(crate) measurement: Entity<crate::measurement::MeasurementView>,
    pub(crate) meetings: Entity<crate::meetings::MeetingsView>,
    /// The open meeting's document, replaced when another meeting opens.
    pub(crate) meeting_view: Option<Entity<crate::pages::meeting::MeetingView>>,
    pub(crate) sidebar_collapsed: bool,
    pub(crate) settings: crate::pages::settings::SettingsView,
    /// Why "Ny optagelse" or an import did not open a screen (not ported).
    pub(crate) home_notice: Option<&'static str>,
    /// The home page's recording state (`recording`).
    pub(crate) home: crate::recording::HomeState,
    /// "Gendan afbrudte møder" opened by itself once this session (Tauri
    /// shows it once per session).
    pub(crate) recovery_offered: bool,
    pub(crate) probe_line: Entity<InputState>,
    pub(crate) probe_text: Entity<TextareaState>,
    pub(crate) dictation: crate::pages::dictation::DictationPage,
    state_path: Option<PathBuf>,
    last_saved: Option<window_state::SavedWindow>,
    pending_save: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl Shell {
    /// Fixed shell state for raster tests, without opening a database,
    /// reading credentials, registering shortcuts or starting audio.
    #[cfg(feature = "screenshot-tests")]
    pub fn fixture(
        page: Page,
        db: DbState,
        variant: FixtureVariant,
        runtime: Handle,
        window: &mut Window,
        cx: &mut App,
    ) -> Entity<Self> {
        let model = cx.new(|_| AppModel {
            db,
            app_data_dir: None,
            log_path: Some(PathBuf::from("fixtures/tingskriver.log")),
            runtime,
            meeting_jobs: Vec::new(),
            clock: StartupClock::start(),
            start_page: page,
            report_first_frame: false,
            display_version: "0.0.0-test",
            about_metadata: Some(crate::about::metadata_with(
                "0.0.0-test",
                "2026-01-01",
                "0000000000000000000000000000000000000000",
                "test",
                "x86_64",
            )),
            interrupted: None,
            mcp_access: Some(tingskriver_core::mcp::Access::default()),
            mcp_message: String::new(),
            mcp_controls: Some(tingskriver_core::mcp::Settings::default()),
            mcp_pending: false,
            mcp_running: 0,
            mcp_log: None,
            sharing_meetings: None,
            sharing_pending: false,
            languages: Default::default(),
            language_error: None,
            rerun_comparison: None,
            mcp_server: Some(std::path::PathBuf::from("/opt/tingskriver/tingskriver-mcp")),
            settings: crate::pages::settings::SettingsModel {
                // Nothing set up yet: Kun lokal on, no targets, no jobs.
                delivery: crate::pages::settings_delivery::DeliveryModel::fixture(),
                ..Default::default()
            },
            recorder: None,
        });
        crate::onboarding::set_fixture(
            match variant {
                FixtureVariant::OnboardingWelcome => Some(crate::onboarding::Step::Welcome),
                FixtureVariant::OnboardingModel => Some(crate::onboarding::Step::Download),
                _ => None,
            },
            model.clone(),
            cx,
        );
        init(model, cx);
        let shell = cx.new(|cx| Shell::new(None, window, cx));
        shell.update(cx, |shell, cx| {
            shell.measurement = cx.new(|cx| {
                crate::measurement::MeasurementView::fixture(
                    shell.model.read(cx).runtime.clone(),
                    window,
                    cx,
                )
            });
            // The fixtures whose goldens show the synthetic meetings in the
            // sidebar; the rest (settings, Diktering, first start, the start
            // error) keep the empty list they were drawn with.
            let with_meetings = matches!(
                variant,
                FixtureVariant::Meetings
                    | FixtureVariant::Recording
                    | FixtureVariant::RecordingPaused
                    | FixtureVariant::RecordingDeviceSwitched
                    | FixtureVariant::RecordingSaving
                    | FixtureVariant::RecordingFinishing
                    | FixtureVariant::Meeting(_)
            );
            let list = with_meetings.then(crate::meetings::fixture_list);
            shell
                .meetings
                .update(cx, |meetings, _| meetings.freeze(list));
            shell.sidebar_collapsed = variant == FixtureVariant::SidebarCollapsed;
            if let FixtureVariant::Settings(tab) = variant {
                shell.settings.select_tab(tab);
            }
            shell.home = crate::recording::fixture(variant);
            if let FixtureVariant::Meeting(kind) = variant {
                shell.open_fixture_meeting(kind, cx);
            }
            shell.dictation = crate::pages::dictation::DictationPage::fixture(variant);
            if variant == FixtureVariant::RecoveryDialog {
                let model = shell.model.clone();
                window.defer(cx, move |window, cx| {
                    crate::recovery::open_fixture(model, window, cx)
                });
            }
            if variant == FixtureVariant::UpdateAvailable {
                // After this update, once the window's Root exists.
                window.defer(cx, |window, cx| {
                    crate::about_dialog::open_update(
                        crate::about::UpdateStatus::ReadyAtRestart {
                            version: "0.0.1-test".to_string(),
                        },
                        "0.0.0-test",
                        window,
                        cx,
                    )
                });
            }
            shell.probe_line.update(cx, |input, cx| {
                input.set_value("æøå ÆØÅ é ü @ €", window, cx)
            });
            shell.probe_text.update(cx, |input, cx| {
                input.set_value(
                    "Et syntetisk eksempel.\nIngen mødedata indlæses.",
                    window,
                    cx,
                )
            });
        });
        shell
    }

    fn new(state_path: Option<PathBuf>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let model = model(cx);
        let focus_handle = cx.focus_handle().tab_stop(false);
        focus_handle.focus(window, cx);
        let probe_line = cx.new(|cx| InputState::new(window, cx).placeholder(strings::PROBE_LINE));
        let probe_text = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder(strings::PROBE_TEXT)
                .rows(4)
        });
        let subscriptions = vec![
            cx.observe(&model, |_, _, cx| cx.notify()),
            cx.observe_window_appearance(window, |_, window, cx| {
                theme::sync_with_window(window, cx);
                cx.notify();
            }),
            cx.observe_window_bounds(window, |this, window, cx| this.schedule_save(window, cx)),
            cx.subscribe(&probe_line, |_, _, _: &InputEvent, cx| cx.notify()),
            cx.subscribe(&probe_text, |_, _, _: &InputEvent, cx| cx.notify()),
        ];
        let measurement = cx.new(|cx| {
            crate::measurement::MeasurementView::new(
                model.read(cx).app_data_dir.clone(),
                model.read(cx).runtime.clone(),
                window,
                cx,
            )
        });
        let meetings = cx.new(|cx| crate::meetings::MeetingsView::new(model.clone(), window, cx));
        let subscriptions = {
            let mut subscriptions = subscriptions;
            subscriptions.push(cx.subscribe(
                &meetings,
                |this, _, _: &crate::meetings::MeetingsEvent, cx| {
                    // A row was opened (or the open meeting deleted).
                    this.page = Page::Home;
                    this.home_notice = None;
                    cx.notify();
                },
            ));
            subscriptions
        };
        let save_path = state_path.clone();
        window.on_window_should_close(cx, move |window, _| {
            if let Some(path) = &save_path {
                let state = window_state::SavedWindow::from_window_bounds(window.window_bounds());
                if let Err(error) = window_state::save(path, &state) {
                    log::warn!("Could not save the window position: {error}");
                }
            }
            true
        });
        let page = model.read(cx).start_page;
        let settings = crate::pages::settings::SettingsView::new(&model, window, cx);
        let about = cx.new(|cx| crate::about_dialog::AboutView::new(model.clone(), cx));
        let mut shell = Shell {
            focus_handle,
            about,
            model,
            page,
            measurement,
            meetings,
            meeting_view: None,
            sidebar_collapsed: false,
            settings,
            home_notice: None,
            home: crate::recording::HomeState::default(),
            recovery_offered: false,
            probe_line,
            probe_text,
            dictation: crate::pages::dictation::DictationPage::new(),
            state_path,
            last_saved: None,
            pending_save: None,
            _subscriptions: subscriptions,
        };
        shell.follow_recorder(cx);
        shell
    }

    /// Open "Om Tingskriver" over the current screen (ⓘ, the logo).
    pub(crate) fn open_about(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        crate::about_dialog::open_about(self.about.clone(), window, cx);
    }

    /// Save the position half a second after the window stops moving.
    fn schedule_save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.state_path.clone() else {
            return;
        };
        let state = window_state::SavedWindow::from_window_bounds(window.window_bounds());
        if self.last_saved == Some(state) {
            return;
        }
        self.last_saved = Some(state);
        let executor = cx.background_executor().clone();
        self.pending_save = Some(cx.background_spawn(async move {
            executor.timer(Duration::from_millis(500)).await;
            if let Err(error) = window_state::save(&path, &state) {
                log::warn!("Could not save the window position: {error}");
            }
        }));
    }
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.page == Page::About {
            // Om is a dialog over the home screen, not a page.
            self.page = Page::Home;
            let shell = cx.entity().downgrade();
            window.defer(cx, move |window, cx| {
                let _ = shell.update(cx, |shell, cx| shell.open_about(window, cx));
            });
        }
        // Where "Tilbage" in Indstillinger returns to (Job 6).
        crate::pages::settings::remember_return(self, cx);
        let palette = palette(cx);
        if self.page != Page::Dictation {
            self.dictation.hide();
        }
        let failure = match &self.model.read(cx).db {
            DbState::Failed(failure) => Some(failure.clone()),
            _ => None,
        };
        // First start replaces the sidebar and the page (Job 9).
        let onboarding = crate::onboarding::shown(cx).filter(|_| failure.is_none());
        let content = if let Some(onboarding) = &onboarding {
            onboarding.clone().into_any_element()
        } else if let Some(comparison) = &self.model.read(cx).rerun_comparison {
            let body = v_flex()
                .gap(space::LG)
                .child(paragraph(
                    if comparison.changed {
                        strings::RERUN_CHANGED
                    } else {
                        strings::RERUN_UNCHANGED
                    },
                    cx,
                ))
                .child(section_title(strings::RERUN_BEFORE))
                .child(paragraph(&comparison.before, cx))
                .child(section_title(strings::RERUN_AFTER))
                .child(paragraph(&comparison.after, cx))
                .child(
                    controls::button("close-rerun-comparison", Variant::Outline)
                        .label(strings::RERUN_CLOSE)
                        .on_click(|_, _, cx| {
                            model(cx).update(cx, |model, cx| {
                                model.rerun_comparison = None;
                                cx.notify();
                            });
                        }),
                );
            page(
                strings::RERUN_TITLE,
                None,
                false,
                layout::DICTATION_MAX,
                body.into_any_element(),
                cx,
            )
        } else {
            match (&failure, self.page) {
                (Some(failure), _) => self.database_error_page(failure, cx),
                (None, Page::Home | Page::About) => {
                    match self.meetings.read(cx).selected_meeting().is_some() {
                        true => self.meeting_page(window, cx),
                        false => self.home_page(cx),
                    }
                }
                (None, Page::Dictation) => self.dictation_page(cx),
                (None, Page::Settings) => self.settings_page(cx),
                (None, Page::Measurement) => self.measurement_page(cx),
            }
        };
        let sidebar = failure.is_none() && onboarding.is_none();
        if sidebar && self.page == Page::Home {
            self.offer_recovery(window, cx);
        }
        h_flex()
            .track_focus(&self.focus_handle)
            .relative()
            .size_full()
            .bg(palette.background)
            .text_color(palette.foreground)
            .font_family(typography::SANS)
            .text_size(typography::BASE)
            .line_height(relative(typography::LINE_HEIGHT))
            .when(sidebar, |this| this.child(self.sidebar(cx)))
            .child(div().flex_1().h_full().min_w_0().child(content))
            // Last, so the handle draws over the sidebar border and the page.
            .when(sidebar, |this| this.child(self.collapse_handle(cx)))
    }
}

/// The Tingskriver palette for the current mode (tokens gpui-component's
/// theme has no slot for, such as `recording`, live here).
pub(crate) fn palette(cx: &App) -> &'static Palette {
    Palette::for_mode(cx.theme().mode)
}

/// The frame around every page: scroll area, page title (Tauri's 30 px
/// regular serif), optional subtitle and the page's content width
/// (`layout::*_MAX`).
pub(crate) fn page(
    title: &str,
    subtitle: Option<&str>,
    virtualized: bool,
    max_width: Pixels,
    body: AnyElement,
    cx: &App,
) -> AnyElement {
    v_flex()
        .id(SharedString::from(format!("page-{title}")))
        .size_full()
        .when(!virtualized, |this| this.overflow_y_scroll())
        .px(space::XXL)
        .py(space::XXL)
        .child(
            v_flex()
                .w_full()
                .max_w(max_width)
                .when(virtualized, |this| this.h_full().min_h_0())
                .gap(space::LG)
                .child(heading(title, Level::Page))
                .when_some(subtitle, |this, subtitle| this.child(muted(subtitle, cx)))
                .child(body),
        )
        .into_any_element()
}

/// The heading styles of the Tauri app; all are Literata (`h1, h2, h3`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    /// Page title (Indstillinger, Diktering): 30 px regular.
    Page,
    /// The meeting title: 36 px semibold.
    Document,
    /// The home and onboarding hero: 30 px semibold.
    Hero,
    /// A section inside a page (Udseende, the database error): 24 px regular.
    Section,
}

/// A serif heading. Tauri also sets `letter-spacing: -0.01em`; GPUI has no
/// letter spacing, so it is left out (docs/design.md).
pub(crate) fn heading(text: &str, level: Level) -> impl IntoElement {
    let (size, weight) = match level {
        Level::Page => (typography::XXXL, FontWeight::NORMAL),
        Level::Document => (typography::DISPLAY, FontWeight::SEMIBOLD),
        Level::Hero => (typography::XXXL, FontWeight::SEMIBOLD),
        Level::Section => (typography::XXL, FontWeight::NORMAL),
    };
    div()
        .font_family(typography::SERIF)
        .font_weight(weight)
        .text_size(size)
        .line_height(relative(typography::LINE_HEIGHT_TIGHT))
        .child(text.to_string())
}

/// The title of a block inside a page: 18 px semibold Literata (Tauri h3).
pub(crate) fn section_title(text: &str) -> impl IntoElement {
    div()
        .font_family(typography::SERIF)
        .font_weight(FontWeight::SEMIBOLD)
        .text_size(typography::LG)
        .line_height(relative(typography::LINE_HEIGHT_TIGHT))
        .child(text.to_string())
}

pub(crate) fn paragraph(text: &str, cx: &App) -> impl IntoElement {
    div()
        .text_size(typography::BASE)
        .text_color(palette(cx).foreground)
        .child(text.to_string())
}

pub(crate) fn muted(text: &str, cx: &App) -> impl IntoElement {
    div()
        .text_size(typography::SM)
        .text_color(palette(cx).muted_foreground)
        .child(text.to_string())
}

pub(crate) fn loading(text: &str, cx: &App) -> AnyElement {
    h_flex()
        .gap(space::SM)
        .py(space::XL)
        .text_color(palette(cx).muted_foreground)
        .child(Spinner::new())
        .child(text.to_string())
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_page_is_in_the_inventory_once() {
        for page in Page::ALL {
            assert_eq!(Page::ALL.iter().filter(|p| **p == page).count(), 1);
        }
    }

    #[test]
    fn pages_can_be_named_on_the_command_line() {
        assert_eq!(Page::from_arg("settings"), Some(Page::Settings));
        assert_eq!(Page::from_arg("about"), Some(Page::About));
        assert_eq!(Page::from_arg("Om"), Some(Page::About));
        assert_eq!(Page::from_arg("Diktering"), Some(Page::Dictation));
        assert_eq!(Page::from_arg("møder"), Some(Page::Home));
        assert_eq!(Page::from_arg("måling"), Some(Page::Measurement));
        assert_eq!(Page::from_arg("assistent"), None);
    }
}
