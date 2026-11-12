//! The meeting list in the sidebar (Tauri `Sidebar/index.tsx`): search,
//! day groups, rows with hover actions, the rename and delete dialogs and
//! recovery of interrupted recordings. Disk and database work stays on Tokio;
//! the list is core's `meeting_list::List`, and the virtual list borrows its
//! rows through an index, so a search never copies the meetings.
use crate::{
    controls::{self, Variant},
    startup::DbState,
    theme::{self, icon_size, layout, sidebar as size, space, typography},
    ui::{self, AppModel},
};
use chrono::NaiveDate;
use gpui::{
    div, list as virtual_list, prelude::*, AnyElement, Context, Entity, EventEmitter, FocusHandle,
    FontWeight, KeyDownEvent, ListAlignment, ListState, SharedString, Subscription, Task, Window,
};
use gpui_component::{
    dialog::DialogFooter,
    h_flex,
    input::{InputEvent, InputState},
    notification::Notification,
    v_flex, Disableable, Icon, WindowExt,
};
use gpui_kit_assets::IconName;
use std::{
    collections::{HashMap, HashSet},
    rc::Rc,
    time::Duration,
};
use tingskriver_core::{
    database::repositories::search,
    meeting_list::{self, List, Row},
    recover, strings,
};

/// How long typing pauses before the full-text search runs.
const SEARCH_DEBOUNCE: Duration = Duration::from_millis(200);

enum Action {
    Rename(String, String),
    Delete(String),
    Recover(String),
}

/// What the list tells the shell.
pub enum MeetingsEvent {
    /// A meeting row was opened: show the meeting in the content area.
    Opened,
}

/// The two focus handles of a row: the row itself and the row with its
/// actions, so the actions show while any of them has keyboard focus
/// (Tauri's `group-focus-within`).
struct RowFocus {
    group: FocusHandle,
    row: FocusHandle,
}

pub struct MeetingsView {
    model: Entity<AppModel>,
    list: Rc<List>,
    /// Indices into `list.rows()` that the search shows.
    shown: Rc<Vec<usize>>,
    viewport: ListState,
    recovery: Vec<recover::Recording>,
    error: Option<String>,
    /// The open meeting (the active row).
    pub(crate) selected: Option<String>,
    busy: bool,
    loaded: bool,
    /// Screenshot fixtures: never reload from disk.
    frozen: bool,
    today: NaiveDate,
    pub(crate) search: Entity<InputState>,
    query: String,
    hits: HashSet<String>,
    snippets: HashMap<String, String>,
    searching: bool,
    search_failed: bool,
    search_task: Option<Task<()>>,
    rename_input: Entity<InputState>,
    rename_id: Option<String>,
    focus: HashMap<String, RowFocus>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<MeetingsEvent> for MeetingsView {}

impl MeetingsView {
    pub fn new(model: Entity<AppModel>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| {
            InputState::new(window, cx).placeholder(SharedString::from(strings::SEARCH_PLACEHOLDER))
        });
        let rename_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(SharedString::from(strings::MEETING_NAME_PLACEHOLDER))
        });
        let subscriptions = vec![
            cx.observe(&model, |this, _, cx| {
                this.refresh(cx);
                cx.notify();
            }),
            cx.subscribe(&search, |this, input, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    let query = input.read(cx).value().to_string();
                    this.set_query(query, cx);
                }
            }),
            cx.subscribe_in(
                &rename_input,
                window,
                |this, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::PressEnter { .. }) {
                        this.confirm_rename(window, cx);
                    }
                },
            ),
        ];
        cx.spawn(async move |view, cx| loop {
            cx.background_executor().timer(Duration::from_secs(3)).await;
            if view.update(cx, |this, cx| this.refresh(cx)).is_err() {
                break;
            }
        })
        .detach();
        Self {
            model,
            list: Rc::new(List::default()),
            shown: Rc::new(Vec::new()),
            viewport: ListState::new(0, ListAlignment::Top, space::XXL),
            recovery: vec![],
            error: None,
            selected: None,
            busy: false,
            loaded: false,
            frozen: false,
            today: chrono::Local::now().date_naive(),
            search,
            query: String::new(),
            hits: HashSet::new(),
            snippets: HashMap::new(),
            searching: false,
            search_failed: false,
            search_task: None,
            rename_input,
            rename_id: None,
            focus: HashMap::new(),
            _subscriptions: subscriptions,
        }
    }

    /// Fixed list for screenshot fixtures: no disk, no timers.
    #[cfg(feature = "screenshot-tests")]
    pub(crate) fn freeze(&mut self, list: Option<(List, NaiveDate)>) {
        self.frozen = true;
        if let Some((list, today)) = list {
            self.list = Rc::new(list);
            self.today = today;
            self.loaded = true;
            self.update_shown();
        }
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        if self.busy || self.frozen {
            return;
        }
        let model = self.model.read(cx);
        let db = match &model.db {
            DbState::Ready(db) => Some(db.clone()),
            DbState::NotCreated => None,
            _ => return,
        };
        let jobs = model.meeting_jobs.clone();
        let task = model.runtime.spawn(async move {
            let list = match db {
                Some(db) => meeting_list::load_with_jobs(db.pool(), &jobs).await,
                None => Ok(List::default()),
            };
            let recordings = tokio::task::spawn_blocking(|| {
                recover::scan(
                    &tingskriver_core::audio::recording_preferences::get_default_recordings_folder(
                    ),
                )
                .map(meeting_list::recovery_candidates)
            })
            .await;
            (list, recordings)
        });
        self.busy = true;
        cx.spawn(async move |view, cx| {
            let result = task.await;
            let _ = view.update(cx, |this, cx| {
                this.busy = false;
                this.loaded = true;
                this.today = chrono::Local::now().date_naive();
                match result {
                    Ok((list, recovery)) => {
                        this.error = None;
                        match list {
                            Ok(list) => {
                                if !this.list.same_rows(&list) {
                                    this.list = Rc::new(list);
                                    this.update_shown();
                                } else {
                                    this.list = Rc::new(list);
                                }
                            }
                            Err(_) => this.error = Some(strings::MEETING_LOAD_FAILED.into()),
                        }
                        match recovery {
                            Ok(Ok(recovery)) => this.recovery = recovery,
                            _ => this.error = Some(strings::MEETING_SCAN_FAILED.into()),
                        }
                    }
                    Err(_) => this.error = Some(strings::MEETING_LOAD_FAILED.into()),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Recompute the rows the search shows and reset the viewport when
    /// they changed.
    fn update_shown(&mut self) {
        let shown = meeting_list::matching_rows(&self.list, &self.query, &self.hits);
        if *self.shown != shown {
            self.viewport.reset(shown.len());
            self.shown = Rc::new(shown);
        }
        let ids: HashSet<&str> = self
            .list
            .rows()
            .iter()
            .filter_map(|row| match row {
                Row::Meeting(m) => Some(m.data.id.as_str()),
                Row::Date { .. } => None,
            })
            .collect();
        self.focus.retain(|id, _| ids.contains(id.as_str()));
        if let Some(id) = &self.selected {
            if self.loaded && !ids.contains(id.as_str()) {
                self.selected = None;
            }
        }
    }

    /// Filter by title at once; run the full-text search (titles, notes,
    /// summaries, transcripts) after a short pause in typing.
    fn set_query(&mut self, query: String, cx: &mut Context<Self>) {
        if query == self.query {
            return;
        }
        self.query = query.clone();
        self.hits.clear();
        self.snippets.clear();
        self.search_failed = false;
        self.update_shown();
        let pool = match &self.model.read(cx).db {
            DbState::Ready(db) => Some(db.pool().clone()),
            _ => None,
        };
        let (Some(pool), false) = (pool, query.trim().is_empty()) else {
            self.searching = false;
            self.search_task = None;
            cx.notify();
            return;
        };
        self.searching = true;
        let runtime = self.model.read(cx).runtime.clone();
        self.search_task = Some(cx.spawn(async move |view, cx| {
            cx.background_executor().timer(SEARCH_DEBOUNCE).await;
            let task = runtime.spawn(async move {
                search::search(&pool, &query, &search::SearchFilters::default()).await
            });
            let result = task.await;
            let _ = view.update(cx, |this, cx| {
                this.searching = false;
                match result {
                    Ok(Ok(hits)) => {
                        for hit in hits {
                            this.snippets
                                .entry(hit.meeting_id.clone())
                                .or_insert_with(|| meeting_list::snippet_text(&hit.snippet));
                            this.hits.insert(hit.meeting_id);
                        }
                    }
                    _ => this.search_failed = true,
                }
                this.update_shown();
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn clear_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.set_query(String::new(), cx);
    }

    fn act(&mut self, action: Action, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let model = self.model.read(cx);
        let db = match &model.db {
            DbState::Ready(db) => Some(db.clone()),
            DbState::NotCreated if matches!(&action, Action::Recover(_)) => None,
            _ => return,
        };
        let data_dir = model.app_data_dir.clone();
        let jobs = model.meeting_jobs.clone();
        let (done, failed): (Option<&'static str>, &'static str) = match &action {
            Action::Rename(..) => (
                Some(strings::MEETING_RENAMED),
                strings::MEETING_RENAME_FAILED,
            ),
            Action::Delete(_) => (
                Some(strings::MEETING_DELETED),
                strings::MEETING_DELETE_FAILED,
            ),
            Action::Recover(_) => (None, strings::MEETING_ACTION_FAILED),
        };
        let deleted = match &action {
            Action::Delete(id) => Some(id.clone()),
            _ => None,
        };
        let task = model.runtime.spawn(async move {
            let db = match db {
                Some(db) => db,
                None => tingskriver_core::database::manager::DatabaseManager::new_from_app_data_dir(
                    &data_dir.ok_or_else(|| anyhow::anyhow!(strings::MEASURE_NO_FOLDER))?
                ).await?,
            };
            let mut notice = None;
            match action {
                Action::Rename(id, value) => meeting_list::rename(db.pool(), &id, &value).await?,
                Action::Delete(id) => meeting_list::delete_with_jobs(db.pool(), &id, true, &jobs).await?,
                Action::Recover(id) => {
                    let root = tingskriver_core::audio::recording_preferences::get_default_recordings_folder();
                    let ffmpeg = tokio::task::spawn_blocking(tingskriver_core::audio::ffmpeg::find_ffmpeg_path).await?;
                    let restored = recover::restore(&root, &id, db.pool(), ffmpeg.as_deref()).await?;
                    notice = Some(if restored.audio.status == tingskriver_core::audio::finalize::AudioSaveStatus::Partial {
                        strings::MEETING_RECOVERED_PARTIAL
                    } else { strings::MEETING_RECOVERED });
                }
            }
            Ok::<_, anyhow::Error>((db, notice))
        });
        self.busy = true;
        self.error = None;
        cx.notify();
        cx.spawn_in(window, async move |view, cx| {
            let result = task.await;
            let _ = view.update_in(cx, |this, window, cx| {
                this.busy = false;
                match result {
                    Ok(Ok((db, notice))) => {
                        this.model.update(cx, |model, cx| {
                            model.db = DbState::Ready(db);
                            cx.notify();
                        });
                        if let Some(text) = notice.or(done) {
                            window.push_notification(Notification::success(text), cx);
                        }
                        if deleted.is_some() && deleted == this.selected {
                            this.selected = None;
                            cx.emit(MeetingsEvent::Opened);
                        }
                        this.refresh(cx);
                    }
                    Ok(Err(error)) => window.push_notification(
                        Notification::error(error.to_string()).title(failed),
                        cx,
                    ),
                    Err(_) => window.push_notification(Notification::error(failed), cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn open(&mut self, id: String, cx: &mut Context<Self>) {
        self.selected = Some(id);
        cx.emit(MeetingsEvent::Opened);
        cx.notify();
    }

    fn meeting_title(&self, id: &str) -> String {
        self.list
            .rows()
            .iter()
            .find_map(|row| match row {
                Row::Meeting(m) if m.data.id == id => Some(m.data.title.clone()),
                _ => None,
            })
            .unwrap_or_else(|| id.to_string())
    }

    /// "Omdøb møde" (Tauri `Sidebar/index.tsx:764-805`).
    fn open_rename(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let title = self.meeting_title(&id);
        self.rename_id = Some(id);
        self.rename_input
            .update(cx, |input, cx| input.set_value(title, window, cx));
        let input = self.rename_input.clone();
        let view = cx.entity().downgrade();
        let first = std::cell::Cell::new(true);
        window.open_dialog(cx, move |dialog, window, cx| {
            if first.replace(false) {
                input.update(cx, |input, cx| input.focus(window, cx));
            }
            let save = view.clone();
            let cancel = view.clone();
            dialog
                .w(size::RENAME_DIALOG_WIDTH)
                .title(ui::section_title(strings::MEETING_RENAME_TITLE))
                .child(
                    v_flex()
                        .gap(space::SM)
                        .py(space::LG)
                        .child(
                            div()
                                .text_size(typography::SM)
                                .font_weight(FontWeight::MEDIUM)
                                .child(strings::MEETING_NAME),
                        )
                        .child(
                            controls::input(&input)
                                .h(layout::CONTROL_HEIGHT)
                                .aria_label(strings::MEETING_NAME),
                        ),
                )
                .footer(
                    DialogFooter::new()
                        .child(
                            controls::button("rename-cancel", Variant::Secondary)
                                .label(strings::DIALOG_CANCEL)
                                .on_click(move |_, window, cx| {
                                    let _ = cancel.update(cx, |this, _| this.rename_id = None);
                                    window.close_dialog(cx);
                                }),
                        )
                        .child(
                            controls::button("rename-save", Variant::Primary)
                                .label(strings::DIALOG_SAVE)
                                .on_click(move |_, window, cx| {
                                    let _ =
                                        save.update(cx, |this, cx| this.confirm_rename(window, cx));
                                }),
                        ),
                )
        });
    }

    fn confirm_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.rename_id.clone() else {
            return;
        };
        let value = self.rename_input.read(cx).value().to_string();
        let Ok(title) = meeting_list::title(&value) else {
            window.push_notification(Notification::error(strings::MEETING_NAME_REQUIRED), cx);
            return;
        };
        let title = title.to_string();
        self.rename_id = None;
        window.close_dialog(cx);
        self.act(Action::Rename(id, title), window, cx);
    }

    /// "Bekræft sletning" (Tauri `ConfirmationModel/confirmation-modal.tsx`).
    fn open_delete(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let view = cx.entity().downgrade();
        let palette = ui::palette(cx);
        let muted = palette.muted_foreground;
        window.open_dialog(cx, move |dialog, _, _| {
            let view = view.clone();
            let id = id.clone();
            dialog
                .w(size::CONFIRM_DIALOG_WIDTH)
                .title(
                    div()
                        .font_family(typography::SERIF)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_size(typography::XL)
                        .child(strings::MEETING_DELETE_TITLE),
                )
                .child(
                    div()
                        .pb(space::XL)
                        .text_color(muted)
                        .child(strings::MEETING_DELETE_CONFIRM),
                )
                .footer(
                    DialogFooter::new()
                        .gap(space::LG)
                        .child(
                            controls::button("delete-cancel", Variant::Ghost)
                                .label(strings::DIALOG_CANCEL)
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            controls::button("delete-confirm", Variant::Danger)
                                .label(strings::MEETING_DELETE)
                                .on_click(move |_, window, cx| {
                                    window.close_dialog(cx);
                                    let id = id.clone();
                                    let _ = view.update(cx, |this, cx| {
                                        this.act(Action::Delete(id), window, cx)
                                    });
                                }),
                        ),
                )
        });
    }

    /// Whether a meeting is being recorded right now, for the record button.
    pub(crate) fn recording(&self) -> bool {
        self.list.rows().iter().any(
            |row| matches!(row, Row::Meeting(m) if m.status == meeting_list::Status::Recording),
        )
    }

    /// The open meeting's row, for the content area.
    pub(crate) fn selected_meeting(&self) -> Option<&meeting_list::Meeting> {
        let id = self.selected.as_deref()?;
        self.list.rows().iter().find_map(|row| match row {
            Row::Meeting(m) if m.data.id == id => Some(m),
            _ => None,
        })
    }

    /// Interrupted recordings that can be restored, shown on the home
    /// screen (Tauri shows a recovery dialog there).
    pub(crate) fn recovery_card(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.recovery.is_empty() {
            return None;
        }
        let palette = ui::palette(cx);
        let mut card = v_flex()
            .id("meeting-recovery")
            .gap(space::SM)
            .p(space::LG)
            .rounded(theme::RADIUS_LG)
            .border_1()
            .border_color(palette.border)
            .bg(palette.card)
            .child(ui::section_title(strings::MEETING_RECOVERY));
        for (index, recording) in self.recovery.iter().enumerate() {
            let id = recording.id.clone();
            card = card.child(
                controls::button(("recover", index), Variant::Outline)
                    .label(strings::meeting_restore(&recording.id))
                    .disabled(self.busy)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.act(Action::Recover(id.clone()), window, cx);
                    })),
            );
        }
        Some(card.into_any_element())
    }

    fn meeting_row(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let palette = ui::palette(cx);
        let Some(&row) = self.shown.get(index) else {
            return div().into_any_element();
        };
        let list = self.list.clone();
        match &list.rows()[row] {
            Row::Date { date, .. } => div()
                .px(space::MD)
                .pb(space::XS)
                .when(index > 0, |this| this.pt(space::MD))
                .text_size(typography::XS)
                .font_weight(FontWeight::MEDIUM)
                .text_color(palette.muted_foreground)
                .id(("meeting-day", index))
                .role(gpui::Role::Heading)
                .child(meeting_list::day_label(*date, self.today))
                .into_any_element(),
            Row::Meeting(meeting) => {
                let id = meeting.data.id.clone();
                let title = meeting.data.title.clone();
                let active = self.selected.as_deref() == Some(id.as_str());
                let focus = self.focus.entry(id.clone()).or_insert_with(|| RowFocus {
                    group: cx.focus_handle().tab_stop(false),
                    row: cx.focus_handle(),
                });
                let (group_focus, row_focus) = (focus.group.clone(), focus.row.clone());
                let show_actions = active || group_focus.contains_focused(window, cx);
                let hover = theme::over(palette.accent, palette.card, theme::LIST_HOVER_ALPHA);
                let can_delete = meeting_list::delete_allowed(true, meeting.status);
                let snippet = self.snippets.get(&id).cloned();
                let open_id = id.clone();
                let key_id = id.clone();
                let rename_id = id.clone();
                let delete_id = id.clone();
                div()
                    .id(("meeting-row", index))
                    .track_focus(&group_focus)
                    .relative()
                    .group("meeting-row")
                    .pb(space::XXS)
                    .child(
                        v_flex()
                            .id(("meeting-open", index))
                            .track_focus(&row_focus)
                            .role(gpui::Role::Button)
                            .aria_label(SharedString::from(title.clone()))
                            .w_full()
                            .min_h(layout::CONTROL_HEIGHT)
                            .justify_center()
                            // Tauri's px-3 py-2 minus the 2 px focus border.
                            .px(space::MD - space::XXS)
                            .py(space::XS2)
                            .rounded(theme::RADIUS)
                            .border_2()
                            .border_color(gpui::transparent_black())
                            .text_color(palette.foreground)
                            .when(active, |this| this.bg(palette.accent))
                            .when(!active, |this| this.hover(move |s| s.bg(hover)))
                            .focus_visible(|s| s.border_color(palette.ring))
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.open(open_id.clone(), cx)),
                            )
                            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                    this.open(key_id.clone(), cx);
                                    cx.stop_propagation();
                                }
                            }))
                            .child(
                                div()
                                    .text_size(typography::SM)
                                    .line_height(size::SM_LINE)
                                    .font_weight(if active {
                                        FontWeight::SEMIBOLD
                                    } else {
                                        FontWeight::MEDIUM
                                    })
                                    .line_clamp(2)
                                    .child(title.clone()),
                            )
                            .child(
                                div()
                                    .text_size(typography::XS)
                                    .line_height(size::XS_LINE)
                                    .text_color(palette.muted_foreground)
                                    .child(match meeting.status.short() {
                                        Some(status) => [meeting.time.as_str(), status].join(" · "),
                                        None => meeting.time.clone(),
                                    }),
                            )
                            .when_some(snippet, |this, snippet| {
                                this.child(
                                    div()
                                        .mt(space::XS)
                                        .text_size(typography::XS)
                                        .text_color(palette.muted_foreground)
                                        .line_clamp(2)
                                        .child(snippet),
                                )
                            }),
                    )
                    .child(
                        h_flex()
                            .absolute()
                            .top(space::XS2)
                            .right(space::XS)
                            .gap(space::XXS)
                            .rounded(theme::RADIUS)
                            .bg(palette.accent)
                            .when(!show_actions, |this| {
                                this.opacity(0.)
                                    .group_hover("meeting-row", |s| s.opacity(1.))
                            })
                            .child(
                                controls::small_icon_button(
                                    ("rename-meeting", index),
                                    IconName::Pencil,
                                    strings::meeting_rename_label(&title),
                                )
                                .on_click(cx.listener(
                                    move |this, _, window, cx| {
                                        this.open_rename(rename_id.clone(), window, cx)
                                    },
                                )),
                            )
                            .child(
                                controls::small_icon_button(
                                    ("delete-meeting", index),
                                    IconName::Trash,
                                    strings::meeting_delete_label(&title),
                                )
                                .disabled(self.busy || !can_delete)
                                .on_click(cx.listener(
                                    move |this, _, window, cx| {
                                        this.open_delete(delete_id.clone(), window, cx)
                                    },
                                )),
                            ),
                    )
                    .into_any_element()
            }
        }
    }

    /// The search field (`InputGroup` with a search icon and a clear X).
    pub(crate) fn search_field(&self, cx: &mut Context<Self>) -> AnyElement {
        let palette = ui::palette(cx);
        controls::input(&self.search)
            .aria_label(strings::SEARCH_LABEL)
            .prefix(
                Icon::from(IconName::Search)
                    .size(icon_size::SM)
                    .text_color(palette.muted_foreground),
            )
            .when(!self.query.is_empty(), |this| {
                this.suffix(
                    controls::small_icon_button("search-clear", IconName::X, strings::SEARCH_CLEAR)
                        .size(size::SEARCH_CLEAR)
                        .on_click(cx.listener(|this, _, window, cx| this.clear_search(window, cx))),
                )
            })
            .into_any_element()
    }

    fn list_message(&self, title: Option<&str>, text: &str, cx: &mut Context<Self>) -> AnyElement {
        let palette = ui::palette(cx);
        v_flex()
            .px(space::MD)
            .py(space::XL)
            .gap(space::XS)
            .text_size(typography::SM)
            .text_color(palette.muted_foreground)
            .when_some(title, |this, title| {
                this.child(
                    div()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette.foreground)
                        .child(title.to_string()),
                )
            })
            .child(text.to_string())
            .into_any_element()
    }
}

impl Render for MeetingsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = ui::palette(cx);
        let mut body = v_flex()
            .id("meeting-list")
            .size_full()
            .min_h_0()
            .px(space::SM)
            .pb(space::SM)
            .role(gpui::Role::List)
            .aria_label(SharedString::from(strings::MEETINGS));
        if !self.query.trim().is_empty() && self.searching {
            body = body.child(
                div()
                    .px(space::MD)
                    .py(space::SM)
                    .text_size(typography::SM)
                    .text_color(palette.muted_foreground)
                    .id("meeting-search-status")
                    .role(gpui::Role::Status)
                    .child(strings::SEARCHING),
            );
        }
        if self.search_failed {
            body = body.child(self.list_message(None, strings::SEARCH_FAILED, cx));
        }
        if let Some(error) = self.error.clone() {
            body = body.child(self.list_message(None, &error, cx)).child(
                div().px(space::MD).child(
                    controls::small_button("refresh-meetings", Variant::Outline)
                        .label(strings::MEETING_REFRESH)
                        .disabled(self.busy)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.error = None;
                            this.refresh(cx);
                            cx.notify();
                        })),
                ),
            );
        }
        let not_created = matches!(self.model.read(cx).db, DbState::NotCreated);
        if !self.loaded && !not_created {
            body = body.child(
                div()
                    .px(space::MD)
                    .text_size(typography::SM)
                    .child(ui::loading(strings::LOADING_MEETINGS, cx)),
            );
        } else if self.shown.is_empty() && !self.query.trim().is_empty() {
            if !self.searching {
                let text = strings::search_no_match(self.query.trim());
                body = body.child(self.list_message(None, &text, cx));
            }
        } else if self.list.rows().is_empty() {
            if self.error.is_none() {
                body = body.child(self.list_message(
                    Some(strings::NO_MEETINGS),
                    strings::SIDEBAR_FIRST_MEETING,
                    cx,
                ));
            }
        } else {
            let view = cx.entity().downgrade();
            body = body.child(
                virtual_list(self.viewport.clone(), move |index, window, cx| {
                    view.update(cx, |this, cx| this.meeting_row(index, window, cx))
                        .unwrap_or_else(|_| div().into_any_element())
                })
                .flex_1()
                .min_h_0(),
            );
        }
        body
    }
}

/// Synthetic meetings for the `meetings` screenshots, as in
/// `docs/screenshots/moedeliste-lys.png`: two today, one yesterday, one on
/// Monday. Times are UTC and "today" is fixed, so the image is the same on
/// every machine.
#[cfg(feature = "screenshot-tests")]
pub(crate) fn fixture_list() -> (List, NaiveDate) {
    use tingskriver_core::database::models::{DateTimeUtc, MeetingModel};
    let meeting = |id: &str, title: &str, at: &str, status| {
        let at = chrono::DateTime::parse_from_rfc3339(at)
            .expect("fixture time")
            .to_utc();
        meeting_list::Meeting {
            data: MeetingModel {
                id: id.into(),
                title: title.into(),
                created_at: DateTimeUtc(at),
                updated_at: DateTimeUtc(at),
                folder_path: None,
                import_key: None,
            },
            status,
            duration: String::new(),
            time: String::new(),
        }
    };
    use meeting_list::Status;
    let list = List::new(
        vec![
            meeting(
                "fixture-1",
                "Ugentligt statusmøde",
                "2026-10-29T09:30:00Z",
                Status::SummaryReady,
            ),
            meeting(
                "fixture-2",
                "Kickoff med Havnens Bageri",
                "2026-10-29T13:05:00Z",
                Status::Transcribing,
            ),
            meeting(
                "fixture-3",
                "Planlægning af efterårskampagne",
                "2026-10-28T10:00:00Z",
                Status::SummaryReady,
            ),
            meeting(
                "fixture-4",
                "Retrospektiv: sidste sprint",
                "2026-10-26T14:15:00Z",
                Status::Transcribed,
            ),
        ],
        &chrono::Utc,
    );
    (
        list,
        NaiveDate::from_ymd_opt(2026, 10, 29).expect("fixture date"),
    )
}
