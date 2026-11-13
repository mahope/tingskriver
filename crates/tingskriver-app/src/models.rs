//! Model management under Indstillinger > Transskription (#174).
//!
//! The logic is core's: the engines in `whisper_engine::commands` and
//! `parakeet_engine::commands` list, download (SHA-256 checked, resumable,
//! swapped in only when verified), cancel and delete models, and the choice is
//! saved with `SettingsRepository::save_transcript_config` like Tauri's
//! `api_save_transcript_config`. This file keeps what the screen shows:
//!
//! * [`State`] is plain data and has every transition (progress events,
//!   cancel, errors, the saved choice), so tests need neither network, audio
//!   devices nor real models;
//! * [`Models`] is the GPUI entity that owns the state and runs the core
//!   calls on the tokio runtime.
//!
//! Nothing is fetched by opening the tab: reading the list only looks at the
//! models folder. A download starts when the user presses "Hent".

use std::collections::HashMap;
use std::path::PathBuf;

use gpui::{App, AppContext, Context, Entity, Global};
use tingskriver_core::database::repositories::setting::SettingsRepository;
use tingskriver_core::host::HostHandle;
use tingskriver_core::strings::models as text;
use tingskriver_core::{model_download, parakeet_engine, whisper_engine};
use tokio::runtime::Handle;
use tokio::sync::mpsc::UnboundedReceiver;

/// The two local engines Tauri offers in `TranscriptSettings.tsx`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Engine {
    Parakeet,
    Whisper,
}

impl Engine {
    pub(crate) const ALL: [Engine; 2] = [Engine::Parakeet, Engine::Whisper];

    /// The provider id Tauri saves in `transcript_settings`.
    pub(crate) fn provider(self) -> &'static str {
        match self {
            Engine::Parakeet => "parakeet",
            Engine::Whisper => "localWhisper",
        }
    }

    pub(crate) fn from_provider(id: &str) -> Option<Engine> {
        Engine::ALL.into_iter().find(|e| e.provider() == id)
    }

    /// The engine name in `model_manifest.json`.
    fn manifest(self) -> &'static str {
        match self {
            Engine::Parakeet => "parakeet",
            Engine::Whisper => "whisper",
        }
    }

    /// The model Tauri marks "Anbefalet".
    fn recommended(self) -> &'static str {
        match self {
            Engine::Parakeet => "parakeet-tdt-0.6b-v3-int8",
            Engine::Whisper => "base",
        }
    }

    /// The event names of this engine's downloads.
    fn events(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Engine::Parakeet => (
                "parakeet-model-download-progress",
                "parakeet-model-download-complete",
                "parakeet-model-download-error",
            ),
            Engine::Whisper => (
                "model-download-progress",
                "model-download-complete",
                "model-download-error",
            ),
        }
    }
}

pub(crate) type Key = (Engine, String);

/// What the engine says about the files on disk.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Base {
    Ready,
    Missing,
    Corrupted,
    Failed(String),
}

/// One model as the engine lists it.
#[derive(Debug, Clone)]
pub(crate) struct Discovered {
    pub engine: Engine,
    pub id: String,
    pub size_mb: u32,
    pub description: String,
    pub path: PathBuf,
    pub base: Base,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) struct Progress {
    pub percent: u8,
    pub downloaded_mb: f64,
    pub total_mb: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Transient {
    Downloading(Progress),
    Cancelling,
}

/// What a card shows.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Status {
    Missing,
    Downloading(Progress),
    Cancelling,
    Ready,
    Corrupted,
    Failed(String),
}

/// The checksum state. Only `Passed` may be shown as verified: it is set by a
/// finished download (core checks the SHA-256 before the file is swapped in)
/// or by [`verify`], never assumed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Check {
    Unknown,
    Checking,
    Passed,
    Mismatch,
}

/// Metadata from the pinned manifest. `None` is shown as unknown.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Meta {
    pub license: Option<String>,
    pub revision: Option<String>,
    pub source: Option<String>,
    /// Exact bytes of all files, when the manifest has the model.
    pub total_bytes: Option<u64>,
    /// The manifest has hashes to check against.
    pub verifiable: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct Row {
    pub found: Discovered,
    pub meta: Meta,
    transient: Option<Transient>,
    /// The last failure of this model, shown until the next attempt.
    pub error: Option<String>,
    pub check: Check,
}

impl Row {
    pub(crate) fn key(&self) -> Key {
        (self.found.engine, self.found.id.clone())
    }

    pub(crate) fn status(&self) -> Status {
        match (self.transient, &self.found.base) {
            (Some(Transient::Downloading(p)), _) => Status::Downloading(p),
            (Some(Transient::Cancelling), _) => Status::Cancelling,
            (None, Base::Ready) => Status::Ready,
            (None, Base::Corrupted) => Status::Corrupted,
            (None, Base::Failed(why)) => Status::Failed(why.clone()),
            (None, Base::Missing) => match &self.error {
                Some(why) => Status::Failed(why.clone()),
                None => Status::Missing,
            },
        }
    }

    pub(crate) fn name(&self) -> String {
        display_name(&self.found)
    }

    pub(crate) fn tagline(&self) -> String {
        tagline(&self.found)
    }

    pub(crate) fn recommended(&self) -> bool {
        self.found.id == self.found.engine.recommended()
    }

    pub(crate) fn size_mb(&self) -> f64 {
        match self.meta.total_bytes {
            Some(bytes) => bytes as f64 / (1024.0 * 1024.0),
            None => f64::from(self.found.size_mb),
        }
    }
}

/// Tauri's friendly names (`lib/parakeet.ts:MODEL_DISPLAY_CONFIG`); Whisper
/// shows the model name.
fn display_name(found: &Discovered) -> String {
    match found.id.as_str() {
        "parakeet-tdt-0.6b-v3-int8" => text::MODEL_PARAKEET_FAST.into(),
        "parakeet-tdt-0.6b-v2-int8" => text::MODEL_PARAKEET_COMPACT.into(),
        "parakeet-tdt-0.6b-v3-fp32" => text::MODEL_PARAKEET_PRECISE.into(),
        other => other.to_string(),
    }
}

fn tagline(found: &Discovered) -> String {
    match found.id.as_str() {
        "parakeet-tdt-0.6b-v3-int8" => text::MODEL_PARAKEET_FAST_TAGLINE.into(),
        "parakeet-tdt-0.6b-v2-int8" => text::MODEL_PARAKEET_COMPACT_TAGLINE.into(),
        "parakeet-tdt-0.6b-v3-fp32" => text::MODEL_PARAKEET_PRECISE_TAGLINE.into(),
        id if found.engine == Engine::Whisper => {
            let badge = if id.ends_with("-q5_1") {
                text::MODEL_WHISPER_Q5_1
            } else if id.ends_with("-q5_0") {
                text::MODEL_WHISPER_Q5_0
            } else if id.ends_with("-q4_0") {
                text::MODEL_WHISPER_Q4_0
            } else {
                text::MODEL_WHISPER_F16
            };
            format!("{} • {}", found.description, badge)
        }
        _ => found.description.clone(),
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) enum Load {
    #[default]
    Idle,
    Loading,
    Ready,
    Failed(String),
}

/// Why a model cannot be deleted right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Busy {
    Recording,
    Dictation,
    Transcribing,
}

impl Busy {
    pub(crate) fn explanation(self) -> &'static str {
        match self {
            Busy::Recording => text::MODEL_BUSY_RECORDING,
            Busy::Dictation => text::MODEL_BUSY_DICTATION,
            Busy::Transcribing => text::MODEL_BUSY_TRANSCRIBING,
        }
    }
}

/// Whether something uses the models right now. A recording or its final
/// transcription, a running dictation and a running transcription job all
/// block a delete.
pub(crate) fn busy_reason(
    phase: tingskriver_host::recorder::Phase,
    dictating: bool,
    job_running: bool,
) -> Option<Busy> {
    use tingskriver_host::recorder::Phase;
    match phase {
        Phase::Idle => {}
        Phase::Finishing => return Some(Busy::Transcribing),
        _ => return Some(Busy::Recording),
    }
    if dictating {
        Some(Busy::Dictation)
    } else if job_running {
        Some(Busy::Transcribing)
    } else {
        None
    }
}

pub(crate) struct State {
    pub load: Load,
    pub rows: Vec<Row>,
    /// The saved choice (`transcript_settings`): engine and model.
    pub selected: Option<Key>,
    /// The saved choice could not be read; never shown as the default.
    pub selected_unreadable: bool,
    /// The saved provider is one this tab does not offer (Tauri can save
    /// Deepgram, ElevenLabs, Groq, OpenAI). Shown, never hidden as "none".
    pub saved_other_provider: Option<String>,
    /// The engine whose models are listed (Tauri's `uiProvider`).
    pub view: Engine,
    /// A message that belongs to no card (a choice that was not saved).
    pub notice: Option<String>,
}

impl Default for State {
    fn default() -> Self {
        State {
            load: Load::Idle,
            rows: Vec::new(),
            selected: None,
            selected_unreadable: false,
            saved_other_provider: None,
            // Tauri's default engine.
            view: Engine::Parakeet,
            notice: None,
        }
    }
}

impl State {
    pub(crate) fn row(&self, key: &Key) -> Option<&Row> {
        self.rows
            .iter()
            .find(|r| r.found.engine == key.0 && r.found.id == key.1)
    }

    fn row_mut(&mut self, key: &Key) -> Option<&mut Row> {
        self.rows
            .iter_mut()
            .find(|r| r.found.engine == key.0 && r.found.id == key.1)
    }

    pub(crate) fn rows_of(&self, engine: Engine) -> impl Iterator<Item = &Row> {
        self.rows.iter().filter(move |r| r.found.engine == engine)
    }

    /// At least one model is on disk and usable.
    pub(crate) fn any_ready(&self) -> bool {
        self.rows.iter().any(|r| r.status() == Status::Ready)
    }

    /// The selected model, when it is installed and usable.
    #[cfg(test)]
    pub(crate) fn selected_row(&self) -> Option<&Row> {
        self.selected
            .as_ref()
            .and_then(|key| self.row(key))
            .filter(|r| r.status() == Status::Ready)
    }

    /// Take a fresh listing from the engines. Downloads that are running, the
    /// last failure and the checksum result of a model are kept.
    pub(crate) fn set_discovered(&mut self, found: Vec<Discovered>) {
        let mut old: HashMap<Key, Row> = self.rows.drain(..).map(|r| (r.key(), r)).collect();
        let mut rows: Vec<Row> = found
            .into_iter()
            .map(|found| {
                let key = (found.engine, found.id.clone());
                let meta = meta_of(&found);
                match old.remove(&key) {
                    Some(previous) => {
                        // A file that changed on disk is not the one that
                        // was checked.
                        let same = previous.found.base == found.base;
                        Row {
                            found,
                            meta,
                            transient: previous.transient,
                            error: previous.error,
                            check: if same { previous.check } else { Check::Unknown },
                        }
                    }
                    None => Row {
                        found,
                        meta,
                        transient: None,
                        error: None,
                        check: Check::Unknown,
                    },
                }
            })
            .collect();
        // Parakeet first; the recommended model leads its engine.
        rows.sort_by_key(|r| {
            (
                r.found.engine != Engine::Parakeet,
                r.found.id != r.found.engine.recommended(),
            )
        });
        self.rows = rows;
        self.load = Load::Ready;
        // The saved choice may name a model this build no longer lists;
        // the card list simply has no selected card then.
    }

    /// Reserve a download. `false` when the model is unknown or busy.
    pub(crate) fn begin_download(&mut self, key: &Key) -> bool {
        let Some(row) = self.row_mut(key) else {
            return false;
        };
        if row.transient.is_some() || row.found.base == Base::Ready {
            return false;
        }
        row.transient = Some(Transient::Downloading(Progress::default()));
        row.error = None;
        true
    }

    /// The download could not start or ended with an error. An installed
    /// model is not touched (core swaps a file in only after it is verified).
    pub(crate) fn fail(&mut self, key: &Key, message: String) {
        if let Some(row) = self.row_mut(key) {
            row.transient = None;
            row.error = Some(message);
        }
    }

    /// "Annullér" was pressed. `false` unless a download is running.
    pub(crate) fn begin_cancel(&mut self, key: &Key) -> bool {
        match self.row_mut(key) {
            Some(row) if matches!(row.transient, Some(Transient::Downloading(_))) => {
                row.transient = Some(Transient::Cancelling);
                true
            }
            _ => false,
        }
    }

    /// Core finished cancelling: back to the state on disk.
    pub(crate) fn cancelled(&mut self, key: &Key) {
        if let Some(row) = self.row_mut(key) {
            row.transient = None;
        }
    }

    /// The toast Tauri shows after a cancel, as a line under the cards.
    pub(crate) fn announce_cancelled(&mut self, key: &Key) {
        if let Some(name) = self.row(key).map(Row::name) {
            self.notice = Some(text::model_cancelled(&name));
        }
    }

    /// The cleanup after a cancel has not finished yet: stay in
    /// "Annullerer…" and say why.
    pub(crate) fn cancel_pending(&mut self, key: &Key) {
        if let Some(row) = self.row_mut(key) {
            // The download call may already have ended the state; then
            // there is nothing left to wait for.
            if row.transient.is_some() {
                row.error = Some(text::MODEL_CANCEL_PENDING.to_string());
            }
        }
    }

    pub(crate) fn set_check(&mut self, key: &Key, check: Check) {
        if let Some(row) = self.row_mut(key) {
            row.check = check;
        }
    }

    /// Apply an event core emitted. Returns whether anything visible changed
    /// (progress is throttled to whole percents).
    pub(crate) fn apply_event(&mut self, name: &str, payload_json: &str) -> bool {
        let Some(event) = parse_event(name, payload_json) else {
            return false;
        };
        match event {
            Event::Progress { key, progress } => {
                let Some(row) = self.row_mut(&key) else {
                    return false;
                };
                match row.transient {
                    // A late progress event after "Annullér" must not bring
                    // the bar back.
                    Some(Transient::Cancelling) => false,
                    Some(Transient::Downloading(old)) if old.percent == progress.percent => false,
                    _ => {
                        row.transient = Some(Transient::Downloading(progress));
                        row.error = None;
                        true
                    }
                }
            }
            Event::Cancelled(key) => {
                let Some(row) = self.row_mut(&key) else {
                    return false;
                };
                row.transient = None;
                self.announce_cancelled(&key);
                true
            }
            Event::Complete(key) => {
                let Some(row) = self.row_mut(&key) else {
                    return false;
                };
                row.transient = None;
                row.error = None;
                row.found.base = Base::Ready;
                // Core checked the SHA-256 of every file before it swapped
                // them in.
                row.check = if row.meta.verifiable {
                    Check::Passed
                } else {
                    Check::Unknown
                };
                true
            }
            Event::Failed { key, message } => {
                let size = self.row(&key).map(|r| r.size_mb());
                let Some(row) = self.row_mut(&key) else {
                    return false;
                };
                row.transient = None;
                row.error = Some(explain_error(&message, size.unwrap_or(0.0)));
                true
            }
        }
    }
}

fn meta_of(found: &Discovered) -> Meta {
    match model_download::entry(found.engine.manifest(), &found.id) {
        Some(entry) => Meta {
            license: Some(entry.license.clone()).filter(|v| !v.is_empty()),
            revision: Some(entry.revision.clone()).filter(|v| !v.is_empty()),
            source: entry.sources.first().map(|s| s.name.clone()),
            total_bytes: Some(entry.total_bytes()).filter(|b| *b > 0),
            verifiable: entry.files.iter().all(|f| !f.sha256.is_empty()),
        },
        None => Meta::default(),
    }
}

enum Event {
    Progress { key: Key, progress: Progress },
    Complete(Key),
    Cancelled(Key),
    Failed { key: Key, message: String },
}

fn parse_event(name: &str, payload_json: &str) -> Option<Event> {
    let engine = Engine::ALL.into_iter().find(|e| {
        let (progress, complete, error) = e.events();
        name == progress || name == complete || name == error
    })?;
    let (progress_name, complete_name, error_name) = engine.events();
    let value: serde_json::Value = serde_json::from_str(payload_json).ok()?;
    let id = value.get("modelName")?.as_str()?.to_string();
    let key = (engine, id);
    if name == complete_name {
        return Some(Event::Complete(key));
    }
    if name == error_name {
        let message = value
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        return Some(Event::Failed { key, message });
    }
    debug_assert_eq!(name, progress_name);
    if value.get("status").and_then(|v| v.as_str()) == Some("cancelled") {
        return Some(Event::Cancelled(key));
    }
    let percent = value.get("progress").and_then(|v| v.as_f64())?;
    let number = |field: &str| value.get(field).and_then(|v| v.as_f64()).unwrap_or(0.0);
    Some(Event::Progress {
        key,
        progress: Progress {
            percent: percent.clamp(0.0, 100.0).round() as u8,
            downloaded_mb: number("downloaded_mb"),
            total_mb: number("total_mb"),
        },
    })
}

/// Turn a core error into something the user can act on: full disk, lost
/// network or a stalled source, a failed checksum. Core's own Danish texts
/// (checksum) pass through; an unknown error gets Tauri's default text.
pub(crate) fn explain_error(raw: &str, size_mb: f64) -> String {
    let lower = raw.to_lowercase();
    let size = text::model_size(size_mb);
    if lower.contains("no space")
        || lower.contains("os error 28")
        || lower.contains("os error 112")
        || lower.contains("disk full")
        || lower.contains("not enough space")
        || lower.contains("quota")
    {
        return if size_mb > 0.0 {
            text::model_err_disk(&size)
        } else {
            text::MODEL_ERR_DISK_UNKNOWN.to_string()
        };
    }
    if lower.contains("kontrolsum") || lower.contains("sha-256") {
        // Core's text, without the command's English prefix.
        let start = raw.find("Modelfilen").unwrap_or(0);
        return raw[start..].to_string();
    }
    if lower.contains("not initialized") {
        return text::MODEL_ERR_NOT_READY.to_string();
    }
    if lower.contains("no response")
        || lower.contains("within")
        || lower.contains("timed out")
        || lower.contains("timeout")
        || lower.contains("failed to start download")
        || lower.contains("kunne ikke hentes")
        || lower.contains("error sending request")
        || lower.contains("connection")
        || lower.contains("could not connect")
        || lower.contains("dns error")
        || lower.contains("dns lookup")
        || lower.contains("network")
        || lower.contains("stalled")
    {
        return text::MODEL_ERR_NETWORK.to_string();
    }
    text::MODEL_DOWNLOAD_FAILED_GENERIC.to_string()
}

/// Is there room for the download? `Err(needed)` when the free space is
/// known and smaller than the model plus a margin. Unknown free space does
/// not block: the error mapping covers a disk that fills up later.
pub(crate) fn enough_space(free: Option<u64>, needed_bytes: u64) -> Result<(), u64> {
    let needed = needed_bytes + needed_bytes / 20;
    match free {
        Some(free) if free < needed => Err(needed),
        _ => Ok(()),
    }
}

// ---------------------------------------------------------------------
// Core calls.

fn from_parakeet(info: parakeet_engine::ModelInfo) -> Discovered {
    use parakeet_engine::ModelStatus as S;
    Discovered {
        engine: Engine::Parakeet,
        id: info.name,
        size_mb: info.size_mb,
        description: info.description,
        path: info.path,
        base: match info.status {
            S::Available => Base::Ready,
            // Our own state tracks a running download.
            S::Missing | S::Downloading { .. } => Base::Missing,
            S::Corrupted { .. } => Base::Corrupted,
            S::Error(why) => Base::Failed(explain_error(&why, 0.0)),
        },
    }
}

fn from_whisper(info: whisper_engine::ModelInfo) -> Discovered {
    use whisper_engine::ModelStatus as S;
    Discovered {
        engine: Engine::Whisper,
        id: info.name,
        size_mb: info.size_mb,
        description: info.description,
        path: info.path,
        base: match info.status {
            S::Available => Base::Ready,
            S::Missing | S::Downloading { .. } => Base::Missing,
            S::Corrupted { .. } => Base::Corrupted,
            S::Error(why) => Base::Failed(explain_error(&why, 0.0)),
        },
    }
}

/// Point both engines at `<app data>/models`, start them and list what is on
/// disk. Local only: no request leaves the computer.
async fn discover(host: &HostHandle) -> Result<Vec<Discovered>, String> {
    let for_dirs = host.clone();
    tokio::task::spawn_blocking(move || {
        parakeet_engine::commands::set_models_directory(&for_dirs);
        whisper_engine::commands::set_models_directory(&for_dirs);
    })
    .await
    .map_err(|e| e.to_string())?;
    parakeet_engine::commands::parakeet_init().await?;
    whisper_engine::commands::whisper_init().await?;
    let mut found: Vec<Discovered> = parakeet_engine::commands::parakeet_get_available_models()
        .await?
        .into_iter()
        .map(from_parakeet)
        .collect();
    found.extend(
        whisper_engine::commands::whisper_get_available_models()
            .await?
            .into_iter()
            .map(from_whisper),
    );
    Ok(found)
}

/// The saved choice: a local model this tab offers, or the name of another
/// saved provider.
type Saved = (Option<Key>, Option<String>);

async fn read_saved(host: &HostHandle) -> Result<Saved, String> {
    let Some(db) = host.database() else {
        // The database is not open yet (first start): nothing is saved.
        return Ok((None, None));
    };
    let saved = SettingsRepository::get_transcript_config(db.pool())
        .await
        .map_err(|e| e.to_string())?;
    Ok(match saved {
        None => (None, None),
        Some(s) => match Engine::from_provider(&s.provider) {
            Some(engine) => (Some((engine, s.model)), None),
            None => (None, Some(s.provider)),
        },
    })
}

async fn start_download(host: HostHandle, key: Key) -> Result<(), String> {
    match key.0 {
        Engine::Parakeet => parakeet_engine::commands::parakeet_download_model(host, key.1).await,
        Engine::Whisper => whisper_engine::commands::whisper_download_model(host, key.1).await,
    }
}

async fn cancel_download(key: Key) -> Result<bool, String> {
    use parakeet_engine::parakeet_engine::CancelDownloadOutcome as Outcome;
    let outcome = match key.0 {
        Engine::Parakeet => parakeet_engine::commands::parakeet_cancel_download(key.1).await?,
        Engine::Whisper => {
            // Same enum type name in both engines.
            match whisper_engine::commands::whisper_cancel_download(key.1).await? {
                whisper_engine::CancelDownloadOutcome::Cancelled => Outcome::Cancelled,
                whisper_engine::CancelDownloadOutcome::Pending => Outcome::Pending,
            }
        }
    };
    Ok(outcome == Outcome::Cancelled)
}

/// Core's `*_delete_corrupted_model` removes the model's folder or file
/// whatever its state (it is the only delete command; the name is from the
/// first use, a corrupt file), so it also deletes healthy models.
async fn delete_model(key: Key) -> Result<(), String> {
    match key.0 {
        Engine::Parakeet => parakeet_engine::commands::parakeet_delete_corrupted_model(key.1).await,
        Engine::Whisper => whisper_engine::commands::whisper_delete_corrupted_model(key.1).await,
    }
    .map(|_| ())
}

/// Check every file of an installed model against the manifest (the engines
/// do the same before they load a model).
async fn verify(found: Discovered) -> Check {
    let dir = match found.engine {
        Engine::Parakeet => found.path.clone(),
        Engine::Whisper => match found.path.parent() {
            Some(parent) => parent.to_path_buf(),
            None => return Check::Unknown,
        },
    };
    match model_download::verify_installed_model(&dir, found.engine.manifest(), &found.id).await {
        Some(true) => Check::Passed,
        Some(false) => Check::Mismatch,
        None => Check::Unknown,
    }
}

// ---------------------------------------------------------------------
// The GPUI entity.

pub(crate) struct Models {
    pub state: State,
    host: Option<HostHandle>,
    runtime: Handle,
}

struct ModelsGlobal(Entity<Models>);

impl Global for ModelsGlobal {}

/// The model manager of this app, if `start` has run.
pub(crate) fn global(cx: &App) -> Option<Entity<Models>> {
    cx.try_global::<ModelsGlobal>().map(|g| g.0.clone())
}

/// Create the manager and route core's download events to it. `host` is
/// `None` without an application data folder.
pub(crate) fn start(
    runtime: Handle,
    host: Option<(
        HostHandle,
        UnboundedReceiver<tingskriver_host::bridge::Event>,
    )>,
    cx: &mut App,
) {
    let (host, events) = match host {
        Some((host, events)) => (Some(host), Some(events)),
        None => (None, None),
    };
    let models = cx.new(|_| Models {
        state: State::default(),
        host,
        runtime,
    });
    // The tab draws from this entity; redraw when it changes.
    cx.observe(&models, |_, cx| cx.refresh_windows()).detach();
    if let Some(mut events) = events {
        let weak = models.downgrade();
        cx.spawn(async move |cx| {
            while let Some(event) = events.recv().await {
                let Some(models) = weak.upgrade() else { break };
                models.update(cx, |models, cx| {
                    if models.state.apply_event(&event.name, &event.payload_json) {
                        cx.notify();
                    }
                });
            }
        })
        .detach();
    }
    cx.set_global(ModelsGlobal(models));
}

/// The states of the screenshot fixtures (docs/gpui-screenshots.md).
#[cfg(feature = "screenshot-tests")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelsFixture {
    /// Nothing fetched: every card offers "Hent".
    Empty,
    /// Two models on disk, one chosen and checked.
    Installed,
    /// One model downloading at 42 %.
    Downloading,
    /// The download was cancelled: the card offers "Hent" again.
    Cancelled,
    /// The download failed on a full disk; an installed model is untouched.
    Failed,
}

/// A manager with fixed state for screenshot fixtures: no host, no core calls.
#[cfg(feature = "screenshot-tests")]
pub(crate) fn set_fixture(kind: ModelsFixture, runtime: Handle, cx: &mut App) {
    let models = cx.new(|_| Models {
        state: fixture_state(kind),
        host: None,
        runtime,
    });
    cx.set_global(ModelsGlobal(models));
}

#[cfg(feature = "screenshot-tests")]
fn fixture_state(kind: ModelsFixture) -> State {
    const FAST: &str = "parakeet-tdt-0.6b-v3-int8";
    const COMPACT: &str = "parakeet-tdt-0.6b-v2-int8";
    let model = |engine, id: &str, size_mb, description: &str, base| Discovered {
        engine,
        id: id.to_string(),
        size_mb,
        description: description.to_string(),
        path: PathBuf::from("fixtures/models").join(id),
        base,
    };
    let installed = matches!(kind, ModelsFixture::Installed | ModelsFixture::Failed);
    let base = |ready| if ready { Base::Ready } else { Base::Missing };
    let mut state = State::default();
    state.set_discovered(vec![
        model(
            Engine::Parakeet,
            FAST,
            670,
            "Realtid på M4 Max, optimeret til hastighed",
            base(installed),
        ),
        model(
            Engine::Parakeet,
            COMPACT,
            661,
            "25 gange realtid, mindre størrelse og god nøjagtighed",
            base(kind == ModelsFixture::Installed),
        ),
        model(
            Engine::Whisper,
            "base",
            142,
            "Good balance of speed and accuracy",
            Base::Missing,
        ),
    ]);
    let fast = (Engine::Parakeet, FAST.to_string());
    let compact = (Engine::Parakeet, COMPACT.to_string());
    match kind {
        ModelsFixture::Empty => {}
        ModelsFixture::Installed => {
            state.selected = Some(fast.clone());
            state.set_check(&fast, Check::Passed);
        }
        ModelsFixture::Downloading => {
            state.begin_download(&compact);
            state.apply_event(
                "parakeet-model-download-progress",
                &format!(
                    r#"{{"modelName":"{COMPACT}","progress":42,"downloaded_mb":277.0,"total_mb":661.0,"status":"downloading"}}"#
                ),
            );
        }
        ModelsFixture::Cancelled => {
            state.begin_download(&compact);
            state.begin_cancel(&compact);
            state.cancelled(&compact);
            state.announce_cancelled(&compact);
        }
        ModelsFixture::Failed => {
            state.selected = Some(fast.clone());
            state.begin_download(&compact);
            state.apply_event(
                "parakeet-model-download-error",
                &format!(
                    r#"{{"modelName":"{COMPACT}","error":"Failed to write x: No space left on device (os error 28)"}}"#
                ),
            );
        }
    }
    state
}

impl Models {
    pub(crate) fn needs_load(&self) -> bool {
        self.state.load == Load::Idle
    }

    /// Read the models folder and the saved choice, once the tab is drawn.
    /// Never downloads.
    pub(crate) fn load(&mut self, cx: &mut Context<Self>) {
        let Some(host) = self.host.clone() else {
            self.state.load = Load::Failed(text::MODELS_NO_HOST.to_string());
            return;
        };
        self.state.load = Load::Loading;
        let task = self.runtime.spawn(async move {
            let found = discover(&host).await?;
            let saved = read_saved(&host).await;
            Ok::<_, String>((found, saved))
        });
        cx.spawn(async move |this, cx| {
            let result = task.await.map_err(|e| e.to_string()).and_then(|r| r);
            this.update(cx, |this, cx| {
                match result {
                    Ok((found, saved)) => {
                        this.state.set_discovered(found);
                        match saved {
                            Ok((saved, other)) => {
                                this.state.selected = saved;
                                this.state.saved_other_provider = other;
                                this.state.selected_unreadable = false;
                            }
                            Err(error) => {
                                log::warn!("Could not read the transcription setting: {error}");
                                this.state.selected_unreadable = true;
                            }
                        }
                        // Show the engine of the saved choice, like Tauri.
                        if let Some((engine, _)) = &this.state.selected {
                            this.state.view = *engine;
                        }
                    }
                    Err(error) => {
                        log::warn!("Could not list the models: {error}");
                        this.state.load = Load::Failed(error);
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Read the list again (after a delete, cancel or finished download).
    fn relist(&mut self, cx: &mut Context<Self>) {
        let Some(host) = self.host.clone() else {
            return;
        };
        let task = self.runtime.spawn(async move { discover(&host).await });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(found)) = task.await {
                this.update(cx, |this, cx| {
                    this.state.set_discovered(found);
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    pub(crate) fn set_view(&mut self, engine: Engine, cx: &mut Context<Self>) {
        if self.state.view != engine {
            self.state.view = engine;
            cx.notify();
        }
    }

    /// "Hent", "Prøv igen" and "Hent igen": the only way a download starts.
    pub(crate) fn download(&mut self, key: Key, cx: &mut Context<Self>) {
        let Some(host) = self.host.clone() else {
            return;
        };
        let Some(row) = self.state.row(&key) else {
            return;
        };
        let needed = row
            .meta
            .total_bytes
            .unwrap_or(u64::from(row.found.size_mb) * 1024 * 1024);
        let size_mb = row.size_mb();
        if !self.state.begin_download(&key) {
            return;
        }
        cx.notify();
        let runtime = self.runtime.clone();
        let task = runtime.spawn({
            let key = key.clone();
            async move {
                // Room for the model before anything is fetched.
                if let Ok(dir) = host.app_data_dir() {
                    let dir = dir.join("models");
                    let free = tokio::task::spawn_blocking(move || {
                        tingskriver_core::database::init::available_space(&dir)
                    })
                    .await
                    .ok()
                    .flatten();
                    if enough_space(free, needed).is_err() {
                        return Err(text::model_err_disk(&text::model_size(size_mb)));
                    }
                }
                start_download(host, key)
                    .await
                    .map_err(|e| explain_error(&e, size_mb))
            }
        });
        cx.spawn(async move |this, cx| {
            let result = task.await.map_err(|e| e.to_string()).and_then(|r| r);
            this.update(cx, |this, cx| {
                // Success, cancel and error also arrive as events; this is
                // the end of the call, so settle whatever is still open and
                // read the disk again.
                match result {
                    Ok(()) => this.state.cancelled(&key),
                    Err(message) => this.state.fail(&key, message),
                }
                this.relist(cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn cancel(&mut self, key: Key, cx: &mut Context<Self>) {
        if !self.state.begin_cancel(&key) {
            return;
        }
        cx.notify();
        let task = self.runtime.spawn(cancel_download(key.clone()));
        cx.spawn(async move |this, cx| {
            let result = task.await.map_err(|e| e.to_string()).and_then(|r| r);
            this.update(cx, |this, cx| {
                match result {
                    Ok(true) => {
                        this.state.cancelled(&key);
                        this.state.announce_cancelled(&key);
                    }
                    // Stays "Annullerer…"; the download call ends the state.
                    Ok(false) => this.state.cancel_pending(&key),
                    Err(error) => {
                        log::warn!("Could not cancel {}: {error}", key.1);
                        this.state.cancelled(&key);
                        this.state.fail(&key, text::MODEL_CANCEL_FAILED.to_string());
                    }
                }
                this.relist(cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Delete after the user confirmed. The caller passes what uses the
    /// models right now, read at the confirmation ([`Busy`]); a delete is
    /// refused with the explanation when that is something.
    pub(crate) fn delete(&mut self, key: Key, busy: Option<Busy>, cx: &mut Context<Self>) {
        if let Some(busy) = busy {
            // Re-checked at the confirmation, not only when the button was drawn.
            self.state.notice = Some(busy.explanation().to_string());
            cx.notify();
            return;
        }
        let name = self.state.row(&key).map(Row::name).unwrap_or_default();
        let task = self.runtime.spawn(delete_model(key.clone()));
        cx.spawn(async move |this, cx| {
            let result = task.await.map_err(|e| e.to_string()).and_then(|r| r);
            this.update(cx, |this, cx| {
                match result {
                    Ok(()) => {
                        this.state.set_check(&key, Check::Unknown);
                        this.state.notice = Some(text::model_deleted(&name));
                    }
                    Err(error) => {
                        log::warn!("Could not delete {}: {error}", key.1);
                        this.state.notice = Some(text::MODEL_DELETE_FAILED.to_string());
                    }
                }
                this.relist(cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Choose an installed model for transcription and save it like Tauri
    /// (`api_save_transcript_config`). The old choice stays if saving fails.
    pub(crate) fn select(&mut self, key: Key, cx: &mut Context<Self>) {
        if self.state.row(&key).map(Row::status) != Some(Status::Ready) {
            return;
        }
        let Some(db) = self.host.as_ref().and_then(|h| h.database()) else {
            self.state.notice = Some(text::MODEL_SELECT_NO_DATABASE.to_string());
            cx.notify();
            return;
        };
        self.state.notice = None;
        let pool = db.pool().clone();
        let saved = key.clone();
        let task = self.runtime.spawn(async move {
            SettingsRepository::save_transcript_config(&pool, saved.0.provider(), &saved.1)
                .await
                .map_err(|e| e.to_string())
        });
        cx.spawn(async move |this, cx| {
            let result = task.await.map_err(|e| e.to_string()).and_then(|r| r);
            this.update(cx, |this, cx| {
                match result {
                    Ok(()) => {
                        this.state.selected = Some(key);
                        this.state.saved_other_provider = None;
                        this.state.selected_unreadable = false;
                    }
                    Err(error) => {
                        log::warn!("Could not save the transcription model: {error}");
                        this.state.notice = Some(text::MODEL_SELECT_SAVE_FAILED.to_string());
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// "Kontrollér": check the installed files against the manifest.
    pub(crate) fn verify(&mut self, key: Key, cx: &mut Context<Self>) {
        let Some(row) = self.state.row(&key) else {
            return;
        };
        if row.found.base != Base::Ready || row.check == Check::Checking {
            return;
        }
        let found = row.found.clone();
        self.state.set_check(&key, Check::Checking);
        cx.notify();
        let task = self.runtime.spawn(verify(found));
        cx.spawn(async move |this, cx| {
            let check = task.await.unwrap_or(Check::Unknown);
            this.update(cx, |this, cx| {
                this.state.set_check(&key, check);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(engine: Engine, id: &str, base: Base) -> Discovered {
        Discovered {
            engine,
            id: id.to_string(),
            size_mb: 100,
            description: "Beskrivelse".into(),
            path: PathBuf::from("/models").join(id),
            base,
        }
    }

    fn state() -> State {
        let mut state = State::default();
        state.set_discovered(vec![
            found(Engine::Whisper, "tiny", Base::Missing),
            found(Engine::Whisper, "base", Base::Ready),
            found(Engine::Parakeet, "parakeet-tdt-0.6b-v2-int8", Base::Missing),
            found(Engine::Parakeet, "parakeet-tdt-0.6b-v3-int8", Base::Missing),
        ]);
        state
    }

    fn parakeet(id: &str) -> Key {
        (Engine::Parakeet, id.to_string())
    }

    const FAST: &str = "parakeet-tdt-0.6b-v3-int8";

    fn progress(id: &str, percent: u32) -> String {
        format!(
            r#"{{"modelName":"{id}","progress":{percent},"downloaded_mb":{percent},"total_mb":100.0,"speed_mbps":5.0,"status":"downloading"}}"#
        )
    }

    #[test]
    fn a_model_in_use_cannot_be_deleted_and_the_reason_is_named() {
        use tingskriver_host::recorder::Phase;
        assert_eq!(busy_reason(Phase::Idle, false, false), None);
        assert_eq!(
            busy_reason(Phase::Recording, false, false),
            Some(Busy::Recording)
        );
        assert_eq!(
            busy_reason(Phase::Paused, false, false),
            Some(Busy::Recording)
        );
        assert_eq!(
            busy_reason(Phase::Starting, false, false),
            Some(Busy::Recording)
        );
        assert_eq!(
            busy_reason(Phase::Finishing, false, false),
            Some(Busy::Transcribing)
        );
        assert_eq!(busy_reason(Phase::Idle, true, false), Some(Busy::Dictation));
        assert_eq!(
            busy_reason(Phase::Idle, false, true),
            Some(Busy::Transcribing)
        );
        // A recording wins over a job: say what the user can stop.
        assert_eq!(
            busy_reason(Phase::Recording, true, true),
            Some(Busy::Recording)
        );
        for busy in [Busy::Recording, Busy::Dictation, Busy::Transcribing] {
            assert!(busy.explanation().starts_with("Kan ikke slettes"));
        }
    }

    #[test]
    fn nothing_is_downloading_after_listing() {
        let state = state();
        assert!(state
            .rows
            .iter()
            .all(|r| !matches!(r.status(), Status::Downloading(_))));
        assert_eq!(state.load, Load::Ready);
    }

    #[test]
    fn recommended_model_leads_its_engine_and_parakeet_comes_first() {
        let state = state();
        let ids: Vec<_> = state.rows.iter().map(|r| r.found.id.as_str()).collect();
        assert_eq!(ids, [FAST, "parakeet-tdt-0.6b-v2-int8", "base", "tiny"]);
        assert!(state.rows[0].recommended());
        assert!(!state.rows[1].recommended());
    }

    #[test]
    fn providers_match_tauri() {
        assert_eq!(Engine::Parakeet.provider(), "parakeet");
        assert_eq!(Engine::Whisper.provider(), "localWhisper");
        assert_eq!(Engine::from_provider("localWhisper"), Some(Engine::Whisper));
        assert_eq!(Engine::from_provider("deepgram"), None);
    }

    #[test]
    fn progress_events_move_the_bar_and_are_throttled_to_whole_percents() {
        let mut state = state();
        let key = parakeet(FAST);
        assert!(state.begin_download(&key));
        assert!(state.apply_event("parakeet-model-download-progress", &progress(FAST, 40)));
        // The same percent again is not a visible change.
        assert!(!state.apply_event("parakeet-model-download-progress", &progress(FAST, 40)));
        match state.row(&key).unwrap().status() {
            Status::Downloading(p) => {
                assert_eq!(p.percent, 40);
                assert_eq!(p.total_mb, 100.0);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn whisper_events_use_their_own_names() {
        let mut state = state();
        let key = (Engine::Whisper, "tiny".to_string());
        assert!(state.begin_download(&key));
        assert!(state.apply_event(
            "model-download-progress",
            r#"{"modelName":"tiny","progress":12}"#
        ));
        // Parakeet's event name does not touch Whisper's model.
        assert!(!state.apply_event("parakeet-model-download-progress", &progress("tiny", 90)));
        assert!(matches!(
            state.row(&key).unwrap().status(),
            Status::Downloading(Progress { percent: 12, .. })
        ));
    }

    #[test]
    fn a_finished_download_is_ready_and_checked() {
        let mut state = state();
        let key = parakeet(FAST);
        state.begin_download(&key);
        state.apply_event("parakeet-model-download-progress", &progress(FAST, 100));
        assert!(state.apply_event(
            "parakeet-model-download-complete",
            &format!(r#"{{"modelName":"{FAST}"}}"#)
        ));
        let row = state.row(&key).unwrap();
        assert_eq!(row.status(), Status::Ready);
        // The manifest has hashes for this model and core verified them.
        assert!(row.meta.verifiable);
        assert_eq!(row.check, Check::Passed);
    }

    #[test]
    fn a_model_without_manifest_hashes_is_never_shown_as_verified() {
        let mut state = State::default();
        state.set_discovered(vec![found(
            Engine::Parakeet,
            "parakeet-ukendt",
            Base::Missing,
        )]);
        let key = parakeet("parakeet-ukendt");
        assert!(state.begin_download(&key));
        state.apply_event(
            "parakeet-model-download-complete",
            r#"{"modelName":"parakeet-ukendt"}"#,
        );
        let row = state.row(&key).unwrap();
        assert_eq!(row.status(), Status::Ready);
        assert_eq!(row.check, Check::Unknown);
        assert_eq!(row.meta, Meta::default());
    }

    #[test]
    fn cancelling_ends_missing_and_a_late_progress_event_does_not_revive_the_bar() {
        let mut state = state();
        let key = parakeet(FAST);
        state.begin_download(&key);
        state.apply_event("parakeet-model-download-progress", &progress(FAST, 30));
        assert!(state.begin_cancel(&key));
        assert_eq!(state.row(&key).unwrap().status(), Status::Cancelling);
        assert!(!state.apply_event("parakeet-model-download-progress", &progress(FAST, 31)));
        assert_eq!(state.row(&key).unwrap().status(), Status::Cancelling);
        // Core's cancelled event.
        assert!(state.apply_event(
            "parakeet-model-download-progress",
            &format!(r#"{{"modelName":"{FAST}","progress":0,"status":"cancelled"}}"#)
        ));
        assert_eq!(state.row(&key).unwrap().status(), Status::Missing);
        // And the model can be fetched again.
        assert!(state.begin_download(&key));
    }

    #[test]
    fn a_late_cancel_pending_answer_leaves_no_error_on_an_idle_row() {
        let mut state = state();
        let key = parakeet(FAST);
        state.begin_download(&key);
        assert!(state.begin_cancel(&key));
        state.cancel_pending(&key);
        assert!(state.row(&key).unwrap().error.is_some());
        // The download call ended first.
        state.cancelled(&key);
        state.row_mut(&key).unwrap().error = None;
        state.cancel_pending(&key);
        assert_eq!(state.row(&key).unwrap().error, None);
    }

    #[test]
    fn unrelated_errors_are_not_shown_as_network_errors() {
        let generic = explain_error("Stream disconnected_by_user at C:\\dnsmasq\\x", 1.0);
        assert!(generic.contains("Hentningen mislykkedes"));
        assert!(explain_error("error trying to connect: dns error", 1.0).contains("afbrudt"));
    }

    #[test]
    fn only_a_running_download_can_be_cancelled() {
        let mut state = state();
        assert!(!state.begin_cancel(&parakeet(FAST)));
        assert!(!state.begin_cancel(&(Engine::Whisper, "base".to_string())));
    }

    #[test]
    fn a_pending_cancel_says_why_it_is_still_stopping() {
        let mut state = state();
        let key = parakeet(FAST);
        state.begin_download(&key);
        state.begin_cancel(&key);
        state.cancel_pending(&key);
        let row = state.row(&key).unwrap();
        assert_eq!(row.status(), Status::Cancelling);
        assert!(row.error.as_deref().unwrap().contains("ved at stoppe"));
    }

    #[test]
    fn a_failed_download_shows_the_reason_and_can_be_retried() {
        let mut state = state();
        let key = parakeet(FAST);
        state.begin_download(&key);
        assert!(state.apply_event(
            "parakeet-model-download-error",
            &format!(r#"{{"modelName":"{FAST}","error":"No response from x within 60 seconds"}}"#)
        ));
        match state.row(&key).unwrap().status() {
            Status::Failed(why) => assert!(why.contains("Tjek internetforbindelsen")),
            other => panic!("{other:?}"),
        }
        assert!(state.begin_download(&key));
        assert_eq!(
            state.row(&key).unwrap().status(),
            Status::Downloading(Progress::default())
        );
    }

    #[test]
    fn a_failed_download_leaves_an_installed_model_and_the_saved_choice_alone() {
        let mut state = state();
        let installed = (Engine::Whisper, "base".to_string());
        state.selected = Some(installed.clone());
        // Fetching Parakeet fails.
        let key = parakeet(FAST);
        state.begin_download(&key);
        state.apply_event(
            "parakeet-model-download-error",
            &format!(r#"{{"modelName":"{FAST}","error":"Failed to write x: No space left on device (os error 28)"}}"#),
        );
        assert_eq!(state.row(&installed).unwrap().status(), Status::Ready);
        assert_eq!(state.selected_row().unwrap().found.id, "base");
        // A model that is installed cannot be downloaded over.
        assert!(!state.begin_download(&installed));
    }

    #[test]
    fn a_new_listing_after_restart_keeps_no_download_state_from_the_disk() {
        // The engine reports a half-finished download as Downloading; this
        // app only shows what it started itself, so after a restart the card
        // offers "Hent" (core resumes the .part file).
        let mut state = State::default();
        state.set_discovered(vec![found(Engine::Parakeet, FAST, Base::Missing)]);
        assert_eq!(
            state.row(&parakeet(FAST)).unwrap().status(),
            Status::Missing
        );
    }

    #[test]
    fn a_relist_keeps_a_running_download_and_the_last_error() {
        let mut state = state();
        let key = parakeet(FAST);
        state.begin_download(&key);
        state.apply_event("parakeet-model-download-progress", &progress(FAST, 55));
        let again = vec![found(Engine::Parakeet, FAST, Base::Missing)];
        state.set_discovered(again);
        assert!(matches!(
            state.row(&key).unwrap().status(),
            Status::Downloading(Progress { percent: 55, .. })
        ));
    }

    #[test]
    fn corrupted_models_are_offered_delete_and_download_again() {
        let mut state = State::default();
        state.set_discovered(vec![found(Engine::Parakeet, FAST, Base::Corrupted)]);
        let key = parakeet(FAST);
        assert_eq!(state.row(&key).unwrap().status(), Status::Corrupted);
        assert!(state.begin_download(&key));
    }

    #[test]
    fn a_changed_file_loses_its_checksum_result() {
        let mut state = state();
        let key = (Engine::Whisper, "base".to_string());
        state.set_check(&key, Check::Passed);
        state.set_discovered(vec![found(Engine::Whisper, "base", Base::Corrupted)]);
        assert_eq!(state.row(&key).unwrap().check, Check::Unknown);
    }

    #[test]
    fn errors_are_explained_in_danish() {
        let disk = explain_error(
            "Failed to write x: No space left on device (os error 28)",
            670.0,
        );
        assert!(disk.contains("ikke plads nok på disken"));
        assert!(disk.contains("670 MB"));
        let windows_disk =
            explain_error("There is not enough space on the disk. (os error 112)", 0.0);
        assert!(windows_disk.contains("Disken er fuld"));
        let net = explain_error(
            "Failed to start download for https://x: error sending request",
            1.0,
        );
        assert!(net.contains("internetforbindelsen"));
        let stalled = explain_error("No response from https://x within 60 seconds", 1.0);
        assert!(stalled.contains("fortsætter, hvor den slap"));
        let hash = explain_error(
            "Failed to download Parakeet model: Modelfilen a.onnx har ikke den forventede kontrolsum (SHA-256) og er slettet. Prøv at hente den igen.",
            1.0,
        );
        assert!(hash.starts_with("Modelfilen a.onnx"));
        assert!(!hash.contains("Failed"));
        assert!(explain_error("noget uventet", 1.0).contains("Hentningen mislykkedes"));
        assert!(explain_error("Parakeet engine not initialized", 1.0).contains("Genstart"));
    }

    #[test]
    fn disk_check_blocks_only_when_the_free_space_is_known_and_too_small() {
        let gb = 1024 * 1024 * 1024;
        assert!(enough_space(Some(10 * gb), 2 * gb).is_ok());
        assert!(enough_space(Some(gb), 2 * gb).is_err());
        // Exactly the model is not enough: there is a margin.
        assert!(enough_space(Some(2 * gb), 2 * gb).is_err());
        // Unknown free space does not block.
        assert!(enough_space(None, 2 * gb).is_ok());
    }

    #[test]
    fn manifest_metadata_is_used_and_missing_metadata_is_unknown() {
        let known = meta_of(&found(Engine::Parakeet, FAST, Base::Missing));
        assert!(known.license.is_some());
        assert!(known.revision.is_some());
        assert!(known.source.is_some());
        assert!(known.total_bytes.unwrap() > 100 * 1024 * 1024);
        assert!(known.verifiable);
        let unknown = meta_of(&found(Engine::Whisper, "finnes-ikke", Base::Missing));
        assert_eq!(unknown, Meta::default());
    }

    #[test]
    fn every_listed_whisper_and_parakeet_model_has_a_name_and_tagline() {
        for (engine, id) in [
            (Engine::Parakeet, FAST),
            (Engine::Parakeet, "parakeet-tdt-0.6b-v2-int8"),
            (Engine::Whisper, "large-v3-q5_0"),
            (Engine::Whisper, "base"),
        ] {
            let row = Row {
                found: found(engine, id, Base::Missing),
                meta: Meta::default(),
                transient: None,
                error: None,
                check: Check::Unknown,
            };
            assert!(!row.name().is_empty());
            assert!(!row.tagline().is_empty());
        }
        assert_eq!(
            Row {
                found: found(Engine::Parakeet, FAST, Base::Missing),
                meta: Meta::default(),
                transient: None,
                error: None,
                check: Check::Unknown,
            }
            .name(),
            "Lynhurtig"
        );
    }
}
