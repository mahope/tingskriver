//! Background tasks that keep the tray and the shortcuts up to date.
//!
//! - [`watch_status`] reads the recording and dictation state every
//!   [`STATUS_POLL`] and at once on `refresh_status_ui`, and hands the tray a
//!   new [`TrayStatus`] only when something changed.
//! - [`watch_shortcuts`] re-reads the shortcut settings every
//!   [`SHORTCUT_POLL`] and re-registers them when they changed, so an edited
//!   shortcut works without a restart.

use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tingskriver_core::shortcuts::{Conflict, ShortcutSettings};
use tokio::sync::Notify;

use crate::status::{Dictation, Recording, TrayStatus};

/// How often the recording state is read. Together with the tray thread's
/// message this keeps the icon change well inside 200 ms.
pub const STATUS_POLL: Duration = Duration::from_millis(100);
/// How often the shortcut settings file is read.
pub const SHORTCUT_POLL: Duration = Duration::from_secs(1);

/// Where the status comes from (core in the app, a fake in tests).
pub trait Probe: Send + Sync + 'static {
    fn recording(&self) -> impl Future<Output = Recording> + Send;
    fn dictation(&self) -> Dictation;
}

/// State shared by the watchers.
#[derive(Default)]
pub struct Shared {
    /// Wakes [`watch_status`] at once (`refresh_status_ui`).
    pub wake: Notify,
    /// Problems from the last shortcut registration.
    pub shortcut_problems: Mutex<Vec<String>>,
    pub elevated: bool,
}

impl Shared {
    pub fn new(elevated: bool) -> Shared {
        Shared {
            elevated,
            ..Shared::default()
        }
    }
}

/// Current status from `probe` and `shared`.
pub async fn read_status<P: Probe>(probe: &P, shared: &Shared) -> TrayStatus {
    TrayStatus {
        recording: probe.recording().await,
        dictation: probe.dictation(),
        shortcut_problems: shared.shortcut_problems.lock().map(|p| p.clone()).unwrap_or_default(),
        elevated: shared.elevated,
    }
}

/// Runs until the task is aborted. `show` gets every changed status.
pub async fn watch_status<P: Probe>(probe: P, shared: Arc<Shared>, show: impl Fn(TrayStatus) + Send) {
    let mut last: Option<TrayStatus> = None;
    loop {
        let status = read_status(&probe, &shared).await;
        if last.as_ref() != Some(&status) {
            show(status.clone());
            last = Some(status);
        }
        tokio::select! {
            _ = shared.wake.notified() => {}
            _ = tokio::time::sleep(STATUS_POLL) => {}
        }
    }
}

/// What the shortcut watcher registers: the dictation shortcut (for the
/// conflict check) and the registered shortcuts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Wanted {
    pub dictation: Option<String>,
    pub settings: ShortcutSettings,
}

/// Runs until the task is aborted. `read` returns the wanted shortcuts or
/// `Err` when the settings can't be read (the last good set stays active);
/// `apply` registers them and returns the problems.
pub async fn watch_shortcuts<R, A, F>(shared: Arc<Shared>, read: R, apply: A)
where
    R: Fn() -> Result<Wanted, String> + Send,
    A: Fn(Wanted) -> F + Send,
    F: Future<Output = Vec<Conflict>> + Send,
{
    let mut last: Option<Wanted> = None;
    let mut last_error: Option<String> = None;
    loop {
        match read() {
            Ok(wanted) => {
                last_error = None;
                if last.as_ref() != Some(&wanted) {
                    let problems = apply(wanted.clone()).await;
                    for problem in &problems {
                        log::warn!("Shortcut problem: {}", problem.message());
                    }
                    if let Ok(mut slot) = shared.shortcut_problems.lock() {
                        *slot = problems.iter().filter(|p| p.blocks()).map(Conflict::message).collect();
                    }
                    shared.wake.notify_one();
                    last = Some(wanted);
                }
            }
            Err(e) => {
                if last_error.as_deref() != Some(e.as_str()) {
                    log::warn!("Shortcut settings could not be read; keeping the current ones: {e}");
                    last_error = Some(e);
                }
            }
        }
        tokio::time::sleep(SHORTCUT_POLL).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::time::Instant;
    use tingskriver_core::shortcuts::ShortcutAction;

    #[derive(Clone, Default)]
    struct FakeProbe {
        recording: Arc<AtomicBool>,
    }

    impl Probe for FakeProbe {
        async fn recording(&self) -> Recording {
            if self.recording.load(Ordering::SeqCst) {
                Recording::Recording
            } else {
                Recording::Idle
            }
        }

        fn dictation(&self) -> Dictation {
            Dictation::Ready("Ctrl+Win".into())
        }
    }

    /// Start and stop of a recording reach the tray within 200 ms, also
    /// without `refresh_status_ui` (poll only).
    #[tokio::test]
    async fn recording_changes_reach_the_tray_within_200_ms() {
        let probe = FakeProbe::default();
        let shared = Arc::new(Shared::default());
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let task = tokio::spawn(watch_status(probe.clone(), shared.clone(), move |s| {
            let _ = tx.send((s, Instant::now()));
        }));
        let (first, _) = rx.recv().await.unwrap();
        assert_eq!(first.recording, Recording::Idle);

        for (flag, expected) in [(true, Recording::Recording), (false, Recording::Idle)] {
            tokio::time::sleep(Duration::from_millis(37)).await;
            let changed = Instant::now();
            probe.recording.store(flag, Ordering::SeqCst);
            let (status, shown) = tokio::time::timeout(Duration::from_secs(2), rx.recv()).await.unwrap().unwrap();
            assert_eq!(status.recording, expected);
            let delay = shown.duration_since(changed);
            assert!(delay < Duration::from_millis(200), "tray updated after {delay:?}");
        }

        // With refresh_status_ui (wake) it is immediate.
        tokio::time::sleep(Duration::from_millis(10)).await;
        let changed = Instant::now();
        probe.recording.store(true, Ordering::SeqCst);
        shared.wake.notify_one();
        let (_, shown) = rx.recv().await.unwrap();
        assert!(shown.duration_since(changed) < Duration::from_millis(50));

        // Unchanged state sends nothing.
        assert!(tokio::time::timeout(Duration::from_millis(300), rx.recv()).await.is_err());
        task.abort();
    }

    #[tokio::test]
    async fn changed_shortcuts_are_applied_once_and_problems_kept() {
        let shared = Arc::new(Shared::default());
        let wanted = Arc::new(Mutex::new(Ok(Wanted {
            dictation: Some("Ctrl+Win".into()),
            settings: ShortcutSettings {
                new_recording: Some("Ctrl+Alt+N".into()),
                ..ShortcutSettings::default()
            },
        })));
        let applied = Arc::new(Mutex::new(Vec::<Wanted>::new()));
        let calls = Arc::new(AtomicUsize::new(0));
        let (w, a, c) = (wanted.clone(), applied.clone(), calls.clone());
        let task = tokio::spawn(watch_shortcuts(
            shared.clone(),
            move || w.lock().unwrap().clone(),
            move |wanted: Wanted| {
                let a = a.clone();
                let c = c.clone();
                async move {
                    c.fetch_add(1, Ordering::SeqCst);
                    a.lock().unwrap().push(wanted.clone());
                    if wanted.settings.new_recording.as_deref() == Some("Ctrl+Alt+X") {
                        vec![Conflict {
                            action: ShortcutAction::NewRecording,
                            shortcut: "Ctrl+Alt+X".into(),
                            kind: tingskriver_core::shortcuts::ConflictKind::TakenByOtherProgram,
                        }]
                    } else {
                        Vec::new()
                    }
                }
            },
        ));
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        // An unreadable file keeps the old set and applies nothing.
        *wanted.lock().unwrap() = Err("broken".into());
        tokio::time::sleep(SHORTCUT_POLL + Duration::from_millis(100)).await;
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        // A changed shortcut is applied on the next poll.
        *wanted.lock().unwrap() = Ok(Wanted {
            dictation: Some("Ctrl+Win".into()),
            settings: ShortcutSettings {
                new_recording: Some("Ctrl+Alt+X".into()),
                ..ShortcutSettings::default()
            },
        });
        tokio::time::sleep(SHORTCUT_POLL + Duration::from_millis(100)).await;
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(
            applied.lock().unwrap()[1].settings.new_recording.as_deref(),
            Some("Ctrl+Alt+X")
        );
        assert_eq!(
            *shared.shortcut_problems.lock().unwrap(),
            vec!["Et andet program bruger allerede Ctrl+Alt+X".to_string()]
        );
        task.abort();
    }
}
