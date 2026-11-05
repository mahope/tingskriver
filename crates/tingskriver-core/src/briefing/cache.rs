use super::BriefError;
use async_trait::async_trait;
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::sync::Mutex;

#[async_trait]
pub trait BriefCache: Send + Sync {
    async fn get(&self, key: &str) -> Result<Option<String>, BriefError>;
    async fn put(&self, key: &str, value: &str) -> Result<(), BriefError>;
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
    async fn put(&self, key: &str, value: &str) -> Result<(), BriefError> {
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
        sqlx::query_scalar("SELECT payload FROM briefing_cache WHERE cache_key = ?")
            .bind(key)
            .fetch_optional(&self.0)
            .await
            .map_err(|_| BriefError::Cache)
    }
    async fn put(&self, key: &str, value: &str) -> Result<(), BriefError> {
        sqlx::query(
            "INSERT INTO briefing_cache (cache_key, payload, created_at) \
             VALUES (?, ?, strftime('%Y-%m-%dT%H:%M:%fZ','now')) \
             ON CONFLICT(cache_key) DO UPDATE SET payload = excluded.payload",
        )
        .bind(key)
        .bind(value)
        .execute(&self.0)
        .await
        .map(|_| ())
        .map_err(|_| BriefError::Cache)
    }
}
