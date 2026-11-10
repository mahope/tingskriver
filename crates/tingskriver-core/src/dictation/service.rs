//! Dictation orchestration: shortcut → record → transcribe → clean → insert.

// The shortcut, overlay and insertion only run on Windows.
#![cfg_attr(not(windows), allow(dead_code))]

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::{Duration, Instant};

use crate::host::{HostExt, HostHandle};

use super::capture::Recorder;
use super::logic::app_style;
use super::logic::audio;
use super::logic::cleanup::{self, Options, Rules, Style};
use super::logic::engine::{self, Attempt, Engine, EnginePreference, Next};
use super::logic::gesture::{Gesture, GestureAction, GestureConfig, GestureEvent};
use super::logic::hotkey::ChordSignal;
use super::logic::order::Order;
use super::logic::spoken::{self, FieldId, LastInsertion, VoiceCommand};
use super::logic::startup::{self, Startup};
use super::logic::target_check::{Focus, Refusal};
use super::logic::timing::{self, Steps};
use super::modes::{Context, Mode};
use super::overlay::{Overlay, OverlayState};
use super::secrets;
use super::settings::{self, DictationSettings};
use super::sound::{self, Sound};
use super::store::{NewHistoryEntry, Store};
use super::syv;
use super::timings::{LatencyReport, LatencyWarning, NewTiming, Timings, LATENCY_WARNING_EVENT};

/// Emitted to the main window when the history list changes.
pub const HISTORY_CHANGED_EVENT: &str = "dictation-history-changed";

mod flow;
use flow::Flow;
pub use flow::{DictationBackend, DictationRun, DictationState};

static SERVICE: OnceLock<Arc<Service>> = OnceLock::new();
static DICTATING: AtomicBool = AtomicBool::new(false);

/// True while the dictation microphone is open. Meeting code can use this to
/// keep dictation speech out of the meeting's microphone track.
pub fn is_dictating() -> bool {
    DICTATING.load(Ordering::Relaxed)
}

pub fn get() -> Result<Arc<Service>, String> {
    SERVICE
        .get()
        .cloned()
        .ok_or_else(|| "Diktering er ikke klar endnu".to_string())
}

pub struct Service {
    app: HostHandle,
    backend: Option<Arc<dyn DictationBackend>>,
    processing_cancel: Mutex<tokio_util::sync::CancellationToken>,
    states: tokio::sync::broadcast::Sender<DictationState>,
    overlay: Arc<dyn Overlay>,
    pub store: Store,
    timings: Timings,
    /// Providers whose median is over the target and already warned about.
    latency_warned: Mutex<BTreeSet<String>>,
    settings: RwLock<DictationSettings>,
    rules: RwLock<Rules>,
    pub http: reqwest::Client,
    app_data_dir: PathBuf,
    audio_dir: PathBuf,
    shortcut_active: AtomicBool,
    /// Dictations are inserted in the order they were spoken: each stop takes
    /// a ticket, and insertion waits for the previous ticket to finish.
    next_ticket: AtomicU64,
    order: Mutex<Order>,
    /// The lowest unfinished ticket, for waiting dictations.
    insert_turn: tokio::sync::watch::Sender<u64>,
    last_error: Mutex<Option<String>>,
    last_insertion: Mutex<Option<LastInsertion>>,
    active_mode: Mutex<Option<String>>,
    #[cfg(windows)]
    hook: Mutex<Option<super::win::KeyboardHook>>,
    /// Feeds the control loop like the hook does (tray menu "Diktér").
    #[cfg(windows)]
    trigger: Mutex<Option<std::sync::mpsc::Sender<(ChordSignal, Instant, Option<String>)>>>,
}

/// Open or close the kept-open microphone to match the settings. Opening the
/// device can take a while, so it happens on its own thread.
fn apply_capture(settings: &DictationSettings) {
    #[cfg(windows)]
    {
        let options = settings.capture_options();
        let keep = settings.enabled && options.keep_open;
        let spawned = std::thread::Builder::new()
            .name("dictation-capture-setup".into())
            .spawn(move || {
                if keep {
                    super::capture::warm_up(&options);
                } else {
                    super::capture::release();
                }
            });
        if let Err(e) = spawned {
            log::warn!("Dictation microphone setup thread not started: {e}");
        }
    }
    #[cfg(not(windows))]
    let _ = settings;
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// Start dictation in the background. Called once from app setup.
pub fn init(app: HostHandle, overlay: Arc<dyn Overlay>) {
    // Runs also if dictation fails to start, since meetings rely on it to
    // unload idle models.
    super::warm::start();
    crate::runtime::spawn(async move {
        if let Err(e) = init_inner(app, overlay).await {
            log::error!("Dictation could not start: {e}");
        }
    });
}

async fn init_inner(app: HostHandle, overlay: Arc<dyn Overlay>) -> Result<(), String> {
    let service = Service::open(app, overlay, None).await?;
    SERVICE
        .set(service.clone())
        .map_err(|_| "dictation already initialised".to_string())?;
    service.apply_shortcut();
    apply_capture(&service.settings());
    let mut interval = tokio::time::interval(Duration::from_secs(10 * 60));
    loop {
        interval.tick().await;
        service.expire_audio().await;
    }
}

impl Service {
    async fn open(
        app: HostHandle,
        overlay: Arc<dyn Overlay>,
        backend: Option<Arc<dyn DictationBackend>>,
    ) -> Result<Arc<Self>, String> {
        let app_data_dir = app.app_data_dir()?;
        let store = Store::open(&app_data_dir.join("dictation.sqlite")).await?;
        let settings = store.settings().await?;
        let mut rules = store.rules().await?;
        let (spoken, spoken_error) = settings::load_spoken_table(&app_data_dir).await;
        rules.spoken = spoken;
        let timings = Timings::open(store.pool()).await?;
        if backend.is_none() {
            super::warm::set_policy(settings.residency_policy());
        }
        let service = Arc::new(Service {
            app: app.clone(),
            backend,
            processing_cancel: Mutex::new(tokio_util::sync::CancellationToken::new()),
            states: tokio::sync::broadcast::channel(128).0,
            overlay,
            store,
            timings,
            latency_warned: Mutex::new(BTreeSet::new()),
            settings: RwLock::new(settings),
            rules: RwLock::new(rules),
            http: crate::http::client(),
            audio_dir: app_data_dir.join("dictation_audio"),
            app_data_dir,
            shortcut_active: AtomicBool::new(false),
            next_ticket: AtomicU64::new(0),
            order: Mutex::new(Order::default()),
            insert_turn: tokio::sync::watch::channel(0).0,
            last_error: Mutex::new(None),
            last_insertion: Mutex::new(None),
            active_mode: Mutex::new(None),
            #[cfg(windows)]
            hook: Mutex::new(None),
            #[cfg(windows)]
            trigger: Mutex::new(None),
        });
        if let Some(e) = spoken_error {
            service.set_error(Some(e));
        }
        Ok(service)
    }
}

impl Service {
    pub fn settings(&self) -> DictationSettings {
        self.settings.read().map(|s| s.clone()).unwrap_or_default()
    }

    pub fn export_modes(&self) -> Result<String, String> {
        self.settings().modes.export()
    }

    pub async fn import_modes(self: &Arc<Self>, json: &str) -> Result<DictationSettings, String> {
        let modes = super::modes::ModeCollection::import(json)?;
        let mut settings = self.settings();
        settings.modes = modes;
        self.save_settings(settings).await
    }

    fn rules(&self) -> Rules {
        self.rules.read().map(|r| r.clone()).unwrap_or_default()
    }

    /// The folder for text hook scripts (created if missing) and the scripts in it.
    pub async fn hook_scripts(&self) -> (String, Vec<String>) {
        let dir = super::text_hook::hook_dir(&self.app_data_dir);
        if let Err(e) = tokio::fs::create_dir_all(&dir).await {
            log::warn!("Dictation: could not create the hook folder: {e}");
        }
        let scripts = super::text_hook::list_scripts(&dir).await;
        (dir.to_string_lossy().into_owned(), scripts)
    }

    pub fn shortcut_active(&self) -> bool {
        self.shortcut_active.load(Ordering::Relaxed)
    }

    pub fn last_error(&self) -> Option<String> {
        self.last_error.lock().ok().and_then(|e| e.clone())
    }

    fn set_error(&self, error: Option<String>) {
        if let Ok(mut slot) = self.last_error.lock() {
            *slot = error;
        }
    }

    pub async fn save_settings(
        self: &Arc<Self>,
        settings: DictationSettings,
    ) -> Result<DictationSettings, String> {
        let settings = settings.normalized()?;
        self.store.save_settings(&settings).await?;
        let previous = self.settings();
        if let Ok(mut slot) = self.settings.write() {
            *slot = settings.clone();
        }
        if previous.residency_policy() != settings.residency_policy() {
            super::warm::set_policy(settings.residency_policy());
        }
        if previous.enabled != settings.enabled
            || previous.shortcut != settings.shortcut
            || previous.modes != settings.modes
        {
            self.apply_shortcut();
        }
        if previous.capture_options() != settings.capture_options()
            || previous.enabled != settings.enabled
        {
            apply_capture(&settings);
        }
        Ok(settings)
    }

    pub async fn reload_rules(&self) -> Result<(), String> {
        let mut rules = self.store.rules().await?;
        let (spoken, spoken_error) = settings::load_spoken_table(&self.app_data_dir).await;
        rules.spoken = spoken;
        if let Some(e) = spoken_error {
            log::warn!("Dictation: {e}");
            self.set_error(Some(e));
        }
        if let Ok(mut slot) = self.rules.write() {
            *slot = rules;
        }
        Ok(())
    }

    /// Median and p95 per provider and step, for the debug page.
    pub async fn latency_report(&self) -> Result<LatencyReport, String> {
        Ok(LatencyReport::from_stats(&self.timings.stats().await?))
    }

    /// Store the timings of one dictation and warn when a provider's median
    /// went over the target.
    async fn record_timing(&self, timing: NewTiming) {
        if let Err(e) = self.timings.add(&timing).await {
            log::warn!("Dictation: could not save timings: {e}");
            return;
        }
        let stats = match self.timings.stats().await {
            Ok(stats) => stats,
            Err(e) => {
                log::warn!("Dictation: could not read timings: {e}");
                return;
            }
        };
        let warnings: Vec<LatencyWarning> = match self.latency_warned.lock() {
            Ok(mut warned) => timing::new_warnings(&stats, &mut warned)
                .into_iter()
                .map(Into::into)
                .collect(),
            Err(_) => Vec::new(),
        };
        for warning in warnings {
            log::warn!(
                "Dictation: median {} ms over the {} ms target for {} (p95 {} ms, {} dictations)",
                warning.median_ms,
                warning.target_ms,
                warning.provider,
                warning.p95_ms,
                warning.count
            );
            let _ = self.app.emit(LATENCY_WARNING_EVENT, warning);
        }
    }

    pub fn preview(&self, text: &str, style: Style) -> String {
        cleanup::clean(text, style, &self.rules())
    }

    /// Name of the mode the latest dictation used.
    pub fn active_mode(&self) -> Option<String> {
        self.active_mode.lock().ok().and_then(|m| m.clone())
    }

    /// Mark a dictionary word as English ("behold engelsk stavning").
    pub async fn set_word_english(&self, id: i64, english: bool) -> Result<(), String> {
        self.store.set_word_english(id, english).await?;
        self.reload_rules().await
    }

    #[allow(clippy::disallowed_methods)]
    pub fn delete_files(&self, paths: Vec<String>) {
        for path in paths {
            let path = PathBuf::from(path);
            // Only ever delete inside our own audio folder.
            if path.starts_with(&self.audio_dir) {
                let _ = std::fs::remove_file(path);
            }
        }
    }

    pub fn notify_history_changed(&self) {
        let _ = self.app.emit(HISTORY_CHANGED_EVENT, ());
    }

    async fn expire_audio(&self) {
        match self.store.expire_audio(now_ms()).await {
            Ok(paths) => self.delete_files(paths),
            Err(e) => log::warn!("Dictation: could not expire old recordings: {e}"),
        }
    }

    /// (Re)install the keyboard hook according to the settings.
    #[cfg(windows)]
    fn apply_shortcut(self: &Arc<Self>) {
        let mut slot = match self.hook.lock() {
            Ok(slot) => slot,
            Err(_) => return,
        };
        // Dropping the old hook also ends its control thread.
        slot.take();
        if let Ok(mut trigger) = self.trigger.lock() {
            trigger.take();
        }
        self.shortcut_active.store(false, Ordering::Relaxed);

        let settings = self.settings();
        if !settings.enabled {
            log::info!("Dictation shortcut disabled");
            return;
        }
        let chord = match settings.chord() {
            Ok(chord) => chord,
            Err(e) => {
                log::warn!("Dictation shortcut invalid: {e}");
                self.set_error(Some(e));
                return;
            }
        };

        let (tx, rx) = std::sync::mpsc::channel();
        let trigger = tx.clone();
        let shortcuts = settings
            .modes
            .modes
            .iter()
            .flat_map(|m| &m.rules)
            .filter_map(|r| {
                if let super::modes::Rule::Shortcut(s) = r {
                    super::logic::hotkey::Chord::parse(s).ok()
                } else {
                    None
                }
            })
            .collect();
        match super::win::KeyboardHook::start(chord, shortcuts, tx) {
            Ok(hook) => {
                *slot = Some(hook);
                if let Ok(mut slot) = self.trigger.lock() {
                    *slot = Some(trigger);
                }
                self.shortcut_active.store(true, Ordering::Relaxed);
                self.set_error(None);
                log::info!("Dictation shortcut active: {}", chord.label());
                let service = self.clone();
                let _ = std::thread::Builder::new()
                    .name("dictation-control".into())
                    .spawn(move || control_loop(service, rx));
                let overlay = self.overlay.clone();
                crate::runtime::spawn(async move {
                    if let Err(e) = overlay.ensure() {
                        log::warn!("{e}");
                    }
                });
            }
            Err(e) => {
                log::error!("Dictation keyboard hook failed: {e}");
                self.set_error(Some(format!("Genvejen kunne ikke registreres: {e}")));
            }
        }
    }

    /// Start hands-free dictation without the shortcut (the tray menu). The
    /// user then clicks into the field and presses the shortcut once to
    /// insert, as after a double tap. While dictating it stops instead.
    #[cfg(windows)]
    pub fn toggle_hands_free(&self) -> Result<(), String> {
        let trigger = self.trigger.lock().ok().and_then(|t| t.clone());
        let trigger = trigger.ok_or_else(|| "Genvejen til diktering er ikke aktiv".to_string())?;
        let now = Instant::now();
        // A double tap locks the recording; one tap stops a locked one.
        let taps = if is_dictating() { 1 } else { 2 };
        for _ in 0..taps {
            for signal in [ChordSignal::Pressed, ChordSignal::Released] {
                trigger
                    .send((signal, now, None))
                    .map_err(|_| "Diktering kører ikke".to_string())?;
            }
        }
        Ok(())
    }

    /// Remove the keyboard hook for good (the app quits). The hook thread
    /// logs the rest of its timing window when it unhooks.
    #[cfg(windows)]
    pub fn stop_shortcut(&self) {
        if let Ok(mut slot) = self.hook.lock() {
            slot.take();
        }
        if let Ok(mut trigger) = self.trigger.lock() {
            trigger.take();
        }
        self.shortcut_active.store(false, Ordering::Relaxed);
    }

    #[cfg(not(windows))]
    pub fn stop_shortcut(&self) {
        self.shortcut_active.store(false, Ordering::Relaxed);
    }

    #[cfg(not(windows))]
    pub fn toggle_hands_free(&self) -> Result<(), String> {
        Err("Diktering findes kun på Windows".to_string())
    }

    #[cfg(not(windows))]
    fn apply_shortcut(self: &Arc<Self>) {
        log::info!("Dictation shortcut is only available on Windows");
        self.shortcut_active.store(false, Ordering::Relaxed);
    }
}

struct Session {
    flow: Flow,
    recorder: Recorder,
    target_app: Option<String>,
    target_window: usize,
    /// The focused field when the shortcut was pressed.
    start_focus: Option<Focus>,
    settings: DictationSettings,
    mode: Option<Mode>,
    /// When the shortcut was pressed.
    pressed: Instant,
    /// Shortcut pressed until the first audio arrived; `None` until the
    /// recording is ready.
    start_ms: Option<u64>,
}

fn gesture_event(signal: ChordSignal) -> GestureEvent {
    match signal {
        ChordSignal::Pressed => GestureEvent::Pressed,
        ChordSignal::Released => GestureEvent::Released,
        ChordSignal::Interrupted => GestureEvent::Interrupted,
    }
}

#[cfg(windows)]
fn foreground_context() -> (Option<String>, Option<String>, Option<String>) {
    super::win::foreground_context()
}

#[cfg(not(windows))]
fn foreground_context() -> (Option<String>, Option<String>, Option<String>) {
    (None, None, None)
}

fn foreground_window() -> usize {
    #[cfg(windows)]
    {
        super::win::foreground_window()
    }
    #[cfg(not(windows))]
    {
        0
    }
}

fn flash_error(overlay: &Arc<dyn Overlay>) {
    flash(overlay, OverlayState::Error);
}

fn flash(overlay: &Arc<dyn Overlay>, state: OverlayState) {
    overlay.set(state);
    let overlay = overlay.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(1500));
        if !is_dictating() {
            overlay.set(OverlayState::Hidden);
        }
    });
}

/// Runs on its own thread for as long as the keyboard hook is installed.
fn control_loop(service: Arc<Service>, rx: Receiver<(ChordSignal, Instant, Option<String>)>) {
    let epoch = Instant::now();
    let ms = |t: Instant| t.saturating_duration_since(epoch).as_millis() as u64;
    let mut gesture = Gesture::new(GestureConfig::default());
    let mut session: Option<Session> = None;
    let mut selected_shortcut: Option<String> = None;

    loop {
        let (actions, at) = match rx.recv_timeout(Duration::from_millis(50)) {
            Ok((signal, at, shortcut)) => {
                if signal == ChordSignal::Interrupted {
                    service.cancel_processing();
                }
                if signal == ChordSignal::Pressed && session.is_none() {
                    selected_shortcut = shortcut;
                }
                (gesture.on_event(gesture_event(signal), ms(at)), at)
            }
            Err(RecvTimeoutError::Timeout) => {
                let now = Instant::now();
                (gesture.tick(ms(now)), now)
            }
            Err(RecvTimeoutError::Disconnected) => break,
        };

        for action in actions {
            match action {
                GestureAction::Start => {
                    if session.is_some() {
                        continue;
                    }
                    let target_window = foreground_window();
                    let start_focus = focus_now();
                    let (target_app, window_class, title) = foreground_context();
                    let settings = service.settings();
                    let mode = settings
                        .modes
                        .select(&Context {
                            process: target_app.as_deref(),
                            window_class: window_class.as_deref(),
                            title: title.as_deref(),
                            shortcut: selected_shortcut
                                .as_deref()
                                .or(Some(settings.shortcut.as_str())),
                            ..Context::default()
                        })
                        .cloned();
                    service.overlay.set_mode(mode.as_ref());
                    let _ = service.app.emit("dictation-mode-changed", &mode);
                    // Load the local model while the user speaks, so neither
                    // the local engine nor the fallback pays for a load.
                    super::warm::prewarm_parakeet();
                    let flow = Flow::new(service.states.clone(), None);
                    match Recorder::start(&settings.capture_options()) {
                        Ok(recorder) => {
                            DICTATING.store(true, Ordering::Relaxed);
                            session = Some(Session {
                                flow,
                                recorder,
                                target_app,
                                target_window,
                                start_focus,
                                settings,
                                mode,
                                pressed: at,
                                start_ms: None,
                            });
                        }
                        Err(e) => {
                            flow.set(DictationState::Error);
                            log::warn!("Dictation microphone failed");
                            service.set_error(Some(e));
                            gesture.reset();
                            flash_error(&service.overlay);
                        }
                    }
                }
                GestureAction::Lock => {}
                GestureAction::Cancel => {
                    if let Some(s) = session.take() {
                        s.flow.set(DictationState::Cancelled);
                        DICTATING.store(false, Ordering::Relaxed);
                        service.overlay.set(OverlayState::Hidden);
                    }
                }
                GestureAction::Stop => {
                    if let Some(s) = session.take() {
                        let samples = s.recorder.stop();
                        DICTATING.store(false, Ordering::Relaxed);
                        if startup::should_play_sound(
                            s.settings.sound_enabled,
                            s.settings.sound_volume,
                            s.start_ms.is_some(),
                        ) {
                            sound::play(Sound::Stop, s.settings.sound_volume);
                        }
                        service.overlay.set(OverlayState::Processing);
                        let cancel = service.processing_token();
                        let service = service.clone();
                        let target_app = s.target_app;
                        let start_ms = s.start_ms;
                        let ticket = service.next_ticket.fetch_add(1, Ordering::SeqCst);
                        crate::runtime::spawn(async move {
                            let turn = Turn {
                                service: service.clone(),
                                ticket,
                                flow: s.flow,
                            };
                            tokio::select! {
                                biased;
                                _ = cancel.cancelled() => turn.flow.set(DictationState::Cancelled),
                                _ = service.process(samples, target_app, s.target_window, s.start_focus, (s.settings, s.mode), at, start_ms, &turn) => {}
                            }
                        });
                    }
                }
            }
        }

        let mut silent = false;
        if let Some(s) = session.as_mut() {
            if s.start_ms.is_none() {
                match startup::startup(s.recorder.age(), s.recorder.first_audio_at().is_some()) {
                    Startup::Ready => {
                        let first = s.recorder.first_audio_at().unwrap_or_else(Instant::now);
                        s.start_ms =
                            Some(first.saturating_duration_since(s.pressed).as_millis() as u64);
                        if startup::should_play_sound(
                            s.settings.sound_enabled,
                            s.settings.sound_volume,
                            true,
                        ) {
                            sound::play(Sound::Start, s.settings.sound_volume);
                        }
                    }
                    Startup::NoSound => silent = true,
                    Startup::Waiting => {}
                }
            }
            // The pill only shows "recording" once audio arrives.
            if s.start_ms.is_some() {
                service.overlay.set(OverlayState::Recording {
                    level: s.recorder.level(),
                    locked: gesture.is_locked(),
                });
            }
        }
        if silent {
            // No sound at all: nothing was recorded, so end it with a message.
            if let Some(s) = session.take() {
                s.flow.set(DictationState::Error);
            }
            DICTATING.store(false, Ordering::Relaxed);
            log::warn!(
                "Dictation microphone delivered no audio within {} ms",
                startup::NO_SOUND_AFTER.as_millis()
            );
            service.set_error(Some(startup::NO_SOUND_MESSAGE.to_string()));
            gesture.reset();
            flash(
                &service.overlay,
                OverlayState::Refused(startup::NO_SOUND_PILL),
            );
        }
    }

    if session.take().is_some() {
        DICTATING.store(false, Ordering::Relaxed);
        service.overlay.set(OverlayState::Hidden);
    }
}

/// A dictation's place in the insertion order. Dropping it (inserted,
/// failed or empty) lets the next dictation insert.
struct Turn {
    service: Arc<Service>,
    ticket: u64,
    flow: Flow,
}

/// A dictation never waits longer than this for the one before it.
const TURN_TIMEOUT: Duration = Duration::from_secs(30);

impl Turn {
    async fn wait(&self) {
        let mut turn = self.service.insert_turn.subscribe();
        let ticket = self.ticket;
        if tokio::time::timeout(TURN_TIMEOUT, turn.wait_for(|current| *current >= ticket))
            .await
            .is_err()
        {
            log::warn!(
                "Dictation: previous dictation still busy after {} s; inserting anyway",
                TURN_TIMEOUT.as_secs()
            );
        }
    }
}

impl Drop for Turn {
    fn drop(&mut self) {
        let next = match self.service.order.lock() {
            Ok(mut order) => order.finish(self.ticket),
            Err(poisoned) => poisoned.into_inner().finish(self.ticket),
        };
        self.service.insert_turn.send_if_modified(|current| {
            let advance = *current < next;
            if advance {
                *current = next;
            }
            advance
        });
    }
}

struct Transcript {
    text: String,
    engine: Engine,
    fallback_reason: Option<String>,
}

async fn transcribe_local(samples: Vec<f32>) -> Result<String, String> {
    super::warm::ensure_parakeet().await?;
    crate::parakeet_engine::commands::parakeet_transcribe_audio(samples).await
}

/// True when Parakeet can transcribe: the engine runs and a model is loaded or downloaded.
async fn local_engine_available() -> bool {
    let engine = crate::parakeet_engine::commands::PARAKEET_ENGINE
        .lock()
        .ok()
        .and_then(|engine| engine.as_ref().cloned());
    let Some(engine) = engine else {
        return false;
    };
    if engine.is_model_loaded().await {
        return true;
    }
    engine
        .discover_models()
        .await
        .map(|models| {
            models
                .iter()
                .any(|model| matches!(model.status, crate::parakeet_engine::ModelStatus::Available))
        })
        .unwrap_or(false)
}

const POLISH_PROMPT: &str = "Du retter dikteret dansk tekst. Ret grammatik og tegnsætning, \
fjern gentagelser og selvrettelser (behold den rettede udgave), og lav punktopstilling hvis \
teksten opremser flere ting. Bevar betydning, sprog og tone. Svar kun med den rettede tekst.";

impl Service {
    async fn transcribe_local(&self, samples: Vec<f32>) -> Result<String, String> {
        match &self.backend {
            Some(b) => b.local(samples).await,
            None => transcribe_local(samples).await,
        }
    }

    async fn transcribe(
        &self,
        settings: &DictationSettings,
        mode: Option<&Mode>,
        samples: &[f32],
        rules: &Rules,
    ) -> Result<Transcript, String> {
        let preference = EnginePreference::parse(&settings.engine);
        let key = match preference {
            EnginePreference::Auto => self
                .backend
                .as_ref()
                .map_or_else(secrets::syv_key, |b| b.syv_key()),
            EnginePreference::LocalOnly => None,
        };
        if mode.and_then(|m| m.provider.as_deref()) == Some("syv") && key.is_none() {
            return Err("Tilstanden kræver en Syv.ai-nøgle".into());
        }
        let available = match &self.backend {
            Some(b) => b.local_available().await,
            None => local_engine_available().await,
        };
        let plan = engine::plan(preference, key.is_some(), available).ok_or_else(|| {
            "Ingen transskription: sæt en Syv.ai-nøgle eller hent en lokal Parakeet-model"
                .to_string()
        })?;

        let Some(key) = key.filter(|_| plan.primary == Engine::Syv) else {
            if let Some(model) = mode.and_then(|m| m.local_model(plan.primary)) {
                crate::parakeet_engine::commands::parakeet_load_model(
                    self.app.clone(),
                    model.to_string(),
                )
                .await?;
            }
            return self
                .transcribe_local(samples.to_vec())
                .await
                .map(|text| Transcript {
                    text,
                    engine: Engine::Local,
                    fallback_reason: None,
                });
        };

        let prompt = format!(
            "{} {}",
            mode.map(Mode::prompt).unwrap_or_default(),
            cleanup::prompt_words(rules).join(", ")
        );
        let language = mode
            .and_then(|m| m.language.clone())
            .unwrap_or_else(|| settings.language.clone());
        let mut syv_task = tokio::spawn({
            let backend = self.backend.clone();
            let settings = settings.clone();
            let client = self.http.clone();
            let endpoint = settings.syv_endpoint.clone();
            let model = settings.syv_model.clone();
            let wav = audio::wav_bytes(samples, audio::TARGET_RATE);
            async move {
                match backend {
                    Some(b) => b.syv(&key, &settings, wav, prompt, language).await,
                    None => {
                        syv::transcribe_language(
                            &client,
                            &endpoint,
                            &key,
                            &model,
                            wav,
                            Some(prompt),
                            &language,
                        )
                        .await
                    }
                }
            }
        });
        let _abort_syv = flow::AbortTask(syv_task.abort_handle());

        let deadline = Duration::from_millis(settings.fallback_after_ms);
        let attempt = match tokio::time::timeout(deadline, &mut syv_task).await {
            Ok(Ok(Ok(text))) => Attempt::Text(text),
            Ok(Ok(Err(e))) => Attempt::Failed(e),
            Ok(Err(e)) => Attempt::Failed(e.to_string()),
            Err(_) => Attempt::Slow,
        };
        let reason = match &attempt {
            Attempt::Failed(e) => e.clone(),
            _ => format!(
                "Syv.ai svarede ikke inden for {} ms",
                settings.fallback_after_ms
            ),
        };

        let syv_result = |r: Result<Result<String, String>, tokio::task::JoinError>| match r {
            Ok(inner) => inner,
            Err(e) => Err(e.to_string()),
        };

        match engine::after_primary(attempt, &plan) {
            Next::Use(text) => Ok(Transcript {
                text,
                engine: Engine::Syv,
                fallback_reason: None,
            }),
            Next::Fail(e) => Err(e),
            Next::Fallback { keep_primary } => {
                log::info!("Dictation falling back to local model");
                if plan.fallback.is_none() {
                    // Slow but nothing else to try: keep waiting for Syv.ai.
                    return syv_result(syv_task.await).map(|text| Transcript {
                        text,
                        engine: Engine::Syv,
                        fallback_reason: None,
                    });
                }
                let local = self.transcribe_local(samples.to_vec());
                if !keep_primary {
                    return local.await.map(|text| Transcript {
                        text,
                        engine: Engine::Local,
                        fallback_reason: Some(reason),
                    });
                }
                // Race: whichever answers first wins.
                tokio::pin!(local);
                tokio::select! {
                    r = &mut syv_task => match syv_result(r) {
                        Ok(text) => Ok(Transcript { text, engine: Engine::Syv, fallback_reason: None }),
                        Err(e) => local.await.map(|text| Transcript {
                            text,
                            engine: Engine::Local,
                            fallback_reason: Some(e),
                        }),
                    },
                    r = &mut local => match r {
                        Ok(text) => {
                            syv_task.abort();
                            Ok(Transcript { text, engine: Engine::Local, fallback_reason: Some(reason) })
                        }
                        Err(_local_error) => {
                            log::warn!("Dictation local fallback failed");
                            syv_result(syv_task.await).map(|text| Transcript {
                                text,
                                engine: Engine::Syv,
                                fallback_reason: None,
                            })
                        }
                    },
                }
            }
        }
    }

    async fn polish(
        &self,
        settings: &DictationSettings,
        mode: Option<&Mode>,
        text: String,
    ) -> String {
        use crate::summary::llm_client::{generate_summary, LLMProvider};

        let provider = match settings.polish_provider.as_str() {
            "ollama" => LLMProvider::Ollama,
            _ => LLMProvider::BuiltInAI,
        };
        let endpoint = Some(settings.polish_endpoint.trim()).filter(|e| !e.is_empty());
        let budget = settings.polish_budget_ms;
        let start = Instant::now();
        let mut model = settings.polish_model.trim().to_string();
        if model.is_empty() && provider == LLMProvider::BuiltInAI {
            // Use the built-in model chosen for summaries.
            model = self.summary_builtin_model().await.unwrap_or_default();
        }
        if model.is_empty() {
            log::info!("Dictation polishing skipped: no model chosen");
            return text;
        }
        let prompt = format!(
            "{POLISH_PROMPT} {}",
            mode.map(Mode::prompt).unwrap_or_default()
        );
        let prompt = if let Some(language) = mode.and_then(|m| m.language.as_deref()) {
            format!("{prompt} Tekstens sprog er {language}; bevar dette sprog.")
        } else {
            prompt
        };
        let prompt =
            super::logic::modes::polish_prompt(&prompt, &cleanup::english_terms(&self.rules()));
        let request = generate_summary(
            &self.http,
            &provider,
            &model,
            "",
            &prompt,
            &text,
            endpoint,
            None,
            Some(512),
            Some(0.2),
            None,
            Some(&self.app_data_dir),
            None,
        );
        let polished = match tokio::time::timeout(Duration::from_millis(budget), request).await {
            Ok(Ok(completion)) => Some(completion.content),
            Ok(Err(e)) => {
                log::info!("Dictation polishing failed, using layer 1 text: {e}");
                None
            }
            Err(_) => {
                log::info!("Dictation polishing slower than {budget} ms, using layer 1 text");
                None
            }
        };
        engine::choose_polished(
            &text,
            polished.as_deref(),
            start.elapsed().as_millis() as u64,
            budget,
        )
    }

    async fn summary_builtin_model(&self) -> Option<String> {
        use crate::database::repositories::setting::SettingsRepository;
        let db = self.app.database()?;
        let config = SettingsRepository::get_model_config(db.pool())
            .await
            .ok()??;
        (config.provider == "builtin-ai").then_some(config.model)
    }

    async fn process(
        self: Arc<Self>,
        samples: Vec<f32>,
        target_app: Option<String>,
        target_window: usize,
        start_focus: Option<Focus>,
        selection: (DictationSettings, Option<Mode>),
        released_at: Instant,
        start_ms: Option<u64>,
        turn: &Turn,
    ) {
        turn.flow.set(DictationState::Processing);
        let (base_settings, mut mode) = selection;
        let mut settings = base_settings.clone();
        if let Some(mode) = &mode {
            settings.apply_mode(mode);
        }
        let capture_ms = released_at.elapsed().as_millis() as u64;
        let audio_ms = audio::duration_ms(samples.len(), audio::TARGET_RATE);
        if audio_ms < 300 || !audio::has_signal(&samples) {
            log::info!("Dictation: no speech in {audio_ms} ms recording");
            self.overlay.set(OverlayState::Hidden);
            return;
        }

        let legacy_style = app_style::style_for(target_app.as_deref(), &settings.style_rules());
        let code_editor = app_style::is_code_editor(target_app.as_deref());
        let rules = self.rules();
        let transcribe_started = Instant::now();
        let mut transcript = match self
            .transcribe(&settings, mode.as_ref(), &samples, &rules)
            .await
        {
            Ok(t) => t,
            Err(e) => {
                turn.flow.set(DictationState::Error);
                log::warn!("Dictation transcription failed");
                self.set_error(Some(e));
                flash_error(&self.overlay);
                return;
            }
        };

        if legacy_style != Style::Raw {
            if let Some((spoken, rest)) = settings
                .modes
                .spoken(&transcript.text, target_app.as_deref())
            {
                let spoken = spoken.clone();
                let rest = rest.to_string();
                let previous = settings.clone();
                settings = base_settings.clone();
                settings.apply_mode(&spoken);
                let needs_transcription = previous.engine != settings.engine
                    || previous.syv_model != settings.syv_model
                    || spoken.transcription_changed(mode.as_ref());
                mode = Some(spoken);
                if needs_transcription {
                    transcript = match self
                        .transcribe(&settings, mode.as_ref(), &samples, &rules)
                        .await
                    {
                        Ok(t) => t,
                        Err(e) => {
                            turn.flow.set(DictationState::Error);
                            self.set_error(Some(e));
                            flash_error(&self.overlay);
                            return;
                        }
                    };
                    if let Some((spoken, rest)) = settings
                        .modes
                        .spoken(&transcript.text, target_app.as_deref())
                    {
                        if mode.as_ref().is_some_and(|m| m.id == spoken.id) {
                            transcript.text = rest.to_string();
                        }
                    }
                } else {
                    transcript.text = rest;
                }
                self.overlay.set_mode(mode.as_ref());
                let _ = self.app.emit("dictation-mode-changed", &mode);
            }
        }
        let style = if code_editor || legacy_style == Style::Raw {
            Style::Raw
        } else {
            mode.as_ref()
                .map(|m| m.style.cleanup())
                .unwrap_or(legacy_style)
        };

        let transcribe_ms = transcribe_started.elapsed().as_millis() as u64;

        let (input, enter) = mode
            .as_ref()
            .map(|m| {
                m.output
                    .prepare(&transcript.text, code_editor || style == Style::Raw)
            })
            .unwrap_or_else(|| (transcript.text.clone(), false));
        if let Ok(mut slot) = self.active_mode.lock() {
            *slot = mode.as_ref().map(|m| m.name.clone());
        }
        let cleaned = cleanup::clean_with(
            &input,
            &cleanup_options(style, &rules, &settings, mode.as_ref()),
        );
        if let Some(command @ (VoiceCommand::DeleteThat | VoiceCommand::SelectThat)) =
            cleaned.command
        {
            self.overlay.set(OverlayState::Hidden);
            turn.wait().await;
            self.run_edit_command(command).await;
            return;
        }
        let mut text = cleaned.text;
        let mut polish_ms = None;
        if settings.polish_enabled && style == Style::Normal && !text.is_empty() {
            let polish_started = Instant::now();
            text = self.polish(&settings, mode.as_ref(), text).await;
            polish_ms = Some(polish_started.elapsed().as_millis() as u64);
        }
        if settings.text_hook_enabled && !text.trim().is_empty() {
            // Fail-open: on any failure `text` comes back unchanged.
            let dir = super::text_hook::hook_dir(&self.app_data_dir);
            text = super::text_hook::apply(&dir, &settings.text_hook_script, text).await;
        }
        self.overlay.set(OverlayState::Hidden);
        if text.trim().is_empty() {
            log::info!("Dictation: transcript empty after cleanup");
            return;
        }

        turn.wait().await;
        let insert_started = Instant::now();
        let options = InsertOptions {
            restore_after: Duration::from_millis(settings.restore_clipboard_ms),
            paste_timeout: Duration::from_millis(settings.paste_timeout_ms),
            app: target_app.clone(),
        };
        turn.flow.set(DictationState::Inserting);
        let inserted = match &self.backend {
            Some(b) => b
                .insert(text.clone())
                .await
                .map(|()| ("fake", Instant::now(), None)),
            None => {
                insert(
                    text.clone(),
                    options,
                    enter,
                    target_window,
                    start_focus.as_ref().map(|f| f.id.clone()),
                )
                .await
            }
        };
        let mut refusal = None;
        let (insert_method, inserted_at) = match inserted {
            Ok((method, at, field)) => {
                self.set_error(None);
                if enter {
                    self.forget_insertion();
                } else {
                    self.remember_insertion(&text, field, at);
                }
                (Some(method), at)
            }
            Err(e) => {
                turn.flow.set(DictationState::Error);
                log::warn!("Dictation insert failed");
                refusal = Refusal::from_message(&e);
                if let Some(r) = refusal {
                    self.set_error(Some(r.message().to_string()));
                    flash(&self.overlay, OverlayState::Refused(r.pill_text()));
                } else {
                    self.set_error(Some(format!("Teksten kunne ikke indsættes: {e}")));
                }
                (None, Instant::now())
            }
        };
        let latency_ms = inserted_at
            .saturating_duration_since(released_at)
            .as_millis() as i64;
        let steps = Steps {
            start_ms,
            audio_ms,
            capture_ms,
            transcribe_ms,
            polish_ms,
            insert_ms: inserted_at
                .saturating_duration_since(insert_started)
                .as_millis() as u64,
            total_ms: latency_ms as u64,
        };
        log::info!(
            "Dictation steps: start {} ms, capture {capture_ms} ms, transcribe {transcribe_ms} ms, polish {} ms, insert {} ms",
            start_ms.map(|ms| ms.to_string()).unwrap_or_else(|| "-".to_string()),
            polish_ms.map(|ms| ms.to_string()).unwrap_or_else(|| "-".to_string()),
            steps.insert_ms,
        );
        self.record_timing(NewTiming {
            created_at: now_ms(),
            provider: transcript.engine.as_str(),
            fallback: transcript.fallback_reason.is_some(),
            inserted: insert_method.is_some(),
            steps,
        })
        .await;
        log::info!(
            "Dictation: {latency_ms} ms from release to text (engine {}, {audio_ms} ms audio, {} chars, app {}, method {})",
            transcript.engine.as_str(),
            text.chars().count(),
            target_app.as_deref().unwrap_or("?"),
            insert_method.unwrap_or("none"),
        );

        // Text dictated into a password field is never kept, not even in the
        // history, whatever made the insertion fail: the field is judged from
        // when dictation started as well as from the insertion.
        let from_password = start_focus
            .as_ref()
            .is_some_and(|f| f.probe.password == Some(true));
        if settings.history_enabled
            && !from_password
            && !refusal.is_some_and(Refusal::keeps_text_out_of_history)
        {
            self.save_history(
                &settings,
                &samples,
                text,
                target_app,
                style,
                transcript,
                latency_ms,
                audio_ms,
                insert_method,
            )
            .await;
        }
    }

    fn remember_insertion(&self, text: &str, field: Option<FieldId>, at: Instant) {
        if let Ok(mut slot) = self.last_insertion.lock() {
            *slot = field.map(|field| LastInsertion {
                text: text.to_string(),
                field,
                at,
                selected: false,
            });
        }
    }

    fn forget_insertion(&self) {
        if let Ok(mut slot) = self.last_insertion.lock() {
            *slot = None;
        }
    }

    /// "slet det" / "vælg det": only while the field of the previous
    /// insertion still has focus.
    async fn run_edit_command(&self, command: VoiceCommand) {
        let current = tokio::task::spawn_blocking(focused_field)
            .await
            .ok()
            .flatten();
        let last = self.last_insertion.lock().ok().and_then(|l| l.clone());
        let Some(plan) =
            spoken::plan_edit(command, last.as_ref(), current.as_ref(), Instant::now())
        else {
            log::info!("Dictation: {command:?} ignored, the field changed, too old or nothing was inserted");
            return;
        };
        match apply_edit(plan).await {
            Ok(()) => {
                if command == VoiceCommand::DeleteThat {
                    self.forget_insertion();
                } else if let Ok(mut slot) = self.last_insertion.lock() {
                    if let Some(last) = slot.as_mut() {
                        last.selected = true;
                    }
                }
            }
            Err(e) => {
                log::warn!("Dictation: {command:?} failed: {e}");
                self.set_error(Some(e));
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn save_history(
        &self,
        settings: &DictationSettings,
        samples: &[f32],
        text: String,
        app: Option<String>,
        style: Style,
        transcript: Transcript,
        latency_ms: i64,
        audio_ms: u64,
        insert_method: Option<&'static str>,
    ) {
        let created_at = now_ms();
        let audio_path = if settings.keep_audio {
            let path = self.audio_dir.join(format!("{created_at}.wav"));
            let audio_dir = self.audio_dir.clone();
            let output = path.clone();
            let samples = samples.to_vec();
            #[allow(clippy::disallowed_methods)]
            // File encoding and writes run on the blocking pool.
            let written = tokio::task::spawn_blocking(move || {
                std::fs::create_dir_all(audio_dir)?;
                std::fs::write(output, audio::wav_bytes(&samples, audio::TARGET_RATE))
            })
            .await
            .map_err(std::io::Error::other)
            .and_then(|result| result);
            match written {
                Ok(()) => Some(path.to_string_lossy().into_owned()),
                Err(e) => {
                    log::warn!("Dictation: could not save recording: {e}");
                    None
                }
            }
        } else {
            None
        };

        let entry = NewHistoryEntry {
            created_at,
            text,
            app,
            style: style.as_str().to_string(),
            engine: transcript.engine.as_str().to_string(),
            fallback_reason: transcript.fallback_reason,
            latency_ms,
            audio_ms: audio_ms as i64,
            insert_method: insert_method.map(str::to_string),
            audio_path,
        };
        match self.store.add_history(entry).await {
            Ok(trimmed) => self.delete_files(trimmed),
            Err(e) => log::warn!("Dictation: could not save history: {e}"),
        }
        self.expire_audio().await;
        self.notify_history_changed();
    }
}

#[cfg(windows)]
use super::win::InsertOptions;

/// Insertion settings (the Windows type, mirrored for other platforms).
#[cfg(not(windows))]
struct InsertOptions {
    restore_after: Duration,
    paste_timeout: Duration,
    app: Option<String>,
}

#[cfg(windows)]
async fn insert(
    text: String,
    options: InsertOptions,
    enter: bool,
    target_window: usize,
    expected: Option<FieldId>,
) -> Result<(&'static str, Instant, Option<FieldId>), String> {
    tokio::task::spawn_blocking(move || {
        super::win::insert_output_at(&text, &options, enter, target_window, expected.as_ref())
    })
    .await
    .map_err(|e| e.to_string())?
    .map(|(method, at, field)| (method.as_str(), at, field))
}

#[cfg(not(windows))]
async fn insert(
    _text: String,
    options: InsertOptions,
    _enter: bool,
    _target_window: usize,
    _expected: Option<FieldId>,
) -> Result<(&'static str, Instant, Option<FieldId>), String> {
    let _ = (options.restore_after, options.paste_timeout, options.app);
    Err("Indsættelse understøttes kun på Windows".to_string())
}

#[cfg(windows)]
async fn apply_edit(plan: spoken::EditPlan) -> Result<(), String> {
    tokio::task::spawn_blocking(move || super::target_win::apply_edit(plan))
        .await
        .map_err(|e| e.to_string())?
}

#[cfg(not(windows))]
async fn apply_edit(_plan: spoken::EditPlan) -> Result<(), String> {
    Err("Stemmekommandoer understøttes kun på Windows".to_string())
}

#[cfg(windows)]
fn focus_now() -> Option<Focus> {
    super::target_win::focus()
}
#[cfg(not(windows))]
fn focus_now() -> Option<Focus> {
    None
}

#[cfg(windows)]
fn focused_field() -> Option<FieldId> {
    super::target_win::focused_field()
}
#[cfg(not(windows))]
fn focused_field() -> Option<FieldId> {
    None
}

fn cleanup_options<'a>(
    style: Style,
    rules: &'a Rules,
    settings: &DictationSettings,
    mode: Option<&Mode>,
) -> Options<'a> {
    Options {
        style,
        rules,
        format_numbers: style == Style::Normal
            && settings.format_numbers
            && mode.and_then(|m| m.formatting.number_format.as_deref()) != Some("ordret"),
        format_addresses: style == Style::Normal && settings.format_addresses,
        collapse_stutters: style == Style::Normal && settings.collapse_stutters,
        enter_triggers: &[],
    }
}

#[cfg(test)]
#[allow(clippy::disallowed_methods)]
mod tests {
    use super::*;

    #[test]
    fn service_raw_cleanup_preserves_numbers_and_addresses() {
        let settings = DictationSettings::default();
        let rules = Rules::default();
        let options = cleanup_options(Style::Raw, &rules, &settings, None);
        assert!(!options.format_numbers);
        assert!(!options.format_addresses);
        assert_eq!(
            cleanup::clean_with("git checkout tretten", &options).text,
            "git checkout tretten"
        );
        assert_eq!(
            cleanup::clean_with("anna snabel-a eksempel punktum dk", &options).text,
            cleanup::clean_with(
                "anna snabel-a eksempel punktum dk",
                &Options::new(Style::Raw, &rules)
            )
            .text
        );
    }

    #[test]
    fn service_normal_cleanup_respects_formatting_settings() {
        let mut settings = DictationSettings::default();
        let rules = Rules::default();
        let options = cleanup_options(Style::Normal, &rules, &settings, None);
        assert!(options.format_numbers);
        assert!(options.format_addresses);
        assert_eq!(cleanup::clean_with("tretten", &options).text, "13");
        let mut mode = settings.modes.modes[0].clone();
        mode.formatting.number_format = Some("ordret".into());
        let options = cleanup_options(Style::Normal, &rules, &settings, Some(&mode));
        assert!(!options.format_numbers);
        assert!(options.format_addresses);
        settings.format_numbers = false;
        settings.format_addresses = false;
        let options = cleanup_options(Style::Normal, &rules, &settings, None);
        assert!(!options.format_numbers);
        assert!(!options.format_addresses);
    }
}
