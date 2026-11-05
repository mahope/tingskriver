//! "Ignorér denne begivenhed" and "Ignorér serien", stored locally. Both the
//! reminder and the automatic start consult this list.
use super::{CalendarError, Event};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

const VERSION: u8 = 1;
const MAX_ENTRIES: usize = 10_000;
/// A single ignored occurrence is forgotten after this long; the calendar
/// cache never holds anything older. Series markings are kept until undone.
const EVENT_KEEP: i64 = 400;

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
struct Occurrence {
    uid: String,
    start: DateTime<Utc>,
}

#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct IgnoredEvents {
    version: u8,
    events: BTreeSet<Occurrence>,
    series: BTreeSet<String>,
}

fn occurrence(event: &Event) -> Occurrence {
    Occurrence {
        uid: event.uid.clone(),
        // The unmoved start, so a moved occurrence stays ignored.
        start: event.original_start.unwrap_or(event.start),
    }
}

impl IgnoredEvents {
    pub fn new() -> Self {
        Self {
            version: VERSION,
            ..Self::default()
        }
    }
    pub fn is_event_ignored(&self, event: &Event) -> bool {
        self.events.contains(&occurrence(event))
    }
    pub fn is_series_ignored(&self, event: &Event) -> bool {
        self.series.contains(&event.uid)
    }
    /// Either this occurrence or its whole series is ignored.
    pub fn is_ignored(&self, event: &Event) -> bool {
        self.is_event_ignored(event) || self.is_series_ignored(event)
    }
    pub fn ignore_event(&mut self, event: &Event) {
        if self.events.len() < MAX_ENTRIES {
            self.events.insert(occurrence(event));
        }
    }
    /// Ignores every occurrence with this UID, also later ones. For an event
    /// that does not recur it is the same as ignoring the event.
    pub fn ignore_series(&mut self, event: &Event) {
        if self.series.len() < MAX_ENTRIES {
            self.series.insert(event.uid.clone());
        }
    }
    pub fn unignore_event(&mut self, event: &Event) {
        self.events.remove(&occurrence(event));
    }
    pub fn unignore_series(&mut self, event: &Event) {
        self.series.remove(&event.uid);
    }
    /// Forget single occurrences that are long past.
    pub fn prune(&mut self, now: DateTime<Utc>) {
        let limit = now - Duration::days(EVENT_KEEP);
        self.events.retain(|o| o.start >= limit);
    }
    pub fn len(&self) -> usize {
        self.events.len() + self.series.len()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn default_path() -> Result<PathBuf, CalendarError> {
        Ok(crate::paths::app_data_dir()
            .ok_or(CalendarError::Cache)?
            .join("calendar/ignored.json"))
    }
    /// A missing file is an empty list. A file that cannot be read or parsed
    /// is an error, never an empty list: the caller must then not start
    /// anything automatically, because an ignored series could come back.
    pub async fn load_from(path: &Path) -> Result<Self, CalendarError> {
        let bytes = match tokio::fs::read(path).await {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::new()),
            Err(_) => return Err(CalendarError::Cache),
        };
        if bytes.len() > 4 * 1024 * 1024 {
            return Err(CalendarError::Cache);
        }
        let list: Self = serde_json::from_slice(&bytes).map_err(|_| CalendarError::Cache)?;
        if list.version != VERSION {
            return Err(CalendarError::Cache);
        }
        Ok(list)
    }
    pub async fn load() -> Result<Self, CalendarError> {
        Self::load_from(&Self::default_path()?).await
    }
    /// Writes a unique file next to `path` and renames it over the old one.
    pub async fn save_to(&self, path: &Path) -> Result<(), CalendarError> {
        use tokio::io::AsyncWriteExt;
        let parent = path.parent().ok_or(CalendarError::Cache)?;
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|_| CalendarError::Cache)?;
        let mut copy = self.clone();
        copy.version = VERSION;
        let bytes = serde_json::to_vec(&copy).map_err(|_| CalendarError::Cache)?;
        let tmp = parent.join(format!(".ignored-{}.tmp", uuid::Uuid::new_v4()));
        let result = async {
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
            let _ = tokio::fs::remove_file(&tmp).await;
        }
        result.map_err(|_| CalendarError::Cache)
    }
    pub async fn save(&self) -> Result<(), CalendarError> {
        self.save_to(&Self::default_path()?).await
    }
}
