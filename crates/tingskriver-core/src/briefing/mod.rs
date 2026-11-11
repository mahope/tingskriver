//! Meeting preparation: a short brief built from the latest related minutes.
//!
//! The idea (rank earlier minutes, three key points each, cache per minutes
//! hash, bounded model call) is reimplemented here; nothing is copied.
//! Everything is local: the brief is built from the user's own minutes and the
//! model call goes through a [`Briefer`], which defaults to a local provider.
//!
//! Series matching: all instances of a recurring calendar event share the
//! iCalendar `UID`; a `RECURRENCE-ID` override keeps the master's `UID`, so
//! `UID` is the stable series key (see `calendar::ics`).
mod cache;
mod llm;

pub use cache::{BriefCache, InMemoryCache, SqliteCache};
pub use llm::LlmBriefer;

use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const MAX_MEETINGS: usize = 5;
pub const MAX_POINTS: usize = 3;
pub const MAX_OUTPUT_TOKENS: u32 = 512;
pub const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(45);
/// Bump when prompts change, so old cache entries are not reused.
pub const PROMPT_VERSION: u32 = 1;
const SHOW_AFTER_END: Duration = Duration::minutes(5);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BriefError {
    #[error("Forberedelsen kunne ikke laves: {0}")]
    Model(String),
    #[error("Forberedelsens cache kunne ikke læses eller gemmes")]
    Cache,
}

/// The upcoming meeting (from the calendar).
#[derive(Debug, Clone)]
pub struct Upcoming {
    pub series_uid: String,
    pub title: String,
    pub attendees: Vec<String>,
    pub own_addresses: Vec<String>,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

/// An earlier recorded meeting with its minutes.
#[derive(Debug, Clone)]
pub struct PastMeeting {
    pub id: String,
    pub title: String,
    pub series_uid: Option<String>,
    pub attendees: Vec<String>,
    pub started_at: DateTime<Utc>,
    pub minutes: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Relation {
    SameSeries,
    SameTitle,
    SharedAttendees,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MeetingPoints {
    pub meeting_id: String,
    pub title: String,
    pub started_at: DateTime<Utc>,
    pub relation: Relation,
    pub points: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Brief {
    pub overview: String,
    pub meetings: Vec<MeetingPoints>,
}

/// The model side. Implementations must be local unless the user chose
/// another provider.
#[async_trait]
pub trait Briefer: Send + Sync {
    /// Identifies provider and model; part of the cache key.
    fn model_id(&self) -> String;
    /// Up to three key points from one set of minutes.
    async fn key_points(&self, title: &str, minutes: &str) -> Result<Vec<String>, BriefError>;
    /// A short joint text from the points of the chosen meetings (newest first).
    async fn overview(&self, meetings: &[MeetingPoints]) -> Result<String, BriefError>;
}

fn normalize(title: &str) -> String {
    title
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn emails(list: &[String]) -> BTreeSet<String> {
    list.iter()
        .map(|e| e.trim().to_lowercase())
        .filter(|e| !e.is_empty())
        .collect()
}

/// Rank earlier minutes: same series, then same title, then shared attendees;
/// newest first within a class. Meetings that start at or after the upcoming
/// one, and empty minutes, never count. At most [`MAX_MEETINGS`].
pub fn rank(upcoming: &Upcoming, past: &[PastMeeting]) -> Vec<(PastMeeting, Relation)> {
    let title = normalize(&upcoming.title);
    let own = emails(&upcoming.own_addresses);
    let people = emails(&upcoming.attendees)
        .difference(&own)
        .cloned()
        .collect();
    let mut ranked: Vec<(PastMeeting, Relation)> = past
        .iter()
        .filter(|m| m.started_at < upcoming.start && !m.minutes.trim().is_empty())
        .filter_map(|m| {
            let relation = if m.series_uid.as_deref() == Some(upcoming.series_uid.as_str())
                && !upcoming.series_uid.is_empty()
            {
                Relation::SameSeries
            } else if !title.is_empty() && normalize(&m.title) == title {
                Relation::SameTitle
            } else if !emails(&m.attendees).is_disjoint(&people) {
                Relation::SharedAttendees
            } else {
                return None;
            };
            Some((m.clone(), relation))
        })
        .collect();
    ranked.sort_by(|a, b| {
        a.1.cmp(&b.1)
            .then(b.0.started_at.cmp(&a.0.started_at))
            .then(a.0.id.cmp(&b.0.id))
    });
    ranked.truncate(MAX_MEETINGS);
    ranked
}

/// The brief shows before the meeting and until five minutes after its end.
pub fn visible(now: DateTime<Utc>, upcoming: &Upcoming) -> bool {
    now <= upcoming.end + SHOW_AFTER_END
}

/// Stable 64-bit FNV-1a, hex encoded. A cache key, not a security feature.
pub fn hash(parts: &[&str]) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for part in parts {
        for b in part.as_bytes().iter().chain(&[0u8]) {
            h ^= u64::from(*b);
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    format!("{h:016x}")
}

fn points_key(model: &str, meeting: &PastMeeting) -> String {
    format!(
        "points:{}",
        hash(&[
            &PROMPT_VERSION.to_string(),
            model,
            &meeting.id,
            &meeting.title,
            &meeting.minutes
        ])
    )
}

fn overview_key(model: &str, meetings: &[MeetingPoints]) -> String {
    let mut parts = vec![PROMPT_VERSION.to_string(), model.to_string()];
    for m in meetings {
        parts.push(m.meeting_id.clone());
        parts.push(m.title.clone());
        parts.extend(m.points.iter().cloned());
    }
    let refs: Vec<&str> = parts.iter().map(String::as_str).collect();
    format!("overview:{}", hash(&refs))
}

/// Build the brief. `Ok(None)` when there is no history (so nothing is shown
/// and no model is called). Unchanged minutes are served from `cache`.
pub async fn build_brief(
    upcoming: &Upcoming,
    past: &[PastMeeting],
    briefer: &dyn Briefer,
    cache: &dyn BriefCache,
) -> Result<Option<Brief>, BriefError> {
    let ranked = rank(upcoming, past);
    if ranked.is_empty() {
        return Ok(None);
    }
    let model = briefer.model_id();
    let mut meetings = Vec::new();
    let mut last_error = None;
    for (meeting, relation) in ranked {
        let key = points_key(&model, &meeting);
        let cached = cache
            .get(&key)
            .await?
            .and_then(|v| serde_json::from_str::<Vec<String>>(&v).ok());
        let points = match cached {
            Some(points) => points,
            None => {
                // One slow or failing minutes text must not remove the brief
                // for the others; fail only if every meeting fails.
                let mut points = match briefer.key_points(&meeting.title, &meeting.minutes).await {
                    Ok(points) => points,
                    Err(e) => {
                        last_error = Some(e);
                        continue;
                    }
                };
                points.retain(|p| !p.trim().is_empty());
                points.truncate(MAX_POINTS);
                if points.is_empty() {
                    continue;
                }
                cache
                    .put(
                        &key,
                        &serde_json::to_string(&points).map_err(|_| BriefError::Cache)?,
                        &[&meeting.id],
                    )
                    .await?;
                points
            }
        };
        if points.is_empty() {
            continue;
        }
        meetings.push(MeetingPoints {
            meeting_id: meeting.id,
            title: meeting.title,
            started_at: meeting.started_at,
            relation,
            points,
        });
    }
    if meetings.is_empty() {
        if let Some(e) = last_error {
            return Err(e);
        }
        return Ok(None);
    }
    let key = overview_key(&model, &meetings);
    let overview = match cache.get(&key).await? {
        Some(text) => text,
        None => {
            let text = briefer
                .overview(&meetings)
                .await
                .unwrap_or_default()
                .trim()
                .to_string();
            if !text.is_empty() {
                let ids: Vec<&str> = meetings.iter().map(|m| m.meeting_id.as_str()).collect();
                cache.put(&key, &text, &ids).await?;
            }
            text
        }
    };
    Ok(Some(Brief { overview, meetings }))
}
