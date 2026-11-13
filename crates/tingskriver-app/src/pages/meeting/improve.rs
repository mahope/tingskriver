//! "Forbedr": transcribe the recording again (Tauri
//! `MeetingDetails/RetranscribeDialog.tsx`). The dialog asks for the
//! language, runs core's retranscription with the transcription model set
//! under Indstillinger > Transskription, shows its progress and ends with a
//! message or an error.
//!
//! Difference from Tauri: the dialog does not offer a model list. Which
//! models are installed is chosen under Transskription, and the dialog uses
//! that choice (docs/design-paritet.md, Job 4).

use std::sync::Arc;

use gpui::{div, prelude::*, AnyWindowHandle, Context, Entity, SharedString, Window};
use gpui_component::{
    dialog::DialogFooter,
    h_flex,
    select::{Select, SelectState},
    v_flex, Icon, WindowExt,
};
use gpui_kit_assets::IconName;
use tingskriver_core::audio::retranscription;
use tingskriver_core::database::repositories::setting::SettingsRepository;
use tingskriver_core::host::{HostHandle, ListenerId};
use tingskriver_core::strings::{self, meeting_summary as text};
use tokio::sync::mpsc::unbounded_channel;

use super::MeetingView;
use crate::controls::{self, Variant};
use crate::startup::DbState;
use crate::summary_run::{Kind, Toast};
use crate::theme::{document as size, icon_size, layout, space, typography};

type Choices = SelectState<Vec<SharedString>>;

/// Where the dialog stands.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(super) enum Stage {
    #[default]
    Idle,
    /// The question: language, then Start.
    Choosing,
    /// Core works; the last progress message and percentage.
    Running {
        message: Option<String>,
        percent: Option<u32>,
    },
    Failed(String),
}

/// An event of core's retranscription, for one meeting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Event {
    Progress { message: String, percent: u32 },
    Done { segments: usize },
    Error(String),
}

impl Stage {
    /// Apply an event. `Some(toast)` when the dialog is finished.
    pub(super) fn event(&mut self, event: Event) -> Option<Toast> {
        if !matches!(self, Stage::Running { .. }) {
            return None;
        }
        match event {
            Event::Progress { message, percent } => {
                *self = Stage::Running {
                    message: Some(message),
                    percent: Some(percent),
                };
                None
            }
            Event::Done { segments } => {
                *self = Stage::Idle;
                Some(Toast {
                    kind: Kind::Success,
                    title: text::improve_done(segments),
                    help: None,
                })
            }
            Event::Error(error) => {
                *self = Stage::Failed(error);
                None
            }
        }
    }

    /// Core's refusal to start, in Tauri's words where it has any.
    pub(super) fn start_failed(&mut self, error: &str) {
        *self = Stage::Failed(if error.contains("already in progress") {
            text::IMPROVE_BUSY.to_string()
        } else {
            error.to_string()
        });
    }
}

/// Read one of core's JSON events.
pub(super) fn parse_event(name: &str, payload: &str, meeting_id: &str) -> Option<Event> {
    let value: serde_json::Value = serde_json::from_str(payload).ok()?;
    if value["meeting_id"].as_str() != Some(meeting_id) {
        return None;
    }
    match name {
        "retranscription-progress" => Some(Event::Progress {
            message: value["message"].as_str().unwrap_or_default().to_string(),
            percent: value["progress_percentage"].as_u64().unwrap_or(0).min(100) as u32,
        }),
        "retranscription-complete" => Some(Event::Done {
            segments: value["segments_count"].as_u64().unwrap_or(0) as usize,
        }),
        "retranscription-error" => Some(Event::Error(
            value["error"].as_str().unwrap_or_default().to_string(),
        )),
        _ => None,
    }
}

/// The three events, until dropped.
struct Listening {
    host: HostHandle,
    ids: Vec<ListenerId>,
}

impl Drop for Listening {
    fn drop(&mut self) {
        for id in self.ids.drain(..) {
            self.host.unlisten(id);
        }
    }
}

/// The dialog's state, part of the meeting view.
#[derive(Default)]
pub(super) struct Improve {
    stage: Stage,
    window: Option<AnyWindowHandle>,
    language: Option<Entity<Choices>>,
    /// The saved transcription engine and model: ("parakeet" | "whisper",
    /// model). `None` until read.
    model: Option<(String, String)>,
    listening: Option<Listening>,
}

impl Improve {
    pub(super) fn is_running(&self) -> bool {
        matches!(self.stage, Stage::Running { .. })
    }
}

/// The language choices: "Find sproget automatisk", then core's languages.
fn language_choices() -> Vec<SharedString> {
    std::iter::once(SharedString::from(text::IMPROVE_LANGUAGE_AUTO))
        .chain(
            tingskriver_core::meeting_language::CHOICES
                .iter()
                .map(|(code, _)| {
                    SharedString::from(tingskriver_core::meeting_language::label(code))
                }),
        )
        .collect()
}

/// The language code for the chosen row (0 is automatic).
fn language_code(row: Option<usize>) -> Option<String> {
    let row = row?.checked_sub(1)?;
    tingskriver_core::meeting_language::CHOICES
        .get(row)
        .map(|(code, _)| (*code).to_string())
}

impl MeetingView {
    /// "Forbedr".
    pub(super) fn open_improve(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.improve.is_running() {
            return;
        }
        let Some(folder) = self.loaded().and_then(|l| l.document.folder_path.clone()) else {
            self.improve.stage = Stage::Failed(text::IMPROVE_FOLDER_MISSING.to_string());
            self.show_improve(window, cx);
            return;
        };
        let _ = folder;
        self.improve.stage = Stage::Choosing;
        self.improve.window = Some(window.window_handle());
        let select =
            cx.new(|cx| SelectState::new(language_choices(), Some(Default::default()), window, cx));
        self.improve.language = Some(select);
        self.read_transcription_model(cx);
        self.show_improve(window, cx);
    }

    /// The saved engine, for the Parakeet note and the model to run.
    fn read_transcription_model(&mut self, cx: &mut Context<Self>) {
        let DbState::Ready(db) = &self.model.read(cx).db else {
            return;
        };
        let pool = db.pool().clone();
        let task = self
            .model
            .read(cx)
            .runtime
            .spawn(async move { SettingsRepository::get_transcript_config(&pool).await });
        cx.spawn(async move |view, cx| {
            let saved = match task.await {
                Ok(Ok(saved)) => saved,
                Ok(Err(error)) => {
                    log::warn!("Could not read the transcription model: {error}");
                    None
                }
                Err(_) => None,
            };
            let _ = view.update(cx, |view, cx| {
                view.improve.model = saved.map(|saved| {
                    let engine = if saved.provider == "parakeet" {
                        "parakeet"
                    } else {
                        "whisper"
                    };
                    (engine.to_string(), saved.model)
                });
                cx.notify();
            });
        })
        .detach();
    }

    /// "Start" in the dialog.
    fn start_improve(&mut self, cx: &mut Context<Self>) {
        let Some(host) = self.model.read(cx).host.clone() else {
            self.improve.stage = Stage::Failed(text::IMPROVE_FOLDER_MISSING.to_string());
            cx.notify();
            return;
        };
        let Some(folder) = self.loaded().and_then(|l| l.document.folder_path.clone()) else {
            self.improve.stage = Stage::Failed(text::IMPROVE_FOLDER_MISSING.to_string());
            cx.notify();
            return;
        };
        let (provider, model) = match self.improve.model.clone() {
            Some((engine, model)) => (Some(engine), Some(model)),
            None => (None, None),
        };
        // Parakeet picks the language itself.
        let language = if provider.as_deref() == Some("parakeet") {
            None
        } else {
            let row = self
                .improve
                .language
                .as_ref()
                .and_then(|select| select.read(cx).selected_index(cx))
                .map(|index| index.row);
            language_code(row)
        };
        let id = self.meeting_id.clone();
        // Listen before starting, so the first message is not missed.
        let (tx, mut rx) = unbounded_channel::<Event>();
        let ids = [
            "retranscription-progress",
            "retranscription-complete",
            "retranscription-error",
        ]
        .into_iter()
        .map(|name| {
            let (tx, id) = (tx.clone(), id.clone());
            host.listen(
                name,
                Box::new(move |payload| {
                    if let Some(event) = parse_event(name, payload, &id) {
                        let _ = tx.send(event);
                    }
                }),
            )
        })
        .collect();
        self.improve.listening = Some(Listening {
            host: Arc::clone(&host),
            ids,
        });
        self.improve.stage = Stage::Running {
            message: None,
            percent: None,
        };
        cx.notify();
        cx.spawn(async move |view, cx| {
            while let Some(event) = rx.recv().await {
                let alive = view.update(cx, |view, cx| view.improve_event(event, cx));
                if alive.is_err() {
                    break;
                }
            }
        })
        .detach();
        let task = self.model.read(cx).runtime.spawn(async move {
            retranscription::start_retranscription_command(
                host, id, folder, language, model, provider,
            )
            .await
        });
        cx.spawn(async move |view, cx| {
            let result = task.await.unwrap_or_else(|error| Err(error.to_string()));
            if let Err(error) = result {
                let _ = view.update(cx, |view, cx| {
                    view.improve.listening = None;
                    view.improve.stage.start_failed(&error);
                    cx.notify();
                });
            }
        })
        .detach();
    }

    fn improve_event(&mut self, event: Event, cx: &mut Context<Self>) {
        let finished = matches!(event, Event::Done { .. });
        let toast = self.improve.stage.event(event);
        if !matches!(self.improve.stage, Stage::Running { .. }) {
            self.improve.listening = None;
        }
        if finished {
            // The new transcript replaces the shown one.
            self.load(false, cx);
            self.meetings.update(cx, |meetings, cx| meetings.reload(cx));
            self.close_improve(cx);
        }
        if let Some(toast) = toast {
            Self::show(vec![toast], cx);
        }
        cx.notify();
    }

    /// "Annullér" while core works: stop it and close the dialog.
    fn cancel_improve(&mut self, cx: &mut Context<Self>) {
        if self.improve.is_running() {
            self.improve.listening = None;
            self.improve.stage = Stage::Idle;
            self.model.read(cx).runtime.spawn(async {
                if let Err(error) = retranscription::cancel_retranscription_command().await {
                    log::warn!("Could not cancel the retranscription: {error}");
                }
            });
            Self::show(
                vec![Toast {
                    kind: Kind::Info,
                    title: text::IMPROVE_CANCELLED.to_string(),
                    help: None,
                }],
                cx,
            );
        }
        self.close_improve(cx);
        cx.notify();
    }

    fn close_improve(&mut self, cx: &mut Context<Self>) {
        if !self.improve.is_running() {
            self.improve.stage = Stage::Idle;
        }
        if let Some(handle) = self.improve.window {
            cx.defer(move |cx| {
                let _ = handle.update(cx, |_, window, cx| window.close_dialog(cx));
            });
        }
    }

    fn show_improve(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let view = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, _, cx| {
            let Some(strong) = view.upgrade() else {
                return dialog;
            };
            let (stage, select, engine) = {
                let this = strong.read(cx);
                (
                    this.improve.stage.clone(),
                    this.improve.language.clone(),
                    this.improve
                        .model
                        .as_ref()
                        .map(|(engine, _)| engine.clone()),
                )
            };
            let palette = crate::ui::palette(cx);
            let running = matches!(stage, Stage::Running { .. });
            let (icon, title, description) = match &stage {
                Stage::Running { message, .. } => (
                    IconName::LoaderCircle,
                    text::IMPROVE_RUNNING_TITLE,
                    message
                        .clone()
                        .filter(|m| !m.is_empty())
                        .unwrap_or_else(|| text::IMPROVE_PROCESSING.to_string()),
                ),
                Stage::Failed(_) => (
                    IconName::CircleAlert,
                    text::IMPROVE_FAILED_TITLE,
                    text::IMPROVE_FAILED_DESCRIPTION.to_string(),
                ),
                _ => (
                    IconName::RefreshCw,
                    text::IMPROVE_TITLE,
                    text::IMPROVE_DESCRIPTION.to_string(),
                ),
            };
            let note = |text: &str| {
                div()
                    .text_size(typography::XS)
                    .text_color(palette.muted_foreground)
                    .child(text.to_string())
            };
            let body = match &stage {
                Stage::Choosing => {
                    let language = if engine.as_deref() == Some("parakeet") {
                        v_flex()
                            .gap(space::SM)
                            .child(note(text::IMPROVE_PARAKEET_NOTE))
                    } else {
                        v_flex()
                            .gap(space::SM)
                            .when_some(select, |this, select| {
                                this.child(Select::new(&select).h(layout::CONTROL_HEIGHT))
                            })
                            .child(note(text::IMPROVE_LANGUAGE_HELP))
                    };
                    v_flex()
                        .gap(space::LG)
                        .py(space::LG)
                        .child(
                            v_flex()
                                .gap(space::SM)
                                .child(
                                    div()
                                        .text_size(typography::SM)
                                        .font_weight(gpui::FontWeight::MEDIUM)
                                        .child(text::IMPROVE_LANGUAGE),
                                )
                                .child(language),
                        )
                        .child(note(text::IMPROVE_WARNING))
                        .into_any_element()
                }
                Stage::Failed(error) => v_flex()
                    .id("improve-error")
                    .role(gpui::Role::Alert)
                    .gap(space::XS)
                    .py(space::LG)
                    .text_size(typography::SM)
                    .text_color(palette.destructive)
                    .child(error.clone())
                    .child(note(text::IMPROVE_FAILED_HELP))
                    .into_any_element(),
                Stage::Running { percent, .. } => v_flex()
                    .id("improve-progress")
                    .role(gpui::Role::Status)
                    .gap(space::SM)
                    .py(space::LG)
                    .child(
                        div()
                            .h(size::HAIRLINE * 6.0)
                            .w_full()
                            .rounded_full()
                            .bg(palette.muted)
                            .child(div().h_full().rounded_full().bg(palette.primary).w(
                                gpui::relative(f32::from(percent.unwrap_or(0) as u16) / 100.0),
                            )),
                    )
                    .when_some(*percent, |this, percent| {
                        this.child(note(&text::percent(percent)))
                    })
                    .into_any_element(),
                Stage::Idle => div().into_any_element(),
            };
            let footer = {
                let cancel_view = view.clone();
                let start_view = view.clone();
                let close_view = view.clone();
                let mut footer = DialogFooter::new();
                match &stage {
                    Stage::Choosing => {
                        footer = footer
                            .child(
                                controls::button("improve-cancel", Variant::Secondary)
                                    .label(text::IMPROVE_CANCEL)
                                    .on_click(move |_, _, cx| {
                                        let _ = close_view
                                            .update(cx, |this, cx| this.cancel_improve(cx));
                                    }),
                            )
                            .child(
                                controls::button("improve-start", Variant::Primary)
                                    .label(text::IMPROVE_START)
                                    .on_click(move |_, _, cx| {
                                        let _ = start_view
                                            .update(cx, |this, cx| this.start_improve(cx));
                                    }),
                            );
                    }
                    Stage::Running { .. } => {
                        footer = footer.child(
                            controls::button("improve-stop", Variant::Secondary)
                                .label(text::IMPROVE_CANCEL)
                                .on_click(move |_, _, cx| {
                                    let _ =
                                        cancel_view.update(cx, |this, cx| this.cancel_improve(cx));
                                }),
                        );
                    }
                    Stage::Failed(_) => {
                        footer = footer
                            .child(
                                controls::button("improve-close", Variant::Secondary)
                                    .label(text::IMPROVE_CLOSE)
                                    .on_click(move |_, _, cx| {
                                        let _ = close_view
                                            .update(cx, |this, cx| this.cancel_improve(cx));
                                    }),
                            )
                            .child(
                                controls::button("improve-retry", Variant::Primary)
                                    .label(text::IMPROVE_RETRY)
                                    .on_click(move |_, _, cx| {
                                        let _ = start_view.update(cx, |this, cx| {
                                            this.improve.stage = Stage::Choosing;
                                            cx.notify();
                                        });
                                    }),
                            );
                    }
                    Stage::Idle => {}
                }
                footer
            };
            let _ = strings::DIALOG_CANCEL;
            dialog
                .w(size::IMPROVE_DIALOG_WIDTH)
                .title(
                    h_flex()
                        .gap(space::SM)
                        .child(Icon::from(icon).size(icon_size::SM).text_color(
                            if matches!(stage, Stage::Failed(_)) {
                                palette.destructive
                            } else {
                                palette.primary
                            },
                        ))
                        .child(crate::ui::section_title(title)),
                )
                .child(
                    v_flex()
                        .child(
                            div()
                                .text_size(typography::SM)
                                .text_color(palette.muted_foreground)
                                .child(description),
                        )
                        .child(body),
                )
                .footer(footer)
                // Core's job cannot be left running behind a closed dialog.
                .close_button(!running)
                .overlay_closable(!running)
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn running() -> Stage {
        Stage::Running {
            message: None,
            percent: None,
        }
    }

    #[test]
    fn progress_updates_the_message_and_percentage() {
        let mut stage = running();
        let done = stage.event(Event::Progress {
            message: "Finder tale".into(),
            percent: 40,
        });
        assert_eq!(done, None);
        assert_eq!(
            stage,
            Stage::Running {
                message: Some("Finder tale".into()),
                percent: Some(40)
            }
        );
    }

    #[test]
    fn done_ends_with_the_tauri_message() {
        let mut stage = running();
        let toast = stage.event(Event::Done { segments: 12 }).unwrap();
        assert_eq!(
            toast.title,
            "Gentranskriberingen er færdig. 12 afsnit er oprettet."
        );
        assert_eq!(stage, Stage::Idle);
    }

    #[test]
    fn an_error_shows_its_text_and_can_be_retried() {
        let mut stage = running();
        assert_eq!(stage.event(Event::Error("Ingen lyd".into())), None);
        assert_eq!(stage, Stage::Failed("Ingen lyd".into()));
    }

    #[test]
    fn events_outside_a_run_are_ignored() {
        for mut stage in [Stage::Idle, Stage::Choosing, Stage::Failed("x".into())] {
            let before = stage.clone();
            assert_eq!(stage.event(Event::Done { segments: 3 }), None);
            assert_eq!(stage, before);
        }
    }

    #[test]
    fn a_busy_core_is_reported_in_danish() {
        let mut stage = Stage::Choosing;
        stage.start_failed("Retranscription already in progress");
        assert_eq!(stage, Stage::Failed(text::IMPROVE_BUSY.into()));
        stage.start_failed("Noget andet");
        assert_eq!(stage, Stage::Failed("Noget andet".into()));
    }

    #[test]
    fn only_events_for_this_meeting_are_read() {
        let progress =
            r#"{"meeting_id":"m1","stage":"vad","progress_percentage":20,"message":"Finder tale"}"#;
        assert_eq!(
            parse_event("retranscription-progress", progress, "m1"),
            Some(Event::Progress {
                message: "Finder tale".into(),
                percent: 20
            })
        );
        assert_eq!(
            parse_event("retranscription-progress", progress, "m2"),
            None
        );
        assert_eq!(
            parse_event(
                "retranscription-complete",
                r#"{"meeting_id":"m1","segments_count":7}"#,
                "m1"
            ),
            Some(Event::Done { segments: 7 })
        );
        assert_eq!(
            parse_event(
                "retranscription-error",
                r#"{"meeting_id":"m1","error":"Ingen lyd"}"#,
                "m1"
            ),
            Some(Event::Error("Ingen lyd".into()))
        );
        assert_eq!(
            parse_event("retranscription-progress", "ikke json", "m1"),
            None
        );
    }

    #[test]
    fn the_language_rows_map_to_core_codes() {
        assert_eq!(language_code(None), None);
        assert_eq!(language_code(Some(0)), None, "row 0 is automatic");
        assert_eq!(language_code(Some(1)).as_deref(), Some("da"));
        assert_eq!(
            language_choices().len(),
            1 + tingskriver_core::meeting_language::CHOICES.len()
        );
    }
}
