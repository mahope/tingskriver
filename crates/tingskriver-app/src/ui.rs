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
    v_flex, ActiveTheme, Icon, Root,
};
use gpui_kit_assets::IconName;
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
    pub clock: StartupClock,
    /// The page a new window opens on.
    pub start_page: Page,
    /// Whether the first-frame time is still to be reported. Off for
    /// `--hidden`, where the window opens long after process start.
    pub report_first_frame: bool,
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
            clock,
            start_page: Page::Meetings,
            report_first_frame: true,
        }
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

fn model(cx: &App) -> Entity<AppModel> {
    cx.global::<Globals>().model.clone()
}

/// Open the database on the tokio runtime. The window keeps drawing; the
/// pages show a loading state until the result arrives.
pub fn load_database(cx: &mut App) {
    let model = model(cx);
    let (runtime, dir) = model.update(cx, |m, cx| {
        m.db = DbState::Loading;
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
        model.update(cx, |m, cx| {
            log::info!(
                "startup: database {outcome} after {} ms ({} ms after process start)",
                started.elapsed().as_millis(),
                m.clock.elapsed().as_millis()
            );
            m.db = state;
            cx.notify();
        });
    })
    .detach();
}

/// Navigate to About in an existing or newly opened window.
pub fn show_about(cx: &mut App) {
    model(cx).update(cx, |model, cx| {
        model.start_page = Page::About;
        cx.notify();
    });
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
    probe_line: Entity<InputState>,
    probe_text: Entity<TextareaState>,
    state_path: Option<PathBuf>,
    last_saved: Option<window_state::SavedWindow>,
    pending_save: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl Shell {
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
                    .child(strings::version(crate::about::VERSION)),
            )
    }

    fn meetings_page(&self, cx: &mut Context<Self>) -> AnyElement {
        let body = match &self.model.read(cx).db {
            DbState::Loading => loading(strings::LOADING_MEETINGS, cx),
            DbState::NotCreated => empty_state(
                IconName::Inbox,
                strings::NO_MEETINGS,
                strings::FIRST_MEETING,
                cx,
            ),
            DbState::Ready(_) => empty_state(
                IconName::Inbox,
                strings::NO_MEETINGS_YET,
                strings::MEETINGS_LATER,
                cx,
            ),
            DbState::Failed(_) => div().into_any_element(),
        };
        page(strings::MEETINGS, None, body, cx)
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
        page(strings::DICTATION, None, body.into_any_element(), cx)
    }

    fn about_page(&self, cx: &mut Context<Self>) -> AnyElement {
        let body =
            v_flex()
                .gap(space::LG)
                .child(paragraph(&crate::about::metadata(), cx))
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
        page(strings::ABOUT, None, body.into_any_element(), cx)
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
            .child(paragraph(strings::SETTINGS_LATER, cx))
            .child(setting_group(
                strings::APPEARANCE,
                vec![strings::appearance(mode)],
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
        page(strings::SETTINGS, None, body.into_any_element(), cx)
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
        let content = match (&failure, self.page) {
            (_, Page::About) => self.about_page(cx),
            (Some(failure), _) => self.database_error_page(failure, cx),
            (None, Page::Meetings) => self.meetings_page(cx),
            (None, Page::Dictation) => self.dictation_page(cx),
            (None, Page::Settings) => self.settings_page(cx),
            (None, Page::Measurement) => self.measurement.clone().into_any_element(),
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

pub(crate) fn page(title: &str, subtitle: Option<&str>, body: AnyElement, cx: &App) -> AnyElement {
    v_flex()
        .id(SharedString::from(format!("page-{title}")))
        .size_full()
        .overflow_y_scroll()
        .px(space::XXL)
        .py(space::XL)
        .child(
            v_flex()
                .w_full()
                .max_w(layout::CONTENT_MAX_WIDTH)
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

fn muted(text: &str, cx: &App) -> impl IntoElement {
    div()
        .text_size(typography::SM)
        .text_color(palette(cx).muted_foreground)
        .child(text.to_string())
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

fn loading(text: &str, cx: &App) -> AnyElement {
    h_flex()
        .gap(space::SM)
        .py(space::XL)
        .text_color(palette(cx).muted_foreground)
        .child(Spinner::new())
        .child(text.to_string())
        .into_any_element()
}

fn empty_state(icon: IconName, title: &str, text: &str, cx: &App) -> AnyElement {
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
