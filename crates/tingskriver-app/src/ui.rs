//! The main window: sidebar navigation and the (placeholder) pages.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use gpui::{
    div, prelude::*, relative, size, AnyElement, App, Bounds, Context, Entity, FontWeight, Global,
    SharedString, Subscription, Task, TitlebarOptions, Window, WindowBounds, WindowHandle,
    WindowOptions,
};
use gpui_component::{
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputEvent, InputState, Textarea, TextareaState},
    sidebar::{Sidebar, SidebarHeader, SidebarMenu, SidebarMenuItem},
    spinner::Spinner,
    v_flex, ActiveTheme, Disableable, Icon, Root, Selectable, WindowExt,
};
use gpui_kit_assets::IconName;
use tingskriver_core::audio::power::InterruptNotice;
use tingskriver_core::strings;
use tokio::runtime::Handle;

use crate::startup::{self, DbFailure, DbState, StartupClock};
use crate::theme::{self, icon_size, layout, space, typography, Palette};
use crate::{probe, window_state};

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
    mcp_access: Option<tingskriver_core::mcp::Access>,
    mcp_message: String,
    mcp_controls: Option<tingskriver_core::mcp::Settings>,
    mcp_pending: bool,
    mcp_running: usize,
    mcp_log: Option<Result<Vec<tingskriver_core::mcp_audit::Entry>, String>>,
    sharing_meetings: Option<Result<Vec<tingskriver_core::mcp::SharingMeeting>, String>>,
    sharing_pending: bool,
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
            start_page: Page::Meetings,
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
            languages: Default::default(),
            language_error: None,
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
    fn change_languages(
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

    fn log_dir(&self) -> Option<PathBuf> {
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

fn dismiss_interrupted(cx: &mut App) {
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

/// Navigate to About in an existing or newly opened window.
pub fn show_about(cx: &mut App) {
    show_main_window(cx);
    if let Some(handle) = cx.global::<Globals>().main_window {
        let shell = cx
            .global::<Globals>()
            .shell
            .as_ref()
            .and_then(|shell| shell.upgrade());
        let _ = handle.update(cx, |_, window, cx| {
            if let Some(shell) = shell {
                shell.update(cx, |shell, cx| {
                    shell.page = Page::About;
                    shell.about_copy_focus.focus(window, cx);
                    cx.notify();
                });
            }
        });
    }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Meetings,
    Dictation,
    Settings,
    About,
    Measurement,
}

impl Page {
    pub const ALL: [Page; 5] = [
        Page::Meetings,
        Page::Dictation,
        Page::Measurement,
        Page::Settings,
        Page::About,
    ];

    /// The page for a `--page` argument (English or Danish name).
    pub fn from_arg(name: &str) -> Option<Page> {
        match name.to_lowercase().as_str() {
            "meetings" | "møder" => Some(Page::Meetings),
            "dictation" | "diktering" => Some(Page::Dictation),
            "about" | "om" => Some(Page::About),
            "settings" | "indstillinger" => Some(Page::Settings),
            "measurement" | "måling" => Some(Page::Measurement),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Page::Meetings => strings::MEETINGS,
            Page::Dictation => strings::DICTATION,
            Page::About => strings::ABOUT,
            Page::Settings => strings::SETTINGS,
            Page::Measurement => strings::MEASURE_TITLE,
        }
    }

    fn icon(self) -> IconName {
        match self {
            Page::Meetings => IconName::Calendar,
            Page::Dictation => IconName::Mic,
            Page::About => IconName::Info,
            Page::Settings => IconName::Settings,
            Page::Measurement => IconName::Mic,
        }
    }
}

pub struct Shell {
    focus_handle: gpui::FocusHandle,
    about_copy_focus: gpui::FocusHandle,
    about_document_scroll: Vec<gpui::ScrollHandle>,
    diagnosis_status: Option<&'static str>,
    model: Entity<AppModel>,
    page: Page,
    measurement: Entity<crate::measurement::MeasurementView>,
    meetings: Entity<crate::meetings::MeetingsView>,
    probe_line: Entity<InputState>,
    probe_text: Entity<TextareaState>,
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
        });
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
        let meetings = cx.new(|cx| crate::meetings::MeetingsView::new(model.clone(), cx));
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
        Shell {
            focus_handle,
            about_copy_focus: cx.focus_handle(),
            about_document_scroll: crate::about::DOCUMENTS
                .iter()
                .map(|_| gpui::ScrollHandle::new())
                .collect(),
            diagnosis_status: None,
            model,
            page,
            measurement,
            meetings,
            probe_line,
            probe_text,
            state_path,
            last_saved: None,
            pending_save: None,
            _subscriptions: subscriptions,
        }
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

    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = palette(cx);
        Sidebar::new("main-sidebar")
            .w(layout::SIDEBAR_WIDTH)
            .collapsible(false)
            .header(
                SidebarHeader::new().child(
                    div()
                        .font_family(typography::SERIF)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_size(typography::XL)
                        .child(strings::APP_NAME),
                ),
            )
            .child(
                SidebarMenu::new().children(
                    Page::ALL
                        .into_iter()
                        .filter(|page| *page != Page::About)
                        .map(|page| {
                            SidebarMenuItem::new(page.label())
                                .icon(page.icon())
                                .active(self.page == page)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.page = page;
                                    cx.notify();
                                }))
                        }),
                ),
            )
            .footer(
                v_flex()
                    .child(
                        Button::new("nav-about")
                            .label(strings::ABOUT)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.page = Page::About;
                                cx.notify();
                            })),
                    )
                    .px(space::SM)
                    .text_size(typography::XS)
                    .text_color(palette.muted_foreground)
                    .child(strings::version(self.model.read(cx).display_version)),
            )
    }

    fn meetings_page(&self, cx: &mut Context<Self>) -> AnyElement {
        // The virtual meeting list owns the page; MCP sharing per meeting
        // stays on Møder below it while the database is open.
        let sharing = matches!(self.model.read(cx).db, DbState::Ready(_));
        v_flex()
            .size_full()
            .min_h_0()
            .child(div().flex_1().min_h_0().child(self.meetings.clone()))
            .when(sharing, |this| {
                this.child(
                    div()
                        .id("meeting-sharing")
                        .flex_none()
                        .max_h(gpui::px(280.))
                        .overflow_y_scroll()
                        .px(space::XXL)
                        .pb(space::XL)
                        .child(
                            v_flex()
                                .w_full()
                                .max_w(layout::CONTENT_MAX_WIDTH)
                                .gap(space::SM)
                                .child(section_title(strings::MCP_TITLE))
                                .child(self.meeting_sharing(cx)),
                        ),
                )
            })
            .into_any_element()
    }

    fn meeting_sharing(&self, cx: &mut Context<Self>) -> AnyElement {
        let state = self.model.read(cx);
        let mut body = v_flex()
            .gap(space::SM)
            .child(paragraph(strings::MCP_PRIVACY, cx))
            .child(
                Button::new("refresh-mcp-sharing")
                    .label(strings::MCP_REFRESH)
                    .disabled(state.sharing_pending)
                    .on_click(|_, _, cx| load_database(cx)),
            )
            .child(
                div()
                    .id("mcp-result")
                    .role(gpui::Role::Status)
                    .aria_label(state.mcp_message.clone())
                    .child(paragraph(&state.mcp_message, cx)),
            );
        match &state.sharing_meetings {
            None => body = body.child(loading(strings::LOADING_MEETINGS, cx)),
            Some(Err(error)) => body = body.child(paragraph(error, cx)),
            Some(Ok(meetings)) if meetings.is_empty() => {
                body = body.child(paragraph(strings::NO_MEETINGS, cx));
            }
            Some(Ok(meetings)) => {
                for (index, meeting) in meetings.iter().enumerate() {
                    let model = self.model.clone();
                    let id = meeting.id.clone();
                    let shared = !meeting.shared;
                    body = body.child(
                        v_flex()
                            .gap(space::SM)
                            .child(paragraph(&meeting.title, cx))
                            .child(
                                Button::new(("share-meeting", index))
                                    .label(if meeting.shared {
                                        strings::MCP_UNSHARE
                                    } else {
                                        strings::MCP_SHARE
                                    })
                                    .selected(meeting.shared)
                                    .disabled(state.sharing_pending)
                                    .on_click(move |_, _, cx| {
                                        let model = model.clone();
                                        let id = id.clone();
                                        let task = model.update(cx, |state, cx| {
                                            if state.sharing_pending {
                                                return None;
                                            }
                                            let DbState::Ready(db) = &state.db else {
                                                return None;
                                            };
                                            let pool = db.pool().clone();
                                            state.sharing_pending = true;
                                            cx.notify();
                                            Some(state.runtime.spawn(async move {
                                                let result = tingskriver_core::mcp::set_shared(
                                                    &pool, &id, shared,
                                                )
                                                .await;
                                                (id, result)
                                            }))
                                        });
                                        let Some(task) = task else {
                                            return;
                                        };
                                        cx.spawn(async move |cx| {
                                            let result = task.await;
                                            model.update(cx, |state, cx| {
                                                state.sharing_pending = false;
                                                if let Ok((id, Ok(()))) = result {
                                                    if let Some(Ok(meetings)) =
                                                        &mut state.sharing_meetings
                                                    {
                                                        if let Some(meeting) =
                                                            meetings.iter_mut().find(|m| m.id == id)
                                                        {
                                                            meeting.shared = shared;
                                                        }
                                                    }
                                                    state.mcp_message = strings::MCP_SAVED.into();
                                                } else {
                                                    state.mcp_message =
                                                        strings::MCP_SAVE_FAILED.into();
                                                }
                                                cx.notify();
                                            });
                                        })
                                        .detach();
                                    }),
                            ),
                    );
                }
            }
        }
        body.into_any_element()
    }

    fn dictation_page(&self, cx: &mut Context<Self>) -> AnyElement {
        let palette = palette(cx);
        let typed = format!(
            "{}\n{}",
            self.probe_line.read(cx).value(),
            self.probe_text.read(cx).value()
        );
        let described = probe::describe(&typed);
        let warning = probe::composition_warning(&typed);
        let body = v_flex()
            .gap(space::LG)
            .child(paragraph(strings::DICTATION_LATER, cx))
            .child(
                v_flex()
                    .gap(space::SM)
                    .child(section_title(strings::PROBE))
                    .child(muted(strings::PROBE_HELP, cx))
                    .child(Input::new(&self.probe_line))
                    .child(Textarea::new(&self.probe_text))
                    .when_some(warning, |this, warning| {
                        this.child(
                            div()
                                .text_size(typography::SM)
                                .text_color(palette.destructive)
                                .child(warning),
                        )
                    })
                    .child(
                        div()
                            .text_size(typography::SM)
                            .text_color(palette.muted_foreground)
                            .child(if described.is_empty() {
                                strings::SPECIAL_EMPTY.to_string()
                            } else {
                                strings::special_characters(&described.join(", "))
                            }),
                    ),
            );
        page(strings::DICTATION, None, false, body.into_any_element(), cx)
    }

    fn about_page(&self, cx: &mut Context<Self>) -> AnyElement {
        let body =
            v_flex()
                .gap(space::LG)
                .child(paragraph(
                    &self
                        .model
                        .read(cx)
                        .about_metadata
                        .clone()
                        .unwrap_or_else(crate::about::metadata),
                    cx,
                ))
                .child(section_title(strings::ABOUT_UPDATES))
                .child(paragraph(&crate::about::update_status().label(), cx))
                .child(
                    Button::new("about-copy")
                        .track_focus(&self.about_copy_focus)
                        .label(strings::ABOUT_COPY)
                        .loading(self.diagnosis_status == Some(strings::ABOUT_COPYING))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.diagnosis_status = Some(strings::ABOUT_COPYING);
                            cx.notify();
                            let task = this
                                .model
                                .read(cx)
                                .runtime
                                .spawn(crate::about::diagnostics());
                            cx.spawn(async move |this, cx| {
                                let status = match task.await {
                                    Ok(report) => {
                                        cx.update(|cx| {
                                            cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                                                report,
                                            ))
                                        });
                                        strings::ABOUT_COPIED
                                    }
                                    Err(_) => strings::ABOUT_COPY_FAILED,
                                };
                                let _ = this.update(cx, |this, cx| {
                                    this.diagnosis_status = Some(status);
                                    cx.notify();
                                });
                            })
                            .detach();
                        })),
                )
                .when_some(self.diagnosis_status, |body, status| {
                    body.child(
                        div()
                            .id("about-copy-status")
                            .role(gpui::Role::Status)
                            .child(status),
                    )
                })
                .child(
                    Button::new("about-privacy")
                        .label(strings::ABOUT_PRIVACY)
                        .on_click(|_, _, cx| {
                            cx.open_url(
                                "https://github.com/mahope/tingskriver/blob/main/PRIVACY_POLICY.md",
                            );
                        }),
                )
                .child(
                    Button::new("about-contact")
                        .label(strings::ABOUT_CONTACT)
                        .on_click(|_, _, cx| {
                            cx.open_url("https://mahoje.dk");
                        }),
                )
                .children(crate::about::DOCUMENTS.iter().enumerate().map(
                    |(index, (title, text))| {
                        v_flex().gap(space::SM).child(section_title(title)).child(
                            div()
                                .id(("about-document", index))
                                .h(gpui::px(240.))
                                .flex_shrink_0()
                                .overflow_y_scroll()
                                .track_scroll(&self.about_document_scroll[index])
                                .on_key_down(cx.listener(
                                    move |this, event: &gpui::KeyDownEvent, _, cx| {
                                        let scroll = &this.about_document_scroll[index];
                                        let mut offset = scroll.offset();
                                        let page = scroll.bounds().size.height;
                                        offset.y = match event.keystroke.key.as_str() {
                                            "down" => offset.y - gpui::px(32.),
                                            "up" => offset.y + gpui::px(32.),
                                            "pagedown" => offset.y - page,
                                            "pageup" => offset.y + page,
                                            "home" => gpui::px(0.),
                                            "end" => -scroll.max_offset().y,
                                            _ => return,
                                        };
                                        offset.y =
                                            offset.y.clamp(-scroll.max_offset().y, gpui::px(0.));
                                        scroll.set_offset(offset);
                                        cx.stop_propagation();
                                        cx.notify();
                                    },
                                ))
                                .tab_index(0)
                                .role(gpui::Role::Document)
                                .aria_label(*title)
                                .child(paragraph(text, cx)),
                        )
                    },
                ));
        page(strings::ABOUT, None, false, body.into_any_element(), cx)
    }

    fn settings_page(&self, cx: &mut Context<Self>) -> AnyElement {
        let model = self.model.read(cx);
        let mode = if cx.theme().mode.is_dark() {
            strings::DARK
        } else {
            strings::LIGHT
        };
        let data_dir = model
            .app_data_dir
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| strings::UNKNOWN.to_string());
        let database = match &model.db {
            DbState::Loading => strings::DB_OPENING,
            DbState::NotCreated => strings::DB_NOT_CREATED,
            DbState::Ready(_) => strings::DB_READY,
            DbState::Failed(_) => strings::DB_FAILED,
        };
        let log_path = model
            .log_path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| strings::NO_LOG.to_string());
        let log_dir = model.log_dir();

        let body = v_flex()
            .gap(space::XL)
            .child(
                Button::new("settings-about")
                    .label(strings::ABOUT)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.page = Page::About;
                        this.about_copy_focus.focus(window, cx);
                        cx.notify();
                    })),
            )
            .child(paragraph(strings::SETTINGS_MCP_HELP, cx))
            .child(self.mcp_settings(cx))
            .child(setting_group(
                strings::APPEARANCE,
                vec![strings::appearance(mode)],
                cx,
            ))
            .child(self.language_group(cx))
            .child(setting_group(
                strings::VOICEPRINT_TITLE,
                voiceprint_lines(),
                cx,
            ))
            .child(setting_group(
                strings::DATA,
                vec![strings::data_dir(&data_dir), strings::database(database)],
                cx,
            ))
            .child(
                v_flex()
                    .gap(space::SM)
                    .child(setting_group(
                        strings::LOG,
                        vec![strings::log_file(&log_path)],
                        cx,
                    ))
                    .child(
                        h_flex()
                            .gap(space::SM)
                            .child(
                                Button::new("open-log-dir")
                                    .label(strings::OPEN_LOG)
                                    .icon(IconName::FolderOpen)
                                    .on_click(move |_, _, _| {
                                        if let Some(dir) = &log_dir {
                                            startup::open_folder(dir);
                                        }
                                    }),
                            )
                            .child(
                                Button::new("quit")
                                    .label(strings::QUIT)
                                    .on_click(|_, _, cx| cx.quit()),
                            ),
                    ),
            );
        page(strings::SETTINGS, None, false, body.into_any_element(), cx)
    }

    fn mcp_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        use tingskriver_core::mcp::Access;
        let state = self.model.read(cx);
        let current = state.mcp_access;
        let mut controls = h_flex().gap(space::SM).flex_wrap();
        for (index, (access, label)) in [
            (Access::Marked, strings::MCP_MARKED),
            (Access::Metadata, strings::MCP_METADATA),
            (Access::All, strings::MCP_ALL),
            (Access::Off, strings::MCP_OFF),
        ]
        .into_iter()
        .enumerate()
        {
            let model = self.model.clone();
            controls = controls.child(
                Button::new(("mcp-access", index))
                    .label(label)
                    .selected(current == Some(access))
                    .toggled(current == Some(access))
                    .accessibility_label(label)
                    .disabled(current.is_none() || state.mcp_pending)
                    .on_click(move |_, _, cx| {
                        let model = model.clone();
                        let state = model.read(cx);
                        let DbState::Ready(db) = &state.db else {
                            return;
                        };
                        let pool = db.pool().clone();
                        let task = state.runtime.spawn(async move {
                            tingskriver_core::mcp::set_access(&pool, access).await
                        });
                        model.update(cx, |state, cx| {
                            state.mcp_pending = true;
                            cx.notify();
                        });
                        cx.spawn(async move |cx| {
                            let result = task.await;
                            model.update(cx, |state, cx| {
                                state.mcp_pending = false;
                                if matches!(result, Ok(Ok(()))) {
                                    state.mcp_access = Some(access);
                                    state.mcp_message = strings::MCP_SAVED.into();
                                } else {
                                    state.mcp_message = strings::MCP_SAVE_FAILED.into();
                                }
                                cx.notify();
                            });
                        })
                        .detach();
                    }),
            );
        }
        let mut body = v_flex()
            .gap(space::SM)
            .child(section_title(strings::MCP_TITLE))
            .child(paragraph(strings::MCP_PRIVACY, cx))
            .child(controls)
            .child(
                div()
                    .id("mcp-result")
                    .role(gpui::Role::Status)
                    .aria_label(state.mcp_message.clone())
                    .child(paragraph(&state.mcp_message, cx)),
            );
        if let Ok(exe) = std::env::current_exe() {
            if let Some(parent) = exe.parent() {
                let path = parent.join(if cfg!(windows) {
                    "tingskriver-mcp.exe"
                } else {
                    "tingskriver-mcp"
                });
                if let Ok(quoted) = serde_json::to_string(&path.to_string_lossy()) {
                    for (index, (label, text)) in [
                        (
                            strings::MCP_COPY_CLAUDE,
                            strings::mcp_claude_command(&path.to_string_lossy()),
                        ),
                        (strings::MCP_COPY_CODEX, strings::mcp_codex_config(&quoted)),
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        body = body.child(
                            Button::new(("mcp-copy", index))
                                .label(label)
                                .disabled(!path.is_file())
                                .on_click(move |_, window, cx| {
                                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                                        text.clone(),
                                    ));
                                    window.push_notification(strings::MCP_COPIED, cx);
                                }),
                        );
                    }
                }
            }
        }
        self.mcp_details(body, cx).into_any_element()
    }

    fn mcp_details(&self, mut body: gpui::Div, cx: &mut Context<Self>) -> gpui::Div {
        let state = self.model.read(cx);
        let exe = state.mcp_server.clone();
        let exists = exe.as_ref().is_some_and(|p| p.is_file());
        let status = match state.mcp_controls {
            None if matches!(state.db, DbState::Loading) => strings::MCP_LOADING,
            None => strings::MCP_LOAD_FAILED,
            Some(_) if !exists => strings::MCP_MISSING,
            Some(value)
                if !value.enabled
                    || state.mcp_access == Some(tingskriver_core::mcp::Access::Off) =>
            {
                strings::MCP_DISABLED
            }
            Some(_) if state.mcp_running > 0 => strings::MCP_ACTIVE,
            Some(_) => strings::MCP_AVAILABLE,
        };
        body = body.child(
            div()
                .id("mcp-status")
                .role(gpui::Role::Status)
                .aria_label(status)
                .child(paragraph(status, cx)),
        );
        if state.mcp_running > 0 {
            body = body.child(paragraph(
                &strings::MCP_RUNNING.replace("{value}", &state.mcp_running.to_string()),
                cx,
            ));
        }
        if let Some(path) = &exe {
            body = body.child(paragraph(
                &strings::MCP_PATH.replace("{value}", &path.display().to_string()),
                cx,
            ));
        }
        if let Some(value) = state.mcp_controls {
            let mut toggles = h_flex().gap(space::SM).flex_wrap();
            for (index, (label, active)) in [
                (strings::MCP_ENABLE, value.enabled),
                (strings::MCP_NOTES, value.notes),
                (strings::MCP_SUMMARY, value.summary),
                (strings::MCP_TRANSCRIPT, value.transcript),
                (strings::MCP_PARTICIPANTS, value.participants),
            ]
            .into_iter()
            .enumerate()
            {
                let model = self.model.clone();
                toggles = toggles.child(
                    Button::new(("mcp-data", index))
                        .label(label)
                        .accessibility_label(label)
                        .selected(active)
                        .toggled(active)
                        .disabled(state.mcp_pending)
                        .on_click(move |_, _, cx| {
                            let mut updated = value;
                            match index {
                                0 => updated.enabled = !active,
                                1 => updated.notes = !active,
                                2 => updated.summary = !active,
                                3 => updated.transcript = !active,
                                _ => updated.participants = !active,
                            }
                            save_mcp_controls(model.clone(), updated, cx);
                        }),
                );
            }
            body = body
                .child(paragraph(strings::MCP_DATA_HELP, cx))
                .child(toggles);
        }
        if let Some(Ok(meetings)) = &state.sharing_meetings {
            body = body.child(paragraph(
                &strings::MCP_SHARED_COUNT.replace(
                    "{value}",
                    &meetings.iter().filter(|m| m.shared).count().to_string(),
                ),
                cx,
            ));
        }
        body = body.child(
            Button::new("mcp-open-meetings")
                .label(strings::MCP_OPEN_MEETINGS)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.page = Page::Meetings;
                    cx.notify();
                })),
        );
        let model = self.model.clone();
        let dir = state.app_data_dir.clone();
        let test_exe = exe.clone();
        body = body.child(
            Button::new("mcp-test")
                .label(strings::MCP_TEST)
                .disabled(state.mcp_pending || !exists || dir.is_none())
                .on_click(move |_, _, cx| {
                    let (Some(exe), Some(dir)) = (test_exe.clone(), dir.clone()) else {
                        return;
                    };
                    let task = model.update(cx, |state, cx| {
                        state.mcp_pending = true;
                        state.mcp_message = strings::MCP_TESTING.into();
                        cx.notify();
                        state.runtime.spawn(async move {
                            tingskriver_core::mcp_probe::test_connection(&exe, &dir).await
                        })
                    });
                    let model = model.clone();
                    cx.spawn(async move |cx| {
                        let result = task.await;
                        model.update(cx, |state, cx| {
                            state.mcp_pending = false;
                            state.mcp_message = match result {
                                Ok(Ok(())) => strings::MCP_TEST_OK.into(),
                                Ok(Err(error)) => error,
                                Err(_) => strings::MCP_FAILED.into(),
                            };
                            cx.notify();
                        });
                    })
                    .detach();
                }),
        );
        body = body.child(section_title(strings::MCP_TOOLS));
        for tool in tingskriver_core::mcp_audit::TOOLS {
            body = body.child(paragraph(tool, cx));
        }
        body = body.child(paragraph(strings::MCP_LOG, cx));
        let mut buttons = h_flex().gap(space::SM);
        for (index, (label, clear)) in [
            (strings::MCP_LOG_REFRESH, false),
            (strings::MCP_LOG_CLEAR, true),
        ]
        .into_iter()
        .enumerate()
        {
            let model = self.model.clone();
            let exe = exe.clone();
            buttons = buttons.child(
                Button::new(("mcp-log", index))
                    .label(label)
                    .disabled(state.mcp_pending || state.app_data_dir.is_none())
                    .on_click(move |_, _, cx| {
                        let task = model.update(cx, |state, cx| {
                            let dir = state
                                .app_data_dir
                                .clone()
                                .expect("button requires data directory");
                            state.mcp_pending = true;
                            cx.notify();
                            let exe = exe.clone();
                            state.runtime.spawn(async move {
                                if clear {
                                    tingskriver_core::mcp_audit::clear(&dir).await?;
                                }
                                let entries = tingskriver_core::mcp_audit::recent(&dir).await?;
                                let running = tokio::task::spawn_blocking(move || {
                                    exe.map(|exe| {
                                        tingskriver_core::mcp_probe::running_servers(&exe)
                                    })
                                    .unwrap_or(0)
                                })
                                .await
                                .unwrap_or(0);
                                Ok::<_, String>((entries, running))
                            })
                        });
                        let model = model.clone();
                        cx.spawn(async move |cx| {
                            let result = task
                                .await
                                .unwrap_or_else(|_| Err(strings::MCP_FAILED.into()));
                            model.update(cx, |state, cx| {
                                state.mcp_pending = false;
                                if clear && result.is_ok() {
                                    state.mcp_message = strings::MCP_LOG_CLEARED.into();
                                }
                                state.mcp_log = Some(result.map(|(entries, running)| {
                                    state.mcp_running = running;
                                    entries
                                }));
                                cx.notify();
                            });
                        })
                        .detach();
                    }),
            );
        }
        body = body.child(buttons);
        match &state.mcp_log {
            Some(Err(error)) => body = body.child(paragraph(error, cx)),
            Some(Ok(entries)) if !entries.is_empty() => {
                for entry in entries {
                    let text = strings::MCP_LOG_ENTRY
                        .replace("{time}", &entry.time)
                        .replace("{tool}", &entry.tool)
                        .replace(
                            "{id}",
                            entry.meeting_id.as_deref().unwrap_or(strings::MCP_NO_ID),
                        )
                        .replace(
                            "{result}",
                            if entry.success {
                                strings::MCP_OK
                            } else {
                                strings::MCP_FAILED
                            },
                        );
                    body = body.child(paragraph(&text, cx));
                }
            }
            _ => body = body.child(paragraph(strings::MCP_LOG_EMPTY, cx)),
        }
        body
    }

    fn language_group(&self, cx: &mut Context<Self>) -> impl IntoElement {
        use tingskriver_core::meeting_language::{label, CHOICES};
        let model = self.model.read(cx);
        let languages = model.languages.clone();
        let error = model.language_error.clone();
        let primary_buttons = CHOICES.iter().enumerate().map(|(at, (code, _))| {
            let code = *code;
            let model = self.model.clone();
            Button::new(("language-primary", at))
                .label(label(code))
                .selected(languages.primary == code)
                .on_click(move |_, _, cx| {
                    model.update(cx, |m, cx| {
                        m.change_languages(cx, |l| l.with_primary(code));
                        cx.notify();
                    })
                })
        });
        let extra_buttons = CHOICES
            .iter()
            .enumerate()
            .filter(|(_, (code, _))| *code != languages.primary)
            .map(|(at, (code, _))| {
                let code = *code;
                let model = self.model.clone();
                Button::new(("language-extra", at))
                    .label(label(code))
                    .selected(languages.extra.iter().any(|c| c == code))
                    .on_click(move |_, _, cx| {
                        model.update(cx, |m, cx| {
                            m.change_languages(cx, |l| l.with_extra_toggled(code));
                            cx.notify();
                        })
                    })
            });
        let summary_model = self.model.clone();
        let summary_button = Button::new("language-summary-primary")
            .label(if languages.summary_in_primary {
                strings::LANGUAGE_SUMMARY_ON
            } else {
                strings::LANGUAGE_SUMMARY_OFF
            })
            .selected(languages.summary_in_primary)
            .on_click(move |_, _, cx| {
                summary_model.update(cx, |m, cx| {
                    m.change_languages(cx, |mut l| {
                        l.summary_in_primary = !l.summary_in_primary;
                        Ok(l)
                    });
                    cx.notify();
                })
            });
        // The selection is also written out, so it does not rest on the
        // button highlight alone (screen readers, high contrast).
        let current = {
            let extra: Vec<String> = languages.extra.iter().map(|c| label(c)).collect();
            let extra = if extra.is_empty() {
                strings::LANGUAGE_NO_EXTRA.to_string()
            } else {
                extra.join(", ")
            };
            strings::language_current(&format!(
                "{}, {}",
                label(&languages.primary),
                strings::language_extra_list(&extra)
            ))
        };
        v_flex()
            .gap(space::SM)
            .p(space::LG)
            .rounded(theme::RADIUS_LG)
            .border_1()
            .border_color(palette(cx).border)
            .bg(palette(cx).card)
            .child(section_title(strings::LANGUAGES))
            .child(paragraph(&current, cx))
            .child(muted(strings::LANGUAGE_PRIMARY, cx))
            .child(muted(strings::LANGUAGE_PRIMARY_HELP, cx))
            .child(
                h_flex()
                    .gap(space::SM)
                    .flex_wrap()
                    .children(primary_buttons),
            )
            .child(muted(strings::LANGUAGE_EXTRA, cx))
            .child(muted(strings::LANGUAGE_EXTRA_HELP, cx))
            .child(h_flex().gap(space::SM).flex_wrap().children(extra_buttons))
            .child(summary_button)
            .child(muted(strings::LANGUAGE_PROVIDERS, cx))
            .child(muted(strings::LANGUAGE_NEW_MEETINGS, cx))
            .children(error.map(|e| paragraph(&e, cx)))
    }

    fn database_error_page(&self, failure: &DbFailure, cx: &mut Context<Self>) -> AnyElement {
        let palette = palette(cx);
        let model = self.model.read(cx);
        let log_dir = model.log_dir();
        let log_line = match &model.log_path {
            Some(path) => strings::log_file(&path.display().to_string()),
            None => strings::LOG_FAILED.to_string(),
        };
        v_flex()
            .id("database-error")
            .size_full()
            .items_center()
            .justify_center()
            .p(space::XXL)
            .overflow_y_scroll()
            .child(
                v_flex()
                    .max_w(layout::NOTICE_MAX_WIDTH)
                    .gap(space::MD)
                    .child(
                        Icon::from(IconName::TriangleAlert)
                            .size(icon_size::ERROR)
                            .text_color(palette.destructive),
                    )
                    .child(heading(strings::DATABASE_FAILED))
                    .child(paragraph(&failure.summary, cx))
                    .child(paragraph(&failure.advice, cx))
                    .child(muted(
                        &strings::database(&failure.database_path.display().to_string()),
                        cx,
                    ))
                    .child(muted(&log_line, cx))
                    .child(
                        div()
                            .p(space::SM)
                            .rounded(theme::RADIUS)
                            .bg(palette.muted)
                            .font_family(cx.theme().mono_font_family.clone())
                            .text_size(typography::XS)
                            .text_color(palette.muted_foreground)
                            .child(failure.detail.clone()),
                    )
                    .child(
                        h_flex()
                            .pt(space::SM)
                            .gap(space::SM)
                            .child(
                                Button::new("db-error-open-log")
                                    .primary()
                                    .label(strings::OPEN_LOG)
                                    .icon(IconName::FolderOpen)
                                    .on_click(move |_, _, _| {
                                        if let Some(dir) = &log_dir {
                                            startup::open_folder(dir);
                                        }
                                    }),
                            )
                            .child(
                                Button::new("db-error-retry")
                                    .label(strings::RETRY)
                                    .icon(IconName::RotateCcw)
                                    .on_click(|_, _, cx| load_database(cx)),
                            ),
                    ),
            )
            .into_any_element()
    }
}

impl Render for Shell {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = palette(cx);
        let failure = match &self.model.read(cx).db {
            DbState::Failed(failure) => Some(failure.clone()),
            _ => None,
        };
        let content = if let Some(comparison) = &self.model.read(cx).rerun_comparison {
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
                    Button::new("close-rerun-comparison")
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
                body.into_any_element(),
                cx,
            )
        } else {
            match (&failure, self.page) {
                (_, Page::About) => self.about_page(cx),
                (Some(failure), _) => self.database_error_page(failure, cx),
                (None, Page::Meetings) => self.meetings_page(cx),
                (None, Page::Dictation) => self.dictation_page(cx),
                (None, Page::Settings) => self.settings_page(cx),
                (None, Page::Measurement) => self.measurement.clone().into_any_element(),
            }
        };
        h_flex()
            .track_focus(&self.focus_handle)
            .size_full()
            .bg(palette.background)
            .text_color(palette.foreground)
            .font_family(typography::SANS)
            .text_size(typography::BASE)
            .line_height(relative(typography::LINE_HEIGHT))
            .when(failure.is_none(), |this| this.child(self.sidebar(cx)))
            .child(div().flex_1().h_full().min_w_0().child(content))
    }
}

/// The Tingskriver palette for the current mode (tokens gpui-component's
/// theme has no slot for, such as `recording`, live here).
fn palette(cx: &App) -> &'static Palette {
    Palette::for_mode(cx.theme().mode)
}

pub(crate) fn page(
    title: &str,
    subtitle: Option<&str>,
    virtualized: bool,
    body: AnyElement,
    cx: &App,
) -> AnyElement {
    v_flex()
        .id(SharedString::from(format!("page-{title}")))
        .size_full()
        .when(!virtualized, |this| this.overflow_y_scroll())
        .px(space::XXL)
        .py(space::XL)
        .child(
            v_flex()
                .w_full()
                .max_w(layout::CONTENT_MAX_WIDTH)
                .when(virtualized, |this| this.h_full().min_h_0())
                .gap(space::LG)
                .child(heading(title))
                .when_some(subtitle, |this, subtitle| this.child(muted(subtitle, cx)))
                .child(body),
        )
        .into_any_element()
}

fn heading(text: &str) -> impl IntoElement {
    div()
        .font_family(typography::SERIF)
        .font_weight(FontWeight::SEMIBOLD)
        .text_size(typography::XXL)
        .line_height(relative(1.2))
        .child(text.to_string())
}

pub(crate) fn section_title(text: &str) -> impl IntoElement {
    div()
        .font_weight(FontWeight::SEMIBOLD)
        .text_size(typography::LG)
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

/// The voiceprint explanation, Danish first and English below on the same
/// page. Voiceprints are off by default and there is no switch yet, so the
/// status line says off.
fn voiceprint_lines() -> Vec<String> {
    use tingskriver_core::voiceprint::texts;
    let paragraphs = texts::explanation();
    let mut lines = vec![strings::VOICEPRINT_STATUS_OFF.to_string()];
    lines.extend(paragraphs.iter().map(|p| p.da.to_string()));
    lines.push(texts::EN_TITLE.to_string());
    lines.push(texts::EN_STATUS_OFF.to_string());
    lines.extend(paragraphs.iter().map(|p| p.en.to_string()));
    lines
}

fn setting_group(title: &str, lines: Vec<String>, cx: &App) -> impl IntoElement {
    let palette = palette(cx);
    v_flex()
        .gap(space::XS)
        .p(space::LG)
        .rounded(theme::RADIUS_LG)
        .border_1()
        .border_color(palette.border)
        .bg(palette.card)
        .child(section_title(title))
        .children(lines.into_iter().map(|line| muted(&line, cx)))
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

pub(crate) fn interrupted_card(notice: &InterruptNotice, cx: &App) -> AnyElement {
    let palette = palette(cx);
    let lines = notice.lines();
    v_flex()
        .id("interrupted-notice")
        .role(gpui::Role::Alert)
        .aria_label(SharedString::from(strings::INTERRUPTED_TITLE))
        .gap(space::SM)
        .p(space::LG)
        .rounded(theme::RADIUS_LG)
        .border_1()
        .border_color(palette.warning)
        .bg(palette.card)
        .child(section_title(strings::INTERRUPTED_TITLE))
        .children(lines.into_iter().map(|line| paragraph(&line, cx)))
        .child(
            h_flex().pt(space::SM).child(
                Button::new("interrupted-dismiss")
                    .primary()
                    .label(strings::INTERRUPTED_DISMISS)
                    .on_click(|_, _, cx| dismiss_interrupted(cx)),
            ),
        )
        .into_any_element()
}

pub(crate) fn empty_state(icon: IconName, title: &str, text: &str, cx: &App) -> AnyElement {
    let palette = palette(cx);
    v_flex()
        .items_center()
        .gap(space::SM)
        .py(space::XXL)
        .px(space::XL)
        .rounded(theme::RADIUS_LG)
        .border_1()
        .border_color(palette.border)
        .bg(palette.card)
        .child(
            Icon::from(icon)
                .size(icon_size::EMPTY_STATE)
                .text_color(palette.muted_foreground),
        )
        .child(section_title(title))
        .child(
            div()
                .max_w(layout::EMPTY_TEXT_MAX_WIDTH)
                .text_center()
                .text_size(typography::SM)
                .text_color(palette.muted_foreground)
                .child(text.to_string()),
        )
        .into_any_element()
}

fn save_mcp_controls(
    model: Entity<AppModel>,
    value: tingskriver_core::mcp::Settings,
    cx: &mut App,
) {
    let state = model.read(cx);
    let DbState::Ready(db) = &state.db else {
        return;
    };
    let pool = db.pool().clone();
    let task = state
        .runtime
        .spawn(async move { tingskriver_core::mcp::save_settings(&pool, value).await });
    model.update(cx, |state, cx| {
        state.mcp_pending = true;
        cx.notify();
    });
    cx.spawn(async move |cx| {
        let result = task.await;
        model.update(cx, |state, cx| {
            state.mcp_pending = false;
            if matches!(result, Ok(Ok(()))) {
                state.mcp_controls = Some(value);
                state.mcp_message = strings::MCP_SAVED.into();
            } else {
                state.mcp_message = strings::MCP_SAVE_FAILED.into();
            }
            cx.notify();
        });
    })
    .detach();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigation_is_danish_and_ordered() {
        let labels: Vec<_> = Page::ALL.iter().map(|p| p.label()).collect();
        assert_eq!(
            labels,
            ["Møder", "Diktering", "Måling", "Indstillinger", "Om"]
        );
    }

    #[test]
    fn pages_can_be_named_on_the_command_line() {
        assert_eq!(Page::from_arg("settings"), Some(Page::Settings));
        assert_eq!(Page::from_arg("about"), Some(Page::About));
        assert_eq!(Page::from_arg("Om"), Some(Page::About));
        assert_eq!(Page::from_arg("Diktering"), Some(Page::Dictation));
        assert_eq!(Page::from_arg("møder"), Some(Page::Meetings));
        assert_eq!(Page::from_arg("assistent"), None);
    }
}
