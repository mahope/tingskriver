//! Keeps the speech models loaded between calls.
//!
//! Meetings no longer unload their model when they stop, so a dictation right
//! after a meeting starts without a model load. This keeper unloads a model
//! after the idle time in the dictation settings (default 10 min), unless the
//! model is marked "keep warm", or earlier under memory pressure. Decisions
//! are in `logic::residency`.

use std::sync::{OnceLock, RwLock};
use std::time::{Duration, Instant};

use super::logic::residency::{self, Decision, Policy, Snapshot};

/// How often loaded models are checked.
const CHECK_EVERY: Duration = Duration::from_secs(30);

static POLICY: RwLock<Option<Policy>> = RwLock::new(None);
static STARTED: OnceLock<()> = OnceLock::new();

pub fn set_policy(policy: Policy) {
    log::info!(
        "Model keeper: unload after {} idle, keep warm: [{}]",
        policy
            .idle_timeout
            .map(|t| format!("{} min", t.as_secs() / 60))
            .unwrap_or_else(|| "never".to_string()),
        policy.keep_warm.join(", ")
    );
    if let Ok(mut slot) = POLICY.write() {
        *slot = Some(policy);
    }
}

fn policy() -> Policy {
    POLICY
        .read()
        .ok()
        .and_then(|p| p.clone())
        .unwrap_or_default()
}

/// Start the background check. Safe to call more than once.
pub fn start() {
    if STARTED.set(()).is_err() {
        return;
    }
    crate::runtime::spawn(async {
        let mut interval = tokio::time::interval(CHECK_EVERY);
        loop {
            interval.tick().await;
            check_once().await;
        }
    });
}

fn parakeet() -> Option<std::sync::Arc<crate::parakeet_engine::ParakeetEngine>> {
    crate::parakeet_engine::commands::PARAKEET_ENGINE
        .lock()
        .ok()
        .and_then(|engine| engine.as_ref().cloned())
}

fn whisper() -> Option<std::sync::Arc<crate::whisper_engine::WhisperEngine>> {
    crate::whisper_engine::commands::WHISPER_ENGINE
        .lock()
        .ok()
        .and_then(|engine| engine.as_ref().cloned())
}

fn memory_pressure() -> bool {
    let mut system = sysinfo::System::new();
    system.refresh_memory();
    residency::memory_pressure(system.available_memory(), system.total_memory())
}

/// Resident memory of this process in MB, for the before/after log line.
pub fn process_memory_mb() -> Option<u64> {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate};
    let pid = sysinfo::get_current_pid().ok()?;
    let mut system = sysinfo::System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[pid]),
        true,
        ProcessRefreshKind::new().with_memory(),
    );
    system.process(pid).map(|p| p.memory() / (1024 * 1024))
}

fn mb(value: Option<u64>) -> String {
    value
        .map(|v| format!("{v} MB"))
        .unwrap_or_else(|| "?".to_string())
}

async fn check_once() {
    // Same lock as recording start/stop and batch unloads, so a model is
    // never unloaded while one of them is loading it.
    let _lifecycle = crate::audio::common::acquire_engine_lifecycle_lock().await;
    let policy = policy();
    let recording = crate::audio::recording_commands::is_recording().await;
    let pressure = memory_pressure();
    let now = Instant::now();

    if let Some(engine) = parakeet() {
        if let (Some((last_used, busy)), Some(model)) =
            (engine.usage().await, engine.get_current_model().await)
        {
            let snapshot = Snapshot {
                model: &model,
                idle_for: now.saturating_duration_since(last_used),
                busy,
                recording,
                memory_pressure: pressure,
            };
            if let Decision::Unload(reason) = residency::decide(&policy, &snapshot) {
                let before = process_memory_mb();
                engine.unload_model().await;
                log::info!(
                    "Model keeper: unloaded Parakeet model '{model}' ({}, idle {} s), process memory {} -> {}",
                    reason.as_str(),
                    snapshot.idle_for.as_secs(),
                    mb(before),
                    mb(process_memory_mb())
                );
            }
        }
    }

    if let Some(engine) = whisper() {
        if let (Some((last_used, busy)), Some(model)) =
            (engine.usage().await, engine.get_current_model().await)
        {
            let snapshot = Snapshot {
                model: &model,
                idle_for: now.saturating_duration_since(last_used),
                busy,
                recording,
                memory_pressure: pressure,
            };
            if let Decision::Unload(reason) = residency::decide(&policy, &snapshot) {
                let before = process_memory_mb();
                engine.unload_model().await;
                log::info!(
                    "Model keeper: unloaded Whisper model '{model}' ({}, idle {} s), process memory {} -> {}",
                    reason.as_str(),
                    snapshot.idle_for.as_secs(),
                    mb(before),
                    mb(process_memory_mb())
                );
            }
        }
    }
}

/// How the local dictation model was made ready.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Readiness {
    /// Already loaded: no model load.
    Warm(String),
    /// Loaded now, taking `ms`.
    Loaded { model: String, ms: u64 },
}

/// Make sure a Parakeet model is loaded. Logs whether a load was needed, which
/// is the marker for "inference right after a meeting starts without a load".
pub async fn ensure_parakeet() -> Result<Readiness, String> {
    let engine = parakeet().ok_or_else(|| "Parakeet engine not initialized".to_string())?;
    if engine.is_model_loaded().await {
        if let Some(model) = engine.get_current_model().await {
            // Restart the idle clock, so the keeper does not unload the model
            // while the user speaks or between this check and transcription.
            engine.mark_used().await;
            log::info!("Dictation: local model '{model}' already loaded (model_load=none)");
            return Ok(Readiness::Warm(model));
        }
    }
    let started = Instant::now();
    let model = crate::parakeet_engine::commands::parakeet_validate_model_ready().await?;
    let ms = started.elapsed().as_millis() as u64;
    log::info!("Dictation: loaded local model '{model}' in {ms} ms (model_load={ms}ms)");
    Ok(Readiness::Loaded { model, ms })
}

/// Start loading the local model in the background, e.g. when the dictation
/// shortcut is pressed, so the load overlaps with the user speaking.
pub fn prewarm_parakeet() {
    crate::runtime::spawn(async {
        if let Err(e) = ensure_parakeet().await {
            log::debug!("Dictation: local model not prewarmed: {e}");
        }
    });
}
