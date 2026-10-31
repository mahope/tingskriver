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
use super::logic::cleanup::{self, Rules, Style};
use super::logic::engine::{self, Attempt, Engine, EnginePreference, Next};
use super::logic::gesture::{Gesture, GestureAction, GestureConfig, GestureEvent};
use super::logic::hotkey::ChordSignal;
use super::logic::order::Order;
use super::logic::timing::{self, Steps};
use super::overlay::{Overlay, OverlayState};
use super::secrets;
use super::settings::DictationSettings;
use super::store::{NewHistoryEntry, Store};
use super::syv;
use super::timings::{LatencyReport, LatencyWarning, NewTiming, Timings, LATENCY_WARNING_EVENT};

/// Emitted to the main window when the history list changes.
pub const HISTORY_CHANGED_EVENT: &str = "dictation-history-changed";

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
    #[cfg(windows)]
    hook: Mutex<Option<super::win::KeyboardHook>>,
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
    let app_data_dir = app.app_data_dir()?;
    let store = Store::open(&app_data_dir.join("dictation.sqlite")).await?;
    let settings = store.settings().await?;
    let rules = store.rules().await?;
    let timings = Timings::open(store.pool()).await?;
    super::warm::set_policy(settings.residency_policy());
    let service = Arc::new(Service {
        app: app.clone(),
        overlay,
        store,
        timings,
        latency_warned: Mutex::new(BTreeSet::new()),
        settings: RwLock::new(settings),
        rules: RwLock::new(rules),
        http: reqwest::Client::new(),
        audio_dir: app_data_dir.join("dictation_audio"),
        app_data_dir,
        shortcut_active: AtomicBool::new(false),
        next_ticket: AtomicU64::new(0),
        order: Mutex::new(Order::default()),
        insert_turn: tokio::sync::watch::channel(0).0,
        last_error: Mutex::new(None),
        #[cfg(windows)]
        hook: Mutex::new(None),
    });
    SERVICE
        .set(service.clone())
        .map_err(|_| "dictation already initialised".to_string())?;

    service.apply_shortcut();
    // Recordings must be gone 24 h after they were made, also when no new
    // dictation happens in between, so check every ten minutes.
    let mut interval = tokio::time::interval(Duration::from_secs(10 * 60));
    loop {
        interval.tick().await;
        service.expire_audio().await;
    }
}

impl Service {
    pub fn settings(&self) -> DictationSettings {
        self.settings.read().map(|s| s.clone()).unwrap_or_default()
    }

    fn rules(&self) -> Rules {
        self.rules.read().map(|r| r.clone()).unwrap_or_default()
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

    pub async fn save_settings(self: &Arc<Self>, settings: DictationSettings) -> Result<DictationSettings, String> {
        let settings = settings.normalized()?;
        self.store.save_settings(&settings).await?;
        let previous = self.settings();
        if let Ok(mut slot) = self.settings.write() {
            *slot = settings.clone();
        }
        if previous.residency_policy() != settings.residency_policy() {
            super::warm::set_policy(settings.residency_policy());
        }
        if previous.enabled != settings.enabled || previous.shortcut != settings.shortcut {
            self.apply_shortcut();
        }
        Ok(settings)
    }

    pub async fn reload_rules(&self) -> Result<(), String> {
        let rules = self.store.rules().await?;
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
            Ok(mut warned) => timing::new_warnings(&stats, &mut warned).into_iter().map(Into::into).collect(),
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
        match super::win::KeyboardHook::start(chord, tx) {
            Ok(hook) => {
                *slot = Some(hook);
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

    #[cfg(not(windows))]
    fn apply_shortcut(self: &Arc<Self>) {
        log::info!("Dictation shortcut is only available on Windows");
        self.shortcut_active.store(false, Ordering::Relaxed);
    }
}

struct Session {
    recorder: Recorder,
    target_app: Option<String>,
    /// Shortcut pressed until the microphone was open.
    start_ms: u64,
}

fn gesture_event(signal: ChordSignal) -> GestureEvent {
    match signal {
        ChordSignal::Pressed => GestureEvent::Pressed,
        ChordSignal::Released => GestureEvent::Released,
        ChordSignal::Interrupted => GestureEvent::Interrupted,
    }
}

#[cfg(windows)]
fn foreground_process() -> Option<String> {
    super::win::foreground_process()
}

#[cfg(not(windows))]
fn foreground_process() -> Option<String> {
    None
}

fn flash_error(overlay: &Arc<dyn Overlay>) {
    overlay.set(OverlayState::Error);
    let overlay = overlay.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(1500));
        if !is_dictating() {
            overlay.set(OverlayState::Hidden);
        }
    });
}

/// Runs on its own thread for as long as the keyboard hook is installed.
fn control_loop(service: Arc<Service>, rx: Receiver<(ChordSignal, Instant)>) {
    let epoch = Instant::now();
    let ms = |t: Instant| t.saturating_duration_since(epoch).as_millis() as u64;
    let mut gesture = Gesture::new(GestureConfig::default());
    let mut session: Option<Session> = None;

    loop {
        let (actions, at) = match rx.recv_timeout(Duration::from_millis(50)) {
            Ok((signal, at)) => (gesture.on_event(gesture_event(signal), ms(at)), at),
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
                    let target_app = foreground_process();
                    // Load the local model while the user speaks, so neither
                    // the local engine nor the fallback pays for a load.
                    super::warm::prewarm_parakeet();
                    match Recorder::start() {
                        Ok(recorder) => {
                            DICTATING.store(true, Ordering::Relaxed);
                            let start_ms = at.elapsed().as_millis() as u64;
                            session = Some(Session {
                                recorder,
                                target_app,
                                start_ms,
                            });
                        }
                        Err(e) => {
                            log::warn!("Dictation microphone failed: {e}");
                            service.set_error(Some(e));
                            gesture.reset();
                            flash_error(&service.overlay);
                        }
                    }
                }
                GestureAction::Lock => {}
                GestureAction::Cancel => {
                    if session.take().is_some() {
                        DICTATING.store(false, Ordering::Relaxed);
                        service.overlay.set(OverlayState::Hidden);
                    }
                }
                GestureAction::Stop => {
                    if let Some(s) = session.take() {
                        let samples = s.recorder.stop();
                        DICTATING.store(false, Ordering::Relaxed);
                        service.overlay.set(OverlayState::Processing);
                        let service = service.clone();
                        let target_app = s.target_app;
                        let start_ms = s.start_ms;
                        let ticket = service.next_ticket.fetch_add(1, Ordering::SeqCst);
                        crate::runtime::spawn(async move {
                            let turn = Turn {
                                service: service.clone(),
                                ticket,
                            };
                            service.process(samples, target_app, at, start_ms, &turn).await;
                        });
                    }
                }
            }
        }

        if let Some(s) = &session {
            service.overlay.set(OverlayState::Recording {
                level: s.recorder.level(),
                locked: gesture.is_locked(),
            });
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
            log::warn!("Dictation: previous dictation still busy after {} s; inserting anyway", TURN_TIMEOUT.as_secs());
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
    async fn transcribe(&self, settings: &DictationSettings, samples: &[f32], rules: &Rules) -> Result<Transcript, String> {
        let preference = EnginePreference::parse(&settings.engine);
        let key = match preference {
            EnginePreference::Auto => secrets::syv_key(),
            EnginePreference::LocalOnly => None,
        };
        let plan = engine::plan(preference, key.is_some(), local_engine_available().await)
            .ok_or_else(|| "Ingen transskription: sæt en Syv.ai-nøgle eller hent en lokal Parakeet-model".to_string())?;

        let Some(key) = key.filter(|_| plan.primary == Engine::Syv) else {
            return transcribe_local(samples.to_vec()).await.map(|text| Transcript {
                text,
                engine: Engine::Local,
                fallback_reason: None,
            });
        };

        let prompt = cleanup::prompt_words(rules).join(", ");
        let mut syv_task = tokio::spawn({
            let client = self.http.clone();
            let endpoint = settings.syv_endpoint.clone();
            let model = settings.syv_model.clone();
            let wav = audio::wav_bytes(samples, audio::TARGET_RATE);
            async move { syv::transcribe(&client, &endpoint, &key, &model, wav, Some(prompt)).await }
        });

        let deadline = Duration::from_millis(settings.fallback_after_ms);
        let attempt = match tokio::time::timeout(deadline, &mut syv_task).await {
            Ok(Ok(Ok(text))) => Attempt::Text(text),
            Ok(Ok(Err(e))) => Attempt::Failed(e),
            Ok(Err(e)) => Attempt::Failed(e.to_string()),
            Err(_) => Attempt::Slow,
        };
        let reason = match &attempt {
            Attempt::Failed(e) => e.clone(),
            _ => format!("Syv.ai svarede ikke inden for {} ms", settings.fallback_after_ms),
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
                log::info!("Dictation falling back to local model: {reason}");
                if plan.fallback.is_none() {
                    // Slow but nothing else to try: keep waiting for Syv.ai.
                    return syv_result(syv_task.await).map(|text| Transcript {
                        text,
                        engine: Engine::Syv,
                        fallback_reason: None,
                    });
                }
                let local = transcribe_local(samples.to_vec());
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
                        Err(local_error) => {
                            log::warn!("Dictation local fallback failed: {local_error}");
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

    async fn polish(&self, settings: &DictationSettings, text: String) -> String {
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
        let request = generate_summary(
            &self.http,
            &provider,
            &model,
            "",
            POLISH_PROMPT,
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
        engine::choose_polished(&text, polished.as_deref(), start.elapsed().as_millis() as u64, budget)
    }

    async fn summary_builtin_model(&self) -> Option<String> {
        use crate::database::repositories::setting::SettingsRepository;
        let db = self.app.database()?;
        let config = SettingsRepository::get_model_config(db.pool()).await.ok()??;
        (config.provider == "builtin-ai").then_some(config.model)
    }

    async fn process(
        self: Arc<Self>,
        samples: Vec<f32>,
        target_app: Option<String>,
        released_at: Instant,
        start_ms: u64,
        turn: &Turn,
    ) {
        let capture_ms = released_at.elapsed().as_millis() as u64;
        let settings = self.settings();
        let audio_ms = audio::duration_ms(samples.len(), audio::TARGET_RATE);
        if audio_ms < 300 || !audio::has_signal(&samples) {
            log::info!("Dictation: no speech in {audio_ms} ms recording");
            self.overlay.set(OverlayState::Hidden);
            return;
        }

        let style = app_style::style_for(target_app.as_deref(), &settings.style_rules());
        let rules = self.rules();
        let transcribe_started = Instant::now();
        let transcript = match self.transcribe(&settings, &samples, &rules).await {
            Ok(t) => t,
            Err(e) => {
                log::warn!("Dictation transcription failed: {e}");
                self.set_error(Some(e));
                flash_error(&self.overlay);
                return;
            }
        };

        let transcribe_ms = transcribe_started.elapsed().as_millis() as u64;

        let mut text = cleanup::clean(&transcript.text, style, &rules);
        let mut polish_ms = None;
        if settings.polish_enabled && style == Style::Normal && !text.is_empty() {
            let polish_started = Instant::now();
            text = self.polish(&settings, text).await;
            polish_ms = Some(polish_started.elapsed().as_millis() as u64);
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
        let inserted = insert(text.clone(), options).await;
        let (insert_method, inserted_at) = match inserted {
            Ok((method, at)) => {
                self.set_error(None);
                (Some(method), at)
            }
            Err(e) => {
                log::warn!("Dictation insert failed: {e}");
                self.set_error(Some(format!("Teksten kunne ikke indsættes: {e}")));
                (None, Instant::now())
            }
        };
        let latency_ms = inserted_at.saturating_duration_since(released_at).as_millis() as i64;
        let steps = Steps {
            start_ms: Some(start_ms),
            audio_ms,
            capture_ms,
            transcribe_ms,
            polish_ms,
            insert_ms: inserted_at.saturating_duration_since(insert_started).as_millis() as u64,
            total_ms: latency_ms as u64,
        };
        log::info!(
            "Dictation steps: start {start_ms} ms, capture {capture_ms} ms, transcribe {transcribe_ms} ms, polish {} ms, insert {} ms",
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

        if settings.history_enabled {
            self.save_history(&settings, &samples, text, target_app, style, transcript, latency_ms, audio_ms, insert_method)
                .await;
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
            let written = std::fs::create_dir_all(&self.audio_dir)
                .and_then(|_| std::fs::write(&path, audio::wav_bytes(samples, audio::TARGET_RATE)));
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
async fn insert(text: String, options: InsertOptions) -> Result<(&'static str, Instant), String> {
    tokio::task::spawn_blocking(move || super::win::insert_text(&text, &options))
        .await
        .map_err(|e| e.to_string())?
        .map(|(method, at)| (method.as_str(), at))
}

#[cfg(not(windows))]
async fn insert(_text: String, options: InsertOptions) -> Result<(&'static str, Instant), String> {
    let _ = (options.restore_after, options.paste_timeout, options.app);
    Err("Indsættelse understøttes kun på Windows".to_string())
}
