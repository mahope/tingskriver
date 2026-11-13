//! The Indstillinger page as in the Tauri app (`app/settings/page.tsx`): a
//! header with "Tilbage" and the title, underline tabs, and the Generelt tab
//! (`PreferenceSettings.tsx`, `delivery/DeliverySettings.tsx`,
//! `mcp/McpSettings.tsx`). Optagelser, Transskription, Opsummering and Beta
//! are in `settings_tabs` (docs/design-paritet.md, Job 6); Diktering is
//! Job 7.

use std::path::PathBuf;

use gpui::{
    div, prelude::*, relative, AnyElement, App, Context, Entity, FocusHandle, FontWeight,
    KeyDownEvent, SharedString, Subscription, Window,
};
use gpui_component::{
    h_flex,
    select::{Select, SelectEvent, SelectState},
    v_flex, ActiveTheme, Disableable, Icon, IndexPath, Selectable, WindowExt,
};
use gpui_kit_assets::IconName;
use tingskriver_core::appearance::Appearance;
use tingskriver_core::mcp::Access;
use tingskriver_core::strings;

use crate::controls::{self, Segment, Variant};
use crate::startup::{self, DbState};
use crate::theme::{self, icon_size, layout, settings as size, space, typography};
use crate::ui::{heading, muted, palette, paragraph, section_title, AppModel, Level, Page, Shell};

/// The tabs, in Tauri's order (`TABS` in `app/settings/page.tsx`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SettingsTab {
    #[default]
    General,
    Recording,
    Transcription,
    Summary,
    Dictation,
    Beta,
}

impl SettingsTab {
    pub const ALL: [SettingsTab; 6] = [
        SettingsTab::General,
        SettingsTab::Recording,
        SettingsTab::Transcription,
        SettingsTab::Summary,
        SettingsTab::Dictation,
        SettingsTab::Beta,
    ];

    /// The fixture name (`settings-<slug>`).
    pub fn slug(self) -> &'static str {
        match self {
            SettingsTab::General => "general",
            SettingsTab::Recording => "recording",
            SettingsTab::Transcription => "transcription",
            SettingsTab::Summary => "summary",
            SettingsTab::Dictation => "dictation",
            SettingsTab::Beta => "beta",
        }
    }

    fn label(self) -> &'static str {
        match self {
            SettingsTab::General => strings::SETTINGS_TAB_GENERAL,
            SettingsTab::Recording => strings::SETTINGS_TAB_RECORDING,
            SettingsTab::Transcription => strings::SETTINGS_TAB_TRANSCRIPTION,
            SettingsTab::Summary => strings::SETTINGS_TAB_SUMMARY,
            SettingsTab::Dictation => strings::DICTATION,
            SettingsTab::Beta => strings::SETTINGS_TAB_BETA,
        }
    }

    fn icon(self) -> IconName {
        match self {
            SettingsTab::General => IconName::Settings2,
            SettingsTab::Recording => IconName::Mic,
            SettingsTab::Transcription => IconName::Database,
            SettingsTab::Summary => IconName::Sparkle,
            SettingsTab::Dictation => IconName::AudioLines,
            SettingsTab::Beta => IconName::FlaskConical,
        }
    }

    fn index(self) -> usize {
        Self::ALL.iter().position(|t| *t == self).unwrap_or(0)
    }
}

/// Settings state shared by every window (part of `AppModel`).
#[derive(Debug, Default)]
pub(crate) struct SettingsModel {
    /// Where recordings are saved (core's default folder, which the
    /// recorder and the import use). `None` in screenshot fixtures.
    pub(crate) recordings_dir: Option<PathBuf>,
    /// Why the Udseende choice could not be read or saved.
    pub(crate) appearance_error: Option<String>,
    /// Counts Udseende saves, so only the newest one reports its result.
    pub(crate) appearance_saves: u64,
    /// Aflevering af referat (`settings_delivery`).
    pub(crate) delivery: super::settings_delivery::DeliveryModel,
    /// The saved engine and summary model (Job 6, `settings_tabs`).
    pub(crate) saved_models: super::settings_tabs::SavedModels,
    /// Whether Windows keeps crash dumps; `None` outside Windows.
    pub(crate) crash_dumps: Option<bool>,
    pub(crate) crash_dumps_pending: bool,
    pub(crate) crash_dumps_failed: bool,
    /// "Notifikationer om start og slut" (Generelt): the Tauri app's
    /// `notifications.json`, read at startup.
    pub(crate) notifications: bool,
    pub(crate) notifications_pending: bool,
    pub(crate) notifications_failed: bool,
    /// The file could not be read (or has no place): the switch is locked.
    pub(crate) notifications_unreadable: bool,
}

impl SettingsModel {
    /// The state with the notification switch read (`Err`: locked).
    fn with_notifications(read: Result<bool, String>) -> Self {
        if let Err(error) = &read {
            log::warn!("The notification settings could not be read: {error}");
        }
        SettingsModel {
            notifications: read.clone().unwrap_or(false),
            notifications_unreadable: read.is_err(),
            ..Default::default()
        }
    }

    pub(crate) fn load() -> Self {
        SettingsModel {
            recordings_dir: Some(tingskriver_core::audio::get_default_recordings_folder()),
            crash_dumps: cfg!(windows).then(tingskriver_core::crash_dumps::enabled),
            ..Self::with_notifications(notifications_enabled())
        }
    }
}

/// Whether the start and end of a recording are announced, in the file the
/// Tauri app keeps (`tingskriver_core::notification_settings`).
fn notifications_enabled() -> Result<bool, String> {
    let path = tingskriver_core::notification_settings::settings_path()
        .ok_or_else(|| "no settings folder".to_string())?;
    tingskriver_core::notification_settings::recording_notifications_enabled(&path)
}

/// Read the saved Udseende choice before the first window opens, so the app
/// starts in the right mode.
pub(crate) fn load_appearance(model: &Entity<AppModel>, cx: &mut App) {
    let Some(dir) = model.read(cx).app_data_dir.clone() else {
        return;
    };
    match Appearance::load(&dir) {
        Ok(choice) => theme::set_choice(choice, cx),
        Err(error) => {
            log::warn!("{error}");
            model.update(cx, |m, _| {
                m.settings.appearance_error = Some(strings::appearance_load_failed(&error))
            });
        }
    }
}

/// Note the page being shown, so "Tilbage" in Indstillinger returns to it.
/// Called on every draw of the shell; Indstillinger, its sub-page Måling and
/// the About dialog are not places to return to.
pub(crate) fn remember_return(shell: &mut Shell, cx: &App) {
    let meeting = shell.meetings.read(cx).selected.clone();
    if let Some(back) = return_point(shell.page, meeting) {
        shell.settings.back = Some(back);
    }
}

/// The place "Tilbage" returns to when `page` is shown with `meeting` open,
/// or `None` when `page` is not a place to return to.
fn return_point(page: Page, meeting: Option<String>) -> Option<(Page, Option<String>)> {
    returns_here(page).then_some((page, meeting))
}

/// Where "Tilbage" goes from the remembered `back`: Home if nothing was
/// remembered, and no meeting if the remembered one no longer exists (it
/// was deleted while Indstillinger was open).
fn back_target(
    back: Option<(Page, Option<String>)>,
    exists: impl Fn(&str) -> bool,
) -> (Page, Option<String>) {
    let (page, meeting) = back.unwrap_or((Page::Home, None));
    (page, meeting.filter(|id| exists(id)))
}

/// "Tilbage" from Måling, a sub-page of Indstillinger: the settings page on
/// the Transskription tab, where Måling is opened.
pub(crate) const MEASUREMENT_BACK: (Page, SettingsTab) =
    (Page::Settings, SettingsTab::Transcription);

/// Whether "Tilbage" may return to `page`.
fn returns_here(page: Page) -> bool {
    !matches!(page, Page::Settings | Page::Measurement | Page::About)
}

/// The MCP access levels in the order of Tauri's select.
const ACCESS: [(Access, &str); 4] = [
    (Access::Marked, strings::MCP_MARKED),
    (Access::Metadata, strings::MCP_METADATA),
    (Access::All, strings::MCP_ALL),
    (Access::Off, strings::MCP_OFF),
];

fn access_index(access: Option<Access>) -> Option<IndexPath> {
    let access = access?;
    ACCESS
        .iter()
        .position(|(a, _)| *a == access)
        .map(IndexPath::new)
}

type AccessSelect = SelectState<Vec<&'static str>>;

/// Per-window state of the settings page (part of `Shell`).
pub(crate) struct SettingsView {
    pub(crate) tab: SettingsTab,
    tab_focus: Vec<FocusHandle>,
    access: Entity<AccessSelect>,
    /// The add-target form of Aflevering af referat.
    pub(crate) delivery: super::settings_delivery::DeliveryForm,
    /// The Diktering tab (`settings_tabs::dictation`).
    pub(crate) dictation: super::settings_tabs::dictation::DictationTab,
    /// The Optagelser tab (`settings_tabs::recording`).
    pub(crate) recording: super::settings_tabs::recording::RecordingTab,
    /// Where "Tilbage" goes: the last page shown before Indstillinger and
    /// the meeting open on it (`remember_return`).
    pub(crate) back: Option<(Page, Option<String>)>,
    _subscriptions: Vec<Subscription>,
}

impl SettingsView {
    pub(crate) fn new(
        model: &Entity<AppModel>,
        window: &mut Window,
        cx: &mut Context<Shell>,
    ) -> Self {
        let selected = access_index(model.read(cx).mcp_access);
        let access = cx.new(|cx| {
            SelectState::new(
                ACCESS.iter().map(|(_, label)| *label).collect::<Vec<_>>(),
                selected,
                window,
                cx,
            )
        });
        let subscriptions = vec![
            cx.subscribe_in(
                &access,
                window,
                |this: &mut Shell, _, event: &SelectEvent<Vec<&'static str>>, _, cx| {
                    let SelectEvent::Confirm(Some(label)) = event else {
                        return;
                    };
                    if let Some((access, _)) = ACCESS.iter().find(|(_, l)| l == label) {
                        save_access(this.model.clone(), *access, cx);
                    }
                },
            ),
            // The level arrives after the database opens, and a failed save
            // keeps the old one: the select always shows the saved level.
            cx.observe_in(model, window, |this: &mut Shell, model, window, cx| {
                let wanted = access_index(model.read(cx).mcp_access);
                let access = this.settings.access.clone();
                if access.read(cx).selected_index(cx) != wanted {
                    access.update(cx, |select, cx| {
                        select.set_selected_index(wanted, window, cx)
                    });
                }
            }),
        ];
        SettingsView {
            tab: SettingsTab::default(),
            // Roving tab stop: only the active tab is reached with Tab; the
            // arrow keys move between the tabs (`select_tab`).
            tab_focus: SettingsTab::ALL
                .iter()
                .map(|tab| cx.focus_handle().tab_stop(*tab == SettingsTab::default()))
                .collect(),
            access,
            delivery: super::settings_delivery::DeliveryForm::new(window, cx),
            dictation: super::settings_tabs::dictation::DictationTab::new(window, cx),
            recording: super::settings_tabs::recording::RecordingTab::new(window, cx),
            back: None,
            _subscriptions: subscriptions,
        }
    }
}

impl SettingsView {
    /// Show a tab and make it the one tab stop of the tab list.
    pub(crate) fn select_tab(&mut self, tab: SettingsTab) {
        self.tab = tab;
        if tab == SettingsTab::Dictation {
            self.dictation.shown();
        }
        if tab == SettingsTab::Recording {
            self.recording.shown();
        }
        for (index, handle) in self.tab_focus.iter_mut().enumerate() {
            *handle = handle.clone().tab_stop(index == tab.index());
        }
    }
}

impl Shell {
    pub(crate) fn settings_page(&self, cx: &mut Context<Self>) -> AnyElement {
        let header = self.sub_page_header(
            "settings-back",
            strings::SETTINGS,
            |this, cx| this.leave_settings(cx),
            cx,
        );
        let tab = self.settings.tab;
        let panel = match tab {
            SettingsTab::General => self.general_tab(cx),
            SettingsTab::Recording => self.recording_settings_tab(cx),
            SettingsTab::Transcription => self.transcription_settings_tab(cx),
            SettingsTab::Summary => self.summary_settings_tab(cx),
            SettingsTab::Beta => self.beta_settings_tab(cx),
            SettingsTab::Dictation => self.dictation_settings_tab(cx),
        };
        let body = v_flex()
            .id("settings-scroll")
            .flex_1()
            .min_h_0()
            .w_full()
            .overflow_y_scroll()
            .child(
                v_flex()
                    .w_full()
                    .max_w(layout::SETTINGS_MAX)
                    .px(space::XXL)
                    .pt(space::XL)
                    .pb(space::XXL)
                    .child(self.tab_bar(cx))
                    .child(
                        div()
                            .id("settings-panel")
                            .role(gpui::Role::TabPanel)
                            .aria_label(SharedString::from(tab.label()))
                            .pt(space::SM)
                            .child(panel),
                    ),
            );
        v_flex()
            .id("page-settings")
            .size_full()
            .child(header)
            .child(body)
            .into_any_element()
    }

    /// The Tauri settings header: sticky, bottom border, px 32 py 24, a muted
    /// "Tilbage" and the 30 px title. Indstillinger and its sub-page Måling
    /// (Job 6) share it.
    pub(crate) fn sub_page_header(
        &self,
        back_id: &'static str,
        title: &str,
        on_back: fn(&mut Shell, &mut Context<Shell>),
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let palette = palette(cx);
        div()
            .w_full()
            .flex_none()
            .border_b_1()
            .border_color(palette.border)
            .bg(palette.background)
            .child(
                h_flex()
                    .w_full()
                    .max_w(layout::SETTINGS_MAX)
                    .px(space::XXL)
                    .py(space::XL)
                    .gap(space::LG)
                    .child(
                        controls::button(back_id, Variant::Ghost)
                            .label(strings::SETTINGS_BACK)
                            .icon(Icon::new(IconName::ArrowLeft).size(icon_size::MD))
                            .px(space::SM)
                            .text_size(typography::BASE)
                            .font_weight(FontWeight::NORMAL)
                            .text_color(palette.muted_foreground)
                            .self_center()
                            .on_click(cx.listener(move |this, _, _, cx| on_back(this, cx))),
                    )
                    .child(heading(title, Level::Page)),
            )
    }

    /// "Tilbage" from Indstillinger: the page it was opened from, with the
    /// meeting that was open (Tauri: `router.back()`). Home if unknown.
    fn leave_settings(&mut self, cx: &mut Context<Self>) {
        let meetings = self.meetings.read(cx);
        let (page, meeting) = back_target(self.settings.back.clone(), |id| meetings.contains(id));
        self.page = page;
        if meeting.is_some() {
            self.meetings.update(cx, |meetings, cx| {
                meetings.selected = meeting;
                cx.notify();
            });
        }
        cx.notify();
    }

    /// Underline tabs: 16 px icon and label, the active one in primary with a
    /// 2 px bar under it. Left and right arrows (and Home and End) move
    /// between tabs, as in Radix tabs.
    fn tab_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = palette(cx);
        let current = self.settings.tab;
        h_flex()
            .id("settings-tabs")
            .role(gpui::Role::TabList)
            .aria_label(SharedString::from(strings::SETTINGS_TABS))
            .self_start()
            .max_w_full()
            .overflow_x_scroll()
            .border_b_1()
            .border_color(palette.border)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                let last = SettingsTab::ALL.len() - 1;
                let at = this.settings.tab.index();
                let next = match event.keystroke.key.as_str() {
                    "left" => at.checked_sub(1).unwrap_or(last),
                    "right" => (at + 1) % SettingsTab::ALL.len(),
                    "home" => 0,
                    "end" => last,
                    _ => return,
                };
                this.settings.select_tab(SettingsTab::ALL[next]);
                this.settings.tab_focus[next].focus(window, cx);
                cx.stop_propagation();
                cx.notify();
            }))
            .children(
                SettingsTab::ALL
                    .into_iter()
                    .enumerate()
                    .map(|(index, tab)| {
                        let active = tab == current;
                        let color = if active {
                            palette.primary
                        } else {
                            palette.muted_foreground
                        };
                        h_flex()
                            .id(("settings-tab", index))
                            .track_focus(&self.settings.tab_focus[index])
                            .role(gpui::Role::Tab)
                            .aria_label(SharedString::from(tab.label()))
                            .aria_selected(active)
                            .relative()
                            .flex_none()
                            .gap(space::SM)
                            // Tauri pads 24 px; six tabs in the narrower GPUI
                            // content area (1100 px window) would cut off Beta.
                            .px(space::LG)
                            .py(space::LG)
                            .border_1()
                            .border_color(gpui::transparent_black())
                            .rounded(theme::RADIUS)
                            .text_size(typography::SM)
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(color)
                            .when(!active, |this| {
                                this.hover(|style| style.text_color(palette.foreground))
                            })
                            .focus_visible(|style| style.border_color(palette.ring))
                            .cursor_pointer()
                            .child(Icon::new(tab.icon()).size(icon_size::SM))
                            .child(tab.label())
                            .when(active, |this| {
                                this.child(
                                    div()
                                        .absolute()
                                        .left_0()
                                        .right_0()
                                        .bottom_0()
                                        .h(size::TAB_INDICATOR)
                                        .bg(palette.primary),
                                )
                            })
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.settings.select_tab(tab);
                                this.settings.tab_focus[index].focus(window, cx);
                                cx.notify();
                            }))
                    }),
            )
    }

    fn general_tab(&self, cx: &mut Context<Self>) -> AnyElement {
        let model = self.model.read(cx);
        let palette = palette(cx);
        let choice = theme::choice(cx);
        let appearance_model = self.model.clone();
        let appearance = v_flex()
            .gap(space::SM)
            .child(
                h_flex().child(controls::segmented(
                    "appearance",
                    strings::APPEARANCE,
                    vec![
                        Segment::new(strings::APPEARANCE_SYSTEM).icon(IconName::Monitor),
                        Segment::new(strings::APPEARANCE_LIGHT).icon(IconName::Sun),
                        Segment::new(strings::APPEARANCE_DARK).icon(IconName::Moon),
                    ],
                    Appearance::ALL
                        .iter()
                        .position(|a| *a == choice)
                        .unwrap_or(0),
                    move |index, window, cx| {
                        set_appearance(appearance_model.clone(), Appearance::ALL[index], window, cx)
                    },
                    cx,
                )),
            )
            .children(
                model
                    .settings
                    .appearance_error
                    .as_deref()
                    .map(|e| alert("appearance-error", e, cx)),
            );

        let model = self.model.read(cx);
        let switch_model = self.model.clone();
        let notifications = v_flex()
            .gap(space::SM)
            .child(switch_row(
                "notifications",
                strings::NOTIFICATIONS_LABEL,
                Some(strings::NOTIFICATIONS_HINT),
                controls::switch("notifications-switch")
                    .checked(model.settings.notifications)
                    .disabled(
                        model.settings.notifications_pending
                            || model.settings.notifications_unreadable,
                    )
                    .accessibility_label(strings::NOTIFICATIONS_LABEL)
                    .on_click(move |value, _, cx| {
                        set_notifications(switch_model.clone(), *value, cx)
                    }),
                cx,
            ))
            .when(
                model.settings.notifications_failed || model.settings.notifications_unreadable,
                |this| {
                    this.child(alert(
                        "notifications-error",
                        tingskriver_core::strings::settings_tabs::NOTIFICATIONS_SAVE_FAILED,
                        cx,
                    ))
                },
            );

        let delivery = self.delivery_settings(cx);

        let quit = controls::button("quit", Variant::Outline)
            .label(strings::QUIT)
            .on_click(|_, _, cx| cx.quit());

        v_flex()
            .w_full()
            .max_w(layout::SETTINGS_SECTION_MAX)
            .gap(size::SECTION_GAP)
            .child(settings_section(
                "settings-appearance",
                Some(heading(strings::APPEARANCE, Level::Section).into_any_element()),
                Some(strings::APPEARANCE_HELP),
                appearance.into_any_element(),
                true,
                cx,
            ))
            .child(settings_section(
                "settings-notifications",
                Some(heading(strings::NOTIFICATIONS_TITLE, Level::Section).into_any_element()),
                Some(strings::NOTIFICATIONS_HELP),
                notifications.into_any_element(),
                false,
                cx,
            ))
            .child(settings_section(
                "settings-storage",
                Some(heading(strings::STORAGE_TITLE, Level::Section).into_any_element()),
                Some(strings::STORAGE_HELP),
                self.storage(cx),
                false,
                cx,
            ))
            .child(settings_section(
                "settings-delivery",
                Some(subheading(strings::DELIVERY_TITLE)),
                Some(strings::DELIVERY_HELP),
                delivery.into_any_element(),
                false,
                cx,
            ))
            .child(settings_section(
                "settings-mcp",
                Some(subheading(strings::MCP_TITLE)),
                Some(strings::MCP_PRIVACY),
                self.mcp_settings(cx),
                false,
                cx,
            ))
            .child(
                div()
                    .border_t_1()
                    .border_color(palette.border)
                    .pt(space::XXL)
                    .child(quit),
            )
            .into_any_element()
    }

    /// Placering af data: recordings folder, log folder, crash dumps
    /// (Windows) and, as GPUI extras, the data folder and database state.
    fn storage(&self, cx: &mut Context<Self>) -> AnyElement {
        let model = self.model.read(cx);
        let recordings = model.settings.recordings_dir.clone();
        let log_dir = model.log_dir();
        let log_path = model
            .log_path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| strings::NO_LOG.to_string());
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
        let runtime = model.runtime.clone();
        let mono = cx.theme().mono_font_family.clone();
        let mut body = v_flex()
            .child(field_label(strings::STORAGE_RECORDINGS))
            .child(
                div().mt(space::XS).mb(space::MD).child(match &recordings {
                    Some(dir) => div()
                        .text_size(typography::SM)
                        .text_color(palette(cx).muted_foreground)
                        .font_family(mono.clone())
                        .child(dir.display().to_string())
                        .into_any_element(),
                    None => muted(strings::STORAGE_UNKNOWN, cx).into_any_element(),
                }),
            )
            .child(
                controls::button("open-recordings", Variant::Outline)
                    .label(strings::STORAGE_OPEN)
                    .icon(IconName::FolderOpen)
                    .disabled(recordings.is_none())
                    .on_click(move |_, _, _| {
                        if let Some(dir) = recordings.clone() {
                            open_recordings_folder(&runtime, dir);
                        }
                    }),
            )
            .child(
                div()
                    .mt(space::XL)
                    .child(field_label(strings::STORAGE_LOGS)),
            )
            .child(
                div()
                    .mt(space::XS)
                    .child(muted(strings::STORAGE_LOGS_HELP, cx)),
            )
            .child(
                div()
                    .mt(space::XS)
                    .mb(space::MD)
                    .child(muted(&strings::log_file(&log_path), cx)),
            )
            .child(
                controls::button("open-log-dir", Variant::Outline)
                    .label(strings::STORAGE_OPEN_LOGS)
                    .icon(IconName::FolderOpen)
                    .on_click(move |_, _, _| {
                        if let Some(dir) = &log_dir {
                            startup::open_folder(dir);
                        }
                    }),
            );
        if let Some(enabled) = model.settings.crash_dumps {
            let switch_model = self.model.clone();
            body = body.child(
                div().mt(space::XL).child(switch_row(
                    "crash-dumps",
                    strings::CRASH_DUMPS,
                    Some(strings::CRASH_DUMPS_HINT),
                    controls::switch("crash-dumps-switch")
                        .checked(enabled)
                        .disabled(model.settings.crash_dumps_pending)
                        .accessibility_label(strings::CRASH_DUMPS)
                        .on_click(move |value, _, cx| {
                            set_crash_dumps(switch_model.clone(), *value, cx)
                        }),
                    cx,
                )),
            );
            if model.settings.crash_dumps_failed {
                body = body.child(div().mt(space::SM).child(alert(
                    "crash-dumps-error",
                    strings::CRASH_DUMPS_FAILED,
                    cx,
                )));
            }
        }
        body.child(
            div()
                .mt(space::XL)
                .child(muted(strings::STORAGE_FOOTER, cx)),
        )
        .child(
            div()
                .mt(space::SM)
                .child(muted(&strings::data_dir(&data_dir), cx)),
        )
        .child(muted(&strings::database(database), cx))
        .into_any_element()
    }

    fn mcp_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        let state = self.model.read(cx);
        let mono = cx.theme().mono_font_family.clone();
        let palette = palette(cx);
        let mut body = v_flex()
            .gap(space::LG)
            .child(
                v_flex()
                    .gap(space::SM)
                    .child(field_label(strings::MCP_ACCESS_LABEL))
                    .child(
                        Select::new(&self.settings.access)
                            .accessibility_label(strings::MCP_ACCESS_LABEL)
                            .disabled(state.mcp_access.is_none() || state.mcp_pending)
                            .w_full()
                            .h(layout::CONTROL_HEIGHT),
                    ),
            )
            .child(muted(strings::MCP_ACCESS_HELP, cx))
            .child(
                div()
                    .id("mcp-result")
                    .role(gpui::Role::Status)
                    .aria_label(state.mcp_message.clone())
                    // Empty until something happens; the region stays for
                    // screen readers.
                    .when(!state.mcp_message.is_empty(), |this| {
                        this.child(paragraph(&state.mcp_message, cx))
                    }),
            );
        // Copy the setup for a client, with the text below the button as in
        // Tauri so it can also be selected by hand.
        if let Some(path) = state.mcp_server.clone() {
            let exists = path.is_file();
            let text_path = path.to_string_lossy().to_string();
            if let Ok(quoted) = serde_json::to_string(&text_path) {
                for (index, (label, client, text)) in [
                    (
                        strings::MCP_COPY_CLAUDE,
                        strings::MCP_CLIENT_CLAUDE,
                        strings::mcp_claude_command(&text_path),
                    ),
                    (
                        strings::MCP_COPY_CODEX,
                        strings::MCP_CLIENT_CODEX,
                        strings::mcp_codex_config(&quoted),
                    ),
                ]
                .into_iter()
                .enumerate()
                {
                    let copy = text.clone();
                    body = body.child(
                        v_flex()
                            .gap(space::SM)
                            .child(
                                controls::button(("mcp-copy", index), Variant::Outline)
                                    .label(label)
                                    .disabled(!exists)
                                    .on_click(move |_, window, cx| {
                                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                                            copy.clone(),
                                        ));
                                        window.push_notification(strings::MCP_COPIED, cx);
                                    }),
                            )
                            .child(
                                div()
                                    .id(("mcp-setup", index))
                                    .role(gpui::Role::Code)
                                    .aria_label(SharedString::from(strings::mcp_setup_label(
                                        client,
                                    )))
                                    .w_full()
                                    .p(space::LG)
                                    .rounded(theme::RADIUS)
                                    .border_1()
                                    .border_color(palette.border)
                                    .font_family(mono.clone())
                                    .text_size(typography::SM)
                                    .child(text),
                            ),
                    );
                }
            }
        }
        self.mcp_details(body, cx).into_any_element()
    }

    /// The GPUI extras of the MCP block: status, allowed data parts, test,
    /// the server's tools and the call log.
    fn mcp_details(&self, mut body: gpui::Div, cx: &mut Context<Self>) -> gpui::Div {
        let state = self.model.read(cx);
        let exe = state.mcp_server.clone();
        let exists = exe.as_ref().is_some_and(|p| p.is_file());
        let status = match state.mcp_controls {
            None if matches!(state.db, DbState::Loading) => strings::MCP_LOADING,
            None => strings::MCP_LOAD_FAILED,
            Some(_) if !exists => strings::MCP_MISSING,
            Some(value) if !value.enabled || state.mcp_access == Some(Access::Off) => {
                strings::MCP_DISABLED
            }
            Some(_) if state.mcp_running > 0 => strings::MCP_ACTIVE,
            Some(_) => strings::MCP_AVAILABLE,
        };
        let mut info = v_flex().gap(space::XS).child(
            div()
                .id("mcp-status")
                .role(gpui::Role::Status)
                .aria_label(status)
                .child(paragraph(status, cx)),
        );
        if state.mcp_running > 0 {
            info = info.child(muted(
                &strings::MCP_RUNNING.replace("{value}", &state.mcp_running.to_string()),
                cx,
            ));
        }
        if let Some(path) = &exe {
            info = info.child(muted(
                &strings::MCP_PATH.replace("{value}", &path.display().to_string()),
                cx,
            ));
        }
        body = body.child(info);
        if let Some(value) = state.mcp_controls {
            let mut parts = v_flex()
                .gap(space::MD)
                .child(muted(strings::MCP_DATA_HELP, cx));
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
                parts = parts.child(switch_row(
                    ("mcp-data", index),
                    label,
                    None,
                    controls::switch(("mcp-data-switch", index))
                        .checked(active)
                        .disabled(state.mcp_pending)
                        .accessibility_label(label)
                        .on_click(move |checked, _, cx| {
                            let mut updated = value;
                            match index {
                                0 => updated.enabled = *checked,
                                1 => updated.notes = *checked,
                                2 => updated.summary = *checked,
                                3 => updated.transcript = *checked,
                                _ => updated.participants = *checked,
                            }
                            save_mcp_controls(model.clone(), updated, cx);
                        }),
                    cx,
                ));
            }
            body = body.child(parts);
        }
        let mut sharing = v_flex().gap(space::XS);
        if let Some(Ok(meetings)) = &state.sharing_meetings {
            sharing = sharing.child(muted(
                &strings::MCP_SHARED_COUNT.replace(
                    "{value}",
                    &meetings.iter().filter(|m| m.shared).count().to_string(),
                ),
                cx,
            ));
        }
        // Sharing is chosen per meeting on its page (pages/meeting.rs).
        body = body.child(sharing.child(muted(strings::MCP_SHARE_HELP, cx)));
        let model = self.model.clone();
        let dir = state.app_data_dir.clone();
        let test_exe = exe.clone();
        body = body.child(
            controls::button("mcp-test", Variant::Outline)
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
        body = body.child(
            v_flex()
                .gap(space::XS)
                .child(section_title(strings::MCP_TOOLS))
                .children(
                    tingskriver_core::mcp_audit::TOOLS
                        .iter()
                        .map(|tool| muted(tool, cx)),
                ),
        );
        let mut buttons = h_flex().gap(space::SM).flex_wrap();
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
                controls::button(("mcp-log", index), Variant::Outline)
                    .label(label)
                    .disabled(state.mcp_pending || state.app_data_dir.is_none())
                    .on_click(move |_, _, cx| {
                        refresh_mcp_log(model.clone(), exe.clone(), clear, cx)
                    }),
            );
        }
        let mut log = v_flex()
            .gap(space::SM)
            .child(muted(strings::MCP_LOG, cx))
            .child(buttons);
        match &state.mcp_log {
            Some(Err(error)) => log = log.child(paragraph(error, cx)),
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
                    log = log.child(muted(&text, cx));
                }
            }
            _ => log = log.child(muted(strings::MCP_LOG_EMPTY, cx)),
        }
        body.child(log)
    }

    /// Meeting and report languages (#209), on the Transskription tab.
    pub(super) fn language_section(&self, cx: &mut Context<Self>) -> impl IntoElement {
        use tingskriver_core::meeting_language::{label, CHOICES};
        let model = self.model.read(cx);
        let languages = model.languages.clone();
        let error = model.language_error.clone();
        let primary_buttons = CHOICES.iter().enumerate().map(|(at, (code, _))| {
            let code = *code;
            let model = self.model.clone();
            let selected = languages.primary == code;
            controls::small_button(("language-primary", at), Variant::Outline)
                .label(label(code))
                .selected(selected)
                .when(selected, |b| b.icon(IconName::Check))
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
                let selected = languages.extra.iter().any(|c| c == code);
                controls::small_button(("language-extra", at), Variant::Outline)
                    .label(label(code))
                    .selected(selected)
                    .when(selected, |b| b.icon(IconName::Check))
                    .on_click(move |_, _, cx| {
                        model.update(cx, |m, cx| {
                            m.change_languages(cx, |l| l.with_extra_toggled(code));
                            cx.notify();
                        })
                    })
            });
        let summary_model = self.model.clone();
        // Job 6: a switch row, as Tauri shows on/off settings.
        let summary_button = controls::switch("language-summary-primary")
            .checked(languages.summary_in_primary)
            .accessibility_label(strings::settings_tabs::LANGUAGE_SUMMARY_SWITCH)
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
        let body = v_flex()
            .gap(space::SM)
            .child(paragraph(&current, cx))
            .child(field_label(strings::LANGUAGE_PRIMARY))
            .child(muted(strings::LANGUAGE_PRIMARY_HELP, cx))
            .child(
                h_flex()
                    .gap(space::SM)
                    .flex_wrap()
                    .children(primary_buttons),
            )
            .child(
                div()
                    .mt(space::SM)
                    .child(field_label(strings::LANGUAGE_EXTRA)),
            )
            .child(muted(strings::LANGUAGE_EXTRA_HELP, cx))
            .child(h_flex().gap(space::SM).flex_wrap().children(extra_buttons))
            .child(div().mt(space::SM).child(switch_row(
                "language-summary",
                strings::settings_tabs::LANGUAGE_SUMMARY_SWITCH,
                Some(strings::settings_tabs::LANGUAGE_SUMMARY_SWITCH_HINT),
                summary_button,
                cx,
            )))
            .child(muted(strings::LANGUAGE_PROVIDERS, cx))
            .child(muted(strings::LANGUAGE_NEW_MEETINGS, cx))
            .children(error.map(|e| alert("language-error", &e, cx)));
        settings_section(
            "settings-languages",
            Some(heading(strings::LANGUAGES, Level::Section).into_any_element()),
            None,
            body.into_any_element(),
            false,
            cx,
        )
    }
}

/// Open the recordings folder in the file manager (Generelt and Optagelser).
/// The folder appears with the first recording, so create it first to give
/// the file manager something to open.
pub(super) fn open_recordings_folder(runtime: &tokio::runtime::Handle, dir: PathBuf) {
    runtime.spawn_blocking(move || {
        if let Err(error) = std::fs::create_dir_all(&dir) {
            log::warn!("Could not create the recordings folder: {error}");
        }
        startup::open_folder(&dir);
    });
}

/// The column of a tab: 672 px (`max-w-2xl`), sections 40 px apart.
pub(super) fn tab_column(children: Vec<AnyElement>) -> AnyElement {
    v_flex()
        .w_full()
        .max_w(layout::SETTINGS_SECTION_MAX)
        .gap(size::SECTION_GAP)
        .children(children)
        .into_any_element()
}

/// A Tauri settings section: title, 14 px muted description and the
/// controls; every section but the first has a top border and 32 px above.
pub(super) fn settings_section(
    id: &'static str,
    title: Option<AnyElement>,
    help: Option<&str>,
    body: AnyElement,
    first: bool,
    cx: &App,
) -> AnyElement {
    let palette = palette(cx);
    v_flex()
        .id(id)
        .role(gpui::Role::Group)
        .when(!first, |this| {
            this.border_t_1()
                .border_color(palette.border)
                .pt(space::XXL)
        })
        .children(title)
        .when_some(help, |this, help| {
            this.child(div().mt(space::XS).child(muted(help, cx)))
        })
        .child(div().mt(space::LG).child(body))
        .into_any_element()
}

/// The 20 px semibold serif title of the Aflevering and MCP sections.
fn subheading(text: &str) -> AnyElement {
    div()
        .font_family(typography::SERIF)
        .font_weight(FontWeight::SEMIBOLD)
        .text_size(typography::XL)
        .line_height(relative(typography::LINE_HEIGHT_TIGHT))
        .child(text.to_string())
        .into_any_element()
}

/// A control's name: body size, medium (`font-medium text-foreground`).
pub(crate) fn field_label(text: &str) -> impl IntoElement {
    div()
        .font_weight(FontWeight::MEDIUM)
        .child(text.to_string())
}

/// An error line in the destructive color, announced as an alert.
pub(super) fn alert(id: &'static str, text: &str, cx: &App) -> AnyElement {
    div()
        .id(id)
        .role(gpui::Role::Alert)
        .aria_label(SharedString::from(text.to_string()))
        .text_size(typography::SM)
        .text_color(palette(cx).destructive)
        .child(text.to_string())
        .into_any_element()
}

/// Label and hint on the left, the switch on the right.
pub(super) fn switch_row(
    id: impl Into<gpui::ElementId>,
    label: &str,
    hint: Option<&str>,
    switch: gpui_component::switch::Switch,
    cx: &App,
) -> impl IntoElement {
    h_flex()
        .id(id)
        .w_full()
        .justify_between()
        .gap(space::XL)
        .child(
            v_flex()
                .min_w_0()
                .flex_1()
                .child(field_label(label))
                .when_some(hint, |this, hint| this.child(muted(hint, cx))),
        )
        .child(div().flex_none().child(switch))
}

/// Apply a Udseende choice at once and save it in the background.
fn set_appearance(model: Entity<AppModel>, choice: Appearance, window: &mut Window, cx: &mut App) {
    theme::set_choice(choice, cx);
    theme::sync_with_window(window, cx);
    window.refresh();
    let (runtime, dir) = model.read_with(cx, |m, _| (m.runtime.clone(), m.app_data_dir.clone()));
    let Some(dir) = dir else {
        return;
    };
    let generation = model.update(cx, |m, _| {
        m.settings.appearance_saves += 1;
        m.settings.appearance_saves
    });
    *LATEST_APPEARANCE.lock().unwrap_or_else(|e| e.into_inner()) = choice;
    let task = runtime.spawn_blocking(move || {
        // Quick clicks start several saves; whichever runs last writes the
        // newest choice, so the file never ends on an older one.
        let _saving = SAVING_APPEARANCE.lock().unwrap_or_else(|e| e.into_inner());
        let latest = *LATEST_APPEARANCE.lock().unwrap_or_else(|e| e.into_inner());
        latest.save(&dir)
    });
    cx.spawn(async move |cx| {
        let error = match task.await {
            Ok(Ok(())) => None,
            Ok(Err(error)) => Some(error),
            Err(error) => Some(error.to_string()),
        };
        model.update(cx, |m, cx| {
            // Only the newest save decides what the page says.
            if m.settings.appearance_saves == generation {
                m.settings.appearance_error = error.map(|e| strings::appearance_save_failed(&e));
                cx.notify();
            }
        });
    })
    .detach();
}

/// The newest Udseende choice and the lock that orders its saves.
static LATEST_APPEARANCE: std::sync::Mutex<Appearance> = std::sync::Mutex::new(Appearance::System);
static SAVING_APPEARANCE: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn set_crash_dumps(model: Entity<AppModel>, enabled: bool, cx: &mut App) {
    let runtime = model.update(cx, |m, cx| {
        m.settings.crash_dumps_pending = true;
        m.settings.crash_dumps_failed = false;
        cx.notify();
        m.runtime.clone()
    });
    let task = runtime.spawn_blocking(move || tingskriver_core::crash_dumps::set_enabled(enabled));
    cx.spawn(async move |cx| {
        let saved = matches!(task.await, Ok(Ok(())));
        model.update(cx, |m, cx| {
            m.settings.crash_dumps_pending = false;
            m.settings.crash_dumps_failed = !saved;
            if saved {
                m.settings.crash_dumps = Some(enabled);
            }
            cx.notify();
        });
    })
    .detach();
}

/// Save the notification switch in the Tauri app's file, then show what was
/// saved (a failed save leaves the switch as it was).
fn set_notifications(model: Entity<AppModel>, enabled: bool, cx: &mut App) {
    let runtime = model.update(cx, |m, cx| {
        m.settings.notifications_pending = true;
        m.settings.notifications_failed = false;
        cx.notify();
        m.runtime.clone()
    });
    let task = runtime.spawn_blocking(move || {
        let path = tingskriver_core::notification_settings::settings_path()
            .ok_or_else(|| "no settings folder".to_string())?;
        tingskriver_core::notification_settings::set_recording_notifications(&path, enabled)
    });
    cx.spawn(async move |cx| {
        let result = task.await.unwrap_or_else(|error| Err(error.to_string()));
        if let Err(error) = &result {
            log::warn!("The notification settings could not be saved: {error}");
        }
        model.update(cx, |m, cx| {
            m.settings.notifications_pending = false;
            m.settings.notifications_failed = result.is_err();
            if result.is_ok() {
                m.settings.notifications = enabled;
            }
            cx.notify();
        });
    })
    .detach();
}

fn save_access(model: Entity<AppModel>, access: Access, cx: &mut App) {
    let state = model.read(cx);
    let DbState::Ready(db) = &state.db else {
        return;
    };
    let pool = db.pool().clone();
    let task = state
        .runtime
        .spawn(async move { tingskriver_core::mcp::set_access(&pool, access).await });
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
}

fn refresh_mcp_log(model: Entity<AppModel>, exe: Option<PathBuf>, clear: bool, cx: &mut App) {
    let task = model.update(cx, |state, cx| {
        let dir = state
            .app_data_dir
            .clone()
            .expect("button requires data directory");
        state.mcp_pending = true;
        cx.notify();
        state.runtime.spawn(async move {
            if clear {
                tingskriver_core::mcp_audit::clear(&dir).await?;
            }
            let entries = tingskriver_core::mcp_audit::recent(&dir).await?;
            let running = tokio::task::spawn_blocking(move || {
                exe.map(|exe| tingskriver_core::mcp_probe::running_servers(&exe))
                    .unwrap_or(0)
            })
            .await
            .unwrap_or(0);
            Ok::<_, String>((entries, running))
        })
    });
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
    fn tabs_follow_the_tauri_order_with_unique_names() {
        let labels: Vec<_> = SettingsTab::ALL.iter().map(|t| t.label()).collect();
        assert_eq!(
            labels,
            [
                "Generelt",
                "Optagelser",
                "Transskription",
                "Opsummering",
                "Diktering",
                "Beta"
            ]
        );
        for (index, tab) in SettingsTab::ALL.iter().enumerate() {
            assert_eq!(tab.index(), index);
        }
        assert_eq!(SettingsTab::default(), SettingsTab::General);
    }

    #[test]
    fn back_returns_to_pages_but_not_to_settings_or_its_sub_page() {
        assert!(returns_here(Page::Home));
        assert!(returns_here(Page::Dictation));
        assert!(!returns_here(Page::Settings));
        assert!(!returns_here(Page::Measurement));
        assert!(!returns_here(Page::About));
    }

    #[test]
    fn back_restores_the_page_and_meeting_it_came_from() {
        let meeting = Some("m1".to_string());
        let back = return_point(Page::Home, meeting.clone());
        assert_eq!(back, Some((Page::Home, meeting.clone())));
        // Opening Måling or the About dialog from Indstillinger keeps it.
        assert_eq!(return_point(Page::Measurement, None), None);
        assert_eq!(return_point(Page::Settings, None), None);
        assert_eq!(back_target(back, |_| true), (Page::Home, meeting));
        let dictation = return_point(Page::Dictation, None);
        assert_eq!(back_target(dictation, |_| true), (Page::Dictation, None));
    }

    #[test]
    fn back_goes_home_when_nothing_was_remembered() {
        assert_eq!(back_target(None, |_| true), (Page::Home, None));
    }

    #[test]
    fn back_drops_a_meeting_deleted_while_settings_was_open() {
        let back = Some((Page::Home, Some("gone".to_string())));
        assert_eq!(back_target(back, |id| id != "gone"), (Page::Home, None));
    }

    #[test]
    fn back_from_measurement_opens_the_transcription_tab() {
        assert_eq!(
            MEASUREMENT_BACK,
            (Page::Settings, SettingsTab::Transcription)
        );
    }

    #[test]
    fn every_access_level_has_a_select_row() {
        for access in [Access::Marked, Access::Metadata, Access::All, Access::Off] {
            let row = access_index(Some(access)).expect("row");
            assert_eq!(ACCESS[row.row].0, access);
        }
        assert_eq!(access_index(None), None);
    }

    #[test]
    fn the_notification_switch_follows_the_file_and_locks_when_it_is_unreadable() {
        let on = SettingsModel::with_notifications(Ok(true));
        assert!(on.notifications && !on.notifications_unreadable);
        let off = SettingsModel::with_notifications(Ok(false));
        assert!(!off.notifications && !off.notifications_unreadable);
        let damaged = SettingsModel::with_notifications(Err("not valid JSON".to_string()));
        assert!(!damaged.notifications && damaged.notifications_unreadable);
    }
}
