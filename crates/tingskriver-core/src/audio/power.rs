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
use std::sync::{Arc, Mutex};

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

/// "5. nov. 2026 kl. 10.00" for an RFC 3339 time; the text as it is when it
/// cannot be read.
pub fn format_when(at: &str) -> String {
    use chrono::{Datelike, Timelike};
    const MONTHS: [&str; 12] = [
        "jan.", "feb.", "mar.", "apr.", "maj", "jun.", "jul.", "aug.", "sep.", "okt.", "nov.",
        "dec.",
    ];
    match chrono::DateTime::parse_from_rfc3339(at) {
        Ok(t) => format!(
            "{}. {} {} kl. {:02}.{:02}",
            t.day(),
            MONTHS[t.month0() as usize],
            t.year(),
            t.hour(),
            t.minute()
        ),
        Err(_) => at.to_string(),
    }
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
            strings::interrupted_when(&format_when(&self.at)),
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
        use std::io::Write;
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(json.as_bytes())?;
        file.sync_all()?;
        std::fs::rename(&tmp, Self::path(app_data_dir))?;
        #[cfg(unix)]
        std::fs::File::open(app_data_dir)?.sync_all()?;
        Ok(())
    }

    /// The saved notice, if any and readable.
    #[allow(clippy::disallowed_methods)]
    pub fn load(app_data_dir: &Path) -> Option<InterruptNotice> {
        let text = match std::fs::read_to_string(Self::path(app_data_dir)) {
            Ok(text) => text,
            Err(e) => {
                if e.kind() != std::io::ErrorKind::NotFound {
                    log::warn!("power: could not read the notice: {e}");
                }
                return None;
            }
        };
        match serde_json::from_str(&text) {
            Ok(notice) => Some(notice),
            Err(e) => {
                log::warn!("power: could not parse the notice: {e}");
                None
            }
        }
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
    /// Shared resume notice, also updated when a timed-out stop completes.
    fn pending_notice(&self) -> Option<Arc<Mutex<Option<InterruptNotice>>>> {
        None
    }
    /// A recording runs now.
    fn recording_active(&self) -> bool;
    /// Stop the recording, wait until the audio is on disk (at most
    /// `cause.budget()`), mark the meeting and save the notice. `None` when
    /// nothing was recording. When the budget runs out first, the notice
    /// has no `audio_status` (saving goes on in the background).
    fn finish_recording(&self, cause: InterruptCause, at: &str) -> Option<InterruptNotice>;
    /// Flush the database to disk, waiting at most `budget`.
    fn save_database(&self, budget: std::time::Duration);
    /// Tell the UI (event `recording-interrupted`).
    fn announce(&self, notice: &InterruptNotice);
}

/// Applies [`decide`] and keeps the notice until the machine is back.
pub struct PowerController<S: PowerSession> {
    session: S,
    pending: Arc<Mutex<Option<InterruptNotice>>>,
}

impl<S: PowerSession> PowerController<S> {
    pub fn new(session: S) -> Self {
        let pending = session.pending_notice().unwrap_or_default();
        PowerController { session, pending }
    }

    /// Handle one event; returns when it is safe for Windows to go on. One
    /// log line per event, with the time.
    pub fn handle(&self, event: PowerEvent) {
        let started = std::time::Instant::now();
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
                        let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
                        // A late completion may already have replaced the timeout notice.
                        if !pending
                            .as_ref()
                            .is_some_and(|p| p.at == at && p.audio_status.is_some())
                        {
                            *pending = Some(notice);
                        }
                    }
                    None => log::warn!("power: no recording to finish at {at}"),
                }
                // One budget for both steps: Windows waits for the handler
                // as a whole, not for each part.
                self.session
                    .save_database(cause.budget().saturating_sub(started.elapsed()));
            }
            PowerDecision::AnnounceResume => {
                let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
                // Keep resume and late completion events in the same order as the state.
                if let Some(notice) = pending.take() {
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
    pending: Arc<Mutex<Option<InterruptNotice>>>,
}

impl CoreSession {
    /// `runtime` is the core runtime; `host` the `AppHost` of the app.
    pub fn new(host: crate::host::HostHandle, runtime: tokio::runtime::Handle) -> Self {
        CoreSession {
            host,
            runtime,
            pending: Arc::default(),
        }
    }

    fn finish_with_stop(
        &self,
        cause: InterruptCause,
        at: &str,
        budget: std::time::Duration,
        stop: impl std::future::Future<Output = Result<Option<super::recording_saver::SaveResult>, String>>
            + Send
            + 'static,
    ) -> Option<InterruptNotice> {
        let started = std::time::Instant::now();
        let host = self.host.clone();
        let fallback = InterruptNotice {
            cause,
            at: at.to_string(),
            meeting_folder: None,
            audio_status: None,
            message: cause.message().to_string(),
        };
        let previous_notice;
        let previous_pending;
        {
            let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
            previous_pending = pending.clone();
            previous_notice = host
                .app_data_dir()
                .ok()
                .and_then(|dir| InterruptNotice::load(&dir));
            if let Ok(dir) = host.app_data_dir() {
                if let Err(e) = fallback.save(&dir) {
                    log::warn!("power: could not save the timeout notice: {e}");
                }
            }
            *pending = Some(fallback.clone());
        }
        let pending = self.pending.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        let at_owned = at.to_string();
        // On the runtime, so the work goes on if Windows stops waiting for us.
        self.runtime.spawn(async move {
            let result = stop.await;
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
            let notice_to_save = notice.clone();
            let completed_at = at_owned.clone();
            let saved = tokio::task::spawn_blocking(move || {
                let mut pending = pending.lock().unwrap_or_else(|e| e.into_inner());
                if pending.as_ref().is_some_and(|p| p.at != completed_at) {
                    return;
                }
                if let Some(notice) = &notice_to_save {
                    if let Ok(dir) = host.app_data_dir() {
                        if let Err(e) = notice.save(&dir) {
                            log::warn!("power: could not save the notice: {e}");
                        }
                    }
                } else if let Ok(dir) = host.app_data_dir() {
                    // A regular Stop may have won the race. Undo only our fallback.
                    if InterruptNotice::load(&dir).is_some_and(|p| p.at == completed_at) {
                        if let Some(previous) = previous_notice {
                            if let Err(e) = previous.save(&dir) {
                                log::warn!("power: could not restore the previous notice: {e}");
                            }
                        } else {
                            InterruptNotice::dismiss(&dir);
                        }
                    }
                }
                if pending.as_ref().is_some_and(|p| p.at == completed_at) {
                    *pending = notice_to_save.or(previous_pending);
                } else if let Some(notice) = &notice_to_save {
                    use crate::host::HostExt;
                    let _ = host.emit("recording-interrupted", notice);
                }
            })
            .await;
            if let Err(e) = saved {
                log::warn!("power: could not finish the notice: {e}");
            }
            let _ = tx.send(notice);
        });
        match rx.recv_timeout(budget.saturating_sub(started.elapsed())) {
            Ok(notice) => notice,
            Err(_) => {
                log::warn!(
                    "power: the recording was not confirmed within {} ms; saving goes on in the background",
                    cause.budget().as_millis()
                );
                // Still a cut recording: the user is told on resume, and the
                // background task replaces the notice with the real status.
                Some(fallback)
            }
        }
    }
}

impl PowerSession for CoreSession {
    fn pending_notice(&self) -> Option<Arc<Mutex<Option<InterruptNotice>>>> {
        Some(self.pending.clone())
    }

    fn recording_active(&self) -> bool {
        super::recording_commands::is_recording_now()
    }

    fn finish_recording(&self, cause: InterruptCause, at: &str) -> Option<InterruptNotice> {
        let host = self.host.clone();
        let stop_at = at.to_string();
        self.finish_with_stop(cause, at, cause.budget(), async move {
            super::recording_commands::stop_recording_for_power(host, cause, stop_at).await
        })
    }

    fn save_database(&self, budget: std::time::Duration) {
        let Some(db) = self.host.database() else {
            return;
        };
        let (tx, rx) = std::sync::mpsc::channel();
        self.runtime.spawn(async move {
            let _ = tx.send(db.checkpoint().await);
        });
        match rx.recv_timeout(budget) {
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
        data_dir: Option<PathBuf>,
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
        fn save_database(&self, _budget: std::time::Duration) {
            self.calls.lock().unwrap().push("database".into());
        }
        fn announce(&self, notice: &InterruptNotice) {
            self.calls
                .lock()
                .unwrap()
                .push(format!("announce:{}", notice.cause.as_str()));
        }
    }

    impl crate::host::AppHost for Fake {
        fn emit_json(&self, event: &str, payload: &str) -> Result<(), String> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("{event}:{payload}"));
            Ok(())
        }
        fn database(&self) -> Option<crate::database::manager::DatabaseManager> {
            None
        }
        fn app_data_dir(&self) -> Result<PathBuf, String> {
            self.data_dir.clone().ok_or_else(|| "no data dir".into())
        }
        fn listen(&self, _: &str, _: Box<dyn Fn(&str) + Send + Sync>) -> crate::host::ListenerId {
            0
        }
        fn unlisten(&self, _: crate::host::ListenerId) {}
        fn read_setting(&self, _: &str, _: &str) -> Result<Option<serde_json::Value>, String> {
            Ok(None)
        }
        fn write_setting(&self, _: &str, _: &str, _: serde_json::Value) -> Result<(), String> {
            Ok(())
        }
    }

    #[test]
    fn core_power_timeout_persists_then_updates_pending_and_restart_notice() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        for cause in [InterruptCause::Sleep, InterruptCause::Logoff] {
            let dir = tempfile::tempdir().unwrap();
            let fake = Arc::new(Fake {
                data_dir: Some(dir.path().to_path_buf()),
                ..Fake::default()
            });
            let calls = fake.calls.clone();
            let session = CoreSession::new(fake, runtime.handle().clone());
            let controller = PowerController::new(session);
            let (release, wait) = tokio::sync::oneshot::channel();
            let at = "2026-11-05T10:00:00+01:00";
            let fallback = controller
                .session
                .finish_with_stop(
                    cause,
                    at,
                    std::time::Duration::from_millis(10),
                    async move {
                        wait.await.unwrap();
                        Ok(Some(super::super::recording_saver::SaveResult {
                            status: super::super::finalize::AudioSaveStatus::Ok,
                            message: String::new(),
                            meeting_folder: Some(PathBuf::from("meeting-1")),
                            duration_seconds: 1.0,
                        }))
                    },
                )
                .unwrap();
            assert_eq!(fallback.audio_status, None);
            assert_eq!(InterruptNotice::load(dir.path()), Some(fallback.clone()));
            assert_eq!(*controller.pending.lock().unwrap(), Some(fallback));
            release.send(()).unwrap();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            loop {
                if controller
                    .pending
                    .lock()
                    .unwrap()
                    .as_ref()
                    .unwrap()
                    .audio_status
                    .as_deref()
                    == Some("ok")
                {
                    break;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "stop did not complete"
                );
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            let saved = InterruptNotice::load(dir.path()).unwrap();
            assert_eq!(saved.at, at);
            assert_eq!(saved.audio_status.as_deref(), Some("ok"));
            assert_eq!(saved.meeting_folder.as_deref(), Some("meeting-1"));
            assert!(!dir.path().join(format!(".{NOTICE_FILE}.tmp")).exists());
            controller.handle(PowerEvent::Resume);
            controller.handle(PowerEvent::Resume);
            let events = calls.lock().unwrap();
            assert_eq!(events.len(), 1);
            let (_, payload) = events[0].split_once(':').unwrap();
            assert_eq!(
                serde_json::from_str::<InterruptNotice>(payload).unwrap(),
                saved
            );
        }
    }

    #[test]
    fn core_power_completion_after_resume_updates_announced_notice() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let fake = Arc::new(Fake {
            data_dir: Some(dir.path().to_path_buf()),
            ..Fake::default()
        });
        let calls = fake.calls.clone();
        let controller = PowerController::new(CoreSession::new(fake, runtime.handle().clone()));
        let (release, wait) = tokio::sync::oneshot::channel();
        controller.session.finish_with_stop(
            InterruptCause::Sleep,
            "2026-11-05T10:00:00+01:00",
            std::time::Duration::from_millis(10),
            async move {
                wait.await.unwrap();
                Ok(Some(super::super::recording_saver::SaveResult {
                    status: super::super::finalize::AudioSaveStatus::Ok,
                    message: String::new(),
                    meeting_folder: Some(PathBuf::from("meeting-1")),
                    duration_seconds: 1.0,
                }))
            },
        );
        controller.handle(PowerEvent::Resume);
        assert_eq!(calls.lock().unwrap().len(), 1);
        release.send(()).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if calls.lock().unwrap().len() == 2 {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "late status was not announced"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        controller.handle(PowerEvent::Resume);
        let events = calls.lock().unwrap();
        assert_eq!(events.len(), 2);
        let (_, payload) = events.last().unwrap().split_once(':').unwrap();
        let notice: InterruptNotice = serde_json::from_str(payload).unwrap();
        assert_eq!(notice.audio_status.as_deref(), Some("ok"));
        assert_eq!(InterruptNotice::load(dir.path()), Some(notice));
    }

    #[test]
    fn core_power_no_recording_restores_previous_notice() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        for previous in [
            None,
            Some(InterruptNotice {
                cause: InterruptCause::Sleep,
                at: "previous-time".into(),
                meeting_folder: Some("previous-meeting".into()),
                audio_status: Some("ok".into()),
                message: InterruptCause::Sleep.message().into(),
            }),
        ] {
            let dir = tempfile::tempdir().unwrap();
            if let Some(notice) = &previous {
                notice.save(dir.path()).unwrap();
            }
            let fake = Arc::new(Fake {
                data_dir: Some(dir.path().to_path_buf()),
                ..Fake::default()
            });
            let session = CoreSession::new(fake, runtime.handle().clone());
            assert_eq!(
                session.finish_with_stop(
                    InterruptCause::Logoff,
                    "new-time",
                    std::time::Duration::from_secs(5),
                    async { Ok(None) },
                ),
                None
            );
            assert_eq!(InterruptNotice::load(dir.path()), previous);
            assert_eq!(*session.pending.lock().unwrap(), None);
        }
    }

    #[test]
    fn power_notice_handles_load_errors_and_reports_rename_failure() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(InterruptNotice::load(dir.path()), None);
        std::fs::write(InterruptNotice::path(dir.path()), "broken json").unwrap();
        assert_eq!(InterruptNotice::load(dir.path()), None);
        std::fs::remove_file(InterruptNotice::path(dir.path())).unwrap();
        std::fs::create_dir(InterruptNotice::path(dir.path())).unwrap();
        let notice = InterruptNotice {
            cause: InterruptCause::Logoff,
            at: "same-time".into(),
            meeting_folder: None,
            audio_status: None,
            message: InterruptCause::Logoff.message().into(),
        };
        assert!(notice.save(dir.path()).is_err());
        assert!(InterruptNotice::path(dir.path()).is_dir());
        assert_eq!(InterruptNotice::load(dir.path()), None);
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
    fn a_recording_not_confirmed_in_time_is_still_announced() {
        struct Slow(Mutex<Vec<String>>);
        impl PowerSession for Slow {
            fn recording_active(&self) -> bool {
                true
            }
            fn finish_recording(&self, cause: InterruptCause, at: &str) -> Option<InterruptNotice> {
                // Budget ran out: no status, but a cut recording all the same.
                Some(InterruptNotice {
                    cause,
                    at: at.to_string(),
                    meeting_folder: None,
                    audio_status: None,
                    message: cause.message().into(),
                })
            }
            fn save_database(&self, budget: std::time::Duration) {
                assert!(budget <= InterruptCause::Sleep.budget());
                self.0.lock().unwrap().push("database".into());
            }
            fn announce(&self, notice: &InterruptNotice) {
                assert_eq!(notice.audio_status, None);
                self.0.lock().unwrap().push("announce".into());
            }
        }
        let controller = PowerController::new(Slow(Mutex::new(Vec::new())));
        controller.handle(PowerEvent::Suspend);
        controller.handle(PowerEvent::Resume);
        assert_eq!(
            *controller.session.0.lock().unwrap(),
            ["database", "announce"]
        );
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
        assert_eq!(lines[1], "Tidspunkt: 5. nov. 2026 kl. 10.00");
        assert_eq!(format_when("not a time"), "not a time");
        assert_eq!(lines[2], strings::INTERRUPTED_AUDIO_OK);
        assert_eq!(lines[2], "Lyden er gemt.");
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
