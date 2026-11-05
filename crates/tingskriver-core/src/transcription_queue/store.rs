use std::path::{Path, PathBuf};

use anyhow::{ensure, Result};
use rand::Rng;
use serde::{Deserialize, Serialize};
use sqlx::{sqlite::SqliteConnectOptions, Row, SqlitePool};

pub const MAX_ATTEMPTS: i64 = 5;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JobSpec {
    pub meeting_id: String,
    /// Saved WAV track (or a WAV chunk); no audio is held in SQLite.
    pub microphone: PathBuf,
    pub system: Option<PathBuf>,
    pub microphone_label: String,
    pub language: String,
    /// Extra spoken languages (issue #209). Jobs stored before this field
    /// existed have none.
    #[serde(default)]
    pub extra_languages: Vec<String>,
    pub offset_ms: u64,
    /// Optional snapshot for "run again with another provider". Never keys.
    pub providers: Option<crate::providers::ProvidersConfig>,
}

impl JobSpec {
    pub fn new(meeting_id: &str, microphone: PathBuf, system: Option<PathBuf>) -> Self {
        Self {
            meeting_id: meeting_id.into(),
            microphone,
            system,
            microphone_label: "Mig".into(),
            language: "da".into(),
            extra_languages: Vec::new(),
            offset_ms: 0,
            providers: None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Job {
    pub id: String,
    pub spec: JobSpec,
    pub attempts: i64,
}

#[derive(Debug)]
pub struct Version {
    pub job_id: String,
    pub spec: JobSpec,
    pub result: serde_json::Value,
}

/// One queue database per application instance. Call `recover_interrupted`
/// once at startup, before running workers. Do not share it across processes.
#[derive(Clone)]
pub struct Queue {
    pool: SqlitePool,
}

impl Queue {
    pub async fn open(path: &Path) -> Result<Self> {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
            .synchronous(sqlx::sqlite::SqliteSynchronous::Full)
            .busy_timeout(std::time::Duration::from_secs(5));
        Self::connect(options).await
    }

    #[cfg(test)]
    pub(super) async fn open_memory() -> Result<Self> {
        Self::connect(SqliteConnectOptions::new().in_memory(true)).await
    }

    async fn connect(options: SqliteConnectOptions) -> Result<Self> {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await?;
        sqlx::raw_sql(
            "CREATE TABLE IF NOT EXISTS transcription_jobs (
                id TEXT PRIMARY KEY, meeting_id TEXT NOT NULL, spec TEXT NOT NULL,
                status TEXT NOT NULL CHECK(status IN ('pending','running','completed','failed')),
                attempts INTEGER NOT NULL DEFAULT 0, next_attempt INTEGER NOT NULL,
                result TEXT, created_at INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS transcription_jobs_due
                ON transcription_jobs(status, next_attempt);
            CREATE TABLE IF NOT EXISTS transcription_provider_cooldowns (
                provider_id TEXT PRIMARY KEY, until INTEGER NOT NULL
            );",
        )
        .execute(&pool)
        .await?;
        Ok(Self { pool })
    }

    pub(super) async fn cooldowns(
        &self,
        now: i64,
    ) -> Result<std::collections::HashMap<String, i64>> {
        let rows = sqlx::query(
            "SELECT provider_id, until FROM transcription_provider_cooldowns WHERE until > ?",
        )
        .bind(now)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| (r.get("provider_id"), r.get("until")))
            .collect())
    }

    pub(super) async fn defer_provider(&self, id: &str, until: i64) -> Result<()> {
        sqlx::query("INSERT INTO transcription_provider_cooldowns (provider_id, until) VALUES (?, ?) ON CONFLICT(provider_id) DO UPDATE SET until = MAX(until, excluded.until)")
            .bind(id).bind(until).execute(&self.pool).await?;
        Ok(())
    }

    /// A rerun is a new job. Earlier successful results remain immutable.
    pub async fn enqueue(&self, spec: &JobSpec, now: i64) -> Result<String> {
        ensure!(!spec.meeting_id.trim().is_empty(), "Missing meeting id");
        ensure!(!spec.language.trim().is_empty(), "Missing language");
        if let Some(config) = &spec.providers {
            config.validate().map_err(anyhow::Error::msg)?;
        }
        let id = uuid::Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO transcription_jobs (id,meeting_id,spec,status,next_attempt,created_at) VALUES (?,?,?,'pending',?,?)")
            .bind(&id).bind(&spec.meeting_id).bind(serde_json::to_string(spec)?)
            .bind(now).bind(now).execute(&self.pool).await?;
        Ok(id)
    }

    pub async fn recover_interrupted(&self) -> Result<u64> {
        // A crash consumes an attempt too, so repeatedly crashing input cannot
        // keep the queue busy forever. Already persisted backoff is untouched.
        Ok(sqlx::query("UPDATE transcription_jobs SET status=CASE WHEN attempts >= ? THEN 'failed' ELSE 'pending' END WHERE status='running'")
            .bind(MAX_ATTEMPTS).execute(&self.pool).await?.rows_affected())
    }

    pub(super) async fn claim(&self, now: i64) -> Result<Option<Job>> {
        let row = sqlx::query("UPDATE transcription_jobs SET status='running', attempts=attempts+1 WHERE id=(SELECT id FROM transcription_jobs WHERE status='pending' AND next_attempt <= ? ORDER BY next_attempt,created_at,rowid LIMIT 1) RETURNING id,spec,attempts")
            .bind(now).fetch_optional(&self.pool).await?;
        row.map(|row| {
            Ok(Job {
                id: row.try_get("id")?,
                spec: serde_json::from_str(row.try_get("spec")?)?,
                attempts: row.try_get("attempts")?,
            })
        })
        .transpose()
    }

    pub(super) async fn complete(&self, job: &Job, result: &serde_json::Value) -> Result<()> {
        let updated = sqlx::query("UPDATE transcription_jobs SET status='completed',result=? WHERE id=? AND status='running' AND attempts=?")
            .bind(serde_json::to_string(result)?).bind(&job.id).bind(job.attempts)
            .execute(&self.pool).await?.rows_affected();
        ensure!(updated == 1, "Job is no longer owned by this attempt");
        Ok(())
    }

    pub(super) async fn fail(
        &self,
        job: &Job,
        retry_after: Option<u64>,
        now: i64,
    ) -> Result<Option<i64>> {
        let next = (job.attempts < MAX_ATTEMPTS).then(|| {
            let base = 5_i64 * (1 << (job.attempts - 1).clamp(0, 10));
            let delay = (base + rand::thread_rng().gen_range(0..=base / 4))
                .max(retry_after.unwrap_or(0).min(i64::MAX as u64) as i64);
            now.saturating_add(delay)
        });
        let updated = sqlx::query("UPDATE transcription_jobs SET status=?,next_attempt=? WHERE id=? AND status='running' AND attempts=?")
            .bind(if next.is_some() { "pending" } else { "failed" })
            .bind(next.unwrap_or(now)).bind(&job.id).bind(job.attempts)
            .execute(&self.pool).await?.rows_affected();
        ensure!(updated == 1, "Job is no longer owned by this attempt");
        Ok(next)
    }

    pub async fn versions(&self, meeting_id: &str) -> Result<Vec<Version>> {
        let rows = sqlx::query("SELECT id,spec,result FROM transcription_jobs WHERE meeting_id=? AND status='completed' ORDER BY created_at,rowid")
            .bind(meeting_id).fetch_all(&self.pool).await?;
        rows.into_iter()
            .map(|row| {
                Ok(Version {
                    job_id: row.try_get("id")?,
                    spec: serde_json::from_str(row.try_get("spec")?)?,
                    result: serde_json::from_str(row.try_get("result")?)?,
                })
            })
            .collect()
    }

    pub async fn statuses(&self) -> Result<Vec<super::QueueEvent>> {
        let rows = sqlx::query("SELECT id,meeting_id,status,attempts,next_attempt FROM transcription_jobs ORDER BY created_at,rowid")
            .fetch_all(&self.pool).await?;
        rows.into_iter()
            .map(|row| {
                Ok(super::QueueEvent {
                    job_id: row.try_get("id")?,
                    meeting_id: row.try_get("meeting_id")?,
                    status: row.try_get("status")?,
                    attempts: row.try_get("attempts")?,
                    next_attempt: if row.try_get::<String, _>("status")? == "pending" {
                        Some(row.try_get("next_attempt")?)
                    } else {
                        None
                    },
                    track: None,
                })
            })
            .collect()
    }
}
