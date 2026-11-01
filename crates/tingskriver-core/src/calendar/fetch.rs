use super::{cache::Cache, parse, CalendarConfig, CalendarError, Event, IcsAddress};
use chrono::{DateTime, Duration, Utc};
use std::{path::PathBuf, time::Duration as Timeout};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshOutcome {
    NotDue,
    Updated,
    NotModified,
    Cached,
}

pub struct Calendar {
    address: IcsAddress,
    config: CalendarConfig,
    client: reqwest::Client,
    path: PathBuf,
    cache: Cache,
    attempted_at: Option<DateTime<Utc>>,
}
impl Calendar {
    pub async fn open(address: IcsAddress, config: CalendarConfig) -> Result<Self, CalendarError> {
        let path = crate::paths::app_data_dir()
            .ok_or(CalendarError::Cache)?
            .join("calendar/events.json");
        Self::open_at(address, config, path, Timeout::from_secs(10)).await
    }
    /// Explicit cache location and shorter timeout for hosts/tests. Timeout is
    /// always clamped to ten seconds; refresh cadence is clamped to 5–15 min.
    pub async fn open_at(
        address: IcsAddress,
        config: CalendarConfig,
        path: PathBuf,
        timeout: Timeout,
    ) -> Result<Self, CalendarError> {
        let client = reqwest::Client::builder()
            .timeout(timeout.min(Timeout::from_secs(10)))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| CalendarError::Fetch)?;
        let cache = Cache::load(&path)
            .await
            .ok()
            .filter(|cache| cache.timezone == config.timezone)
            .unwrap_or_default();
        Ok(Self {
            address,
            config,
            client,
            path,
            cache,
            attempted_at: None,
        })
    }
    /// Active and nearby events: yesterday through tomorrow. Stale cached
    /// occurrences are deliberately usable offline, within the cached range.
    pub fn events_around(&self, now: DateTime<Utc>) -> Vec<Event> {
        self.cache
            .events
            .iter()
            .filter(|e| e.start < now + Duration::days(1) && e.end >= now - Duration::days(1))
            .filter(|e| {
                !e.attendees.iter().any(|a| {
                    a.declined
                        && self
                            .config
                            .own_addresses
                            .iter()
                            .any(|own| own.trim().eq_ignore_ascii_case(&a.email))
                })
            })
            .cloned()
            .collect()
    }
    pub async fn refresh_if_due(&mut self, now: DateTime<Utc>) -> RefreshOutcome {
        let interval = Duration::minutes(i64::from(self.config.refresh_minutes.clamp(5, 15)));
        let last = self.attempted_at.or(self.cache.fetched_at);
        if last.is_some_and(|last| now >= last && now - last < interval) {
            return RefreshOutcome::NotDue;
        }
        self.attempted_at = Some(now);
        match self.refresh(now).await {
            Ok(outcome) => outcome,
            Err(_) => {
                log::warn!("Calendar refresh failed; using local cache");
                RefreshOutcome::Cached
            }
        }
    }
    async fn refresh(&mut self, now: DateTime<Utc>) -> Result<RefreshOutcome, CalendarError> {
        // Refetch the body when the expansion window nears its edge, even if
        // the feed has not changed, to keep infinite recurrences moving.
        let window_valid = self
            .cache
            .range_start
            .is_some_and(|d| d <= now - Duration::days(1))
            && self
                .cache
                .range_end
                .is_some_and(|d| d > now + Duration::days(30));
        let mut request = self.client.get(&self.address.0);
        if window_valid {
            if let Some(etag) = &self.cache.etag {
                request = request.header(reqwest::header::IF_NONE_MATCH, etag);
            }
        }
        let response = request.send().await.map_err(|_| CalendarError::Fetch)?;
        if response.status() == reqwest::StatusCode::NOT_MODIFIED && window_valid {
            self.cache.fetched_at = Some(now);
            self.cache.save(&self.path).await?;
            return Ok(RefreshOutcome::NotModified);
        }
        if response.status() != reqwest::StatusCode::OK {
            return Err(CalendarError::Fetch);
        }
        let etag = response
            .headers()
            .get(reqwest::header::ETAG)
            .and_then(|h| h.to_str().ok())
            .filter(|s| s.len() < 4096 && !s.contains(&self.address.0))
            .map(str::to_owned);
        if response
            .content_length()
            .is_some_and(|n| n > 8 * 1024 * 1024)
        {
            return Err(CalendarError::Fetch);
        }
        // Bound both chunked bodies and the complete operation, including reads.
        let body = tokio::time::timeout(Timeout::from_secs(10), async {
            let mut response = response;
            let mut bytes = Vec::new();
            while let Some(chunk) = response.chunk().await.map_err(|_| CalendarError::Fetch)? {
                if bytes.len() + chunk.len() > 8 * 1024 * 1024 {
                    return Err(CalendarError::Fetch);
                }
                bytes.extend_from_slice(&chunk);
            }
            String::from_utf8(bytes).map_err(|_| CalendarError::Parse)
        })
        .await
        .map_err(|_| CalendarError::Fetch)??;
        let from = now - Duration::days(30);
        let to = now + Duration::days(366);
        // Keep decline metadata so changing own_addresses also works offline.
        let mut parse_config = self.config.clone();
        parse_config.own_addresses.clear();
        let events = parse(&body, &parse_config, from, to)?;
        let cache = Cache {
            version: 2,
            timezone: self.config.timezone.clone(),
            events,
            etag,
            fetched_at: Some(now),
            range_start: Some(from),
            range_end: Some(to),
        };
        // Memory remains useful even when the disk is full or read-only.
        let saved = cache.save(&self.path).await;
        self.cache = cache;
        saved?;
        Ok(RefreshOutcome::Updated)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn actual_network_error_has_no_url_in_debug_or_display() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!(
            "http://{}/private-synthetic-token",
            listener.local_addr().unwrap()
        );
        drop(listener);
        let dir = tempfile::tempdir().unwrap();
        let mut calendar = Calendar::open_at(
            IcsAddress::new(url.clone()).unwrap(),
            CalendarConfig::default(),
            dir.path().join("events.json"),
            Timeout::from_millis(30),
        )
        .await
        .unwrap();
        let error = calendar.refresh(Utc::now()).await.unwrap_err();
        let formatted = format!("{error:?} {error}");
        assert!(!formatted.contains(&url));
        assert!(!formatted.contains("private-synthetic-token"));
    }
}
