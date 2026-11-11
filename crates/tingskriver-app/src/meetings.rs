//! Presentation of core's meeting list; disk and database work stays on Tokio.
use crate::{
    startup::DbState,
    theme::space,
    ui::{self, AppModel},
};
use gpui::{
    div, list as virtual_list, prelude::*, Context, Entity, ListAlignment, ListState, Subscription,
    Window,
};
use gpui_component::{
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputState},
    v_flex, ActiveTheme, Disableable,
};
use gpui_kit_assets::IconName;
use std::{rc::Rc, time::Duration};
use tingskriver_core::{
    meeting_list::{self, List, Row},
    recover, strings,
};

enum Action {
    Rename(String, String),
    Delete(String),
    Recover(String),
}

pub struct MeetingsView {
    model: Entity<AppModel>,
    list: Rc<List>,
    viewport: ListState,
    recovery: Vec<recover::Recording>,
    error: Option<String>,
    notice: Option<&'static str>,
    selected: Option<String>,
    rename: Option<(String, Entity<InputState>)>,
    delete: Option<String>,
    busy: bool,
    loaded: bool,
    _subscription: Subscription,
}
impl MeetingsView {
    pub fn new(model: Entity<AppModel>, cx: &mut Context<Self>) -> Self {
        let subscription = cx.observe(&model, |this, _, cx| {
            this.refresh(cx);
            cx.notify();
        });
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
            viewport: ListState::new(0, ListAlignment::Top, space::XXL),
            recovery: vec![],
            error: None,
            notice: None,
            selected: None,
            rename: None,
            delete: None,
            busy: false,
            loaded: false,
            _subscription: subscription,
        }
    }
    fn refresh(&mut self, cx: &mut Context<Self>) {
        if self.busy || self.rename.is_some() || self.delete.is_some() || self.selected.is_some() {
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
                match result {
                    Ok((list, recovery)) => {
                        this.error = None;
                        match list {
                            Ok(list) => {
                                if !this.list.same_rows(&list) {
                                    this.viewport
                                        .splice(0..this.list.rows().len(), list.rows().len());
                                }
                                this.list = Rc::new(list);
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
    fn act(&mut self, action: Action, cx: &mut Context<Self>) {
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
        self.notice = None;
        cx.notify();
        cx.spawn(async move |view, cx| {
            let result = task.await;
            let _ = view.update(cx, |this, cx| {
                this.busy = false;
                match result {
                    Ok(Ok((db, notice))) => {
                        this.model.update(cx, |model, cx| {
                            model.db = DbState::Ready(db);
                            cx.notify();
                        });
                        this.notice = notice;
                        this.rename = None;
                        this.delete = None;
                        this.refresh(cx);
                    }
                    Ok(Err(error)) => this.error = Some(error.to_string()),
                    Err(_) => this.error = Some(strings::MEETING_ACTION_FAILED.into()),
                }
                cx.notify();
            });
        })
        .detach();
    }
    fn meeting_row(&self, index: usize, cx: &mut Context<Self>) -> gpui::AnyElement {
        let Row::Meeting(meeting) = &self.list.rows()[index] else {
            let Row::Date { label, .. } = &self.list.rows()[index] else {
                unreachable!()
            };
            return div()
                .py(space::SM)
                .flex()
                .items_center()
                .child(ui::section_title(label))
                .into_any_element();
        };
        let id = meeting.data.id.clone();
        let rename_id = id.clone();
        let open_id = id.clone();
        let title = meeting.data.title.clone();
        let can_delete = meeting_list::delete_allowed(true, meeting.status);
        v_flex()
            .py(space::MD)
            .gap(space::SM)
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .child(div().truncate().child(title.clone()))
                    .child(ui::muted(&meeting.duration, cx))
                    .child(ui::muted(meeting.status.text(), cx)),
            )
            .child(
                h_flex()
                    .gap(space::SM)
                    .child(
                        Button::new(("open", index))
                            .label(strings::MEETING_OPEN)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.rename = None;
                                this.delete = None;
                                this.selected = Some(open_id.clone());
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new(("rename", index))
                            .label(strings::MEETING_RENAME)
                            .disabled(self.busy)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.delete = None;
                                this.selected = None;
                                this.rename = Some((
                                    rename_id.clone(),
                                    cx.new(|cx| {
                                        InputState::new(window, cx).default_value(title.clone())
                                    }),
                                ));
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new(("delete", index))
                            .label(strings::MEETING_DELETE)
                            .disabled(self.busy || !can_delete)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.rename = None;
                                this.selected = None;
                                this.delete = Some(id.clone());
                                cx.notify();
                            })),
                    ),
            )
            .into_any_element()
    }
}
impl Render for MeetingsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut body = v_flex().size_full().min_h_0().gap(space::MD).child(
            h_flex()
                .gap(space::SM)
                .child(
                    Button::new("new-recording")
                        .primary()
                        .label(strings::MEETING_NEW)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.notice = Some(strings::MEETING_RECORDING_LATER);
                            cx.notify();
                        })),
                )
                .child(
                    Button::new("refresh-meetings")
                        .label(strings::MEETING_REFRESH)
                        .disabled(self.busy)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.error = None;
                            this.refresh(cx);
                            cx.notify();
                        })),
                ),
        );
        if let Some(notice) = self.notice {
            body = body.child(ui::paragraph(notice, cx));
        }
        if let Some(error) = &self.error {
            body = body
                .child(ui::paragraph(strings::MEETING_ACTION_FAILED, cx))
                .child(ui::paragraph(error, cx));
        }
        if !self.recovery.is_empty() {
            // Keep startup recovery usable even when the list itself is empty.
            body = body.child(ui::section_title(strings::MEETING_RECOVERY));
            for (index, recording) in self.recovery.iter().enumerate() {
                let id = recording.id.clone();
                body = body.child(
                    Button::new(("recover", index))
                        .label(strings::meeting_restore(&recording.id))
                        .disabled(self.busy)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.act(Action::Recover(id.clone()), cx);
                        })),
                );
            }
        }
        if let Some((id, input)) = &self.rename {
            let id = id.clone();
            let input = input.clone();
            body = body.child(Input::new(&input)).child(
                h_flex()
                    .gap(space::SM)
                    .child(
                        Button::new("save-title")
                            .label(strings::MEETING_SAVE)
                            .disabled(self.busy)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.act(
                                    Action::Rename(id.clone(), input.read(cx).value().to_string()),
                                    cx,
                                );
                            })),
                    )
                    .child(
                        Button::new("cancel-rename")
                            .label(strings::MEETING_CANCEL)
                            .disabled(self.busy)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.rename = None;
                                cx.notify();
                            })),
                    ),
            );
        }
        if let Some(id) = &self.delete {
            let id = id.clone();
            let title = self
                .list
                .rows()
                .iter()
                .find_map(|row| match row {
                    Row::Meeting(m) if m.data.id == id => Some(m.data.title.as_str()),
                    _ => None,
                })
                .unwrap_or(&id);
            body = body
                .child(ui::section_title(title))
                .child(ui::paragraph(strings::MEETING_DELETE_CONFIRM, cx))
                .child(
                    h_flex()
                        .gap(space::SM)
                        .child(
                            Button::new("confirm-delete")
                                .danger()
                                .label(strings::MEETING_DELETE)
                                .disabled(self.busy)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.act(Action::Delete(id.clone()), cx)
                                })),
                        )
                        .child(
                            Button::new("cancel-delete")
                                .label(strings::MEETING_CANCEL)
                                .disabled(self.busy)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.delete = None;
                                    cx.notify();
                                })),
                        ),
                );
        }
        if let Some(id) = &self.selected {
            if let Some(meeting) = self.list.rows().iter().find_map(|row| match row {
                Row::Meeting(meeting) if &meeting.data.id == id => Some(meeting),
                _ => None,
            }) {
                body = body
                    .child(ui::section_title(&meeting.data.title))
                    .child(ui::paragraph(strings::MEETING_DOCUMENT_LATER, cx));
                if let Some(folder) = &meeting.data.folder_path {
                    let folder = std::path::PathBuf::from(folder);
                    body = body.child(
                        Button::new("open-meeting-folder")
                            .label(strings::MEETING_FOLDER)
                            .on_click(move |_, _, _| crate::startup::open_folder(&folder)),
                    );
                }
                body = body.child(
                    Button::new("back-to-meetings")
                        .label(strings::MEETING_BACK)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.selected = None;
                            this.refresh(cx);
                            cx.notify();
                        })),
                );
            }
        } else if !self.loaded {
            body = body.child(match &self.model.read(cx).db {
                DbState::NotCreated => ui::empty_state(
                    IconName::Inbox,
                    strings::NO_MEETINGS,
                    strings::FIRST_MEETING,
                    cx,
                ),
                _ => ui::loading(strings::LOADING_MEETINGS, cx),
            });
        } else if self.list.rows().is_empty() && self.error.is_none() {
            body = body.child(ui::empty_state(
                IconName::Inbox,
                strings::NO_MEETINGS,
                strings::FIRST_MEETING,
                cx,
            ));
        } else if !self.list.rows().is_empty() {
            let view = cx.entity().downgrade();
            body = body.child(
                virtual_list(self.viewport.clone(), move |index, _, cx| {
                    view.update(cx, |this, cx| this.meeting_row(index, cx))
                        .unwrap_or_else(|_| div().into_any_element())
                })
                .flex_1()
                .min_h_0(),
            );
        }
        ui::page(
            strings::MEETINGS,
            None,
            true,
            div().flex_1().min_h_0().child(body).into_any_element(),
            cx,
        )
    }
}
