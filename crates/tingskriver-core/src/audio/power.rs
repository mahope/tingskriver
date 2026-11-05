//! A recording that sleep or logoff would cut off (issue #179).
//!
//! The host turns Windows' power and session messages into [`PowerEvent`]s
//! and hands them to a [`PowerController`], on the thread that got the
//! message. That thread must not return before the recording is saved: when
//! the handler returns, Windows goes on to sleep or end the session. So
//! [`PowerController::handle`] is synchronous, and everything it needs from
//! the recording sits behind [`PowerSession`], which a fake replaces in
//! tests (on Linux too).
//!
//! The rules ([`decide`]) are plain data in, plain data out:
//!
//! - suspend or a really ending session while recording: finish the
//!   recording (the audio is on disk when stop returns), mark the meeting,
//!   save the database;
//! - resume: tell the user once what happened; **never start a recording**;
//! - lock, unlock, a session end that was cancelled, a query that may still
//!   be vetoed: log only. A locked screen is an ordinary meeting.
//!
//! Every event is logged with the time it arrived.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

/// What Windows told the host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerEvent {
    /// `PBT_APMSUSPEND`: the machine is about to sleep.
    Suspend,
    /// `PBT_APMRESUMEAUTOMATIC`: it woke up.
    Resume,
    /// `WM_QUERYENDSESSION`: logoff or shutdown is proposed. Another program
    /// can still veto it, so nothing is cut off yet.
    QueryEndSession,
    /// `WM_ENDSESSION`: `true` when the session really ends.
    EndSession { ending: bool },
    /// `WTS_SESSION_LOCK`.
    Lock,
    /// `WTS_SESSION_UNLOCK`.
    Unlock,
}

impl PowerEvent {
    /// The Windows name, for the log.
    pub fn name(self) -> &'static str {
        match self {
            PowerEvent::Suspend => "PBT_APMSUSPEND",
            PowerEvent::Resume => "PBT_APMRESUMEAUTOMATIC",
            PowerEvent::QueryEndSession => "WM_QUERYENDSESSION",
            PowerEvent::EndSession { ending: true } => "WM_ENDSESSION (ending)",
            PowerEvent::EndSession { ending: false } => "WM_ENDSESSION (cancelled)",
            PowerEvent::Lock => "WTS_SESSION_LOCK",
            PowerEvent::Unlock => "WTS_SESSION_UNLOCK",
        }
    }
}

/// Why a recording was ended by the system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InterruptCause {
    /// The computer went to sleep.
    Sleep,
    /// The user logged off or the computer shut down.
    Logoff,
}

impl InterruptCause {
    /// Stored in `metadata.json` as `ended_by`.
    pub fn as_str(self) -> &'static str {
        match self {
            InterruptCause::Sleep => "sleep",
            InterruptCause::Logoff => "logoff",
        }
    }

    /// Plain Danish text for the user, also the stop reason of the event.
    pub fn message(self) -> &'static str {
        match self {
            InterruptCause::Sleep => crate::strings::INTERRUPTED_BY_SLEEP,
            InterruptCause::Logoff => crate::strings::INTERRUPTED_BY_LOGOFF,
        }
    }

    /// How long the handler may keep Windows waiting. Sleep gives about two
    /// seconds; logoff a few more before the process is ended.
    pub fn budget(self) -> std::time::Duration {
        match self {
            InterruptCause::Sleep => std::time::Duration::from_millis(2000),
            InterruptCause::Logoff => std::time::Duration::from_millis(4500),
        }
    }
}

/// What to do about one event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerDecision {
    /// A recording runs and is about to be cut off: finish it.
    FinishRecording(InterruptCause),
    /// The machine is back: show the notice if a recording was ended.
    AnnounceResume,
    /// Only the log line.
    LogOnly,
}

/// The decision for `event`. `recording` is whether a recording runs now.
pub fn decide(event: PowerEvent, recording: bool) -> PowerDecision {
    match event {
        PowerEvent::Suspend if recording => PowerDecision::FinishRecording(InterruptCause::Sleep),
        PowerEvent::EndSession { ending: true } if recording => {
            PowerDecision::FinishRecording(InterruptCause::Logoff)
        }
        PowerEvent::Resume => PowerDecision::AnnounceResume,
        _ => PowerDecision::LogOnly,
    }
}

/// Written to `interrupted-recording.json` in the data folder and sent to
/// the UI as `recording-interrupted`, so the user sees why a meeting ended.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InterruptNotice {
    pub cause: InterruptCause,
    /// RFC 3339, local time.
    pub at: String,
    pub meeting_folder: Option<String>,
    /// `ok`, `delvis` or `fejlet`, as for any recording.
    pub audio_status: Option<String>,
    /// Plain Danish text.
    pub message: String,
}

/// File name of the notice, in the app data folder.
pub const NOTICE_FILE: &str = "interrupted-recording.json";

impl InterruptNotice {
    /// The lines the UI shows: why, when, what happened to the audio, and
    /// that nothing starts by itself.
    pub fn lines(&self) -> Vec<String> {
        use crate::strings;
        let audio = match self.audio_status.as_deref() {
            Some("ok") => strings::INTERRUPTED_AUDIO_OK,
            Some("delvis") => strings::INTERRUPTED_AUDIO_PARTIAL,
            Some("fejlet") => strings::INTERRUPTED_AUDIO_FAILED,
            _ => strings::INTERRUPTED_AUDIO_UNKNOWN,
        };
        vec![
            self.message.clone(),
            strings::interrupted_when(&self.at),
            audio.to_string(),
            strings::INTERRUPTED_NO_RESTART.to_string(),
        ]
    }

    pub fn path(app_data_dir: &Path) -> PathBuf {
        app_data_dir.join(NOTICE_FILE)
    }

    /// Write via a temporary file, so a cut never leaves half a file.
    #[allow(clippy::disallowed_methods)]
    pub fn save(&self, app_data_dir: &Path) -> std::io::Result<()> {
        let json = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        let tmp = app_data_dir.join(format!(".{NOTICE_FILE}.tmp"));
        std::fs::create_dir_all(app_data_dir)?;
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, Self::path(app_data_dir))
    }

    /// The saved notice, if any and readable.
    #[allow(clippy::disallowed_methods)]
    pub fn load(app_data_dir: &Path) -> Option<InterruptNotice> {
        let text = std::fs::read_to_string(Self::path(app_data_dir)).ok()?;
        serde_json::from_str(&text).ok()
    }

    /// The user has seen it.
    #[allow(clippy::disallowed_methods)]
    pub fn dismiss(app_data_dir: &Path) {
        let _ = std::fs::remove_file(Self::path(app_data_dir));
    }
}

/// What the controller needs from the recording and the app. The real
/// implementation is [`CoreSession`]; tests use a fake.
pub trait PowerSession: Send + Sync {
    /// A recording runs now.
    fn recording_active(&self) -> bool;
    /// Stop the recording, wait until the audio is on disk (at most
    /// `cause.budget()`), mark the meeting and save the notice. `None` when
    /// nothing was recording.
    fn finish_recording(&self, cause: InterruptCause, at: &str) -> Option<InterruptNotice>;
    /// Flush the database to disk.
    fn save_database(&self);
    /// Tell the UI (event `recording-interrupted`).
    fn announce(&self, notice: &InterruptNotice);
}

/// Applies [`decide`] and keeps the notice until the machine is back.
pub struct PowerController<S: PowerSession> {
    session: S,
    pending: Mutex<Option<InterruptNotice>>,
}

impl<S: PowerSession> PowerController<S> {
    pub fn new(session: S) -> Self {
        PowerController {
            session,
            pending: Mutex::new(None),
        }
    }

    /// Handle one event; returns when it is safe for Windows to go on. One
    /// log line per event, with the time.
    pub fn handle(&self, event: PowerEvent) {
        let at = chrono::Local::now().to_rfc3339();
        let recording = self.session.recording_active();
        let decision = decide(event, recording);
        log::info!(
            "power: {} at {at}; recording={recording}; action={decision:?}",
            event.name()
        );
        match decision {
            PowerDecision::FinishRecording(cause) => {
                match self.session.finish_recording(cause, &at) {
                    Some(notice) => {
                        log::info!(
                            "power: recording ended by {} at {at}; audio {}; folder saved: {}",
                            cause.as_str(),
                            notice.audio_status.as_deref().unwrap_or("unknown"),
                            notice.meeting_folder.is_some()
                        );
                        *self.pending.lock().unwrap_or_else(|e| e.into_inner()) = Some(notice);
                    }
                    None => log::warn!("power: no recording to finish at {at}"),
                }
                self.session.save_database();
            }
            PowerDecision::AnnounceResume => {
                let pending = self
                    .pending
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .take();
                if let Some(notice) = pending {
                    log::info!(
                        "power: telling the user that the meeting ended ({})",
                        notice.cause.as_str()
                    );
                    self.session.announce(&notice);
                }
            }
            PowerDecision::LogOnly => {}
        }
    }
}

/// The real session: core's recording commands on the core runtime.
pub struct CoreSession {
    host: crate::host::HostHandle,
    runtime: tokio::runtime::Handle,
}

impl CoreSession {
    /// `runtime` is the core runtime; `host` the `AppHost` of the app.
    pub fn new(host: crate::host::HostHandle, runtime: tokio::runtime::Handle) -> Self {
        CoreSession { host, runtime }
    }
}

impl PowerSession for CoreSession {
    fn recording_active(&self) -> bool {
        super::recording_commands::is_recording_now()
    }

    fn finish_recording(&self, cause: InterruptCause, at: &str) -> Option<InterruptNotice> {
        let host = self.host.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        let at_owned = at.to_string();
        // On the runtime, so the work goes on if Windows stops waiting for us.
        self.runtime.spawn(async move {
            let result =
                super::recording_commands::stop_recording_for_power(host.clone(), cause).await;
            let notice = match result {
                Ok(Some(saved)) => Some(InterruptNotice {
                    cause,
                    at: at_owned.clone(),
                    meeting_folder: saved
                        .meeting_folder
                        .as_ref()
                        .map(|p| p.to_string_lossy().to_string()),
                    audio_status: Some(saved.status.as_str().to_string()),
                    message: cause.message().to_string(),
                }),
                Ok(None) => None,
                Err(e) => {
                    log::error!("power: stopping the recording failed: {e}");
                    Some(InterruptNotice {
                        cause,
                        at: at_owned.clone(),
                        meeting_folder: None,
                        audio_status: Some("fejlet".to_string()),
                        message: cause.message().to_string(),
                    })
                }
            };
            if let Some(notice) = &notice {
                if let Ok(dir) = host.app_data_dir() {
                    let notice = notice.clone();
                    let saved = tokio::task::spawn_blocking(move || notice.save(&dir)).await;
                    if !matches!(saved, Ok(Ok(()))) {
                        log::warn!("power: could not save the notice: {saved:?}");
                    }
                }
            }
            let _ = tx.send(notice);
        });
        match rx.recv_timeout(cause.budget()) {
            Ok(notice) => notice,
            Err(_) => {
                log::warn!(
                    "power: the recording was not saved within {} ms; saving goes on in the background",
                    cause.budget().as_millis()
                );
                None
            }
        }
    }

    fn save_database(&self) {
        let Some(db) = self.host.database() else {
            return;
        };
        let (tx, rx) = std::sync::mpsc::channel();
        self.runtime.spawn(async move {
            let _ = tx.send(db.checkpoint().await);
        });
        match rx.recv_timeout(std::time::Duration::from_millis(1000)) {
            Ok(Ok(())) => log::info!("power: database flushed to disk"),
            Ok(Err(e)) => log::warn!("power: database flush failed: {e}"),
            Err(_) => log::warn!("power: database flush still running; going on"),
        }
    }

    fn announce(&self, notice: &InterruptNotice) {
        use crate::host::HostExt;
        let _ = self.host.emit("recording-interrupted", notice);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
#[allow(clippy::disallowed_methods)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// Records the order of calls, like the real thing would do the work.
    #[derive(Default)]
    struct Fake {
        recording: Mutex<bool>,
        calls: Arc<Mutex<Vec<String>>>,
    }

    impl Fake {
        fn recording() -> (Self, Arc<Mutex<Vec<String>>>) {
            let fake = Fake::default();
            *fake.recording.lock().unwrap() = true;
            let calls = fake.calls.clone();
            (fake, calls)
        }
    }

    impl PowerSession for Fake {
        fn recording_active(&self) -> bool {
            *self.recording.lock().unwrap()
        }
        fn finish_recording(&self, cause: InterruptCause, at: &str) -> Option<InterruptNotice> {
            // The audio is on disk when this returns, then nothing records.
            self.calls
                .lock()
                .unwrap()
                .push(format!("finish:{}", cause.as_str()));
            *self.recording.lock().unwrap() = false;
            Some(InterruptNotice {
                cause,
                at: at.to_string(),
                meeting_folder: Some("meeting-1".into()),
                audio_status: Some("ok".into()),
                message: cause.message().into(),
            })
        }
        fn save_database(&self) {
            self.calls.lock().unwrap().push("database".into());
        }
        fn announce(&self, notice: &InterruptNotice) {
            self.calls
                .lock()
                .unwrap()
                .push(format!("announce:{}", notice.cause.as_str()));
        }
    }

    #[test]
    fn suspend_while_recording_finishes_the_recording() {
        assert_eq!(
            decide(PowerEvent::Suspend, true),
            PowerDecision::FinishRecording(InterruptCause::Sleep)
        );
        assert_eq!(
            decide(PowerEvent::EndSession { ending: true }, true),
            PowerDecision::FinishRecording(InterruptCause::Logoff)
        );
    }

    #[test]
    fn nothing_else_cuts_a_recording() {
        for event in [
            PowerEvent::QueryEndSession,
            PowerEvent::EndSession { ending: false },
            PowerEvent::Lock,
            PowerEvent::Unlock,
        ] {
            assert_eq!(decide(event, true), PowerDecision::LogOnly, "{event:?}");
        }
        // Without a recording there is nothing to finish.
        assert_eq!(decide(PowerEvent::Suspend, false), PowerDecision::LogOnly);
        assert_eq!(
            decide(PowerEvent::EndSession { ending: true }, false),
            PowerDecision::LogOnly
        );
    }

    #[test]
    fn the_recording_is_saved_before_the_handler_returns() {
        let (fake, calls) = Fake::recording();
        let controller = PowerController::new(fake);
        controller.handle(PowerEvent::Suspend);
        // Same thread, same order: saved and database flushed before Windows may sleep.
        assert_eq!(*calls.lock().unwrap(), ["finish:sleep", "database"]);
    }

    #[test]
    fn resume_announces_once_and_never_starts_a_recording() {
        let (fake, calls) = Fake::recording();
        let controller = PowerController::new(fake);
        controller.handle(PowerEvent::Suspend);
        controller.handle(PowerEvent::Resume);
        controller.handle(PowerEvent::Resume);
        controller.handle(PowerEvent::Unlock);
        assert_eq!(
            *calls.lock().unwrap(),
            ["finish:sleep", "database", "announce:sleep"],
            "one notice, and no call that could start a recording"
        );
        assert!(!controller.session.recording_active());
    }

    #[test]
    fn resume_without_a_cut_recording_says_nothing() {
        let fake = Fake::default();
        let calls = fake.calls.clone();
        let controller = PowerController::new(fake);
        controller.handle(PowerEvent::Suspend);
        controller.handle(PowerEvent::Resume);
        assert!(calls.lock().unwrap().is_empty());
    }

    #[test]
    fn a_vetoed_logoff_keeps_recording() {
        let (fake, calls) = Fake::recording();
        let controller = PowerController::new(fake);
        controller.handle(PowerEvent::QueryEndSession);
        controller.handle(PowerEvent::EndSession { ending: false });
        controller.handle(PowerEvent::Lock);
        assert!(calls.lock().unwrap().is_empty());
        assert!(controller.session.recording_active());
        controller.handle(PowerEvent::EndSession { ending: true });
        assert_eq!(*calls.lock().unwrap(), ["finish:logoff", "database"]);
    }

    #[test]
    fn the_notice_says_why_when_and_what_happened_to_the_audio() {
        use crate::strings;
        let mut notice = InterruptNotice {
            cause: InterruptCause::Sleep,
            at: "2026-11-05T10:00:00+01:00".into(),
            meeting_folder: None,
            audio_status: Some("ok".into()),
            message: InterruptCause::Sleep.message().into(),
        };
        let lines = notice.lines();
        assert_eq!(
            lines[0],
            "Mødet blev afsluttet, fordi computeren gik i dvale."
        );
        assert_eq!(lines[1], "Tidspunkt: 2026-11-05T10:00:00+01:00");
        assert_eq!(lines[2], strings::INTERRUPTED_AUDIO_OK);
        assert_eq!(lines[3], strings::INTERRUPTED_NO_RESTART);
        for (status, expected) in [
            (Some("delvis"), strings::INTERRUPTED_AUDIO_PARTIAL),
            (Some("fejlet"), strings::INTERRUPTED_AUDIO_FAILED),
            (None, strings::INTERRUPTED_AUDIO_UNKNOWN),
        ] {
            notice.audio_status = status.map(String::from);
            assert_eq!(notice.lines()[2], expected);
        }
    }

    #[test]
    fn the_notice_survives_a_restart_until_dismissed() {
        let dir =
            std::env::temp_dir().join(format!("tingskriver-power-notice-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(InterruptNotice::load(&dir), None);
        let notice = InterruptNotice {
            cause: InterruptCause::Logoff,
            at: "2026-11-05T10:00:00+01:00".into(),
            meeting_folder: None,
            audio_status: Some("delvis".into()),
            message: InterruptCause::Logoff.message().into(),
        };
        notice.save(&dir).unwrap();
        assert_eq!(InterruptNotice::load(&dir), Some(notice));
        InterruptNotice::dismiss(&dir);
        assert_eq!(InterruptNotice::load(&dir), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
