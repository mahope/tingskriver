use super::*;
use crate::Bridge;
use serde_json::json;
use tingskriver_core::host::HostExt;

/// Core as far as the recorder sees it: the commands emit the events the
/// real ones emit, and the snapshot follows them. No sound devices.
#[derive(Default)]
struct FakeCore {
    snapshot: Mutex<RecordingSnapshot>,
    calls: Mutex<Vec<String>>,
    start_error: Mutex<Option<String>>,
    /// The final encode reports before `recording-stopped` (a short
    /// recording finishes encoding while the stop is still under way).
    finalize_first: bool,
    /// The final encode does not report by itself; the test emits it.
    hold_finalize: bool,
    audio_status: Option<&'static str>,
    transcribe_error: Option<String>,
    /// Pause does not answer until `release_pause` is notified.
    hold_pause: bool,
    release_pause: tokio::sync::Notify,
}

const FOLDER: &str = "/optagelser/Møde 12. nov. 2026 09.00";

impl FakeCore {
    fn new() -> FakeCore {
        FakeCore {
            audio_status: Some("ok"),
            ..Default::default()
        }
    }

    fn call(&self, name: &str) {
        self.calls.lock().unwrap().push(name.to_string());
    }

    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }

    fn set(&self, f: impl FnOnce(&mut RecordingSnapshot)) {
        f(&mut self.snapshot.lock().unwrap());
    }
}

impl RecorderCore for Arc<FakeCore> {
    fn start(&self, app: HostHandle, title: String) -> CoreFuture<()> {
        let this = self.clone();
        Box::pin(async move {
            this.call(&format!("start {title}"));
            app.emit("recording-starting", json!({}))?;
            if let Some(error) = this.start_error.lock().unwrap().clone() {
                return Err(error);
            }
            this.set(|s| {
                s.is_recording = true;
                s.microphone = Some("Headset (USB)".into());
                s.system = Some("Højttalere".into());
                s.microphone_level = 0.5;
                s.active_seconds = 0.0;
            });
            app.emit("recording-started", json!({}))
        })
    }

    fn pause(&self, app: HostHandle) -> CoreFuture<()> {
        let this = self.clone();
        Box::pin(async move {
            this.call("pause");
            if this.hold_pause {
                this.release_pause.notified().await;
            }
            this.set(|s| s.is_paused = true);
            app.emit("recording-paused", json!({}))
        })
    }

    fn resume(&self, app: HostHandle) -> CoreFuture<()> {
        let this = self.clone();
        Box::pin(async move {
            this.call("resume");
            this.set(|s| s.is_paused = false);
            app.emit("recording-resumed", json!({}))
        })
    }

    fn stop(&self, app: HostHandle) -> CoreFuture<()> {
        let this = self.clone();
        Box::pin(async move {
            this.call("stop");
            this.set(|s| s.is_stopping = true);
            app.emit("recording-shutdown-progress", json!({"stage": "saving"}))?;
            let finalized = json!({
                "meeting_folder": FOLDER,
                "status": this.audio_status.unwrap_or("fejlet"),
                "message": "Lyden er gemt.",
                "audio_file": format!("{FOLDER}/audio.mp4"),
            });
            if this.finalize_first {
                app.emit("recording-audio-finalized", &finalized)?;
            }
            this.set(|s| *s = RecordingSnapshot::default());
            app.emit(
                "recording-stopped",
                json!({
                    "reason": null,
                    "ended_by": null,
                    "folder_path": FOLDER,
                    "meeting_name": "Møde 12. nov. 2026 09.00",
                    "audio_status": this.audio_status,
                    "audio_message": "Der blev ikke optaget nogen lyd.",
                }),
            )?;
            if !this.finalize_first && !this.hold_finalize && this.audio_status.is_some() {
                app.emit("recording-audio-finalized", &finalized)?;
            }
            Ok(())
        })
    }

    fn snapshot(&self) -> RecordingSnapshot {
        self.snapshot.lock().unwrap().clone()
    }

    fn save_meeting(&self, _app: HostHandle, title: String, folder: String) -> CoreFuture<String> {
        let this = self.clone();
        Box::pin(async move {
            this.call(&format!("save {title} in {folder}"));
            Ok("meeting-1".to_string())
        })
    }

    fn transcribe(&self, _app: HostHandle, meeting_id: String, folder: String) -> CoreFuture<()> {
        let this = self.clone();
        Box::pin(async move {
            this.call(&format!("transcribe {meeting_id} from {folder}"));
            match &this.transcribe_error {
                Some(error) => Err(error.clone()),
                None => Ok(()),
            }
        })
    }
}

struct Setup {
    core: Arc<FakeCore>,
    bridge: Arc<Bridge>,
    recorder: Arc<Recorder>,
    states: watch::Receiver<RecorderState>,
    notices: mpsc::UnboundedReceiver<Notice>,
    _task: JoinHandle<()>,
}

fn setup(core: FakeCore) -> Setup {
    let dir = std::env::temp_dir().join(format!(
        "tingskriver-host-recorder-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let bridge = Arc::new(Bridge::new(dir));
    let core = Arc::new(core);
    let (recorder, task) = Recorder::start(
        bridge.clone(),
        Handle::current(),
        Arc::new(core.clone()),
        bridge.subscribe(),
    );
    Setup {
        core,
        bridge,
        states: recorder.subscribe(),
        notices: recorder.take_notices().unwrap(),
        recorder,
        _task: task,
    }
}

async fn wait_for(
    states: &mut watch::Receiver<RecorderState>,
    what: &str,
    f: impl FnMut(&RecorderState) -> bool,
) -> RecorderState {
    let found = match tokio::time::timeout(Duration::from_secs(5), states.wait_for(f)).await {
        Ok(Ok(state)) => Some(state.clone()),
        _ => None,
    };
    found.unwrap_or_else(|| panic!("timed out waiting for {what}: {:?}", states.borrow()))
}

async fn next_notice(notices: &mut mpsc::UnboundedReceiver<Notice>) -> Notice {
    tokio::time::timeout(Duration::from_secs(5), notices.recv())
        .await
        .expect("a notice")
        .expect("the channel is open")
}

async fn notices_until_open(notices: &mut mpsc::UnboundedReceiver<Notice>) -> Vec<Notice> {
    let mut seen = Vec::new();
    loop {
        let notice = next_notice(notices).await;
        let done = matches!(notice, Notice::OpenMeeting(_));
        seen.push(notice);
        if done {
            return seen;
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn start_pause_resume_stop_saves_and_transcribes_the_meeting() {
    let mut s = setup(FakeCore::new());
    assert_eq!(s.recorder.state().phase, Phase::Idle);

    s.recorder.start_recording();
    let state = wait_for(&mut s.states, "recording", |st| {
        st.phase == Phase::Recording && st.microphone.is_some()
    })
    .await;
    assert_eq!(state.microphone.as_deref(), Some("Headset (USB)"));
    assert_eq!(state.system.as_deref(), Some("Højttalere"));
    assert!(s.core.calls()[0].starts_with("start Møde "));

    // Time and levels come from core's snapshot.
    s.core.set(|snap| snap.active_seconds = 75.4);
    let state = wait_for(&mut s.states, "time", |st| st.seconds == 75).await;
    assert!(state.levels[0] > 0.5, "{:?}", state.levels);
    assert_eq!(state.levels[2], 0.0);

    s.recorder.toggle_pause();
    let state = wait_for(&mut s.states, "paused", |st| st.phase == Phase::Paused).await;
    assert_eq!(state.levels, [0.0; 3]);
    s.recorder.toggle_pause();
    wait_for(&mut s.states, "resumed", |st| st.phase == Phase::Recording).await;

    s.recorder.stop_recording();
    let notices = notices_until_open(&mut s.notices).await;
    assert_eq!(
        notices,
        vec![
            Notice::MeetingSaved("meeting-1".into()),
            Notice::Success(strings::RECORDING_SAVED.into()),
            Notice::OpenMeeting("meeting-1".into()),
        ]
    );
    wait_for(&mut s.states, "idle", |st| st.phase == Phase::Idle).await;
    assert_eq!(
        s.core.calls()[1..],
        [
            "pause".to_string(),
            "resume".to_string(),
            "stop".to_string(),
            format!("save Møde 12. nov. 2026 09.00 in {FOLDER}"),
            format!("transcribe meeting-1 from {FOLDER}"),
        ]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stop_shows_saving_then_finishing() {
    let mut s = setup(FakeCore {
        hold_finalize: true,
        ..FakeCore::new()
    });
    s.recorder.start_recording();
    wait_for(&mut s.states, "recording", |st| {
        st.phase == Phase::Recording
    })
    .await;
    s.recorder.stop_recording();
    // Stop takes effect at once ("Gemmer optagelsen…"); the fake may already
    // be further along.
    assert!(matches!(
        s.recorder.state().phase,
        Phase::Stopping | Phase::SavingMeeting | Phase::Finishing
    ));
    // Saved as a meeting, then waiting for the final audio.
    assert_eq!(
        next_notice(&mut s.notices).await,
        Notice::MeetingSaved("meeting-1".into())
    );
    wait_for(&mut s.states, "finishing", |st| {
        st.phase == Phase::Finishing
    })
    .await;
    assert!(!s.core.calls().iter().any(|c| c.starts_with("transcribe")));
    s.bridge
        .emit(
            "recording-audio-finalized",
            json!({"meeting_folder": FOLDER, "status": "ok", "message": "Lyden er gemt.",
                   "audio_file": format!("{FOLDER}/audio.mp4")}),
        )
        .unwrap();
    wait_for(&mut s.states, "idle", |st| st.phase == Phase::Idle).await;
    assert!(s
        .core
        .calls()
        .contains(&format!("transcribe meeting-1 from {FOLDER}")));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn final_audio_before_the_stop_event_is_not_missed() {
    let mut s = setup(FakeCore {
        finalize_first: true,
        ..FakeCore::new()
    });
    s.recorder.start_recording();
    wait_for(&mut s.states, "recording", |st| {
        st.phase == Phase::Recording
    })
    .await;
    s.recorder.stop_recording();
    let notices = notices_until_open(&mut s.notices).await;
    assert_eq!(
        notices.last(),
        Some(&Notice::OpenMeeting("meeting-1".into()))
    );
    assert!(s
        .core
        .calls()
        .contains(&format!("transcribe meeting-1 from {FOLDER}")));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn failed_transcription_still_keeps_and_opens_the_meeting() {
    let mut s = setup(FakeCore {
        transcribe_error: Some("Retranscription requires Whisper".into()),
        ..FakeCore::new()
    });
    s.recorder.start_recording();
    wait_for(&mut s.states, "recording", |st| {
        st.phase == Phase::Recording
    })
    .await;
    s.recorder.stop_recording();
    let notices = notices_until_open(&mut s.notices).await;
    assert!(notices.contains(&Notice::Error {
        title: strings::RECORDING_TRANSCRIBE_FAILED.into(),
        text: Some("Retranscription requires Whisper".into()),
        sticky: false,
    }));
    assert_eq!(
        notices.last(),
        Some(&Notice::OpenMeeting("meeting-1".into()))
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_recording_without_audio_saves_no_meeting() {
    let mut s = setup(FakeCore {
        audio_status: Some("fejlet"),
        ..FakeCore::new()
    });
    s.recorder.start_recording();
    wait_for(&mut s.states, "recording", |st| {
        st.phase == Phase::Recording
    })
    .await;
    s.recorder.stop_recording();
    assert_eq!(
        next_notice(&mut s.notices).await,
        Notice::Error {
            title: strings::RECORDING_SAVE_FAILED.into(),
            text: Some("Der blev ikke optaget nogen lyd.".into()),
            sticky: false,
        }
    );
    wait_for(&mut s.states, "idle", |st| st.phase == Phase::Idle).await;
    assert!(!s.core.calls().iter().any(|c| c.starts_with("save")));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn start_errors_use_the_tauri_categories() {
    for (message, expected) in [
        (
            "Failed to start recording: Microphone access required: denied",
            StartError::Microphone,
        ),
        (
            "Failed to start recording: no system audio device",
            StartError::System,
        ),
        (
            "Failed to start recording: permission",
            StartError::Permission,
        ),
        ("Failed to start recording: disk full", StartError::Other),
    ] {
        let mut s = setup(FakeCore::new());
        *s.core.start_error.lock().unwrap() = Some(message.into());
        s.recorder.start_recording();
        let state = wait_for(&mut s.states, "error", |st| st.start_error.is_some()).await;
        assert_eq!(state.start_error, Some(expected), "{message}");
        assert_eq!(state.phase, Phase::Idle);
        s.recorder.dismiss_start_error();
        assert_eq!(s.recorder.state().start_error, None);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_running_recording_is_found_at_startup() {
    let core = FakeCore::new();
    core.set(|snap| {
        snap.is_recording = true;
        snap.is_paused = true;
        snap.active_seconds = 754.0;
        snap.microphone = Some("Mikrofon (Realtek)".into());
    });
    let s = setup(core);
    let state = s.recorder.state();
    assert_eq!(state.phase, Phase::Paused);
    assert_eq!(state.seconds, 754);
    assert_eq!(state.microphone.as_deref(), Some("Mikrofon (Realtek)"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_recording_started_elsewhere_is_followed() {
    // The tray or MCP starts core directly; only the events and the snapshot
    // tell the recorder.
    let mut s = setup(FakeCore::new());
    s.core.set(|snap| {
        snap.is_recording = true;
        snap.active_seconds = 3.0;
    });
    s.bridge.emit("recording-started", json!({})).unwrap();
    wait_for(&mut s.states, "recording", |st| {
        st.phase == Phase::Recording && st.seconds == 3
    })
    .await;
    // The tray stops it as well.
    s.core.set(|snap| *snap = RecordingSnapshot::default());
    s.bridge
        .emit("recording-stopped", json!({"folder_path": null}))
        .unwrap();
    wait_for(&mut s.states, "idle", |st| st.phase == Phase::Idle).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn storage_errors_show_while_recording() {
    let mut s = setup(FakeCore::new());
    s.recorder.start_recording();
    wait_for(&mut s.states, "recording", |st| {
        st.phase == Phase::Recording
    })
    .await;
    let full = "Lyden gemmes ikke: disken er fuld";
    s.core.set(|snap| snap.storage_error = Some(full.into()));
    s.bridge
        .emit(
            "recording-storage-error",
            json!({"message": full, "kind": "disk_full"}),
        )
        .unwrap();
    assert_eq!(
        next_notice(&mut s.notices).await,
        Notice::Error {
            title: full.into(),
            text: None,
            sticky: true
        }
    );
    let state = wait_for(&mut s.states, "storage error", |st| {
        st.storage_error.is_some()
    })
    .await;
    assert_eq!(state.storage_error.as_deref(), Some(full));
}

#[test]
fn microphone_events_become_notices() {
    let mut state = RecorderState {
        phase: Phase::Recording,
        ..Default::default()
    };
    apply_event(
        &mut state,
        "mic-device-switched",
        &json!({"device_name": "Headset (USB)"}),
    );
    assert_eq!(
        state.notice,
        Some(DeviceNotice::Switched("Headset (USB)".into()))
    );
    assert_eq!(state.microphone.as_deref(), Some("Headset (USB)"));
    apply_event(
        &mut state,
        "mic-recovery-exhausted",
        &json!({"device_name": "Headset (USB)"}),
    );
    assert_eq!(
        state.notice,
        Some(DeviceNotice::Lost("Headset (USB)".into()))
    );
    // A late swap failure after stop does not alarm (Tauri gates it the same way).
    let mut idle = RecorderState::default();
    apply_event(
        &mut idle,
        "mic-swap-failed",
        &json!({"device_name": "X", "error": "manager gone"}),
    );
    assert_eq!(idle.notice, None);
    assert_eq!(
        apply_event(&mut idle, "mic-unavailable", &json!({})),
        vec![Effect::Notice(Notice::error(
            strings::RECORDING_MIC_UNAVAILABLE,
            None
        ))]
    );
}

#[test]
fn an_audio_error_stop_is_reported() {
    let mut state = RecorderState {
        phase: Phase::Stopping,
        ..Default::default()
    };
    let effects = apply_event(
        &mut state,
        "recording-stopped",
        &json!({"reason": "Audio stream encountered an error", "ended_by": null,
                "folder_path": FOLDER, "meeting_name": "Møde", "audio_status": "delvis"}),
    );
    assert_eq!(
        effects,
        vec![
            Effect::Notice(Notice::error(strings::RECORDING_STOPPED_BY_ERROR, None)),
            Effect::Finish {
                folder: FOLDER.into(),
                title: "Møde".into()
            }
        ]
    );
    // A stop for sleep or logoff has its own card instead.
    let effects = apply_event(
        &mut state,
        "recording-stopped",
        &json!({"reason": "Computeren gik i dvale", "ended_by": "sleep",
                "folder_path": null}),
    );
    assert!(effects.is_empty());
    assert_eq!(state.phase, Phase::Idle);
}

#[test]
fn levels_use_a_decibel_scale_and_fall_slowly() {
    assert_eq!(bar(0.0), 0.0);
    assert_eq!(bar(1.0), 1.0);
    assert!((bar(0.1) - 0.6).abs() < 1e-6);
    assert_eq!(bar(0.001), 0.0);
    let mut state = RecorderState::default();
    let mut snapshot = RecordingSnapshot {
        is_recording: true,
        microphone_level: 1.0,
        ..Default::default()
    };
    apply_snapshot(&mut state, &snapshot);
    assert_eq!(state.levels, [1.0, 1.0, 0.0]);
    snapshot.microphone_level = 0.0;
    apply_snapshot(&mut state, &snapshot);
    assert!((state.levels[0] - 0.6).abs() < 1e-6);
    snapshot.is_paused = true;
    apply_snapshot(&mut state, &snapshot);
    assert_eq!(state.phase, Phase::Paused);
    assert_eq!(state.levels, [0.0; 3]);
}

#[test]
fn a_missed_stop_is_noticed() {
    let mut state = RecorderState {
        phase: Phase::Recording,
        ..Default::default()
    };
    assert!(apply_snapshot(&mut state, &RecordingSnapshot::default()));
    assert!(!apply_snapshot(
        &mut RecorderState::default(),
        &RecordingSnapshot::default()
    ));
}

fn finalized(status: &str) -> Value {
    json!({"meeting_folder": FOLDER, "status": status, "message": format!("Lyden: {status}"),
           "audio_file": format!("{FOLDER}/audio.mp4")})
}

/// Start with the button, stop, and wait until the meeting is saved and its
/// final audio is awaited ("Gør transskriptionen færdig…").
async fn record_until_finishing(s: &mut Setup) {
    s.recorder.start_recording();
    wait_for(&mut s.states, "recording", |st| {
        st.phase == Phase::Recording
    })
    .await;
    s.recorder.stop_recording();
    assert_eq!(
        next_notice(&mut s.notices).await,
        Notice::MeetingSaved("meeting-1".into())
    );
    wait_for(&mut s.states, "finishing", |st| {
        st.phase == Phase::Finishing
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_recording_started_elsewhere_while_transcribing_takes_over() {
    let mut s = setup(FakeCore {
        hold_finalize: true,
        ..FakeCore::new()
    });
    record_until_finishing(&mut s).await;
    // MCP starts core directly while the last meeting is transcribed.
    s.core.set(|snap| {
        snap.is_recording = true;
        snap.active_seconds = 2.0;
    });
    s.bridge.emit("recording-started", json!({})).unwrap();
    wait_for(&mut s.states, "recording", |st| {
        st.phase == Phase::Recording
    })
    .await;
    // The last meeting finishes in the background and does not take the
    // screen from the new recording.
    s.bridge
        .emit("recording-audio-finalized", finalized("ok"))
        .unwrap();
    assert_eq!(
        next_notice(&mut s.notices).await,
        Notice::Success(strings::RECORDING_SAVED.into())
    );
    assert_eq!(
        next_notice(&mut s.notices).await,
        Notice::MeetingSaved("meeting-1".into())
    );
    assert!(s
        .core
        .calls()
        .contains(&format!("transcribe meeting-1 from {FOLDER}")));
    assert_eq!(s.recorder.state().phase, Phase::Recording);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_snapshot_shows_a_recording_started_while_transcribing() {
    let mut s = setup(FakeCore {
        hold_finalize: true,
        ..FakeCore::new()
    });
    record_until_finishing(&mut s).await;
    // The event was missed; the snapshot finds the recording.
    s.core.set(|snap| {
        snap.is_recording = true;
        snap.is_paused = true;
    });
    wait_for(&mut s.states, "paused", |st| st.phase == Phase::Paused).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn new_recording_starts_while_the_last_one_is_transcribed() {
    let mut s = setup(FakeCore {
        hold_finalize: true,
        ..FakeCore::new()
    });
    record_until_finishing(&mut s).await;
    s.recorder.start_recording();
    wait_for(&mut s.states, "recording", |st| {
        st.phase == Phase::Recording
    })
    .await;
    let starts = s
        .core
        .calls()
        .iter()
        .filter(|c| c.starts_with("start "))
        .count();
    assert_eq!(starts, 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_start_while_saving_says_why_it_waits() {
    let mut s = setup(FakeCore::new());
    for phase in [Phase::Stopping, Phase::SavingMeeting] {
        s.recorder.state.send_modify(|st| st.phase = phase);
        s.recorder.start_recording();
        assert_eq!(
            next_notice(&mut s.notices).await,
            Notice::Warning {
                text: strings::RECORDING_START_WHILE_SAVING.into(),
                sticky: false,
            }
        );
        assert_eq!(s.recorder.state().phase, phase);
    }
    assert!(s.core.calls().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_double_click_on_pause_pauses_once() {
    let mut s = setup(FakeCore {
        hold_pause: true,
        ..FakeCore::new()
    });
    s.recorder.start_recording();
    wait_for(&mut s.states, "recording", |st| {
        st.phase == Phase::Recording
    })
    .await;
    s.recorder.toggle_pause();
    s.recorder.toggle_pause();
    s.core.release_pause.notify_one();
    wait_for(&mut s.states, "paused", |st| st.phase == Phase::Paused).await;
    let pauses = s.core.calls().iter().filter(|c| *c == "pause").count();
    assert_eq!(pauses, 1);
    // Answered: the next click resumes.
    s.recorder.toggle_pause();
    wait_for(&mut s.states, "resumed", |st| st.phase == Phase::Recording).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_lost_start_race_with_no_recording_left_returns_to_idle() {
    let mut s = setup(FakeCore::new());
    *s.core.start_error.lock().unwrap() = Some("Recording already in progress".into());
    s.recorder.start_recording();
    wait_for(&mut s.states, "idle", |st| st.phase == Phase::Idle).await;
    assert_eq!(s.recorder.state().start_error, None);
    // The next start is not ignored.
    *s.core.start_error.lock().unwrap() = None;
    s.recorder.start_recording();
    wait_for(&mut s.states, "recording", |st| {
        st.phase == Phase::Recording
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn failed_final_audio_is_reported_once() {
    let mut s = setup(FakeCore {
        hold_finalize: true,
        ..FakeCore::new()
    });
    record_until_finishing(&mut s).await;
    s.bridge
        .emit("recording-audio-finalized", finalized("fejlet"))
        .unwrap();
    let notices = notices_until_open(&mut s.notices).await;
    let errors: Vec<_> = notices
        .iter()
        .filter(|n| matches!(n, Notice::Error { .. }))
        .collect();
    assert_eq!(
        errors,
        vec![&Notice::error("Lyden: fejlet", None)],
        "{notices:?}"
    );
    assert!(!s.core.calls().iter().any(|c| c.starts_with("transcribe")));
}

#[test]
fn finished_encodes_drop_the_oldest_when_full() {
    let finalized = Finalized::default();
    let audio = FinalAudio {
        usable: true,
        message: String::new(),
        reported: false,
    };
    for n in 0..=FINALIZED_KEPT {
        finalized.insert(format!("møde {n}"), audio.clone());
    }
    let done = finalized.done.lock().unwrap();
    assert_eq!(done.len(), FINALIZED_KEPT);
    assert!(!done.contains_key("møde 0"));
    assert!(done.contains_key("møde 1"));
    assert!(done.contains_key(&format!("møde {FINALIZED_KEPT}")));
}
