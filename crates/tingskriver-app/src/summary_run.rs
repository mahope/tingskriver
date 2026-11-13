//! The state of one meeting's summary run, as Tauri's
//! `useSummaryGeneration.ts` keeps it: starting, running, stopping and what
//! to say when it ends. No window and no database in here, so the
//! transitions are tested without either (the view feeds it what it starts,
//! stops and reads back).
//!
//! The run itself is core's ([`tingskriver_core::meeting_summary`]); the
//! database is the source of truth, so a run started before the page was
//! opened, or by "Automatisk opsummering", is picked up from the stored
//! status.

use gpui::{App, Entity};
use gpui_component::notification::Notification;
use tingskriver_core::meeting_document::{Summary, SummaryStatus};
use tingskriver_core::meeting_summary::{self, Auto, Started, Stopped};
use tingskriver_core::strings::meeting_summary as text;

use crate::startup::DbState;
use crate::ui::AppModel;

/// How the message looks (Tauri's `toast.info/success/warning/error`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Info,
    Success,
    Warning,
    Error,
}

/// A message for the corner of the window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Toast {
    pub kind: Kind,
    pub title: String,
    pub help: Option<String>,
}

impl Toast {
    fn new(kind: Kind, title: impl Into<String>, help: Option<String>) -> Self {
        Toast {
            kind,
            title: title.into(),
            help,
        }
    }

    pub(crate) fn with_help(kind: Kind, title: &str, help: &str) -> Self {
        Toast::new(kind, title, Some(help.to_string()))
    }

    pub(crate) fn notification(&self) -> Notification {
        let body = self.help.clone().unwrap_or_default();
        match (self.kind, self.help.is_some()) {
            (Kind::Info, true) => Notification::info(body).title(self.title.clone()),
            (Kind::Success, true) => Notification::success(body).title(self.title.clone()),
            (Kind::Warning, true) => Notification::warning(body).title(self.title.clone()),
            (Kind::Error, true) => Notification::error(body).title(self.title.clone()),
            (Kind::Info, false) => Notification::info(self.title.clone()),
            (Kind::Success, false) => Notification::success(self.title.clone()),
            (Kind::Warning, false) => Notification::warning(self.title.clone()),
            (Kind::Error, false) => Notification::error(self.title.clone()),
        }
    }
}

/// Where the run stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Phase {
    #[default]
    Idle,
    /// Asked for, not yet registered in the database.
    Starting { regeneration: bool },
    /// Written in the background.
    Running { regeneration: bool },
}

#[derive(Debug, Default)]
pub(crate) struct Run {
    pub phase: Phase,
    /// "Stop" was pressed and core has not answered.
    pub stopping: bool,
    /// Why the last start failed, shown in the summary area until the next
    /// try.
    pub error: Option<String>,
}

impl Run {
    pub(crate) fn is_active(&self) -> bool {
        self.phase != Phase::Idle
    }

    /// The summary is being written (Stop is offered).
    pub(crate) fn is_running(&self) -> bool {
        matches!(self.phase, Phase::Running { .. })
    }

    /// The button was pressed. `false` while a run is already going.
    pub(crate) fn begin(&mut self, has_summary: bool) -> bool {
        if self.is_active() {
            return false;
        }
        self.phase = Phase::Starting {
            regeneration: has_summary,
        };
        self.stopping = false;
        self.error = None;
        true
    }

    /// Core has registered the run.
    pub(crate) fn started(&mut self, started: &Started) -> Vec<Toast> {
        let regeneration = match self.phase {
            Phase::Starting { regeneration } | Phase::Running { regeneration } => regeneration,
            Phase::Idle => false,
        };
        self.phase = Phase::Running { regeneration };
        let mut toasts = vec![Toast::with_help(
            Kind::Info,
            if regeneration {
                text::SUMMARY_REMAKING
            } else {
                text::SUMMARY_MAKING
            },
            &text::summary_using(&started.provider, &started.model),
        )];
        toasts.extend(
            started
                .notices
                .iter()
                .map(|notice| Toast::with_help(Kind::Warning, notice.title, notice.help)),
        );
        toasts
    }

    /// Core refused or failed before a run existed.
    pub(crate) fn start_failed(&mut self, message: String) -> Vec<Toast> {
        let regeneration = matches!(self.phase, Phase::Starting { regeneration: true });
        self.phase = Phase::Idle;
        self.stopping = false;
        self.error = Some(message.clone());
        vec![Toast::with_help(
            Kind::Error,
            if regeneration {
                text::SUMMARY_FAILED_AGAIN
            } else {
                text::SUMMARY_FAILED_NEW
            },
            &message,
        )]
    }

    /// The stored summary was read again.
    pub(crate) fn observe(&mut self, summary: &Summary) -> Vec<Toast> {
        match (self.phase, summary.status) {
            (Phase::Idle, SummaryStatus::Running) => {
                self.phase = Phase::Running {
                    regeneration: summary.has_content(),
                };
                Vec::new()
            }
            (Phase::Running { .. }, SummaryStatus::Completed) => {
                self.finish();
                vec![Toast::with_help(
                    Kind::Success,
                    text::SUMMARY_READY,
                    text::SUMMARY_READY_HELP,
                )]
            }
            (Phase::Running { regeneration }, SummaryStatus::Failed) => {
                self.finish();
                let message = summary
                    .error
                    .clone()
                    .filter(|m| !m.trim().is_empty())
                    .unwrap_or_else(|| text::SUMMARY_FAILED_NEW.to_string());
                if regeneration && summary.has_content() {
                    vec![Toast::with_help(
                        Kind::Error,
                        text::SUMMARY_FAILED_AGAIN,
                        &text::summary_previous_restored(message.trim_end_matches('.')),
                    )]
                } else {
                    vec![Toast::with_help(
                        Kind::Error,
                        if regeneration {
                            text::SUMMARY_FAILED_AGAIN
                        } else {
                            text::SUMMARY_FAILED_NEW
                        },
                        &message,
                    )]
                }
            }
            // Stopped (here or elsewhere): the previous summary is back.
            (Phase::Running { .. }, SummaryStatus::Idle) => {
                self.finish();
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    /// "Stop" was pressed. `false` when there is nothing to stop.
    pub(crate) fn stop_pressed(&mut self) -> bool {
        if self.is_running() && !self.stopping {
            self.stopping = true;
            return true;
        }
        false
    }

    /// Core answered the stop.
    pub(crate) fn stopped(&mut self, result: Result<Stopped, String>) -> Vec<Toast> {
        self.stopping = false;
        match result {
            Ok(Stopped::Cancelled | Stopped::NothingRunning) => {
                self.phase = Phase::Idle;
                vec![Toast::with_help(
                    Kind::Info,
                    text::SUMMARY_STOPPED,
                    text::SUMMARY_STOPPED_HELP,
                )]
            }
            Ok(Stopped::AlmostDone) => vec![Toast::with_help(
                Kind::Info,
                text::SUMMARY_ALMOST_DONE,
                text::SUMMARY_ALMOST_DONE_HELP,
            )],
            Err(error) => {
                log::warn!("The summary could not be stopped: {error}");
                vec![Toast::with_help(
                    Kind::Error,
                    text::SUMMARY_STOP_FAILED,
                    text::SUMMARY_STOP_FAILED_HELP,
                )]
            }
        }
    }

    fn finish(&mut self) {
        self.phase = Phase::Idle;
        self.stopping = false;
    }
}

/// "Automatisk opsummering": a recording has been transcribed, so make the
/// summary when the setting is on. A failure is shown, because the user
/// asked for the summary by turning the setting on.
pub(crate) fn auto_summary(model: Entity<AppModel>, meeting_id: String, cx: &mut App) {
    let state = model.read(cx);
    let (DbState::Ready(db), Some(host)) = (&state.db, state.host.clone()) else {
        return;
    };
    let db = db.clone();
    let task = state.runtime.spawn(async move {
        meeting_summary::auto_start(host, &db, &meeting_id)
            .await
            .into_result(meeting_id)
    });
    cx.spawn(async move |cx| {
        let Ok(Some((meeting_id, result))) = task.await else {
            return;
        };
        cx.update(|cx| {
            crate::ui::meetings_changed(cx);
            let toast = match result {
                Ok(started) => {
                    log::info!("Automatic summary started for {meeting_id}");
                    Toast::with_help(
                        Kind::Info,
                        text::SUMMARY_MAKING,
                        &text::summary_using(&started.provider, &started.model),
                    )
                }
                Err(message) => Toast::with_help(Kind::Error, text::SUMMARY_FAILED_NEW, &message),
            };
            crate::ui::notify_main_window(toast.notification(), cx);
        });
    })
    .detach();
}

/// [`Auto`] without the case that needs no message.
trait IntoResult {
    fn into_result(self, meeting_id: String) -> Option<(String, Result<Started, String>)>;
}

impl IntoResult for Auto {
    fn into_result(self, meeting_id: String) -> Option<(String, Result<Started, String>)> {
        match self {
            Auto::Started(started) => Some((meeting_id, Ok(started))),
            Auto::Failed(message) => Some((meeting_id, Err(message))),
            Auto::Skipped => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tingskriver_core::meeting_summary::Notice;

    fn started() -> Started {
        Started {
            process_id: "2026-11-13T09:00:00.000000000Z".into(),
            provider: "ollama".into(),
            model: "gemma3:1b".into(),
            notices: vec![Notice {
                title: text::SUMMARY_BILINGUAL,
                help: text::SUMMARY_BILINGUAL_HELP,
            }],
        }
    }

    fn summary(status: SummaryStatus, markdown: Option<&str>, error: Option<&str>) -> Summary {
        Summary {
            status,
            markdown: markdown.map(Into::into),
            error: error.map(Into::into),
        }
    }

    #[test]
    fn a_first_run_goes_from_starting_to_running_to_done() {
        let mut run = Run::default();
        assert!(run.begin(false));
        assert!(
            !run.begin(false),
            "a second press while starting does nothing"
        );
        let toasts = run.started(&started());
        assert_eq!(
            run.phase,
            Phase::Running {
                regeneration: false
            }
        );
        assert_eq!(toasts[0].title, text::SUMMARY_MAKING);
        assert_eq!(toasts[0].help.as_deref(), Some("Bruger ollama/gemma3:1b"));
        assert_eq!(toasts[1].kind, Kind::Warning);
        assert_eq!(toasts[1].title, text::SUMMARY_BILINGUAL);

        assert!(run
            .observe(&summary(SummaryStatus::Running, None, None))
            .is_empty());
        let done = run.observe(&summary(SummaryStatus::Completed, Some("# A"), None));
        assert_eq!(done[0].kind, Kind::Success);
        assert_eq!(done[0].title, text::SUMMARY_READY);
        assert!(!run.is_active());
    }

    #[test]
    fn a_new_run_over_a_summary_says_so() {
        let mut run = Run::default();
        assert!(run.begin(true));
        let toasts = run.started(&started());
        assert_eq!(toasts[0].title, text::SUMMARY_REMAKING);
    }

    #[test]
    fn a_failed_new_run_shows_the_error_and_can_be_tried_again() {
        let mut run = Run::default();
        run.begin(false);
        run.started(&started());
        let toasts = run.observe(&summary(
            SummaryStatus::Failed,
            None,
            Some("Modellen svarede ikke."),
        ));
        assert_eq!(toasts[0].kind, Kind::Error);
        assert_eq!(toasts[0].title, text::SUMMARY_FAILED_NEW);
        assert_eq!(toasts[0].help.as_deref(), Some("Modellen svarede ikke."));
        assert!(run.begin(false), "the button works again");
    }

    #[test]
    fn a_failed_regeneration_says_the_old_summary_is_back() {
        let mut run = Run::default();
        run.begin(true);
        run.started(&started());
        let toasts = run.observe(&summary(
            SummaryStatus::Failed,
            Some("# Gammelt"),
            Some("Modellen svarede ikke."),
        ));
        assert_eq!(toasts[0].title, text::SUMMARY_FAILED_AGAIN);
        assert_eq!(
            toasts[0].help.as_deref(),
            Some("Modellen svarede ikke. Din tidligere opsummering er gendannet.")
        );
    }

    #[test]
    fn a_start_that_fails_leaves_the_error_for_the_page() {
        let mut run = Run::default();
        run.begin(false);
        let toasts = run.start_failed(text::SUMMARY_NO_TRANSCRIPT.to_string());
        assert_eq!(run.phase, Phase::Idle);
        assert_eq!(run.error.as_deref(), Some(text::SUMMARY_NO_TRANSCRIPT));
        assert_eq!(toasts[0].title, text::SUMMARY_FAILED_NEW);
        assert_eq!(toasts[0].help.as_deref(), Some(text::SUMMARY_NO_TRANSCRIPT));
        // The next try clears it.
        assert!(run.begin(false));
        assert_eq!(run.error, None);
    }

    #[test]
    fn a_run_that_was_going_when_the_page_opened_is_picked_up() {
        let mut run = Run::default();
        assert!(run
            .observe(&summary(SummaryStatus::Running, Some("# Gammelt"), None))
            .is_empty());
        assert_eq!(run.phase, Phase::Running { regeneration: true });
        assert!(run.is_running());
        // Nothing is said about a finished run the page never saw start.
        let mut idle = Run::default();
        assert!(idle
            .observe(&summary(SummaryStatus::Completed, Some("# A"), None))
            .is_empty());
    }

    #[test]
    fn stopping_goes_through_the_core_answer() {
        let mut run = Run::default();
        run.begin(false);
        assert!(!run.stop_pressed(), "nothing to stop before the run exists");
        run.started(&started());
        assert!(run.stop_pressed());
        assert!(!run.stop_pressed(), "pressed twice");
        let toasts = run.stopped(Ok(Stopped::AlmostDone));
        assert_eq!(toasts[0].title, text::SUMMARY_ALMOST_DONE);
        assert!(
            run.is_running(),
            "it carries on until the database says otherwise"
        );
        assert!(run.stop_pressed());
        let toasts = run.stopped(Ok(Stopped::Cancelled));
        assert_eq!(toasts[0].title, text::SUMMARY_STOPPED);
        assert!(!run.is_active());
    }

    #[test]
    fn a_stop_that_fails_keeps_the_run() {
        let mut run = Run::default();
        run.begin(false);
        run.started(&started());
        run.stop_pressed();
        let toasts = run.stopped(Err("databasen er låst".into()));
        assert_eq!(toasts[0].kind, Kind::Error);
        assert_eq!(toasts[0].title, text::SUMMARY_STOP_FAILED);
        assert!(run.is_running());
        assert!(!run.stopping);
    }

    #[test]
    fn a_run_stopped_elsewhere_ends_quietly() {
        let mut run = Run::default();
        run.begin(false);
        run.started(&started());
        assert!(run
            .observe(&summary(SummaryStatus::Idle, None, None))
            .is_empty());
        assert!(!run.is_active());
    }
}
