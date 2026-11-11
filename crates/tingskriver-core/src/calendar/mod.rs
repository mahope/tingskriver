//! Private ICS calendars. Parsing and matching do not perform I/O.
mod autostart;
mod cache;
mod fetch;
mod ics;
mod ignored;
mod links;
mod matching;
mod recurrence;
mod timezone;

pub use autostart::{
    auto_start_action, should_auto_start, should_remind, AutoRecordMode, AutoStartAction, MicHold,
    SkipReason, StartDecision, EARLY_JOIN_WINDOW, GRACE, REMINDER_LEAD,
};
use chrono::{DateTime, Utc};
pub use fetch::{Calendar, RefreshOutcome};
pub(crate) use ics::email;
pub use ics::parse;
pub use ignored::IgnoredEvents;
pub use links::{find_meeting_link, MeetingLink, Platform};
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

#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attendee {
    pub name: Option<String>,
    pub email: String,
    #[serde(default)]
    pub declined: bool,
    /// The address is one of `own_addresses`.
    #[serde(skip)]
    pub is_self: bool,
    #[serde(default)]
    pub partstat: Option<String>,
    #[serde(default)]
    pub organizer: bool,
}
/// The user's own answer to an invitation. `Unknown` means the feed says
/// nothing about the user (no matching ATTENDEE or ORGANIZER, or no
/// `own_addresses` configured); it never starts a recording.
#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OwnStatus {
    Organizer,
    Accepted,
    Tentative,
    NeedsAction,
    Declined,
    #[default]
    Unknown,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    pub uid: String,
    pub title: String,
    pub attendees: Vec<Attendee>,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub all_day: bool,
    /// Meeting link from URL, LOCATION or DESCRIPTION (known platforms only).
    #[serde(default)]
    pub meeting_link: Option<MeetingLink>,
    #[serde(skip)]
    pub own_status: OwnStatus,
    /// Part of a recurring series (master or one of its overrides).
    #[serde(default)]
    pub recurring: bool,
    /// Start of the occurrence in the series before any move (RECURRENCE-ID);
    /// equals `start` for everything else. Keys "ignore this event".
    #[serde(default)]
    pub original_start: Option<DateTime<Utc>>,
}
impl Default for Event {
    fn default() -> Self {
        Self {
            uid: String::new(),
            title: String::new(),
            attendees: Vec::new(),
            start: DateTime::UNIX_EPOCH,
            end: DateTime::UNIX_EPOCH,
            all_day: false,
            meeting_link: None,
            own_status: OwnStatus::Unknown,
            recurring: false,
            original_start: None,
        }
    }
}

impl Event {
    pub(super) fn resolve_identity(&mut self, config: &CalendarConfig) {
        self.own_status = OwnStatus::Unknown;
        for a in &mut self.attendees {
            a.is_self = config
                .own_addresses
                .iter()
                .any(|own| !own.trim().is_empty() && own.trim().eq_ignore_ascii_case(&a.email));
        }
        if self.attendees.iter().any(|a| a.is_self && a.organizer) {
            self.own_status = OwnStatus::Organizer;
        } else if let Some(a) = self.attendees.iter().find(|a| a.is_self) {
            self.own_status = match a
                .partstat
                .as_deref()
                .map(str::to_ascii_uppercase)
                .as_deref()
            {
                Some("ACCEPTED") => OwnStatus::Accepted,
                Some("TENTATIVE") => OwnStatus::Tentative,
                Some("DECLINED") => OwnStatus::Declined,
                Some("NEEDS-ACTION") | None => OwnStatus::NeedsAction,
                _ => OwnStatus::Unknown,
            };
        }
    }
}
