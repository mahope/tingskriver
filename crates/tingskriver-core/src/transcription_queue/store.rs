use std::path::{Path, PathBuf};

use crate::transcript_merge::reconcile::Correction;
use anyhow::{ensure, Result};
use rand::Rng;
use serde::{Deserialize, Serialize};
use sqlx::{sqlite::SqliteConnectOptions, Row, SqliteConnection, SqlitePool};

pub const MAX_ATTEMPTS: i64 = 5;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JobSpec {
    pub meeting_id: String,
    /// Saved WAV track (or a WAV chunk); no audio is held in SQLite.
    pub microphone: PathBuf,
    pub system: Option<PathBuf>,
    pub microphone_label: String,
    pub language: String,
    pub offset_ms: u64,
    /// Optional snapshot for "run again with another provider". Never keys.
    pub providers: Option<crate::providers::ProvidersConfig>,
    #[serde(default)]
    pub hotwords: crate::hotwords::Hotwords,
}

impl JobSpec {
    pub fn new(meeting_id: &str, microphone: PathBuf, system: Option<PathBuf>) -> Self {
        Self {
            meeting_id: meeting_id.into(),
            microphone,
            system,
            microphone_label: "Mig".into(),
            language: "da".into(),
            offset_ms: 0,
            providers: None,
            hotwords: Default::default(),
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

impl Version {
    /// True if the result was stored but must not become the current version.
    pub fn is_rejected(&self) -> bool {
        self.result.get("rejected").is_some()
    }
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
            CREATE TABLE IF NOT EXISTS transcription_speaker_names (
                job_id TEXT NOT NULL, speaker INTEGER NOT NULL, name TEXT NOT NULL,
                PRIMARY KEY(job_id, speaker)
            );
            CREATE TABLE IF NOT EXISTS transcription_speaker_corrections (
                job_id TEXT NOT NULL, start_ms INTEGER NOT NULL, end_ms INTEGER NOT NULL,
                speaker INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS transcription_speaker_corrections_job
                ON transcription_speaker_corrections(job_id);
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

    #[cfg(test)]
    pub(super) async fn complete(&self, job: &Job, result: &serde_json::Value) -> Result<()> {
        self.complete_with(job, &mut result.clone(), false).await
    }

    /// Store the result and, in the same transaction, the speaker names and
    /// corrections read from the previous current version.
    pub(super) async fn complete_with(
        &self,
        job: &Job,
        result: &mut serde_json::Value,
        carry: bool,
    ) -> Result<()> {
        self.complete_with_after_snapshot(job, result, carry, std::future::ready(Ok(())))
            .await
    }

    pub(super) async fn complete_with_after_snapshot(
        &self,
        job: &Job,
        result: &mut serde_json::Value,
        carry: bool,
        after_snapshot: impl std::future::Future<Output = Result<()>>,
    ) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        let previous = if carry {
            Self::versions_on(&mut tx, &job.spec.meeting_id)
                .await?
                .into_iter()
                .rev()
                .find(|v| v.spec.offset_ms == job.spec.offset_ms && !v.is_rejected())
        } else {
            None
        };
        let carried = if let Some(previous) = previous {
            let names = Self::speaker_names_on(&mut tx, &previous.job_id).await?;
            let corrections = Self::speaker_corrections_on(&mut tx, &previous.job_id).await?;
            super::carry::apply(
                &super::carry::Previous {
                    job_id: &previous.job_id,
                    result: &previous.result,
                    corrections: &corrections,
                    names: &names,
                },
                result,
            );
            Some((names, corrections))
        } else {
            None
        };
        after_snapshot.await?;
        let updated = sqlx::query("UPDATE transcription_jobs SET status='completed',result=? WHERE id=? AND status='running' AND attempts=?")
            .bind(serde_json::to_string(result)?).bind(&job.id).bind(job.attempts)
            .execute(&mut *tx).await?.rows_affected();
        ensure!(updated == 1, "Job is no longer owned by this attempt");
        if let Some((names, corrections)) = &carried {
            for (number, name) in names {
                sqlx::query("INSERT OR REPLACE INTO transcription_speaker_names (job_id,speaker,name) VALUES (?,?,?)")
                    .bind(&job.id).bind(*number as i64).bind(name).execute(&mut *tx).await?;
            }
            for c in corrections {
                sqlx::query("INSERT INTO transcription_speaker_corrections (job_id,start_ms,end_ms,speaker) VALUES (?,?,?,?)")
                    .bind(&job.id).bind(c.start_ms as i64).bind(c.slut_ms as i64)
                    .bind(c.nummer as i64).execute(&mut *tx).await?;
            }
        }
        tx.commit().await?;
        Ok(())
    }

    /// Confirm a speaker's name ("Taler N" has the number N) for one version.
    pub async fn set_speaker_name(&self, job_id: &str, number: usize, name: &str) -> Result<()> {
        ensure!(!name.trim().is_empty(), "Missing speaker name");
        sqlx::query("INSERT OR REPLACE INTO transcription_speaker_names (job_id,speaker,name) VALUES (?,?,?)")
            .bind(job_id).bind(number as i64).bind(name.trim()).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn speaker_names(&self, job_id: &str) -> Result<Vec<(usize, String)>> {
        Self::speaker_names_on(&mut *self.pool.acquire().await?, job_id).await
    }

    async fn speaker_names_on(
        connection: &mut SqliteConnection,
        job_id: &str,
    ) -> Result<Vec<(usize, String)>> {
        let rows = sqlx::query(
            "SELECT speaker,name FROM transcription_speaker_names WHERE job_id=? ORDER BY speaker",
        )
        .bind(job_id)
        .fetch_all(connection)
        .await?;
        rows.into_iter()
            .map(|r| Ok((r.try_get::<i64, _>("speaker")? as usize, r.try_get("name")?)))
            .collect()
    }

    /// Record that the user assigned a stretch of the meeting to a speaker.
    pub async fn add_speaker_correction(&self, job_id: &str, correction: Correction) -> Result<()> {
        ensure!(correction.start_ms < correction.slut_ms, "Empty correction");
        sqlx::query("INSERT INTO transcription_speaker_corrections (job_id,start_ms,end_ms,speaker) VALUES (?,?,?,?)")
            .bind(job_id).bind(correction.start_ms as i64).bind(correction.slut_ms as i64)
            .bind(correction.nummer as i64).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn speaker_corrections(&self, job_id: &str) -> Result<Vec<Correction>> {
        Self::speaker_corrections_on(&mut *self.pool.acquire().await?, job_id).await
    }

    async fn speaker_corrections_on(
        connection: &mut SqliteConnection,
        job_id: &str,
    ) -> Result<Vec<Correction>> {
        let rows = sqlx::query("SELECT start_ms,end_ms,speaker FROM transcription_speaker_corrections WHERE job_id=? ORDER BY rowid")
            .bind(job_id).fetch_all(connection).await?;
        rows.into_iter()
            .map(|r| {
                Ok(Correction {
                    start_ms: r.try_get::<i64, _>("start_ms")? as u64,
                    slut_ms: r.try_get::<i64, _>("end_ms")? as u64,
                    nummer: r.try_get::<i64, _>("speaker")? as usize,
                })
            })
            .collect()
    }

    /// The newest stored version of the chunk at `offset_ms` that was not
    /// rejected (for example as truncated). Rejected versions stay in
    /// `versions` but never become current.
    pub async fn current_version(
        &self,
        meeting_id: &str,
        offset_ms: u64,
    ) -> Result<Option<Version>> {
        Ok(self
            .versions(meeting_id)
            .await?
            .into_iter()
            .rev()
            .find(|v| v.spec.offset_ms == offset_ms && !v.is_rejected()))
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
        Self::versions_on(&mut *self.pool.acquire().await?, meeting_id).await
    }

    async fn versions_on(
        connection: &mut SqliteConnection,
        meeting_id: &str,
    ) -> Result<Vec<Version>> {
        let rows = sqlx::query("SELECT id,spec,result FROM transcription_jobs WHERE meeting_id=? AND status='completed' ORDER BY created_at,rowid")
            .bind(meeting_id).fetch_all(connection).await?;
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
        Self::statuses_from_pool(&self.pool).await
    }

    pub(crate) async fn statuses_from_pool(pool: &SqlitePool) -> Result<Vec<super::QueueEvent>> {
        let rows = sqlx::query("SELECT id,meeting_id,status,attempts,next_attempt,CASE WHEN json_type(result, '$.rejected.message') = 'text' THEN json_extract(result, '$.rejected.message') END AS rejected FROM transcription_jobs ORDER BY created_at,rowid")
            .fetch_all(pool).await?;
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
                    rejected: row.try_get("rejected")?,
                })
            })
            .collect()
    }
}
