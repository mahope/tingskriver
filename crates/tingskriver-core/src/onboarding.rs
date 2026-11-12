//! First start: the saved onboarding status, the two model downloads and the
//! last step. The GPUI app draws the screens
//! (`crates/tingskriver-app/src/onboarding.rs`); what it decides and saves
//! lives here.
//!
//! The status is the Tauri app's `onboarding-status.json` (key `status`,
//! `frontend/src-tauri/src/onboarding.rs`), so a user who finished onboarding
//! in one app does not see it again in the other. Progress arrives as the
//! same host events the Tauri screen listens to
//! (`steps/DownloadProgressStep.tsx`); [`parse_event`] turns them into
//! [`Change`]s for a [`Download`].

use std::sync::OnceLock;

use serde_json::Value;

use crate::database::manager::DatabaseManager;
use crate::database::repositories::setting::SettingsRepository;
use crate::host::{AppHost, HostHandle};
use crate::parakeet_engine::commands as parakeet;
use crate::summary::summary_engine::commands::{self as summary, ModelManagerState};

/// The settings store shared with the Tauri app.
pub const STORE: &str = "onboarding-status.json";
pub const STATUS_KEY: &str = "status";

/// The transcription model onboarding fetches (Tauri `PARAKEET_MODEL`).
pub const TRANSCRIPTION_MODEL: &str = crate::config::DEFAULT_PARAKEET_MODEL;
/// Its size before the first progress event (Tauri `totalMb: 670`).
pub const TRANSCRIPTION_TOTAL_MB: f64 = 670.0;
/// Summary model when the recommendation cannot be computed (Tauri
/// `initialize_fresh_database`).
pub const FALLBACK_SUMMARY_MODEL: &str = "qwen3.5:2b";
/// Visible steps: welcome and download. The Tauri app adds a third,
/// permissions, on macOS only.
pub const STEPS: usize = 2;

pub const PARAKEET_PROGRESS: &str = "parakeet-model-download-progress";
pub const PARAKEET_COMPLETE: &str = "parakeet-model-download-complete";
pub const PARAKEET_ERROR: &str = "parakeet-model-download-error";
pub const SUMMARY_PROGRESS: &str = "builtin-ai-download-progress";

/// True when onboarding was finished (in either app). A missing or
/// unreadable status shows onboarding, as in Tauri.
pub fn is_completed(host: &dyn AppHost) -> bool {
    match host.read_setting(STORE, STATUS_KEY) {
        Ok(Some(status)) => status_completed(&status),
        Ok(None) => false,
        Err(error) => {
            log::warn!("Could not read the onboarding status: {error}");
            false
        }
    }
}

/// The `completed` flag of a saved status.
pub fn status_completed(status: &Value) -> bool {
    status
        .get("completed")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

/// The status Tauri's `complete_onboarding` saves, keeping fields this
/// version does not know. Every field Tauri's `OnboardingStatus` requires is
/// set, so the Tauri app can read it.
pub fn completed_status(previous: Option<Value>, summary_model: &str, now: &str) -> Value {
    let mut status = match previous {
        Some(Value::Object(map)) => map,
        _ => serde_json::Map::new(),
    };
    status
        .entry("version")
        .or_insert_with(|| Value::from("1.0"));
    status.insert("completed".into(), Value::from(true));
    status.insert("current_step".into(), Value::from(4));
    let mut models = match status.remove("model_status") {
        Some(Value::Object(map)) => map,
        _ => serde_json::Map::new(),
    };
    models.insert("parakeet".into(), Value::from("downloaded"));
    models.insert("summary".into(), Value::from("downloaded"));
    models.insert("selected_summary_model".into(), Value::from(summary_model));
    status.insert("model_status".into(), Value::Object(models));
    status.insert("last_updated".into(), Value::from(now));
    Value::Object(status)
}

/// The approximate download size of a summary model in MiB
/// (`lib/onboarding-summary-model.ts`); 0 when unknown.
pub fn summary_model_size_mb(model: &str) -> f64 {
    match model {
        "qwen3.5:2b" => 1221.0,
        "qwen3.5:4b" => 2614.0,
        "gemma3:1b" => 1019.0,
        "gemma3:4b" => 2374.0,
        _ => 0.0,
    }
}

/// "~1.2 GiB" or "~1019 MiB"; empty when the size is unknown
/// (`formatSummaryModelSizeLabelFromMb`).
pub fn summary_size_label(model: &str) -> String {
    let mb = summary_model_size_mb(model);
    if mb == 0.0 {
        String::new()
    } else if mb >= 1024.0 {
        format!("~{:.1} GiB", mb / 1024.0)
    } else {
        format!("~{mb} MiB")
    }
}

/// Where a download is (Tauri `DownloadStatus`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Waiting,
    Downloading,
    Completed,
    Cancelled,
    Error,
}

/// One model card's state (Tauri `DownloadState`).
#[derive(Debug, Clone, PartialEq)]
pub struct Download {
    pub status: Status,
    /// 0 to 100.
    pub progress: f64,
    pub downloaded_mb: f64,
    pub total_mb: f64,
    pub speed_mbps: f64,
    pub error: Option<String>,
}

impl Download {
    pub fn waiting(total_mb: f64) -> Self {
        Download {
            status: Status::Waiting,
            progress: 0.0,
            downloaded_mb: 0.0,
            total_mb,
            speed_mbps: 0.0,
            error: None,
        }
    }

    pub fn completed(total_mb: f64) -> Self {
        Download {
            status: Status::Completed,
            progress: 100.0,
            ..Download::waiting(total_mb)
        }
    }

    pub fn is_done(&self) -> bool {
        self.status == Status::Completed
    }

    /// A (re)started download: back to zero, no error.
    pub fn start(&mut self) {
        *self = Download {
            status: Status::Downloading,
            ..Download::waiting(self.total_mb)
        };
    }

    /// The start or retry call itself failed.
    pub fn fail(&mut self, error: String) {
        self.status = Status::Error;
        self.error = Some(error);
    }

    /// Apply a progress event, the way the Tauri listeners do.
    pub fn apply(&mut self, change: Change) {
        match change {
            Change::Progress {
                progress,
                downloaded_mb,
                total_mb,
                speed_mbps,
            } => {
                self.status = if progress >= 100.0 {
                    Status::Completed
                } else {
                    Status::Downloading
                };
                self.progress = progress.clamp(0.0, 100.0);
                self.downloaded_mb = downloaded_mb.unwrap_or(self.downloaded_mb);
                self.total_mb = total_mb.filter(|t| *t > 0.0).unwrap_or(self.total_mb);
                self.speed_mbps = speed_mbps.unwrap_or(self.speed_mbps);
                self.error = None;
            }
            Change::Completed => {
                self.status = Status::Completed;
                self.progress = 100.0;
                self.error = None;
            }
            Change::Cancelled => {
                *self = Download {
                    status: Status::Cancelled,
                    ..Download::waiting(self.total_mb)
                };
            }
            Change::Failed(error) => self.fail(error),
        }
    }
}

/// Which card an event belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Model {
    Transcription,
    Summary,
}

/// What a progress event says.
#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    Progress {
        progress: f64,
        downloaded_mb: Option<f64>,
        total_mb: Option<f64>,
        speed_mbps: Option<f64>,
    },
    Completed,
    Cancelled,
    Failed(String),
}

/// Read a host event. Events for other models (another Parakeet model, a
/// summary model the user did not pick) are ignored, like in Tauri.
pub fn parse_event(
    name: &str,
    payload_json: &str,
    summary_model: Option<&str>,
) -> Option<(Model, Change)> {
    if ![
        PARAKEET_PROGRESS,
        PARAKEET_COMPLETE,
        PARAKEET_ERROR,
        SUMMARY_PROGRESS,
    ]
    .contains(&name)
    {
        return None;
    }
    let payload: Value = serde_json::from_str(payload_json).ok()?;
    let text = |key: &str| payload.get(key).and_then(Value::as_str);
    let number = |key: &str| payload.get(key).and_then(Value::as_f64);
    let progress = || Change::Progress {
        progress: number("progress").unwrap_or(0.0),
        downloaded_mb: number("downloaded_mb"),
        total_mb: number("total_mb"),
        speed_mbps: number("speed_mbps"),
    };
    if name == SUMMARY_PROGRESS {
        if summary_model.is_none() || text("model") != summary_model {
            return None;
        }
        let change = match text("status") {
            Some("completed") => Change::Completed,
            Some("error") => Change::Failed(text("error").unwrap_or_default().to_string()),
            Some("cancelled") => Change::Cancelled,
            _ => progress(),
        };
        return Some((Model::Summary, change));
    }
    if text("modelName") != Some(TRANSCRIPTION_MODEL) {
        return None;
    }
    let change = match name {
        PARAKEET_COMPLETE => Change::Completed,
        PARAKEET_ERROR => Change::Failed(text("error").unwrap_or_default().to_string()),
        _ => match text("status") {
            Some("cancelled") => Change::Cancelled,
            Some("completed") => Change::Completed,
            _ => progress(),
        },
    };
    Some((Model::Transcription, change))
}

/// The summary model manager onboarding downloads with (Tauri keeps one in
/// its app state; the GPUI app has no other user yet).
fn summary_manager() -> &'static ModelManagerState {
    static STATE: OnceLock<ModelManagerState> = OnceLock::new();
    STATE.get_or_init(|| ModelManagerState(Default::default()))
}

/// Point the Parakeet engine at `<app data>/models` and start it. Returns
/// whether a transcription model is already on disk.
pub async fn prepare_transcription(host: &HostHandle) -> Result<bool, String> {
    let models_host = host.clone();
    tokio::task::spawn_blocking(move || parakeet::set_models_directory(&models_host))
        .await
        .map_err(|e| e.to_string())?;
    parakeet::parakeet_init().await?;
    parakeet::parakeet_has_available_models().await
}

/// Fetch the transcription model; progress arrives as host events. Also
/// the retry (Tauri `parakeet_retry_download` calls the same function).
pub async fn download_transcription(host: HostHandle) -> Result<(), String> {
    parakeet::parakeet_download_model(host, TRANSCRIPTION_MODEL.to_string()).await
}

/// The summary model recommended for this computer's memory.
pub async fn recommended_summary_model() -> String {
    summary::builtin_ai_get_recommended_model()
        .await
        .unwrap_or_else(|error| {
            log::warn!("No summary model recommendation: {error}");
            FALLBACK_SUMMARY_MODEL.to_string()
        })
}

/// Whether `model` is already downloaded and valid.
pub async fn summary_ready(host: HostHandle, model: String) -> bool {
    summary::builtin_ai_is_model_ready(host, summary_manager(), model, Some(true))
        .await
        .unwrap_or(false)
}

/// Fetch the summary model; progress arrives as host events.
pub async fn download_summary(host: HostHandle, model: String) -> Result<(), String> {
    summary::builtin_ai_download_model(host, summary_manager(), model).await
}

/// The last step (Tauri `initialize_fresh_database` + `complete_onboarding`):
/// open or create the database, choose Parakeet and the built-in summary
/// model, and only then mark onboarding as done. Downloads still running
/// carry on.
///
/// Unlike Tauri, a model choice already in the database is kept: a database
/// that predates the status file belongs to someone who has set the app up.
pub async fn complete(
    host: HostHandle,
    database: Option<DatabaseManager>,
    summary_model: String,
) -> Result<DatabaseManager, String> {
    let database = match database {
        Some(database) => database,
        None => {
            let dir = host.app_data_dir()?;
            DatabaseManager::new_from_app_data_dir(&dir)
                .await
                .map_err(|e| e.to_string())?
        }
    };
    let pool = database.pool();
    let error = |e: sqlx::Error| e.to_string();
    if SettingsRepository::get_model_config(pool)
        .await
        .map_err(error)?
        .is_none()
    {
        SettingsRepository::save_model_config(pool, "builtin-ai", &summary_model, "large-v3", None)
            .await
            .map_err(error)?;
    }
    if SettingsRepository::get_transcript_config(pool)
        .await
        .map_err(error)?
        .is_none()
    {
        SettingsRepository::save_transcript_config(pool, "parakeet", TRANSCRIPTION_MODEL)
            .await
            .map_err(error)?;
    }
    let now = chrono::Utc::now().to_rfc3339();
    tokio::task::spawn_blocking(move || {
        let previous = host.read_setting(STORE, STATUS_KEY).ok().flatten();
        host.write_setting(
            STORE,
            STATUS_KEY,
            completed_status(previous, &summary_model, &now),
        )
    })
    .await
    .map_err(|e| e.to_string())??;
    log::info!("Onboarding completed");
    Ok(database)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn only_a_completed_status_hides_onboarding() {
        assert!(status_completed(&json!({"completed": true})));
        assert!(!status_completed(&json!({"completed": false})));
        assert!(!status_completed(&json!({"current_step": 3})));
        assert!(!status_completed(&json!("garbage")));
    }

    #[test]
    fn completed_status_keeps_unknown_fields_and_fills_tauris() {
        let previous = json!({
            "version": "1.0",
            "completed": false,
            "current_step": 3,
            "model_status": {"parakeet": "downloading", "summary": "not_downloaded", "extra": 1},
            "last_updated": "2026-01-01T00:00:00Z",
            "future": "kept"
        });
        let status = completed_status(Some(previous), "qwen3.5:4b", "2026-11-12T10:00:00Z");
        assert_eq!(
            status,
            json!({
                "version": "1.0",
                "completed": true,
                "current_step": 4,
                "model_status": {
                    "parakeet": "downloaded",
                    "summary": "downloaded",
                    "selected_summary_model": "qwen3.5:4b",
                    "extra": 1
                },
                "last_updated": "2026-11-12T10:00:00Z",
                "future": "kept"
            })
        );
        let fresh = completed_status(None, "qwen3.5:2b", "now");
        for key in [
            "version",
            "completed",
            "current_step",
            "model_status",
            "last_updated",
        ] {
            assert!(fresh.get(key).is_some(), "{key} missing");
        }
        assert!(status_completed(&fresh));
    }

    #[test]
    fn summary_sizes_follow_the_tauri_labels() {
        assert_eq!(summary_size_label("qwen3.5:2b"), "~1.2 GiB");
        assert_eq!(summary_size_label("qwen3.5:4b"), "~2.6 GiB");
        assert_eq!(summary_size_label("gemma3:1b"), "~1019 MiB");
        assert_eq!(summary_size_label("unknown"), "");
    }

    #[test]
    fn parakeet_events_move_the_transcription_card() {
        let mut card = Download::waiting(TRANSCRIPTION_TOTAL_MB);
        let progress = format!(
            r#"{{"modelName":"{TRANSCRIPTION_MODEL}","progress":42,"downloaded_mb":281.4,"total_mb":670.0,"speed_mbps":12.5,"status":"downloading"}}"#
        );
        let (model, change) = parse_event(PARAKEET_PROGRESS, &progress, None).unwrap();
        assert_eq!(model, Model::Transcription);
        card.apply(change);
        assert_eq!(card.status, Status::Downloading);
        assert_eq!(card.progress, 42.0);
        assert_eq!(card.downloaded_mb, 281.4);
        assert_eq!(card.speed_mbps, 12.5);

        let cancelled =
            format!(r#"{{"modelName":"{TRANSCRIPTION_MODEL}","progress":0,"status":"cancelled"}}"#);
        card.apply(parse_event(PARAKEET_PROGRESS, &cancelled, None).unwrap().1);
        assert_eq!(card.status, Status::Cancelled);
        assert_eq!(card.downloaded_mb, 0.0);
        assert_eq!(card.total_mb, TRANSCRIPTION_TOTAL_MB);

        let error = format!(r#"{{"modelName":"{TRANSCRIPTION_MODEL}","error":"net down"}}"#);
        card.apply(parse_event(PARAKEET_ERROR, &error, None).unwrap().1);
        assert_eq!(card.status, Status::Error);
        assert_eq!(card.error.as_deref(), Some("net down"));

        card.start();
        assert_eq!(card.status, Status::Downloading);
        assert_eq!(card.error, None);
        let done = format!(r#"{{"modelName":"{TRANSCRIPTION_MODEL}"}}"#);
        card.apply(parse_event(PARAKEET_COMPLETE, &done, None).unwrap().1);
        assert!(card.is_done());
        assert_eq!(card.progress, 100.0);
    }

    #[test]
    fn events_for_other_models_are_ignored() {
        let other = r#"{"modelName":"parakeet-tdt-0.6b-v2-int8","progress":5}"#;
        assert_eq!(parse_event(PARAKEET_PROGRESS, other, None), None);
        let summary = r#"{"model":"gemma3:1b","progress":5,"status":"downloading"}"#;
        assert_eq!(
            parse_event(SUMMARY_PROGRESS, summary, Some("qwen3.5:2b")),
            None
        );
        assert_eq!(parse_event(SUMMARY_PROGRESS, summary, None), None);
        assert_eq!(parse_event("transcript-update", "{}", None), None);
        assert_eq!(parse_event(PARAKEET_PROGRESS, "not json", None), None);
    }

    #[test]
    fn summary_events_move_the_summary_card() {
        let mut card = Download::waiting(summary_model_size_mb("qwen3.5:2b"));
        let event = r#"{"model":"qwen3.5:2b","progress":10,"downloaded_mb":122.1,"total_mb":0,"speed_mbps":3.0,"status":"downloading"}"#;
        let (model, change) = parse_event(SUMMARY_PROGRESS, event, Some("qwen3.5:2b")).unwrap();
        assert_eq!(model, Model::Summary);
        card.apply(change);
        assert_eq!(card.status, Status::Downloading);
        // A zero total keeps the known size, like Tauri's `|| getSummaryModelSizeMb`.
        assert_eq!(card.total_mb, 1221.0);
        let failed = r#"{"model":"qwen3.5:2b","progress":0,"status":"error","error":"disk full"}"#;
        card.apply(
            parse_event(SUMMARY_PROGRESS, failed, Some("qwen3.5:2b"))
                .unwrap()
                .1,
        );
        assert_eq!(card.status, Status::Error);
        let done = r#"{"model":"qwen3.5:2b","progress":100,"status":"completed"}"#;
        card.apply(
            parse_event(SUMMARY_PROGRESS, done, Some("qwen3.5:2b"))
                .unwrap()
                .1,
        );
        assert!(card.is_done());
        assert_eq!(card.error, None);
    }

    /// A host with settings in memory and a temporary data folder.
    struct FakeHost {
        dir: std::path::PathBuf,
        settings: std::sync::Mutex<std::collections::HashMap<(String, String), Value>>,
    }

    impl AppHost for FakeHost {
        fn emit_json(&self, _event: &str, _payload_json: &str) -> Result<(), String> {
            Ok(())
        }
        fn database(&self) -> Option<DatabaseManager> {
            None
        }
        fn app_data_dir(&self) -> Result<std::path::PathBuf, String> {
            Ok(self.dir.clone())
        }
        fn listen(
            &self,
            _event: &str,
            _handler: Box<dyn Fn(&str) + Send + Sync>,
        ) -> crate::host::ListenerId {
            0
        }
        fn unlisten(&self, _id: crate::host::ListenerId) {}
        fn read_setting(&self, store: &str, key: &str) -> Result<Option<Value>, String> {
            Ok(self
                .settings
                .lock()
                .unwrap()
                .get(&(store.into(), key.into()))
                .cloned())
        }
        fn write_setting(&self, store: &str, key: &str, value: Value) -> Result<(), String> {
            self.settings
                .lock()
                .unwrap()
                .insert((store.into(), key.into()), value);
            Ok(())
        }
    }

    #[tokio::test]
    async fn completing_creates_the_database_then_marks_the_status() {
        let dir = tempfile::tempdir().unwrap();
        let host = std::sync::Arc::new(FakeHost {
            dir: dir.path().to_path_buf(),
            settings: Default::default(),
        });
        assert!(!is_completed(&*host));
        let handle: HostHandle = host.clone();
        let database = complete(handle, None, "qwen3.5:2b".into()).await.unwrap();
        assert!(is_completed(&*host));
        assert!(dir.path().join(crate::paths::DATABASE_FILE).exists());
        let transcript = SettingsRepository::get_transcript_config(database.pool())
            .await
            .unwrap()
            .expect("transcript settings saved");
        assert_eq!(transcript.provider, "parakeet");
        assert_eq!(transcript.model, TRANSCRIPTION_MODEL);

        // A second run (a database from before the status file) keeps the
        // user's choice.
        SettingsRepository::save_transcript_config(database.pool(), "groq", "whisper-large-v3")
            .await
            .unwrap();
        let handle: HostHandle = host.clone();
        let database = complete(handle, Some(database), "qwen3.5:4b".into())
            .await
            .unwrap();
        let transcript = SettingsRepository::get_transcript_config(database.pool())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(transcript.provider, "groq");
        let summary = SettingsRepository::get_model_config(database.pool())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(summary.model, "qwen3.5:2b");
    }

    #[test]
    fn a_full_progress_event_completes_the_card() {
        let mut card = Download::waiting(10.0);
        card.apply(Change::Progress {
            progress: 100.0,
            downloaded_mb: Some(10.0),
            total_mb: Some(10.0),
            speed_mbps: None,
        });
        assert!(card.is_done());
    }
}
