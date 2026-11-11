use super::*;
use crate::providers::secrets::{ApiKey, KeyStore};
use sqlx::{sqlite::SqliteConnectOptions, Row, SqlitePool};
use std::{path::Path, sync::Arc};
use tokio::sync::Mutex;

const MAX_ATTEMPTS: i64 = 5;

#[derive(Clone, Debug, Serialize)]
pub struct JobStatus {
    pub id: String,
    pub target_id: String,
    pub status: String,
    pub attempts: i64,
    pub next_attempt: i64,
    pub error: Option<String>,
    pub meeting_id: Option<String>,
}

/// Share this service per host. The lock serializes policy changes and sends.
/// A queue may only be opened by one application process at a time.
#[derive(Clone)]
pub struct Delivery {
    dir: std::path::PathBuf,
    pool: SqlitePool,
    keys: Arc<dyn KeyStore>,
    gate: Arc<Mutex<()>>,
}

fn credential(id: &str) -> String {
    format!("Tingskriver/delivery/{id}")
}

/// Ids never contain `/`, so this cannot collide with another target's URL.
fn signing_credential(id: &str) -> String {
    format!("Tingskriver/delivery/{id}/signing")
}

impl Delivery {
    pub async fn open(dir: &Path, keys: Arc<dyn KeyStore>) -> Result<Self, Error> {
        tokio::fs::create_dir_all(dir)
            .await
            .map_err(|_| Error::Storage)?;
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(
                SqliteConnectOptions::new()
                    .filename(dir.join("delivery.sqlite"))
                    .create_if_missing(true)
                    .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
                    .synchronous(sqlx::sqlite::SqliteSynchronous::Full)
                    .busy_timeout(std::time::Duration::from_secs(5)),
            )
            .await
            .map_err(|_| Error::Storage)?;
        sqlx::raw_sql("CREATE TABLE IF NOT EXISTS delivery_settings (id TEXT PRIMARY KEY, value TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS delivery_targets (id TEXT PRIMARY KEY, value TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS delivery_signing (id TEXT PRIMARY KEY);
            CREATE TABLE IF NOT EXISTS delivery_jobs (
                id TEXT PRIMARY KEY, target_id TEXT NOT NULL, bundle TEXT NOT NULL,
                status TEXT NOT NULL, attempts INTEGER NOT NULL DEFAULT 0,
                next_attempt INTEGER NOT NULL, error TEXT);
            CREATE TABLE IF NOT EXISTS delivery_approval_requests (
                job_id TEXT PRIMARY KEY, meeting_id TEXT NOT NULL);
            UPDATE delivery_jobs SET status = 'pending' WHERE status = 'running';")
            .execute(&pool).await.map_err(|_| Error::Storage)?;
        Ok(Self {
            dir: dir.to_path_buf(),
            pool,
            keys,
            gate: Arc::new(Mutex::new(())),
        })
    }

    pub async fn policy(&self) -> Result<Policy, Error> {
        let value: Option<String> =
            sqlx::query_scalar("SELECT value FROM delivery_settings WHERE id = 'policy'")
                .fetch_optional(&self.pool)
                .await
                .map_err(|_| Error::Storage)?;
        let mut policy: Policy = value
            .map(|s| serde_json::from_str(&s).map_err(|_| Error::Storage))
            .unwrap_or(Ok(Policy::default()))?;
        policy.local_only |= crate::privacy::local_only_in(&self.dir)
            .await
            .map_err(|_| Error::Storage)?;
        Ok(policy)
    }

    pub async fn set_policy(&self, policy: Policy) -> Result<(), Error> {
        let _guard = self.gate.lock().await;
        sqlx::query("INSERT INTO delivery_settings VALUES ('policy', ?) ON CONFLICT(id) DO UPDATE SET value = excluded.value")
            .bind(serde_json::to_string(&policy).map_err(|_| Error::Storage)?)
            .execute(&self.pool).await.map_err(|_| Error::Storage)?;
        Ok(())
    }

    pub async fn targets(&self) -> Result<Vec<Target>, Error> {
        let rows: Vec<String> =
            sqlx::query_scalar("SELECT value FROM delivery_targets ORDER BY id")
                .fetch_all(&self.pool)
                .await
                .map_err(|_| Error::Storage)?;
        rows.iter()
            .map(|s| serde_json::from_str(s).map_err(|_| Error::Storage))
            .collect()
    }

    async fn target(&self, id: &str) -> Result<Target, Error> {
        self.targets()
            .await?
            .into_iter()
            .find(|t| t.id == id)
            .ok_or(Error::Missing)
    }

    /// `webhook_url` is write-only. None preserves the stored credential.
    pub async fn save_target(
        &self,
        target: Target,
        webhook_url: Option<String>,
    ) -> Result<(), Error> {
        target.validate()?;
        let _guard = self.gate.lock().await;
        let pending: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM delivery_jobs WHERE target_id = ? AND status NOT IN ('completed', 'awaiting_approval')",
        )
        .bind(&target.id)
        .fetch_one(&self.pool)
        .await
        .map_err(|_| Error::Storage)?;
        if pending > 0 {
            return Err(Error::Invalid);
        }
        if matches!(target.destination, Destination::Webhook { .. }) {
            if let Some(url) = webhook_url {
                super::transport::validate_url(&url)?;
                let key = ApiKey::new(url).ok_or(Error::Invalid)?;
                let keys = self.keys.clone();
                let name = credential(&target.id);
                tokio::task::spawn_blocking(move || keys.set(&name, &key))
                    .await
                    .map_err(|_| Error::Credential)?
                    .map_err(|_| Error::Credential)?;
            } else if self.webhook_key(&target.id).await?.is_none() {
                return Err(Error::Credential);
            }
        } else {
            self.delete_signing_key(&target.id).await?;
        }
        // Changing a destination under an approved queued job would change consent.
        let mut tx = self.pool.begin().await.map_err(|_| Error::Storage)?;
        sqlx::query("INSERT INTO delivery_targets VALUES (?, ?) ON CONFLICT(id) DO UPDATE SET value = excluded.value")
            .bind(&target.id).bind(serde_json::to_string(&target).map_err(|_| Error::Invalid)?)
            .execute(&mut *tx).await.map_err(|_| Error::Storage)?;
        if !matches!(target.destination, Destination::Webhook { .. }) {
            sqlx::query("DELETE FROM delivery_signing WHERE id = ?")
                .bind(&target.id)
                .execute(&mut *tx)
                .await
                .map_err(|_| Error::Storage)?;
        }
        tx.commit().await.map_err(|_| Error::Storage)?;
        Ok(())
    }

    /// Whether a signing secret is set. Only a flag is kept in the database;
    /// the secret itself lives in the key store and is never read back.
    pub async fn has_signing_secret(&self, id: &str) -> Result<bool, Error> {
        let flag: Option<String> =
            sqlx::query_scalar("SELECT id FROM delivery_signing WHERE id = ?")
                .bind(id)
                .fetch_optional(&self.pool)
                .await
                .map_err(|_| Error::Storage)?;
        Ok(flag.is_some())
    }

    /// Write-only optional HMAC key for a webhook target. `None` removes it.
    /// It cannot change while jobs are queued or running; a failed job may be
    /// retried after the key is corrected. Keys with surrounding whitespace are refused.
    pub async fn set_signing_secret(&self, id: &str, secret: Option<String>) -> Result<(), Error> {
        let _guard = self.gate.lock().await;
        let target = self.target(id).await?;
        if !matches!(target.destination, Destination::Webhook { .. }) {
            return Err(Error::Invalid);
        }
        let pending: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM delivery_jobs WHERE target_id = ? AND status IN ('pending', 'running')",
        )
        .bind(id)
        .fetch_one(&self.pool)
        .await
        .map_err(|_| Error::Storage)?;
        if pending > 0 {
            return Err(Error::Invalid);
        }
        // The receiver must use the exact same bytes, so never trim silently.
        if secret.as_deref().is_some_and(|s| s.trim() != s) {
            return Err(Error::Invalid);
        }
        let keys = self.keys.clone();
        let name = signing_credential(id);
        match secret {
            Some(secret) => {
                let key = ApiKey::new(secret).ok_or(Error::Invalid)?;
                // A missing key must block delivery if storing the credential fails.
                sqlx::query("INSERT OR IGNORE INTO delivery_signing VALUES (?)")
                    .bind(id)
                    .execute(&self.pool)
                    .await
                    .map_err(|_| Error::Storage)?;
                tokio::task::spawn_blocking(move || keys.set(&name, &key))
                    .await
                    .map_err(|_| Error::Credential)?
                    .map_err(|_| Error::Credential)?;
            }
            None => {
                tokio::task::spawn_blocking(move || keys.delete(&name))
                    .await
                    .map_err(|_| Error::Credential)?
                    .map_err(|_| Error::Credential)?;
                sqlx::query("DELETE FROM delivery_signing WHERE id = ?")
                    .bind(id)
                    .execute(&self.pool)
                    .await
                    .map_err(|_| Error::Storage)?;
            }
        }
        Ok(())
    }

    async fn signing_key(&self, id: &str) -> Result<Option<ApiKey>, Error> {
        if !self.has_signing_secret(id).await? {
            return Ok(None);
        }
        let keys = self.keys.clone();
        let name = signing_credential(id);
        let key = tokio::task::spawn_blocking(move || keys.get(&name))
            .await
            .map_err(|_| Error::Credential)?
            .map_err(|_| Error::Credential)?;
        // A flagged target whose secret has vanished must not send unsigned.
        key.map(Some).ok_or(Error::Credential)
    }

    pub async fn delete_target(&self, id: &str) -> Result<(), Error> {
        let _guard = self.gate.lock().await;
        self.delete_signing_key(id).await?;
        let mut tx = self.pool.begin().await.map_err(|_| Error::Storage)?;
        sqlx::query("DELETE FROM delivery_signing WHERE id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(|_| Error::Storage)?;
        sqlx::query("DELETE FROM delivery_targets WHERE id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(|_| Error::Storage)?;
        tx.commit().await.map_err(|_| Error::Storage)?;
        let keys = self.keys.clone();
        let name = credential(id);
        tokio::task::spawn_blocking(move || keys.delete(&name))
            .await
            .map_err(|_| Error::Credential)?
            .map_err(|_| Error::Credential)?;
        Ok(())
    }

    async fn delete_signing_key(&self, id: &str) -> Result<(), Error> {
        if self.has_signing_secret(id).await? {
            let keys = self.keys.clone();
            let name = signing_credential(id);
            tokio::task::spawn_blocking(move || keys.delete(&name))
                .await
                .map_err(|_| Error::Credential)?
                .map_err(|_| Error::Credential)?;
        }
        Ok(())
    }

    async fn webhook_key(&self, id: &str) -> Result<Option<ApiKey>, Error> {
        let keys = self.keys.clone();
        let name = credential(id);
        tokio::task::spawn_blocking(move || keys.get(&name))
            .await
            .map_err(|_| Error::Credential)?
            .map_err(|_| Error::Credential)
    }

    /// An immutable approved snapshot. A private meeting can never be queued.
    pub async fn enqueue(
        &self,
        target_id: &str,
        bundle: Bundle,
        approved: bool,
        private: bool,
    ) -> Result<String, Error> {
        let _guard = self.gate.lock().await;
        self.policy().await?.check()?;
        if private {
            return Err(Error::Blocked);
        }
        if !approved {
            return Err(Error::Approval);
        }
        bundle.validate()?;
        self.target(target_id).await?;
        let id = uuid::Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO delivery_jobs (id, target_id, bundle, status, next_attempt) VALUES (?, ?, ?, 'pending', ?)")
            .bind(&id).bind(target_id).bind(serde_json::to_string(&bundle).map_err(|_| Error::Invalid)?)
            .bind(Utc::now().timestamp()).execute(&self.pool).await.map_err(|_| Error::Storage)?;
        Ok(id)
    }

    pub async fn test_target(&self, id: &str) -> Result<String, Error> {
        self.enqueue(id, Bundle::test_sample(), true, false).await
    }

    /// MCP can request consent, but only the app's approval flow can release it.
    pub async fn request_approval(
        &self,
        meeting_id: &str,
        target_id: &str,
        bundle: Bundle,
    ) -> Result<String, Error> {
        let _guard = self.gate.lock().await;
        self.policy().await?.check()?;
        bundle.validate()?;
        self.target(target_id).await?;
        let id = uuid::Uuid::new_v4().to_string();
        let mut tx = self.pool.begin().await.map_err(|_| Error::Storage)?;
        sqlx::query("INSERT INTO delivery_jobs (id,target_id,bundle,status,next_attempt) VALUES (?,?,?,'awaiting_approval',?)")
            .bind(&id).bind(target_id).bind(serde_json::to_string(&bundle).map_err(|_| Error::Invalid)?)
            .bind(Utc::now().timestamp()).execute(&mut *tx).await.map_err(|_| Error::Storage)?;
        sqlx::query("INSERT INTO delivery_approval_requests VALUES (?,?)")
            .bind(&id)
            .bind(meeting_id)
            .execute(&mut *tx)
            .await
            .map_err(|_| Error::Storage)?;
        tx.commit().await.map_err(|_| Error::Storage)?;
        Ok(id)
    }

    /// Replace the requested snapshot with the exact summary the user reviewed.
    pub async fn approve_request(
        &self,
        id: &str,
        meeting_id: &str,
        target_id: &str,
        bundle: Bundle,
        private: bool,
    ) -> Result<String, Error> {
        let _guard = self.gate.lock().await;
        self.policy().await?.check()?;
        if private {
            return Err(Error::Blocked);
        }
        bundle.validate()?;
        self.target(target_id).await?.validate()?;
        let result = sqlx::query("UPDATE delivery_jobs SET bundle=?,status='pending',next_attempt=? WHERE id=? AND target_id=? AND status='awaiting_approval' AND EXISTS (SELECT 1 FROM delivery_approval_requests WHERE job_id=? AND meeting_id=?)")
            .bind(serde_json::to_string(&bundle).map_err(|_| Error::Invalid)?)
            .bind(Utc::now().timestamp()).bind(id).bind(target_id).bind(id).bind(meeting_id)
            .execute(&self.pool).await.map_err(|_| Error::Storage)?;
        if result.rows_affected() != 1 {
            return Err(Error::Missing);
        }
        Ok(id.into())
    }

    pub async fn jobs(&self) -> Result<Vec<JobStatus>, Error> {
        let rows = sqlx::query("SELECT j.id, j.target_id, j.status, j.attempts, j.next_attempt, j.error, r.meeting_id FROM delivery_jobs j LEFT JOIN delivery_approval_requests r ON r.job_id=j.id ORDER BY j.rowid DESC")
            .fetch_all(&self.pool).await.map_err(|_| Error::Storage)?;
        Ok(rows
            .into_iter()
            .map(|r| JobStatus {
                id: r.get(0),
                target_id: r.get(1),
                status: r.get(2),
                attempts: r.get(3),
                next_attempt: r.get(4),
                error: r.get(5),
                meeting_id: r.get(6),
            })
            .collect())
    }

    pub async fn retry(&self, id: &str) -> Result<(), Error> {
        let _guard = self.gate.lock().await;
        self.policy().await?.check()?;
        let result = sqlx::query("UPDATE delivery_jobs SET status = 'pending', attempts = 0, next_attempt = ?, error = NULL WHERE id = ? AND status IN ('pending', 'failed')")
            .bind(Utc::now().timestamp()).bind(id).execute(&self.pool).await.map_err(|_| Error::Storage)?;
        if result.rows_affected() == 0 {
            return Err(Error::Missing);
        }
        Ok(())
    }

    /// Called at startup and periodically by the host; blocked jobs stay queued.
    pub async fn run_due(&self) -> Result<(), Error> {
        for _ in 0..20 {
            let _guard = self.gate.lock().await;
            if self.policy().await?.check().is_err() {
                return Ok(());
            }
            let row = sqlx::query("SELECT id, target_id, bundle, attempts FROM delivery_jobs WHERE status = 'pending' AND next_attempt <= ? ORDER BY rowid LIMIT 1")
                .bind(Utc::now().timestamp()).fetch_optional(&self.pool).await.map_err(|_| Error::Storage)?;
            let Some(row) = row else {
                break;
            };
            let id: String = row.get(0);
            let target_id: String = row.get(1);
            let raw: String = row.get(2);
            let attempts: i64 = row.get::<i64, _>(3) + 1;
            sqlx::query("UPDATE delivery_jobs SET status = 'running', attempts = ? WHERE id = ?")
                .bind(attempts)
                .bind(&id)
                .execute(&self.pool)
                .await
                .map_err(|_| Error::Storage)?;
            let result = async {
                let bundle: Bundle = serde_json::from_str(&raw).map_err(|_| Error::Invalid)?;
                bundle.validate()?;
                let target = self.target(&target_id).await?;
                target.validate()?;
                let url = if matches!(target.destination, Destination::Webhook { .. }) {
                    Some(
                        self.webhook_key(&target_id)
                            .await?
                            .ok_or(Error::Credential)?,
                    )
                } else {
                    None
                };
                let signing = if url.is_some() {
                    self.signing_key(&target_id).await?
                } else {
                    None
                };
                super::transport::send(
                    &target.destination,
                    &bundle,
                    &id,
                    url.as_ref(),
                    signing.as_ref(),
                )
                .await
            }
            .await;
            let (status, error) = match result {
                Ok(()) => ("completed", None),
                Err(e) => (
                    if attempts >= MAX_ATTEMPTS {
                        "failed"
                    } else {
                        "pending"
                    },
                    Some(e.to_string()),
                ),
            };
            sqlx::query("UPDATE delivery_jobs SET status = ?, error = ?, next_attempt = ?, bundle = CASE WHEN ? = 'completed' THEN '' ELSE bundle END WHERE id = ?")
                .bind(status).bind(error).bind(Utc::now().timestamp() + 30 * (1_i64 << attempts.min(10)))
                .bind(status).bind(&id).execute(&self.pool).await.map_err(|_| Error::Storage)?;
        }
        Ok(())
    }
}
