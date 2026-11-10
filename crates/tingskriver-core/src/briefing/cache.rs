use super::BriefError;
use async_trait::async_trait;
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::sync::Mutex;

#[async_trait]
pub trait BriefCache: Send + Sync {
    async fn get(&self, key: &str) -> Result<Option<String>, BriefError>;
    async fn put(&self, key: &str, value: &str, meeting_ids: &[&str]) -> Result<(), BriefError>;
}

#[derive(Default)]
pub struct InMemoryCache(Mutex<HashMap<String, String>>);

#[async_trait]
impl BriefCache for InMemoryCache {
    async fn get(&self, key: &str) -> Result<Option<String>, BriefError> {
        Ok(self
            .0
            .lock()
            .map_err(|_| BriefError::Cache)?
            .get(key)
            .cloned())
    }
    async fn put(&self, key: &str, value: &str, _meeting_ids: &[&str]) -> Result<(), BriefError> {
        self.0
            .lock()
            .map_err(|_| BriefError::Cache)?
            .insert(key.into(), value.into());
        Ok(())
    }
}

/// Persists in the local database (table `briefing_cache`).
pub struct SqliteCache(pub SqlitePool);

#[async_trait]
impl BriefCache for SqliteCache {
    async fn get(&self, key: &str) -> Result<Option<String>, BriefError> {
        sqlx::query_scalar("SELECT payload FROM briefing_cache WHERE cache_key = ? LIMIT 1")
            .bind(key)
            .fetch_optional(&self.0)
            .await
            .map_err(|_| BriefError::Cache)
    }
    async fn put(&self, key: &str, value: &str, meeting_ids: &[&str]) -> Result<(), BriefError> {
        let mut tx = self.0.begin().await.map_err(|_| BriefError::Cache)?;
        for meeting_id in meeting_ids {
            sqlx::query(
                "INSERT INTO briefing_cache (cache_key, payload, meeting_id, created_at) \
             VALUES (?, ?, ?, strftime('%Y-%m-%dT%H:%M:%fZ','now')) \
             ON CONFLICT(cache_key, meeting_id) DO UPDATE SET payload = excluded.payload",
            )
            .bind(key)
            .bind(value)
            .bind(meeting_id)
            .execute(&mut *tx)
            .await
            .map_err(|_| BriefError::Cache)?;
        }
        tx.commit().await.map_err(|_| BriefError::Cache)
    }
}
