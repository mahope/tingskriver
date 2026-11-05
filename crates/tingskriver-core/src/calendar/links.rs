//! Meeting links in calendar text. Pure; only known meeting platforms count,
//! so an agenda document or a map link never makes an event "a meeting".
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    Teams,
    Zoom,
    GoogleMeet,
    Webex,
    GoToMeeting,
    Jitsi,
    Whereby,
}
impl Platform {
    /// Host (lower case, no port) to platform. Subdomains match.
    fn from_host(host: &str) -> Option<Self> {
        let is = |domain: &str| host == domain || host.ends_with(&format!(".{domain}"));
        if is("teams.microsoft.com") || is("teams.live.com") || is("teams.microsoft.us") {
            Some(Self::Teams)
        } else if is("zoom.us") || is("zoomgov.com") || is("zoom.com") {
            Some(Self::Zoom)
        } else if host == "meet.google.com" {
            Some(Self::GoogleMeet)
        } else if is("webex.com") {
            Some(Self::Webex)
        } else if is("gotomeeting.com") || is("goto.com") {
            Some(Self::GoToMeeting)
        } else if host == "meet.jit.si" {
            Some(Self::Jitsi)
        } else if is("whereby.com") {
            Some(Self::Whereby)
        } else {
            None
        }
    }
}

/// The link carries the meeting's passcode, so like the rest of the calendar
/// data it is only kept in the local cache. No `Debug`.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MeetingLink {
    pub platform: Platform,
    pub url: String,
}

/// First meeting link in `texts`, in the order given (callers pass URL, then
/// LOCATION, then DESCRIPTION). Only https links to a known platform count.
pub fn find_meeting_link<'a>(texts: impl IntoIterator<Item = &'a str>) -> Option<MeetingLink> {
    texts.into_iter().find_map(in_text)
}

fn in_text(text: &str) -> Option<MeetingLink> {
    let lower = text.to_ascii_lowercase();
    let mut from = 0;
    while let Some(found) = lower[from..].find("https://") {
        let start = from + found;
        let end = text[start..]
            .find(|c: char| {
                c.is_whitespace() || matches!(c, '<' | '>' | '"' | '\'' | ')' | ']' | '\\')
            })
            .map_or(text.len(), |n| start + n);
        let candidate = text[start..end].trim_end_matches(['.', ',', ';', ':', '!', '?']);
        if let Ok(url) = reqwest::Url::parse(candidate) {
            if url.username().is_empty() && url.password().is_none() {
                if let Some(platform) = url.host_str().and_then(Platform::from_host) {
                    return Some(MeetingLink {
                        platform,
                        url: candidate.to_owned(),
                    });
                }
            }
        }
        from = end.max(start + 8);
    }
    None
}
