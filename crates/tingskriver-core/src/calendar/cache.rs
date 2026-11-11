use super::{CalendarError, Event};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::Path;

// Version 3 requires raw attendee response and organizer metadata. Older
// caches must be refetched without their ETag to recover that information.
pub(super) const CACHE_VERSION: u8 = 3;

#[derive(Default, Serialize, Deserialize)]
pub(super) struct Cache {
    pub version: u8,
    pub timezone: String,
    pub events: Vec<Event>,
    pub etag: Option<String>,
    pub fetched_at: Option<DateTime<Utc>>,
    pub range_start: Option<DateTime<Utc>>,
    pub range_end: Option<DateTime<Utc>>,
}
impl Cache {
    pub async fn load(path: &Path) -> Result<Self, CalendarError> {
        let bytes = tokio::fs::read(path)
            .await
            .map_err(|_| CalendarError::Cache)?;
        if bytes.len() > 16 * 1024 * 1024 {
            return Err(CalendarError::Cache);
        }
        let cache: Self = serde_json::from_slice(&bytes).map_err(|_| CalendarError::Cache)?;
        if cache.version != CACHE_VERSION {
            return Err(CalendarError::Cache);
        }
        Ok(cache)
    }
    pub async fn save(&self, path: &Path) -> Result<(), CalendarError> {
        let parent = path.parent().ok_or(CalendarError::Cache)?;
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|_| CalendarError::Cache)?;
        let bytes = serde_json::to_vec(self).map_err(|_| CalendarError::Cache)?;
        let tmp = parent.join(format!(".calendar-{}.tmp", uuid::Uuid::new_v4()));
        let result = async {
            use tokio::io::AsyncWriteExt;
            let mut options = tokio::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            options.mode(0o600);
            let mut file = options.open(&tmp).await?;
            file.write_all(&bytes).await?;
            file.sync_all().await?;
            drop(file);
            tokio::fs::rename(&tmp, path).await
        }
        .await;
        if result.is_err() {
            let _ = tokio::fs::remove_file(tmp).await;
        }
        result.map_err(|_| CalendarError::Cache)
    }
}
