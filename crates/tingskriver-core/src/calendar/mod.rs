//! Private ICS calendars. Parsing and matching do not perform I/O.
mod cache;
mod fetch;
mod ics;
mod matching;
mod recurrence;
mod timezone;

use chrono::{DateTime, Utc};
pub use fetch::{Calendar, RefreshOutcome};
pub use ics::parse;
pub use matching::match_meeting;
use serde::{Deserialize, Serialize};
use std::fmt;

pub const CREDENTIAL_TARGET: &str = "Tingskriver/calendar-ics";

#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CalendarConfig {
    pub own_addresses: Vec<String>,
    pub timezone: String,
    pub refresh_minutes: u8,
}
impl Default for CalendarConfig {
    fn default() -> Self {
        Self {
            own_addresses: Vec::new(),
            timezone: "Europe/Copenhagen".into(),
            refresh_minutes: 10,
        }
    }
}

/// Has no serialization or public accessor; only the HTTP boundary sees it.
#[derive(Clone)]
pub struct IcsAddress(String);
impl IcsAddress {
    pub fn new(value: String) -> Result<Self, CalendarError> {
        let url = reqwest::Url::parse(&value).map_err(|_| CalendarError::Address)?;
        if !matches!(url.scheme(), "https" | "http")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err(CalendarError::Address);
        }
        Ok(Self(value))
    }
    pub fn save(&self) -> Result<(), CalendarError> {
        crate::dictation::secrets::write(CREDENTIAL_TARGET, &self.0)
            .map_err(|_| CalendarError::Credential)
    }
    pub fn load() -> Result<Option<Self>, CalendarError> {
        crate::dictation::secrets::read(CREDENTIAL_TARGET)
            .map_err(|_| CalendarError::Credential)?
            .map(Self::new)
            .transpose()
    }
    pub fn delete() -> Result<(), CalendarError> {
        crate::dictation::secrets::delete(CREDENTIAL_TARGET).map_err(|_| CalendarError::Credential)
    }
}
impl fmt::Debug for IcsAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("IcsAddress([hidden])")
    }
}
impl fmt::Display for IcsAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[hidden]")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CalendarError {
    #[error("Kalenderadressen er ugyldig")]
    Address,
    #[error("Kalendernøglen kunne ikke læses eller gemmes")]
    Credential,
    #[error("Kalenderen kunne ikke hentes")]
    Fetch,
    #[error("Kalenderformatet understøttes ikke eller er ugyldigt")]
    Parse,
    #[error("Kalendercachen kunne ikke læses eller gemmes")]
    Cache,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attendee {
    pub name: Option<String>,
    pub email: String,
    #[serde(default)]
    pub declined: bool,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    pub uid: String,
    pub title: String,
    pub attendees: Vec<Attendee>,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub all_day: bool,
}
