//! Making, stopping and editing the summary on the meeting page: the glue
//! between the buttons, [`crate::summary_run::Run`] (what to show and say)
//! and core's [`meeting_summary`] (what happens). The page reads the stored
//! summary again every 1.5 s while a run is going, as Tauri polls
//! `api_get_summary`.
//!
//! The summary is edited as plain Markdown text in a text area. Tauri uses
//! the BlockNote rich-text editor; that is a web editor and has no GPUI
//! counterpart (docs/design-paritet.md, Job 4).

use std::time::Duration;

use gpui::{prelude::*, Context, Entity, Subscription, Window};
use gpui_component::input::{InputEvent, TextareaState};
use tingskriver_core::database::manager::DatabaseManager;
use tingskriver_core::host::HostHandle;
use tingskriver_core::meeting_summary::{self, Preferences, Template};
use tingskriver_core::strings::meeting_summary as text;

use super::MeetingView;
use crate::startup::DbState;
use crate::summary_run::{Kind, Toast};

/// How often the stored summary is read while a run is going.
const POLL: Duration = Duration::from_millis(1500);
/// Visible lines of the editor before it scrolls.
const EDITOR_ROWS: usize = 18;

/// The summary text being edited.
pub(super) struct Editor {
    pub(super) input: Entity<TextareaState>,
    /// The text when editing began; a different text is "Gem ændringer".
    pub(super) original: String,
    pub(super) saving: bool,
    pub(super) error: Option<String>,
    _subscription: Subscription,
}

impl Editor {
    pub(super) fn is_dirty(&self, cx: &gpui::App) -> bool {
        self.input.read(cx).value().as_ref() != self.original
    }
}

/// The toolbar's choices for this meeting.
#[derive(Default)]
pub(super) struct Choices {
    pub(super) templates: Vec<Template>,
    pub(super) template: String,
    /// The summary language; `None` is Automatisk.
    pub(super) language: Option<String>,
    /// The meeting has no folder, so the language lives only in this window.
    pub(super) language_local: bool,
    /// The quick-pick languages.
    pub(super) recents: Vec<String>,
    pub(super) pinned: Option<String>,
}

impl Choices {
    pub(super) fn with_template(template: &str) -> Choices {
        Choices {
            template: template.to_string(),
            ..Choices::default()
        }
    }

    /// The template menu of the reference image, without reading core.
    #[cfg(feature = "screenshot-tests")]
    pub(super) fn fixture() -> Choices {
        let template = |id: &str, name: &str| Template {
            id: id.into(),
            name: name.into(),
            description: String::new(),
        };
        Choices {
            templates: vec![
                template("standard", "Standardmøde"),
                template("daily_standup", "Daglig stand-up"),
            ],
            ..Choices::with_template("standard")
        }
    }

}

impl MeetingView {
    fn db_and_host(&self, cx: &gpui::App) -> Option<(DatabaseManager, HostHandle)> {
        let model = self.model.read(cx);
        match (&model.db, &model.host) {
            (DbState::Ready(db), Some(host)) => Some((db.clone(), host.clone())),
            _ => None,
        }
    }

    pub(super) fn show(toasts: Vec<Toast>, cx: &mut gpui::App) {
        for toast in toasts {
            crate::ui::notify_main_window(toast.notification(), cx);
        }
    }

    /// After every read of the stored document: say what changed in the run,
    /// and keep reading while it goes.
    pub(super) fn after_load(&mut self, cx: &mut Context<Self>) {
        let toasts = match &self.state {
            super::DocState::Ready(loaded) => self.run.observe(&loaded.document.summary),
            _ => Vec::new(),
        };
        if !toasts.is_empty() {
            // The model may have given the meeting a title.
            self.meetings.update(cx, |meetings, cx| meetings.reload(cx));
        }
        Self::show(toasts, cx);
        self.watch(cx);
    }

    /// Read the stored summary every [`POLL`] while a run is going.
    pub(super) fn watch(&mut self, cx: &mut Context<Self>) {
        if self.watching || !self.run.is_active() {
            return;
        }
        self.watching = true;
        cx.spawn(async move |view, cx| loop {
            cx.background_executor().timer(POLL).await;
            let keep = view.update(cx, |view, cx| {
                if view.run.is_active() {
                    view.load(false, cx);
                    true
                } else {
                    view.watching = false;
                    false
                }
            });
            if !matches!(keep, Ok(true)) {
                break;
            }
        })
        .detach();
    }

    /// "Lav opsummering" and "Lav om" (`handleGenerateSummary`).
    pub(super) fn generate(&mut self, cx: &mut Context<Self>) {
        let has_summary = self
            .loaded()
            .is_some_and(|loaded| loaded.document.summary.has_content());
        if !self.run.begin(has_summary) {
            return;
        }
        cx.notify();
        let Some((db, host)) = self.db_and_host(cx) else {
            let toasts = self.run.start_failed(text::SUMMARY_START_FAILED.to_string());
            Self::show(toasts, cx);
            return;
        };
        let id = self.meeting_id.clone();
        let template = self.choices.template.clone();
        let session_language = self.choices.language.clone();
        let task = self.model.read(cx).runtime.spawn(async move {
            meeting_summary::start(host, &db, &id, &template, session_language.as_deref()).await
        });
        cx.spawn(async move |view, cx| {
            let result = task.await;
            let _ = view.update(cx, |view, cx| {
                let toasts = match result {
                    Ok(Ok(started)) => view.run.started(&started),
                    Ok(Err(message)) => view.run.start_failed(message),
                    Err(_) => view.run.start_failed(text::SUMMARY_START_FAILED.to_string()),
                };
                Self::show(toasts, cx);
                // A read that began before the run existed would show the
                // old summary as the result of this one.
                view.load = None;
                view.load(false, cx);
                view.watch(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// "Stop" while the summary is written (`handleStopGeneration`).
    pub(super) fn stop(&mut self, cx: &mut Context<Self>) {
        if !self.run.stop_pressed() {
            return;
        }
        cx.notify();
        let Some((db, _)) = self.db_and_host(cx) else {
            let toasts = self.run.stopped(Err(text::SUMMARY_STOP_FAILED.to_string()));
            Self::show(toasts, cx);
            return;
        };
        let id = self.meeting_id.clone();
        let task = self
            .model
            .read(cx)
            .runtime
            .spawn(async move { meeting_summary::stop(&db, &id).await });
        cx.spawn(async move |view, cx| {
            let result = task.await.unwrap_or_else(|error| Err(error.to_string()));
            let _ = view.update(cx, |view, cx| {
                let toasts = view.run.stopped(result);
                Self::show(toasts, cx);
                view.load(false, cx);
                cx.notify();
            });
        })
        .detach();
    }

    // ---- editing ----

    /// "Rediger": the summary becomes a text field with its Markdown.
    pub(super) fn begin_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.run.is_active() || self.editor.is_some() {
            return;
        }
        let Some(loaded) = self.loaded() else {
            return;
        };
        let summary = &loaded.document.summary;
        if summary.status == tingskriver_core::meeting_document::SummaryStatus::Running {
            return;
        }
        let markdown = summary.markdown.clone().unwrap_or_default();
        let input = cx.new(|cx| TextareaState::new(window, cx).rows(EDITOR_ROWS));
        input.update(cx, |input, cx| input.set_value(markdown.clone(), window, cx));
        let subscription = cx.subscribe(&input, |_, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        });
        self.editor = Some(Editor {
            input,
            original: markdown,
            saving: false,
            error: None,
            _subscription: subscription,
        });
        cx.notify();
    }

    /// "Fortryd": leave the editor without saving.
    pub(super) fn cancel_edit(&mut self, cx: &mut Context<Self>) {
        if self.editor.as_ref().is_some_and(|editor| editor.saving) {
            return;
        }
        self.editor = None;
        cx.notify();
    }

    /// "Gem ændringer" (`handleSave` in `useMeetingData.ts`).
    pub(super) fn save_edit(&mut self, cx: &mut Context<Self>) {
        let Some(editor) = self.editor.as_mut() else {
            return;
        };
        if editor.saving {
            return;
        }
        let markdown = editor.input.read(cx).value().to_string();
        if markdown == editor.original {
            self.editor = None;
            cx.notify();
            return;
        }
        editor.saving = true;
        editor.error = None;
        let Some((db, _)) = self.db_and_host(cx) else {
            self.edit_failed(text::SUMMARY_START_FAILED.to_string(), cx);
            return;
        };
        cx.notify();
        let id = self.meeting_id.clone();
        let task = self
            .model
            .read(cx)
            .runtime
            .spawn(async move { meeting_summary::save_edit(&db, &id, &markdown).await });
        cx.spawn(async move |view, cx| {
            let result = task.await.unwrap_or_else(|error| Err(error.to_string()));
            let _ = view.update(cx, |view, cx| match result {
                Ok(()) => {
                    view.editor = None;
                    Self::show(
                        vec![Toast {
                            kind: Kind::Success,
                            title: text::SUMMARY_SAVED.to_string(),
                            help: None,
                        }],
                        cx,
                    );
                    view.load(false, cx);
                    cx.notify();
                }
                Err(message) => view.edit_failed(message, cx),
            });
        })
        .detach();
    }

    fn edit_failed(&mut self, message: String, cx: &mut Context<Self>) {
        if let Some(editor) = self.editor.as_mut() {
            editor.saving = false;
            editor.error = Some(message.clone());
        }
        Self::show(
            vec![Toast {
                kind: Kind::Error,
                title: text::SUMMARY_SAVE_FAILED.to_string(),
                help: Some(message),
            }],
            cx,
        );
        cx.notify();
    }

    // ---- language and template ----

    /// Read the templates, the meeting's language and the quick picks.
    pub(super) fn load_choices(&mut self, cx: &mut Context<Self>) {
        let Some((db, host)) = self.db_and_host(cx) else {
            return;
        };
        let id = self.meeting_id.clone();
        let task = self.model.read(cx).runtime.spawn(async move {
            let templates = meeting_summary::templates().await;
            let language = meeting_summary::read_language(&db, &id).await;
            let preferences = match host.app_data_dir() {
                Ok(dir) => Preferences::load_async(dir).await,
                Err(error) => Err(error),
            };
            (templates, language, preferences)
        });
        cx.spawn(async move |view, cx| {
            let Ok((templates, language, preferences)) = task.await else {
                return;
            };
            let _ = view.update(cx, |view, cx| {
                match templates {
                    Ok(templates) => {
                        view.choices.template =
                            meeting_summary::pick_template(&templates, &view.choices.template);
                        view.choices.templates = templates;
                    }
                    Err(error) => log::warn!("Could not list the summary templates: {error}"),
                }
                match language {
                    Ok(saved) => {
                        view.choices.language = saved.language;
                        view.choices.language_local = saved.local_only;
                    }
                    Err(error) => {
                        log::warn!("Could not read the summary language: {error}");
                        Self::show(
                            vec![Toast {
                                kind: Kind::Warning,
                                title: text::SUMMARY_LANGUAGE_READ_FAILED.to_string(),
                                help: Some(text::SUMMARY_LANGUAGE_READ_FAILED_HELP.to_string()),
                            }],
                            cx,
                        );
                    }
                }
                if let Ok(preferences) = preferences {
                    view.choices.recents = preferences.language_recents;
                    view.choices.pinned = preferences.language_default;
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// A language from the menu (`handleLangChange`); `None` is Automatisk.
    pub(super) fn set_language(&mut self, language: Option<String>, cx: &mut Context<Self>) {
        let previous = (self.choices.language.clone(), self.choices.language_local);
        self.choices.language = language.clone();
        cx.notify();
        let Some((db, host)) = self.db_and_host(cx) else {
            self.choices.language = previous.0;
            return;
        };
        let id = self.meeting_id.clone();
        let task = self.model.read(cx).runtime.spawn(async move {
            let saved = meeting_summary::save_language(&db, &id, language.as_deref()).await?;
            let mut recents = None;
            if let (Some(code), Ok(dir)) = (saved.language.as_deref(), host.app_data_dir()) {
                // The quick picks are cosmetic: a failure here is only logged.
                match Preferences::load_async(dir.clone()).await {
                    Ok(mut preferences) => {
                        preferences.add_recent(code);
                        recents = Some(preferences.language_recents.clone());
                        if let Err(error) = preferences.save_async(dir).await {
                            log::warn!("Could not save the language quick picks: {error}");
                        }
                    }
                    Err(error) => log::warn!("Could not read the language quick picks: {error}"),
                }
            }
            Ok::<_, String>((saved, recents))
        });
        cx.spawn(async move |view, cx| {
            let result = task
                .await
                .unwrap_or_else(|error| Err(error.to_string()));
            let _ = view.update(cx, |view, cx| {
                match result {
                    Ok((saved, recents)) => {
                        view.choices.language = saved.language;
                        view.choices.language_local = saved.local_only;
                        if let Some(recents) = recents {
                            view.choices.recents = recents;
                        }
                        if saved.local_only {
                            Self::show(
                                vec![Toast {
                                    kind: Kind::Info,
                                    title: text::SUMMARY_LANGUAGE_SAVED_LOCALLY.to_string(),
                                    help: Some(
                                        text::SUMMARY_LANGUAGE_SAVED_LOCALLY_HELP.to_string(),
                                    ),
                                }],
                                cx,
                            );
                        }
                    }
                    Err(error) => {
                        log::warn!("Could not save the summary language: {error}");
                        view.choices.language = previous.0;
                        view.choices.language_local = previous.1;
                        Self::show(
                            vec![Toast {
                                kind: Kind::Error,
                                title: text::SUMMARY_LANGUAGE_SAVE_FAILED.to_string(),
                                help: Some(text::SUMMARY_LANGUAGE_SAVE_FAILED_HELP.to_string()),
                            }],
                            cx,
                        );
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// A template from the menu (`handleTemplateSelection`).
    pub(super) fn set_template(&mut self, id: String, cx: &mut Context<Self>) {
        let Some(template) = self.choices.templates.iter().find(|t| t.id == id) else {
            return;
        };
        let name = template.name.clone();
        self.choices.template = id;
        Self::show(
            vec![Toast {
                kind: Kind::Success,
                title: text::SUMMARY_TEMPLATE_CHOSEN.to_string(),
                help: Some(text::template_chosen(&name)),
            }],
            cx,
        );
        cx.notify();
    }
}
