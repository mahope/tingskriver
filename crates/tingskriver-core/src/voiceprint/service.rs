//! Consent, enrolment, matching and deletion of voiceprints.
//!
//! Rules (issue #198, decided by the owner 5 Nov 2026):
//! - off by default; nothing is stored or matched while off,
//! - nothing is stored for a person without active consent,
//! - the numbers live only in the [`SecretBackend`] (Credential Manager),
//!   the database holds consent records and dates,
//! - withdrawing consent deletes the person's voiceprints at once,
//! - voiceprints not matched for 30/90/180 days (default 90) are deleted,
//! - a match is only a suggestion the user confirms.
//!
//! Log lines here carry counts only: no names, ids, scores or numbers.

use std::sync::Arc;

use chrono::{DateTime, Duration, SecondsFormat, Utc};
use sqlx::SqlitePool;
use uuid::Uuid;

use super::embedder::Embedding;
use super::matching::{self, Candidate, Suggestion, Thresholds};
use super::store::{self, SecretBackend};
use super::texts::CONSENT_TEXT_VERSION;

/// Allowed retention settings, in days. "Never" is not offered.
pub const RETENTION_CHOICES: [u32; 3] = [30, 90, 180];
pub const DEFAULT_RETENTION_DAYS: u32 = 90;
/// Voiceprints kept per person; the oldest is replaced.
pub const MAX_EXEMPLARS_PER_PERSON: usize = 3;
/// Expired voiceprints are looked for at least this often.
pub const PURGE_INTERVAL_HOURS: i64 = 24;
/// How the consent was obtained (the only way for now).
pub const METHOD_CONFIRMED_IN_APP: &str = "confirmed_in_app";

/// Time source, so the automatic deletion can be tested.
pub trait Clock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum VoiceprintError {
    #[error("voiceprints are turned off")]
    Disabled,
    #[error("the person has no active consent")]
    NoConsent,
    #[error("consent must be confirmed: the person is informed and has said yes")]
    NotConfirmed,
    #[error("a name is required")]
    NameRequired,
    #[error("retention must be 30, 90 or 180 days")]
    InvalidRetention,
    #[error("the voiceprint could not be stored or deleted")]
    Storage,
    #[error("the database failed")]
    Database,
}

impl From<sqlx::Error> for VoiceprintError {
    fn from(error: sqlx::Error) -> Self {
        // The detail may name a table, never a voiceprint; keep it out of the
        // error value anyway and log the kind only.
        log::warn!("voiceprint: database error ({})", db_kind(&error));
        VoiceprintError::Database
    }
}

fn db_kind(error: &sqlx::Error) -> &'static str {
    match error {
        sqlx::Error::Database(_) => "query",
        sqlx::Error::PoolTimedOut | sqlx::Error::PoolClosed => "pool",
        _ => "other",
    }
}

type Result<T> = std::result::Result<T, VoiceprintError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub enabled: bool,
    pub retention_days: u32,
    pub last_purge_at: Option<DateTime<Utc>>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            enabled: false,
            retention_days: DEFAULT_RETENTION_DAYS,
            last_purge_at: None,
        }
    }
}

/// What the user confirms in the app before a person is remembered.
#[derive(Debug, Clone)]
pub struct ConsentInput {
    pub person_name: String,
    /// The user ticked: the person is informed and has said yes.
    pub person_informed_and_agreed: bool,
    pub note: Option<String>,
}

/// A consent record as shown on the privacy page. No biometric data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsentRecord {
    pub id: String,
    pub person_name: String,
    pub consented_at: DateTime<Utc>,
    pub method: String,
    pub note: Option<String>,
    pub text_version: u32,
    pub active: bool,
    pub withdrawn_at: Option<DateTime<Utc>>,
    pub exemplars: usize,
}

/// A proposal with the person's name, for the one-click confirmation.
#[derive(Debug, Clone, PartialEq)]
pub struct NamedSuggestion {
    pub suggestion: Suggestion,
    pub person_name: String,
}

/// One row of the consent query, in column order.
type ConsentRow = (
    String,
    String,
    String,
    String,
    Option<String>,
    i64,
    String,
    Option<String>,
    i64,
);

pub struct VoiceprintService {
    pool: SqlitePool,
    backend: Arc<dyn SecretBackend>,
    clock: Arc<dyn Clock>,
    thresholds: Thresholds,
}

fn stamp(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn parse(text: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|t| t.with_timezone(&Utc))
}

impl VoiceprintService {
    pub fn new(
        pool: SqlitePool,
        backend: Arc<dyn SecretBackend>,
        clock: Arc<dyn Clock>,
    ) -> VoiceprintService {
        VoiceprintService {
            pool,
            backend,
            clock,
            thresholds: Thresholds::default(),
        }
    }

    // ---- settings -------------------------------------------------------

    pub async fn settings(&self) -> Result<Settings> {
        let row: Option<(i64, i64, Option<String>)> = sqlx::query_as(
            "SELECT enabled, retention_days, last_purge_at FROM voiceprint_settings WHERE id = 1",
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(match row {
            None => Settings::default(),
            Some((enabled, days, purged)) => Settings {
                enabled: enabled == 1,
                retention_days: u32::try_from(days).unwrap_or(DEFAULT_RETENTION_DAYS),
                last_purge_at: purged.as_deref().and_then(parse),
            },
        })
    }

    async fn require_enabled(&self) -> Result<Settings> {
        let settings = self.settings().await?;
        if settings.enabled {
            Ok(settings)
        } else {
            Err(VoiceprintError::Disabled)
        }
    }

    /// Turning it off keeps existing voiceprints but nothing is stored or
    /// matched until it is turned on again; "forget all" removes them.
    pub async fn set_enabled(&self, enabled: bool) -> Result<()> {
        sqlx::query(
            "INSERT INTO voiceprint_settings (id, enabled) VALUES (1, $1)
             ON CONFLICT(id) DO UPDATE SET enabled = excluded.enabled",
        )
        .bind(i64::from(enabled))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn set_retention_days(&self, days: u32) -> Result<()> {
        if !RETENTION_CHOICES.contains(&days) {
            return Err(VoiceprintError::InvalidRetention);
        }
        sqlx::query(
            "INSERT INTO voiceprint_settings (id, retention_days) VALUES (1, $1)
             ON CONFLICT(id) DO UPDATE SET retention_days = excluded.retention_days",
        )
        .bind(i64::from(days))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    // ---- consent --------------------------------------------------------

    pub async fn grant_consent(&self, input: ConsentInput) -> Result<String> {
        self.require_enabled().await?;
        if !input.person_informed_and_agreed {
            return Err(VoiceprintError::NotConfirmed);
        }
        let name = input.person_name.trim();
        if name.is_empty() {
            return Err(VoiceprintError::NameRequired);
        }
        let note = input
            .note
            .as_deref()
            .map(str::trim)
            .filter(|n| !n.is_empty());
        let id = Uuid::new_v4().to_string();
        let now = stamp(self.clock.now());
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO voiceprint_consents
             (id, person_name, consented_at, method, note, text_version, status)
             VALUES ($1, $2, $3, $4, $5, $6, 'active')",
        )
        .bind(&id)
        .bind(name)
        .bind(&now)
        .bind(METHOD_CONFIRMED_IN_APP)
        .bind(note)
        .bind(i64::from(CONSENT_TEXT_VERSION))
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO voiceprint_consent_log (consent_id, event, at) VALUES ($1, 'granted', $2)",
        )
        .bind(&id)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(id)
    }

    pub async fn consents(&self) -> Result<Vec<ConsentRecord>> {
        let rows: Vec<ConsentRow> = sqlx::query_as(
            "SELECT c.id, c.person_name, c.consented_at, c.method, c.note, c.text_version,
                        c.status, c.withdrawn_at,
                        (SELECT COUNT(*) FROM voiceprint_exemplars e WHERE e.consent_id = c.id)
                 FROM voiceprint_consents c ORDER BY c.consented_at, c.id",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(
                |(id, person_name, at, method, note, version, status, withdrawn, count)| {
                    ConsentRecord {
                        id,
                        person_name,
                        consented_at: parse(&at).unwrap_or_default(),
                        method,
                        note,
                        text_version: u32::try_from(version).unwrap_or(0),
                        active: status == "active",
                        withdrawn_at: withdrawn.as_deref().and_then(parse),
                        exemplars: usize::try_from(count).unwrap_or(0),
                    }
                },
            )
            .collect())
    }

    /// Withdraw the consent and delete the person's voiceprints at once.
    /// The consent is marked withdrawn first, so nothing new can be stored
    /// even if deleting fails; calling this again retries the deletion.
    pub async fn withdraw_consent(&self, consent_id: &str) -> Result<()> {
        let now = stamp(self.clock.now());
        let mut tx = self.pool.begin().await?;
        let updated = sqlx::query(
            "UPDATE voiceprint_consents SET status = 'withdrawn', withdrawn_at = $2
             WHERE id = $1 AND status = 'active'",
        )
        .bind(consent_id)
        .bind(&now)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if updated > 0 {
            sqlx::query("INSERT INTO voiceprint_consent_log (consent_id, event, at) VALUES ($1, 'withdrawn', $2)")
                .bind(consent_id)
                .bind(&now)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        self.delete_exemplars_of(consent_id).await?;
        log::info!("voiceprint: consent withdrawn, voiceprints deleted");
        Ok(())
    }

    // ---- enrolment and matching -----------------------------------------

    /// Remember a voice for a person with active consent.
    pub async fn enroll(&self, consent_id: &str, embedding: &Embedding) -> Result<String> {
        self.require_enabled().await?;
        let active: Option<(String,)> = sqlx::query_as(
            "SELECT id FROM voiceprint_consents WHERE id = $1 AND status = 'active'",
        )
        .bind(consent_id)
        .fetch_optional(&self.pool)
        .await?;
        if active.is_none() {
            return Err(VoiceprintError::NoConsent);
        }
        let id = Uuid::new_v4().to_string();
        self.backend
            .write(&store::target_for(&id), &store::encode(embedding))
            .map_err(|_| VoiceprintError::Storage)?;
        let inserted = sqlx::query(
            "INSERT INTO voiceprint_exemplars (id, consent_id, created_at) VALUES ($1, $2, $3)",
        )
        .bind(&id)
        .bind(consent_id)
        .bind(stamp(self.clock.now()))
        .execute(&self.pool)
        .await;
        if let Err(error) = inserted {
            // Never leave a voiceprint the database does not know about.
            let _ = self.backend.delete(&store::target_for(&id));
            return Err(error.into());
        }
        self.prune_oldest(consent_id).await?;
        Ok(id)
    }

    async fn prune_oldest(&self, consent_id: &str) -> Result<()> {
        let ids: Vec<(String,)> = sqlx::query_as(
            "SELECT id FROM voiceprint_exemplars WHERE consent_id = $1 ORDER BY created_at DESC, rowid DESC",
        )
        .bind(consent_id)
        .fetch_all(&self.pool)
        .await?;
        for (id,) in ids.into_iter().skip(MAX_EXEMPLARS_PER_PERSON) {
            self.delete_exemplar(&id).await?;
        }
        Ok(())
    }

    /// Propose names for the speakers of a meeting. Only people with active
    /// consent take part. Nothing is named and nothing is recorded: the user
    /// confirms with [`confirm_suggestion`](Self::confirm_suggestion).
    pub async fn suggest(&self, speakers: &[(String, Embedding)]) -> Result<Vec<NamedSuggestion>> {
        self.require_enabled().await?;
        let people: Vec<(String, String)> = sqlx::query_as(
            "SELECT id, person_name FROM voiceprint_consents WHERE status = 'active' ORDER BY id",
        )
        .fetch_all(&self.pool)
        .await?;
        let mut loaded: Vec<(String, Vec<Embedding>)> = Vec::new();
        for (consent_id, _) in &people {
            let ids = self.exemplar_ids(consent_id).await?;
            let mut exemplars = Vec::new();
            for id in ids {
                // An unreadable entry is skipped, not an error: matching is
                // best effort and must not block naming.
                if let Ok(Some(text)) = self.backend.read(&store::target_for(&id)) {
                    exemplars.extend(store::decode(&text));
                }
            }
            if !exemplars.is_empty() {
                loaded.push((consent_id.clone(), exemplars));
            }
        }
        let candidates: Vec<Candidate<'_>> = loaded
            .iter()
            .map(|(id, exemplars)| Candidate {
                person_id: id,
                exemplars,
            })
            .collect();
        Ok(matching::suggest(speakers, &candidates, self.thresholds)
            .into_iter()
            .filter_map(|suggestion| {
                let name = people
                    .iter()
                    .find(|(id, _)| *id == suggestion.person_id)
                    .map(|(_, name)| name.clone())?;
                Some(NamedSuggestion {
                    suggestion,
                    person_name: name,
                })
            })
            .collect())
    }

    /// The user accepted a suggestion. This is what counts as "matched" for
    /// the automatic deletion. Returns the name to put on the speaker.
    pub async fn confirm_suggestion(&self, suggestion: &Suggestion) -> Result<String> {
        self.require_enabled().await?;
        let person: Option<(String,)> = sqlx::query_as(
            "SELECT person_name FROM voiceprint_consents WHERE id = $1 AND status = 'active'",
        )
        .bind(&suggestion.person_id)
        .fetch_optional(&self.pool)
        .await?;
        let Some((name,)) = person else {
            return Err(VoiceprintError::NoConsent);
        };
        sqlx::query("UPDATE voiceprint_exemplars SET last_matched_at = $2 WHERE consent_id = $1")
            .bind(&suggestion.person_id)
            .bind(stamp(self.clock.now()))
            .execute(&self.pool)
            .await?;
        Ok(name)
    }

    // ---- deletion -------------------------------------------------------

    async fn exemplar_ids(&self, consent_id: &str) -> Result<Vec<String>> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT id FROM voiceprint_exemplars WHERE consent_id = $1 ORDER BY created_at, rowid",
        )
        .bind(consent_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|(id,)| id).collect())
    }

    /// Secret first, then the index row: a failed secret delete leaves the
    /// row, so the deletion is retried instead of forgotten.
    async fn delete_exemplar(&self, id: &str) -> Result<()> {
        self.backend
            .delete(&store::target_for(id))
            .map_err(|_| VoiceprintError::Storage)?;
        sqlx::query("DELETE FROM voiceprint_exemplars WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn delete_exemplars_of(&self, consent_id: &str) -> Result<usize> {
        let ids = self.exemplar_ids(consent_id).await?;
        for id in &ids {
            self.delete_exemplar(id).await?;
        }
        Ok(ids.len())
    }

    /// "Forget this person": all their voiceprints go, the consent record
    /// stays (withdraw the consent to end it).
    pub async fn forget_person(&self, consent_id: &str) -> Result<usize> {
        let count = self.delete_exemplars_of(consent_id).await?;
        log::info!("voiceprint: forgot a person ({count} voiceprints)");
        Ok(count)
    }

    /// "Forget all": every voiceprint, including Credential Manager entries
    /// the database no longer knows about.
    pub async fn forget_all(&self) -> Result<usize> {
        let swept = self
            .backend
            .delete_prefix(store::TARGET_PREFIX)
            .map_err(|_| VoiceprintError::Storage)?;
        let rows = sqlx::query("DELETE FROM voiceprint_exemplars")
            .execute(&self.pool)
            .await?
            .rows_affected();
        log::info!("voiceprint: forgot everyone ({swept} credential entries, {rows} index rows)");
        Ok(swept.max(usize::try_from(rows).unwrap_or(0)))
    }

    /// Delete voiceprints not matched within the retention period. A
    /// voiceprint never matched counts from the day it was made. Runs also
    /// while the feature is off.
    pub async fn purge_expired(&self) -> Result<usize> {
        let settings = self.settings().await?;
        let now = self.clock.now();
        let cutoff = now - Duration::days(i64::from(settings.retention_days));
        let rows: Vec<(String, String, Option<String>)> =
            sqlx::query_as("SELECT id, created_at, last_matched_at FROM voiceprint_exemplars")
                .fetch_all(&self.pool)
                .await?;
        let mut removed = 0;
        for (id, created, matched) in rows {
            // An unreadable date is treated as expired: delete rather than keep.
            let last_use = matched.as_deref().or(Some(&created)).and_then(parse);
            if last_use.is_none_or(|t| t < cutoff) {
                self.delete_exemplar(&id).await?;
                removed += 1;
            }
        }
        sqlx::query(
            "INSERT INTO voiceprint_settings (id, last_purge_at) VALUES (1, $1)
             ON CONFLICT(id) DO UPDATE SET last_purge_at = excluded.last_purge_at",
        )
        .bind(stamp(now))
        .execute(&self.pool)
        .await?;
        log::info!(
            "voiceprint: automatic deletion removed {removed} voiceprints (retention {} days)",
            settings.retention_days
        );
        Ok(removed)
    }

    /// Call at startup and from a timer: purges if it was never done or the
    /// last run is a day old. Returns `None` when nothing was due.
    pub async fn purge_if_due(&self) -> Result<Option<usize>> {
        let settings = self.settings().await?;
        let due = settings
            .last_purge_at
            .is_none_or(|at| self.clock.now() - at >= Duration::hours(PURGE_INTERVAL_HOURS));
        if due {
            Ok(Some(self.purge_expired().await?))
        } else {
            Ok(None)
        }
    }
}
