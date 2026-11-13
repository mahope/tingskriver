//! Meeting recording for the GPUI app: start, pause, resume and stop through
//! core, the state a UI draws, and what happens after stop.
//!
//! The state comes from core, never from what this side asked for: a
//! snapshot when the recorder starts (`recording_snapshot`), core's events,
//! and a snapshot five times a second while a recording runs (time, levels,
//! devices). A recording started from the tray, a shortcut, MCP or a later
//! out-of-process recorder (#292) therefore shows the same way as one started
//! with the button, and an app that opens while a recording runs finds it.
//!
//! After `recording-stopped` the meeting is saved, the final audio awaited
//! (`recording-audio-finalized`) and the saved audio transcribed, as the Tauri
//! app does in `useRecordingStop.ts`. That runs on the core runtime, so it
//! finishes with the window closed. One-off messages for the user (errors,
//! "Optagelsen er gemt", the meeting to open) go out as [`Notice`]s.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::Value;
use tingskriver_core::audio::recording_commands::RecordingSnapshot;
use tingskriver_core::host::HostHandle;
use tingskriver_core::strings;
use tokio::runtime::Handle;
use tokio::sync::{mpsc, watch, Notify};
use tokio::task::JoinHandle;

use crate::Event;

/// A call into core.
pub type CoreFuture<T> = Pin<Box<dyn Future<Output = Result<T, String>> + Send>>;

/// How often the time and the levels are read while a recording runs.
const TICK: Duration = Duration::from_millis(200);
/// While idle, the snapshot is read every this many ticks (1 s), so a
/// recording started elsewhere shows up even if its events were missed.
const IDLE_TICKS: u32 = 5;
/// Snapshots that say "not recording" while the state still shows a
/// recording, before the state gives up waiting for `recording-stopped`.
const STALE_TICKS: u32 = 10;
/// Finished encodes kept for a stop that has not asked for its folder yet.
const FINALIZED_KEPT: usize = 20;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Phase {
    #[default]
    Idle,
    /// "Start optagelse" was pressed, or core said `recording-starting`.
    Starting,
    Recording,
    Paused,
    /// The audio is being written to disk ("Gemmer optagelsen…").
    Stopping,
    /// The meeting is being saved ("Gemmer mødet…").
    SavingMeeting,
    /// The final audio is encoded and transcribed ("Gør transskriptionen
    /// færdig…").
    Finishing,
}

impl Phase {
    /// A recording runs or is paused.
    pub fn is_recording(self) -> bool {
        matches!(self, Phase::Recording | Phase::Paused)
    }

    /// Core may start a new recording in this phase, and the state then
    /// follows it: nothing records, or only the last meeting is still being
    /// saved or transcribed (that carries on in the background).
    fn yields_to_core(self) -> bool {
        matches!(
            self,
            Phase::Idle | Phase::Starting | Phase::SavingMeeting | Phase::Finishing
        )
    }
}

/// A microphone change during the recording (`RecordingStateContext.tsx`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeviceNotice {
    Switched(String),
    SwitchFailed(String),
    Lost(String),
}

/// Why a recording could not start (`RecordingControls.tsx`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartError {
    Microphone,
    System,
    Permission,
    Other,
}

impl StartError {
    /// The same reading of core's error text as Tauri's `handleStartRecording`,
    /// but ignoring case: core says "Microphone access required", which
    /// Tauri's case-sensitive check reads as a general start error.
    pub fn from_message(message: &str) -> StartError {
        let message = message.to_lowercase();
        if message.contains("microphone") || message.contains("mic") || message.contains("input") {
            StartError::Microphone
        } else if message.contains("system audio")
            || message.contains("speaker")
            || message.contains("output")
        {
            StartError::System
        } else if message.contains("permission") {
            StartError::Permission
        } else {
            StartError::Other
        }
    }
}

/// What the recording screen shows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RecorderState {
    pub phase: Phase,
    /// Recorded time without pauses.
    pub seconds: u64,
    /// Microphone, both, system audio; 0 to 1 on a decibel scale.
    pub levels: [f32; 3],
    /// The devices core records from (`None` before the first snapshot).
    pub microphone: Option<String>,
    pub system: Option<String>,
    pub notice: Option<DeviceNotice>,
    /// The audio is not being saved ("Lyden gemmes ikke: disken er fuld").
    pub storage_error: Option<String>,
    pub start_error: Option<StartError>,
}

/// A one-off message for the user or the UI.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Notice {
    Error {
        title: String,
        text: Option<String>,
        /// Stays until closed (Tauri `duration: Infinity`).
        sticky: bool,
    },
    Warning {
        text: String,
        sticky: bool,
    },
    Success(String),
    /// A meeting was saved: the meeting list should show it.
    MeetingSaved(String),
    /// The recording is saved and transcribed: open this meeting.
    OpenMeeting(String),
}

impl Notice {
    fn error(title: &str, text: Option<String>) -> Notice {
        Notice::Error {
            title: title.to_string(),
            text,
            sticky: false,
        }
    }
}

/// What a recording needs from core. [`CoreRecorder`] calls core; tests use
/// a fake, so they need no sound devices.
pub trait RecorderCore: Send + Sync + 'static {
    fn start(&self, app: HostHandle, title: String) -> CoreFuture<()>;
    fn pause(&self, app: HostHandle) -> CoreFuture<()>;
    fn resume(&self, app: HostHandle) -> CoreFuture<()>;
    fn stop(&self, app: HostHandle) -> CoreFuture<()>;
    fn snapshot(&self) -> RecordingSnapshot;
    /// Save a stopped recording as a meeting and return its id.
    fn save_meeting(&self, app: HostHandle, title: String, folder: String) -> CoreFuture<String>;
    /// Transcribe a meeting's saved audio; returns when it is done.
    fn transcribe(&self, app: HostHandle, meeting_id: String, folder: String) -> CoreFuture<()>;
}

/// What an event asks of the recorder besides the new state.
#[derive(Clone, Debug, PartialEq)]
enum Effect {
    Notice(Notice),
    /// Save and transcribe this stopped recording.
    Finish {
        folder: String,
        title: String,
    },
    /// The final audio of this folder is encoded (or could not be).
    Finalized {
        folder: String,
        audio: FinalAudio,
    },
}

fn text(payload: &Value, key: &str) -> Option<String> {
    payload
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Apply one of core's events to the state.
fn apply_event(state: &mut RecorderState, name: &str, payload: &Value) -> Vec<Effect> {
    let mut effects = Vec::new();
    let message = text(payload, "message");
    match name {
        // Events can arrive after the direct result of a call (the start
        // failed, Stop was pressed), so they only move the phase forward.
        // A recording started from the tray or MCP while the last meeting is
        // transcribed takes over the screen.
        "recording-starting" if state.phase.yields_to_core() => {
            // mic-device-switched for this start comes before recording-started.
            state.notice = None;
            state.storage_error = None;
        }
        "recording-started" if state.phase.yields_to_core() => {
            state.phase = Phase::Recording;
            state.seconds = 0;
            state.levels = [0.0; 3];
            state.start_error = None;
        }
        "recording-paused" if state.phase.is_recording() => {
            state.phase = Phase::Paused;
            state.levels = [0.0; 3];
        }
        "recording-resumed" if state.phase.is_recording() => state.phase = Phase::Recording,
        "recording-shutdown-progress"
            if state.phase.is_recording() || state.phase == Phase::Starting =>
        {
            state.phase = Phase::Stopping;
            state.levels = [0.0; 3];
        }
        "recording-stopped" => {
            state.levels = [0.0; 3];
            state.notice = None;
            state.storage_error = None;
            // An audio error stopped it (a sleep or logoff stop has its own
            // card, `recording-interrupted`).
            if text(payload, "reason").is_some() && text(payload, "ended_by").is_none() {
                effects.push(Effect::Notice(Notice::error(
                    strings::RECORDING_STOPPED_BY_ERROR,
                    None,
                )));
            }
            let status = text(payload, "audio_status");
            let saved = matches!(status.as_deref(), Some("ok" | "delvis"));
            match (text(payload, "folder_path"), text(payload, "meeting_name")) {
                (Some(folder), Some(title)) if saved => {
                    state.phase = Phase::SavingMeeting;
                    effects.push(Effect::Finish { folder, title });
                }
                _ => {
                    state.phase = Phase::Idle;
                    // Nothing usable on disk: Tauri saves no meeting then.
                    if let (Some(_), Some(text)) = (status, text(payload, "audio_message")) {
                        effects.push(Effect::Notice(Notice::error(
                            strings::RECORDING_SAVE_FAILED,
                            Some(text),
                        )));
                    }
                }
            }
        }
        "mic-device-switched" => {
            if let Some(device) = text(payload, "device_name") {
                state.microphone = Some(device.clone());
                state.notice = Some(DeviceNotice::Switched(device));
            }
        }
        "mic-swap-failed" if state.phase.is_recording() => {
            if let Some(device) = text(payload, "device_name") {
                state.notice = Some(DeviceNotice::SwitchFailed(device));
            }
        }
        "mic-recovery-exhausted" if state.phase.is_recording() => {
            if let Some(device) = text(payload, "device_name") {
                state.notice = Some(DeviceNotice::Lost(device));
            }
        }
        "mic-unavailable" => effects.push(Effect::Notice(Notice::error(
            strings::RECORDING_MIC_UNAVAILABLE,
            None,
        ))),
        "recording-storage-error" => {
            if let Some(message) = message {
                if state.phase.is_recording() {
                    state.storage_error = Some(message.clone());
                }
                effects.push(Effect::Notice(Notice::Error {
                    title: message,
                    text: None,
                    sticky: true,
                }));
            }
        }
        "recording-storage-recovered" => {
            state.storage_error = None;
            if let Some(message) = message {
                effects.push(Effect::Notice(Notice::Success(message)));
            }
        }
        "recording-storage-backlog" | "recording-disk-low" => {
            if let Some(message) = message {
                effects.push(Effect::Notice(Notice::Warning {
                    text: message,
                    sticky: name == "recording-disk-low",
                }));
            }
        }
        "recording-disk-ok" if state.phase.is_recording() => {
            if let Some(message) = message {
                effects.push(Effect::Notice(Notice::Success(message)));
            }
        }
        "recording-audio-finalized" => {
            if let Some(folder) = text(payload, "meeting_folder") {
                let status = text(payload, "status").unwrap_or_default();
                let message = message.unwrap_or_default();
                let reported = status != "ok" && !message.is_empty();
                if reported {
                    effects.push(Effect::Notice(Notice::error(&message, None)));
                }
                effects.push(Effect::Finalized {
                    folder,
                    audio: FinalAudio {
                        usable: status != "fejlet" && text(payload, "audio_file").is_some(),
                        message,
                        reported,
                    },
                });
            }
        }
        _ => {}
    }
    effects
}

/// A peak (0 to 1) as a bar height: -50 dB and below is empty, 0 dB full.
fn bar(peak: f32) -> f32 {
    if peak > 0.0 {
        ((20.0 * peak.log10() + 50.0) / 50.0).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// Apply a snapshot from core. Returns whether core says no recording runs
/// although the state shows one (the stop event is late or was missed).
fn apply_snapshot(state: &mut RecorderState, snapshot: &RecordingSnapshot) -> bool {
    if !snapshot.is_recording {
        return state.phase.is_recording() || state.phase == Phase::Stopping;
    }
    if snapshot.is_stopping {
        if state.phase.yields_to_core() || state.phase.is_recording() {
            state.phase = Phase::Stopping;
            state.levels = [0.0; 3];
        }
        return false;
    }
    if state.phase.yields_to_core() || state.phase.is_recording() {
        state.phase = if snapshot.is_paused {
            Phase::Paused
        } else {
            Phase::Recording
        };
    }
    if state.phase.is_recording() {
        state.seconds = snapshot.active_seconds.max(0.0) as u64;
        if !matches!(state.notice, Some(DeviceNotice::Lost(_))) {
            state.microphone = snapshot.microphone.clone();
        }
        state.system = snapshot.system.clone();
        state.storage_error = snapshot.storage_error.clone();
        state.levels = if state.phase == Phase::Paused {
            [0.0; 3]
        } else {
            let target = [
                bar(snapshot.microphone_level),
                bar(snapshot.microphone_level.max(snapshot.system_level)),
                bar(snapshot.system_level),
            ];
            // Rise at once, fall over a few ticks, so speech does not flicker.
            let mut levels = state.levels;
            for (level, target) in levels.iter_mut().zip(target) {
                *level = target.max(*level * 0.6);
            }
            levels
        };
    }
    false
}

/// How a meeting's final audio turned out.
#[derive(Clone, Debug, PartialEq, Eq)]
struct FinalAudio {
    usable: bool,
    message: String,
    /// The `recording-audio-finalized` event already told the user why.
    reported: bool,
}

/// Encodes that have finished, for the stop that waits for them.
#[derive(Default)]
struct Finalized {
    /// By folder, with the order they arrived in.
    done: Mutex<HashMap<String, (u64, FinalAudio)>>,
    arrived: AtomicU64,
    changed: Notify,
}

impl Finalized {
    fn insert(&self, folder: String, audio: FinalAudio) {
        let order = self.arrived.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut done) = self.done.lock() {
            // Drop the oldest unclaimed one (Tauri's recording-finalization.ts),
            // never one a stop that just ended may still ask for.
            if done.len() >= FINALIZED_KEPT && !done.contains_key(&folder) {
                let oldest = done
                    .iter()
                    .min_by_key(|(_, (order, _))| *order)
                    .map(|(folder, _)| folder.clone());
                if let Some(oldest) = oldest {
                    done.remove(&oldest);
                }
            }
            done.insert(folder, (order, audio));
        }
        self.changed.notify_waiters();
    }

    async fn wait(&self, folder: &str) -> FinalAudio {
        loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if let Some((_, audio)) = self.done.lock().ok().and_then(|mut d| d.remove(folder)) {
                return audio;
            }
            changed.await;
        }
    }
}

pub struct Recorder {
    app: HostHandle,
    core: Arc<dyn RecorderCore>,
    runtime: Handle,
    state: watch::Sender<RecorderState>,
    notices: mpsc::UnboundedSender<Notice>,
    notice_receiver: Mutex<Option<mpsc::UnboundedReceiver<Notice>>>,
    finalized: Finalized,
    /// The newest stop being saved and transcribed; an older one that ends
    /// later leaves the state alone.
    finishing: AtomicU64,
    /// A pause or resume call has not answered yet.
    pausing: AtomicBool,
}

impl Recorder {
    /// Read core's state and follow its events from now on. `events` is a
    /// subscription to the bridge (`Bridge::subscribe`).
    pub fn start(
        app: HostHandle,
        runtime: Handle,
        core: Arc<dyn RecorderCore>,
        events: mpsc::UnboundedReceiver<Event>,
    ) -> (Arc<Recorder>, JoinHandle<()>) {
        let mut state = RecorderState::default();
        apply_snapshot(&mut state, &core.snapshot());
        let (notices, notice_receiver) = mpsc::unbounded_channel();
        let recorder = Arc::new(Recorder {
            app,
            core,
            runtime: runtime.clone(),
            state: watch::channel(state).0,
            notices,
            notice_receiver: Mutex::new(Some(notice_receiver)),
            finalized: Finalized::default(),
            finishing: AtomicU64::new(0),
            pausing: AtomicBool::new(false),
        });
        let task = runtime.spawn(recorder.clone().follow(events));
        (recorder, task)
    }

    /// The current state, and every change from now on.
    pub fn subscribe(&self) -> watch::Receiver<RecorderState> {
        self.state.subscribe()
    }

    pub fn state(&self) -> RecorderState {
        self.state.borrow().clone()
    }

    /// The host core runs against, for the settings that belong to the
    /// recording (devices, start reminder).
    pub fn host(&self) -> HostHandle {
        self.app.clone()
    }

    /// The messages for the user. There is one reader: the first caller.
    pub fn take_notices(&self) -> Option<mpsc::UnboundedReceiver<Notice>> {
        self.notice_receiver.lock().ok()?.take()
    }

    /// "Start optagelse" with the devices chosen in the settings. Ignored
    /// while a recording runs; while the last one is written to disk or saved
    /// as a meeting, the user is told to wait. The transcription of the last
    /// meeting carries on in the background.
    pub fn start_recording(self: &Arc<Self>) {
        let mut begun = false;
        let mut saving = false;
        self.state.send_if_modified(|state| {
            begun = matches!(state.phase, Phase::Idle | Phase::Finishing);
            saving = matches!(state.phase, Phase::Stopping | Phase::SavingMeeting);
            if begun {
                state.phase = Phase::Starting;
                state.start_error = None;
            }
            begun
        });
        if saving {
            self.notify(Notice::Warning {
                text: strings::RECORDING_START_WHILE_SAVING.to_string(),
                sticky: false,
            });
        }
        if !begun {
            return;
        }
        let this = self.clone();
        let title = strings::recording_title_now();
        self.runtime.spawn(async move {
            let result = this.core.start(this.app.clone(), title).await;
            match result {
                Ok(()) => {
                    let snapshot = this.core.snapshot();
                    this.state.send_modify(|state| {
                        apply_snapshot(state, &snapshot);
                    });
                }
                // A start from the tray or MCP won the race: that recording
                // is live, so the screen follows it (unless it has ended
                // already).
                Err(error) if error.contains("already in progress") => {
                    let snapshot = this.core.snapshot();
                    this.state.send_modify(|state| {
                        apply_snapshot(state, &snapshot);
                        if state.phase == Phase::Starting && !snapshot.is_recording {
                            state.phase = Phase::Idle;
                        }
                    });
                }
                Err(error) => {
                    log::warn!("Recording could not start: {error}");
                    this.state.send_modify(|state| {
                        if state.phase == Phase::Starting {
                            state.phase = Phase::Idle;
                        }
                        state.start_error = Some(StartError::from_message(&error));
                    });
                }
            }
        });
    }

    /// "Pause" or "Genoptag". A click while the last one has not answered is
    /// ignored (a double click would pause twice and report an error).
    pub fn toggle_pause(self: &Arc<Self>) {
        let phase = self.state.borrow().phase;
        if !phase.is_recording() || self.pausing.swap(true, Ordering::SeqCst) {
            return;
        }
        let this = self.clone();
        let (call, title, help) = match phase {
            Phase::Recording => (
                this.core.pause(this.app.clone()),
                strings::RECORDING_PAUSE_FAILED,
                strings::RECORDING_PAUSE_FAILED_HELP,
            ),
            Phase::Paused => (
                this.core.resume(this.app.clone()),
                strings::RECORDING_RESUME_FAILED,
                strings::RECORDING_RESUME_FAILED_HELP,
            ),
            _ => unreachable!("checked above"),
        };
        self.runtime.spawn(async move {
            let result = call.await;
            this.pausing.store(false, Ordering::SeqCst);
            if let Err(error) = result {
                log::warn!("Pause or resume failed: {error}");
                this.notify(Notice::error(title, Some(help.to_string())));
            }
        });
    }

    /// "Stop og gem".
    pub fn stop_recording(self: &Arc<Self>) {
        let mut begun = false;
        self.state.send_if_modified(|state| {
            begun = state.phase.is_recording();
            if begun {
                state.phase = Phase::Stopping;
                state.levels = [0.0; 3];
            }
            begun
        });
        if !begun {
            return;
        }
        let this = self.clone();
        self.runtime.spawn(async move {
            if let Err(error) = this.core.stop(this.app.clone()).await {
                log::warn!("Recording could not stop: {error}");
                this.notify(Notice::error(
                    strings::RECORDING_STOP_FAILED,
                    Some(strings::RECORDING_STOP_FAILED_HELP.to_string()),
                ));
                // Core decides what runs now.
                let snapshot = this.core.snapshot();
                this.state.send_modify(|state| {
                    if state.phase == Phase::Stopping && snapshot.is_recording {
                        state.phase = if snapshot.is_paused {
                            Phase::Paused
                        } else {
                            Phase::Recording
                        };
                    }
                    apply_snapshot(state, &snapshot);
                });
            }
        });
    }

    /// Close the start error under the pill.
    pub fn dismiss_start_error(&self) {
        self.state
            .send_if_modified(|state| state.start_error.take().is_some());
    }

    /// Close the microphone notice in the status bar.
    pub fn dismiss_notice(&self) {
        self.state
            .send_if_modified(|state| state.notice.take().is_some());
    }

    /// Transcribe a meeting's saved audio in the background (a restored
    /// recording); a failure is reported as a notice.
    pub fn transcribe_in_background(self: &Arc<Self>, meeting_id: String, folder: String) {
        let this = self.clone();
        self.runtime.spawn(async move {
            if let Err(error) = this
                .core
                .transcribe(this.app.clone(), meeting_id, folder)
                .await
            {
                log::warn!("Transcribing a restored meeting failed: {error}");
                this.notify(Notice::error(
                    strings::RECORDING_TRANSCRIBE_FAILED,
                    Some(error),
                ));
            }
        });
    }

    fn notify(&self, notice: Notice) {
        let _ = self.notices.send(notice);
    }

    async fn follow(self: Arc<Self>, mut events: mpsc::UnboundedReceiver<Event>) {
        let mut tick = tokio::time::interval(TICK);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut idle = 0u32;
        let mut stale = 0u32;
        loop {
            tokio::select! {
                event = events.recv() => {
                    let Some(event) = event else { break };
                    self.on_event(&event);
                }
                _ = tick.tick() => {
                    if self.state.borrow().phase == Phase::Idle {
                        idle += 1;
                        if idle < IDLE_TICKS {
                            continue;
                        }
                    }
                    idle = 0;
                    let snapshot = self.core.snapshot();
                    let mut missed_stop = false;
                    self.state.send_if_modified(|state| {
                        let before = state.clone();
                        missed_stop = apply_snapshot(state, &snapshot);
                        *state != before
                    });
                    stale = if missed_stop { stale + 1 } else { 0 };
                    if stale >= STALE_TICKS {
                        log::warn!("Core stopped recording without a stop event");
                        stale = 0;
                        self.state.send_modify(|state| {
                            *state = RecorderState::default();
                        });
                    }
                }
            }
        }
    }

    fn on_event(self: &Arc<Self>, event: &Event) {
        let payload = serde_json::from_str::<Value>(&event.payload_json).unwrap_or(Value::Null);
        let mut effects = Vec::new();
        self.state.send_if_modified(|state| {
            let before = state.clone();
            effects = apply_event(state, &event.name, &payload);
            *state != before
        });
        for effect in effects {
            match effect {
                Effect::Notice(notice) => self.notify(notice),
                Effect::Finalized { folder, audio } => self.finalized.insert(folder, audio),
                Effect::Finish { folder, title } => {
                    let finish = self.finishing.fetch_add(1, Ordering::SeqCst) + 1;
                    self.runtime
                        .spawn(self.clone().finish(folder, title, finish));
                }
            }
        }
    }

    /// Save the stopped recording as a meeting, wait for its final audio and
    /// transcribe it, then open it (`useRecordingStop.ts`).
    ///
    /// The phase only moves on while it still belongs to this stop: a
    /// recording started meanwhile (tray, MCP, "Ny optagelse") or a newer
    /// stop owns the screen, and this one finishes in the background.
    async fn finish(self: Arc<Self>, folder: String, title: String, finish: u64) {
        let set_phase = |from: Phase, to: Phase| {
            let mut moved = false;
            self.state.send_if_modified(|state| {
                moved = state.phase == from && self.finishing.load(Ordering::SeqCst) == finish;
                if moved {
                    state.phase = to;
                }
                moved
            });
            moved
        };
        let meeting_id = match self
            .core
            .save_meeting(self.app.clone(), title, folder.clone())
            .await
        {
            Ok(id) => id,
            Err(error) => {
                log::error!("The recorded meeting could not be saved: {error}");
                self.notify(Notice::error(strings::RECORDING_SAVE_FAILED, Some(error)));
                set_phase(Phase::SavingMeeting, Phase::Idle);
                return;
            }
        };
        self.notify(Notice::MeetingSaved(meeting_id.clone()));
        set_phase(Phase::SavingMeeting, Phase::Finishing);
        let audio = self.finalized.wait(&folder).await;
        let transcribed = if audio.usable {
            self.core
                .transcribe(self.app.clone(), meeting_id.clone(), folder)
                .await
        } else if audio.reported {
            // The final audio event already said why: one message per cause.
            Ok(())
        } else {
            Err(audio.message)
        };
        if let Err(error) = transcribed {
            log::warn!("The recorded meeting could not be transcribed: {error}");
            self.notify(Notice::error(
                strings::RECORDING_TRANSCRIBE_FAILED,
                Some(error).filter(|e| !e.is_empty()),
            ));
        }
        self.notify(Notice::Success(strings::RECORDING_SAVED.to_string()));
        // Open the meeting unless another recording has the screen now.
        let open =
            set_phase(Phase::Finishing, Phase::Idle) || self.state.borrow().phase == Phase::Idle;
        if open {
            self.notify(Notice::OpenMeeting(meeting_id));
        } else {
            self.notify(Notice::MeetingSaved(meeting_id));
        }
    }
}

/// [`RecorderCore`] on the real core.
pub struct CoreRecorder;

impl RecorderCore for CoreRecorder {
    fn start(&self, app: HostHandle, title: String) -> CoreFuture<()> {
        Box::pin(async move {
            // Loads the devices chosen in the settings, as Tauri's start does.
            tingskriver_core::audio::recording_commands::start_recording_with_meeting_name(
                app,
                Some(title),
            )
            .await
        })
    }

    fn pause(&self, app: HostHandle) -> CoreFuture<()> {
        Box::pin(tingskriver_core::audio::recording_commands::pause_recording(app))
    }

    fn resume(&self, app: HostHandle) -> CoreFuture<()> {
        Box::pin(tingskriver_core::audio::recording_commands::resume_recording(app))
    }

    fn stop(&self, app: HostHandle) -> CoreFuture<()> {
        Box::pin(async move {
            use tingskriver_core::audio::recording_commands as rec;
            let args = rec::RecordingArgs {
                save_path: String::new(),
            };
            rec::stop_recording(app, args).await.map(|_| ())
        })
    }

    fn snapshot(&self) -> RecordingSnapshot {
        tingskriver_core::audio::recording_commands::recording_snapshot()
    }

    fn save_meeting(&self, app: HostHandle, title: String, folder: String) -> CoreFuture<String> {
        Box::pin(async move {
            use tingskriver_core::database::repositories::transcript::TranscriptsRepository;
            let db = app
                .database()
                .ok_or_else(|| "The database is not open".to_string())?;
            TranscriptsRepository::save_transcript(db.pool(), &title, &[], Some(folder))
                .await
                .map_err(|e| e.to_string())
        })
    }

    fn transcribe(&self, app: HostHandle, meeting_id: String, folder: String) -> CoreFuture<()> {
        Box::pin(async move {
            let db = app
                .database()
                .ok_or_else(|| "The database is not open".to_string())?;
            let config = tingskriver_core::api::get_transcript_config(&db)
                .await?
                .ok_or_else(|| "No transcription model is set up".to_string())?;
            // Whisper takes the meeting's language; Parakeet has none to set
            // (Tauri passes none for Parakeet and for "auto").
            let language = if config.provider == "parakeet" {
                None
            } else {
                app.app_data_dir()
                    .ok()
                    .and_then(|dir| {
                        tingskriver_core::meeting_language::MeetingLanguages::load(&dir).ok()
                    })
                    .filter(|languages| languages.extra.is_empty())
                    .map(|languages| languages.primary)
            };
            let model = Some(config.model).filter(|m| !m.is_empty());
            tingskriver_core::audio::retranscription::start_retranscription(
                app,
                meeting_id,
                folder,
                language,
                model,
                Some(config.provider),
            )
            .await
            .map(|_| ())
            .map_err(|e| e.to_string())
        })
    }
}

#[cfg(test)]
mod tests;
