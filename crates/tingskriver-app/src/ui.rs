//! The main window: sidebar navigation and the (placeholder) pages.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use gpui::{
    div, prelude::*, px, relative, size, AnyElement, App, Bounds, Context, Entity, FontWeight,
    Global, SharedString, Subscription, Task, TitlebarOptions, Window, WindowBounds, WindowHandle,
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
use tokio::runtime::Handle;

use crate::startup::{self, DbFailure, DbState, StartupClock};
use crate::theme::{self, layout, space, typography, Palette};
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
    first_frame_logged: bool,
}

impl AppModel {
    pub fn new(runtime: Handle, clock: StartupClock, log_path: Option<PathBuf>) -> Self {
        AppModel {
            db: DbState::Loading,
            app_data_dir: tingskriver_core::paths::app_data_dir(),
            log_path,
            runtime,
            clock,
            start_page: Page::Meetings,
            first_frame_logged: false,
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
}

impl Global for Globals {}

pub fn init(model: Entity<AppModel>, cx: &mut App) {
    cx.set_global(Globals {
        model,
        main_window: None,
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
        Err(error) => log::error!("Could not open the main window: {error:#}"),
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
                title: Some("Tingskriver".into()),
                ..Default::default()
            }),
            app_id: Some(tingskriver_core::paths::APP_ID.to_string()),
            window_min_size: Some(size(layout::MIN_WINDOW_WIDTH, layout::MIN_WINDOW_HEIGHT)),
            ..Default::default()
        },
        move |window, cx| {
            theme::sync_with_window(window, cx);
            let shell = cx.new(|cx| Shell::new(state_path, window, cx));
            let model = model(cx);
            window.on_next_frame(move |_, cx| {
                model.update(cx, |m, _| {
                    if !m.first_frame_logged {
                        m.first_frame_logged = true;
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
}

impl Page {
    pub const ALL: [Page; 3] = [Page::Meetings, Page::Dictation, Page::Settings];

    /// The page for a `--page` argument (English or Danish name).
    pub fn from_arg(name: &str) -> Option<Page> {
        match name.to_lowercase().as_str() {
            "meetings" | "møder" => Some(Page::Meetings),
            "dictation" | "diktering" => Some(Page::Dictation),
            "settings" | "indstillinger" => Some(Page::Settings),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Page::Meetings => "Møder",
            Page::Dictation => "Diktering",
            Page::Settings => "Indstillinger",
        }
    }

    fn icon(self) -> IconName {
        match self {
            Page::Meetings => IconName::Calendar,
            Page::Dictation => IconName::Mic,
            Page::Settings => IconName::Settings,
        }
    }
}

pub struct Shell {
    model: Entity<AppModel>,
    page: Page,
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
        let probe_line = cx
            .new(|cx| InputState::new(window, cx).placeholder("Skriv fx æøå, é, ü, ê, @ og € her"));
        let probe_text = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder(
                    "Flere linjer: prøv også IME-komposition, fx Windows' emoji-panel (Win+.)",
                )
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
            model,
            page,
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
                        .child("Tingskriver"),
                ),
            )
            .child(SidebarMenu::new().children(Page::ALL.map(|page| {
                SidebarMenuItem::new(page.label())
                    .icon(page.icon())
                    .active(self.page == page)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.page = page;
                        cx.notify();
                    }))
            })))
            .footer(
                div()
                    .px(space::SM)
                    .text_size(typography::XS)
                    .text_color(palette.muted_foreground)
                    .child(format!("Version {}", env!("CARGO_PKG_VERSION"))),
            )
    }

    fn meetings_page(&self, cx: &mut Context<Self>) -> AnyElement {
        let body = match &self.model.read(cx).db {
            DbState::Loading => loading("Indlæser møder …", cx),
            DbState::NotCreated => empty_state(
                IconName::Inbox,
                "Ingen møder endnu",
                "Når du har optaget dit første møde, står det her.",
                cx,
            ),
            DbState::Ready(_) => empty_state(
                IconName::Inbox,
                "Ingen møder at vise endnu",
                "Mødelisten kommer hertil i en senere version. Indtil da finder du dine møder i den nuværende app.",
                cx,
            ),
            DbState::Failed(_) => div().into_any_element(),
        };
        page("Møder", None, body, cx)
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
            .child(paragraph(
                "Hold Ctrl+Win nede, tal, og slip. Teksten skrives der, hvor markøren står. Diktering flytter hertil i en senere version.",
                cx,
            ))
            .child(
                v_flex()
                    .gap(space::SM)
                    .child(section_title("Prøvefelt"))
                    .child(muted(
                        "Brug felterne til at teste tastaturet: æøå, dødtaster (^ ¨ ~ ´ `) efterfulgt af en vokal, AltGr-tegn (@ £ $ € { [ ] } | \\) og IME-komposition.",
                        cx,
                    ))
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
                                "Særlige tegn vises her med deres Unicode-nummer.".to_string()
                            } else {
                                format!("Særlige tegn: {}", described.join(", "))
                            }),
                    ),
            );
        page("Diktering", None, body.into_any_element(), cx)
    }

    fn settings_page(&self, cx: &mut Context<Self>) -> AnyElement {
        let model = self.model.read(cx);
        let mode = if cx.theme().mode.is_dark() {
            "mørk"
        } else {
            "lys"
        };
        let data_dir = model
            .app_data_dir
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "ukendt".to_string());
        let database = match &model.db {
            DbState::Loading => "åbnes …",
            DbState::NotCreated => "ikke oprettet endnu",
            DbState::Ready(_) => "klar",
            DbState::Failed(_) => "kunne ikke åbnes",
        };
        let log_path = model
            .log_path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "ingen logfil (kun konsollen)".to_string());
        let log_dir = model.log_dir();

        let body = v_flex()
            .gap(space::XL)
            .child(paragraph(
                "Indstillingerne flytter hertil i en senere version. Her kan du se, hvordan appen kører.",
                cx,
            ))
            .child(setting_group(
                "Udseende",
                vec![format!(
                    "Følger Windows' indstilling for lys og mørk tilstand. Lige nu: {mode}."
                )],
                cx,
            ))
            .child(setting_group(
                "Data",
                vec![format!("Datamappe: {data_dir}"), format!("Database: {database}")],
                cx,
            ))
            .child(
                v_flex()
                    .gap(space::SM)
                    .child(setting_group("Log", vec![format!("Logfil: {log_path}")], cx))
                    .child(
                        h_flex()
                            .gap(space::SM)
                            .child(
                                Button::new("open-log-dir")
                                    .label("Åbn logmappen")
                                    .icon(IconName::FolderOpen)
                                    .on_click(move |_, _, _| {
                                        if let Some(dir) = &log_dir {
                                            startup::open_folder(dir);
                                        }
                                    }),
                            )
                            .child(
                                Button::new("quit")
                                    .label("Afslut Tingskriver")
                                    .on_click(|_, _, cx| cx.quit()),
                            ),
                    ),
            );
        page("Indstillinger", None, body.into_any_element(), cx)
    }

    fn database_error_page(&self, failure: &DbFailure, cx: &mut Context<Self>) -> AnyElement {
        let palette = palette(cx);
        let model = self.model.read(cx);
        let log_dir = model.log_dir();
        let log_line = match &model.log_path {
            Some(path) => format!("Logfil: {}", path.display()),
            None => "Logfilen kunne ikke oprettes, så detaljerne står kun i konsollen.".to_string(),
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
                    .max_w(px(600.0))
                    .gap(space::MD)
                    .child(
                        Icon::from(IconName::TriangleAlert)
                            .size(px(32.0))
                            .text_color(palette.destructive),
                    )
                    .child(heading("Tingskriver kunne ikke åbne dine møder"))
                    .child(paragraph(&failure.summary, cx))
                    .child(paragraph(&failure.advice, cx))
                    .child(muted(
                        &format!("Database: {}", failure.database_path.display()),
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
                                    .label("Åbn logmappen")
                                    .icon(IconName::FolderOpen)
                                    .on_click(move |_, _, _| {
                                        if let Some(dir) = &log_dir {
                                            startup::open_folder(dir);
                                        }
                                    }),
                            )
                            .child(
                                Button::new("db-error-retry")
                                    .label("Prøv igen")
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
            (Some(failure), _) => self.database_error_page(failure, cx),
            (None, Page::Meetings) => self.meetings_page(cx),
            (None, Page::Dictation) => self.dictation_page(cx),
            (None, Page::Settings) => self.settings_page(cx),
        };
        h_flex()
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

fn page(title: &str, subtitle: Option<&str>, body: AnyElement, cx: &App) -> AnyElement {
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

fn section_title(text: &str) -> impl IntoElement {
    div()
        .font_weight(FontWeight::SEMIBOLD)
        .text_size(typography::LG)
        .child(text.to_string())
}

fn paragraph(text: &str, cx: &App) -> impl IntoElement {
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
                .size(px(28.0))
                .text_color(palette.muted_foreground),
        )
        .child(section_title(title))
        .child(
            div()
                .max_w(px(420.0))
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
        assert_eq!(labels, ["Møder", "Diktering", "Indstillinger"]);
    }

    #[test]
    fn pages_can_be_named_on_the_command_line() {
        assert_eq!(Page::from_arg("settings"), Some(Page::Settings));
        assert_eq!(Page::from_arg("Diktering"), Some(Page::Dictation));
        assert_eq!(Page::from_arg("møder"), Some(Page::Meetings));
        assert_eq!(Page::from_arg("assistent"), None);
    }
}
