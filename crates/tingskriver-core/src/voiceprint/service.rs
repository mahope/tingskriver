//! Consent, enrolment, matching and deletion of voiceprints.
//!
//! Rules (issue #198, decided by the owner 5 Nov 2026):
//! - off by default; nothing is stored or matched while off,
//! - nothing is stored for a person without active consent,
//! - the numbers live only in the [`SecretBackend`] (Credential Manager),
//!   the database holds consent records and dates,
//! - withdrawing consent deletes the person's voiceprints at once,
//! - voiceprints not matched for 30/90/180 days (default 90) are deleted,
//!   and so are entries the database does not know (orphans),
//! - a match is only a suggestion the user confirms.
//!
//! Log lines here carry counts only: no names, ids, scores or numbers.

use std::collections::HashSet;
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
#[derive(Clone)]
pub struct ConsentInput {
    pub person_name: String,
    /// The user ticked: the person is informed and has said yes.
    pub person_informed_and_agreed: bool,
    pub note: Option<String>,
}

/// A consent record as shown on the privacy page. No biometric data.
#[derive(Clone, PartialEq, Eq)]
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

impl std::fmt::Debug for ConsentInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConsentInput")
            .field("person_name", &"[redacted]")
            .field(
                "person_informed_and_agreed",
                &self.person_informed_and_agreed,
            )
            .field("note", &"[redacted]")
            .finish()
    }
}

impl std::fmt::Debug for ConsentRecord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConsentRecord")
            .field("id", &self.id)
            .field("person_name", &"[redacted]")
            .field("consented_at", &self.consented_at)
            .field("method", &self.method)
            .field("note", &"[redacted]")
            .field("text_version", &self.text_version)
            .field("active", &self.active)
            .field("withdrawn_at", &self.withdrawn_at)
            .field("exemplars", &self.exemplars)
            .finish()
    }
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

/// What one automatic deletion did, so it can go on after a failure.
#[derive(Default)]
struct Sweep {
    removed: usize,
    /// Of `removed`: entries the index did not know.
    orphans: usize,
    failed: usize,
    first_error: Option<VoiceprintError>,
}

impl Sweep {
    fn fail(&mut self, error: VoiceprintError) {
        self.failed += 1;
        self.first_error.get_or_insert(error);
    }
}

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
    /// Try marking consent withdrawn before deleting. A database failure is
    /// reported after attempting deletion of all known voiceprints.
    pub async fn withdraw_consent(&self, consent_id: &str) -> Result<()> {
        // Retain known targets even if the transaction or a later read fails.
        let mut ids = self.exemplar_ids(consent_id).await?;
        let withdrawal = self.mark_consent_withdrawn(consent_id).await;
        // Include enrollments that completed before withdrawal committed.
        let refreshed = self.exemplar_ids(consent_id).await;
        if let Ok(current) = &refreshed {
            for id in current {
                if !ids.contains(id) {
                    ids.push(id.clone());
                }
            }
        }
        let deletion = self.delete_exemplar_ids(&ids).await;
        withdrawal?;
        refreshed?;
        deletion?;
        log::info!("voiceprint: consent withdrawn, voiceprints deleted");
        Ok(())
    }

    async fn mark_consent_withdrawn(&self, consent_id: &str) -> Result<()> {
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
        if updated == 0 {
            let known: Option<(String,)> =
                sqlx::query_as("SELECT id FROM voiceprint_consents WHERE id = $1")
                    .bind(consent_id)
                    .fetch_optional(&mut *tx)
                    .await?;
            if known.is_none() {
                return Err(VoiceprintError::NoConsent);
            }
        }
        if updated > 0 {
            sqlx::query("INSERT INTO voiceprint_consent_log (consent_id, event, at) VALUES ($1, 'withdrawn', $2)")
                .bind(consent_id)
                .bind(&now)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
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
        // Commit the cleanup reference before touching Credential Manager.
        let inserted = sqlx::query(
            "INSERT INTO voiceprint_exemplars (id, consent_id, created_at)
             SELECT $1, $2, $3 WHERE EXISTS (
                 SELECT 1 FROM voiceprint_consents WHERE id = $2 AND status = 'active'
             )",
        )
        .bind(&id)
        .bind(consent_id)
        .bind(stamp(self.clock.now()))
        .execute(&self.pool)
        .await?;
        if inserted.rows_affected() == 0 {
            return Err(VoiceprintError::NoConsent);
        }
        let mut tx = self.pool.begin().await?;
        // Serialize secret writes with deletion and consent withdrawal. The
        // committed reference survives a failed write, commit or compensation.
        let ready = sqlx::query(
            "UPDATE voiceprint_exemplars SET id = id WHERE id = $1 AND EXISTS (
                 SELECT 1 FROM voiceprint_consents WHERE id = $2 AND status = 'active'
             )",
        )
        .bind(&id)
        .bind(consent_id)
        .execute(&mut *tx)
        .await?;
        if ready.rows_affected() == 0 {
            tx.rollback().await?;
            self.delete_exemplar(&id).await?;
            return Err(VoiceprintError::NoConsent);
        }
        let written = self
            .backend
            .write(&store::target_for(&id), &store::encode(embedding));
        tx.commit().await?;
        if written.is_err() {
            self.delete_exemplar(&id).await?;
            return Err(VoiceprintError::Storage);
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
        let excess: Vec<String> = ids
            .into_iter()
            .skip(MAX_EXEMPLARS_PER_PERSON)
            .map(|(id,)| id)
            .collect();
        self.delete_exemplar_ids(&excess).await?;
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
        let mut tx = self.pool.begin().await?;
        // Take SQLite's write lock before deleting the secret, so an in-flight
        // enrollment cannot write it back after its index row has been removed.
        sqlx::query("UPDATE voiceprint_exemplars SET id = id WHERE id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        self.backend
            .delete(&store::target_for(id))
            .map_err(|_| VoiceprintError::Storage)?;
        sqlx::query("DELETE FROM voiceprint_exemplars WHERE id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn delete_exemplars_of(&self, consent_id: &str) -> Result<usize> {
        let ids = self.exemplar_ids(consent_id).await?;
        self.delete_exemplar_ids(&ids).await
    }

    async fn delete_exemplar_ids(&self, ids: &[String]) -> Result<usize> {
        let mut first_error = None;
        for id in ids {
            if let Err(error) = self.delete_exemplar(id).await {
                first_error.get_or_insert(error);
            }
        }
        if let Some(error) = first_error {
            return Err(error);
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
        let mut tx = self.pool.begin().await?;
        sqlx::query("UPDATE voiceprint_exemplars SET id = id")
            .execute(&mut *tx)
            .await?;
        let swept = self
            .backend
            .delete_prefix(store::TARGET_PREFIX)
            .map_err(|_| VoiceprintError::Storage)?;
        let rows = sqlx::query("DELETE FROM voiceprint_exemplars")
            .execute(&mut *tx)
            .await?
            .rows_affected();
        tx.commit().await?;
        log::info!("voiceprint: forgot everyone ({swept} credential entries, {rows} index rows)");
        Ok(swept.max(usize::try_from(rows).unwrap_or(0)))
    }

    /// Delete voiceprints not matched within the retention period. A
    /// voiceprint never matched counts from the day it was made. Credential
    /// Manager entries the index does not know (a recreated database, a
    /// restored safety copy) are deleted too, since nothing else could reach
    /// them. Runs also while the feature is off. The time of the run is
    /// recorded only when nothing failed, so a failed run is tried again at
    /// the next hourly check instead of a day later.
    pub async fn purge_expired(&self) -> Result<usize> {
        let settings = self.settings().await?;
        let now = self.clock.now();
        let cutoff = now - Duration::days(i64::from(settings.retention_days));
        let rows: Vec<(String, String, Option<String>, String)> = sqlx::query_as(
            "SELECT e.id, e.created_at, e.last_matched_at, c.status
                           FROM voiceprint_exemplars e
                           JOIN voiceprint_consents c ON c.id = e.consent_id",
        )
        .fetch_all(&self.pool)
        .await?;
        let mut sweep = Sweep::default();
        for (id, created, matched, status) in rows {
            // An unreadable date is treated as expired: delete rather than keep.
            let last_use = matched.as_deref().or(Some(&created)).and_then(parse);
            if status == "withdrawn" || last_use.map_or(true, |t| t < cutoff) {
                match self.delete_exemplar(&id).await {
                    Ok(()) => sweep.removed += 1,
                    Err(error) => sweep.fail(error),
                }
            }
        }
        self.delete_orphans(&mut sweep).await;
        log::info!(
            "voiceprint: automatic deletion removed {} voiceprints ({} without an index row, retention {} days)",
            sweep.removed,
            sweep.orphans,
            settings.retention_days
        );
        if let Some(error) = sweep.first_error {
            log::warn!(
                "voiceprint: automatic deletion failed for {} voiceprints",
                sweep.failed
            );
            return Err(error);
        }
        sqlx::query(
            "INSERT INTO voiceprint_settings (id, last_purge_at) VALUES (1, $1)
             ON CONFLICT(id) DO UPDATE SET last_purge_at = excluded.last_purge_at",
        )
        .bind(stamp(now))
        .execute(&self.pool)
        .await?;
        Ok(sweep.removed)
    }

    /// Delete the entries under the voiceprint prefix that have no index row
    /// (orphans). Only entries under [`store::TARGET_PREFIX`] are touched,
    /// never any other credential.
    async fn delete_orphans(&self, sweep: &mut Sweep) {
        // List first, read the index second. An enrolment commits its index
        // row before it writes the secret, so a listed entry has its row when
        // the index is read, unless it was deleted on purpose in between.
        let listed = match self.backend.list_prefix(store::TARGET_PREFIX) {
            Ok(listed) => listed,
            Err(_) => {
                sweep.fail(VoiceprintError::Storage);
                return;
            }
        };
        let rows: Vec<(String,)> = match sqlx::query_as("SELECT id FROM voiceprint_exemplars")
            .fetch_all(&self.pool)
            .await
        {
            Ok(rows) => rows,
            Err(error) => {
                sweep.fail(error.into());
                return;
            }
        };
        let known: HashSet<String> = rows.iter().map(|(id,)| store::target_for(id)).collect();
        for target in listed {
            if !target.starts_with(store::TARGET_PREFIX) || known.contains(&target) {
                continue;
            }
            match self.backend.delete(&target) {
                Ok(()) => {
                    sweep.removed += 1;
                    sweep.orphans += 1;
                }
                Err(_) => sweep.fail(VoiceprintError::Storage),
            }
        }
    }

    /// Call at startup and from a timer: purges if it was never done, the
    /// last successful run is a day old, or the last run failed (a failed run
    /// is not recorded). Returns `None` when nothing was due.
    pub async fn purge_if_due(&self) -> Result<Option<usize>> {
        let settings = self.settings().await?;
        let due = settings.last_purge_at.map_or(true, |at| {
            self.clock.now() - at >= Duration::hours(PURGE_INTERVAL_HOURS)
        });
        if due {
            Ok(Some(self.purge_expired().await?))
        } else {
            Ok(None)
        }
    }
}
