//! Pure update policy. Network and installer operations belong to the host.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Preferences {
    pub enabled: bool,
    pub weekly: bool,
    pub last_check: Option<u64>,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            enabled: true,
            weekly: false,
            last_check: None,
        }
    }
}

impl Preferences {
    pub fn due(&self, now: u64) -> bool {
        self.enabled
            && self.last_check.map_or(true, |last| {
                now.saturating_sub(last) >= if self.weekly { 604800 } else { 86400 }
            })
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Release {
    pub version: String,
    pub bytes: u64,
    pub notes: String,
    pub published: Option<String>,
}

impl Release {
    pub fn size_label(&self) -> String {
        format!("{:.1} MB", self.bytes as f64 / 1_000_000.).replace('.', ",")
    }

    pub fn date_label(&self) -> String {
        let Some(date) = &self.published else {
            return crate::strings::UPDATE_NO_DATE.into();
        };
        let parts: Vec<_> = date.split('-').collect();
        if parts.len() == 3 && parts[0].len() == 4 && parts[1].len() == 2 && parts[2].len() == 2 {
            format!("{}.{}.{}", parts[2], parts[1], parts[0])
        } else {
            date.clone()
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum State {
    #[default]
    Inactive,
    Idle,
    Checking,
    Available(Release),
    Downloading(Release, u8),
    Ready(Release),
    Error(String),
    Blocked(Release, bool),
}

#[derive(Clone, Debug)]
pub enum Event {
    Check,
    Found(Option<Release>),
    Download,
    Progress(u8),
    Downloaded,
    Restart,
    Failed(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    None,
    Check,
    Download,
    Restart,
}

/// Effects are the only permission to call the feed or installer.
pub fn transition(state: &State, event: Event, enabled: bool, busy: bool) -> (State, Effect) {
    use Effect as E;
    use State as S;
    match (state, event) {
        (S::Inactive, _) => (S::Inactive, E::None),
        (_, Event::Check)
            if enabled && !matches!(state, S::Checking | S::Downloading(..) | S::Ready(_)) =>
        {
            (S::Checking, E::Check)
        }
        (S::Checking, Event::Found(Some(release))) => (S::Available(release), E::None),
        (S::Checking, Event::Found(None)) => (S::Idle, E::None),
        (S::Available(r) | S::Blocked(r, false), Event::Download) if enabled => {
            if busy {
                (S::Blocked(r.clone(), false), E::None)
            } else {
                (S::Downloading(r.clone(), 0), E::Download)
            }
        }
        (S::Downloading(r, _), Event::Progress(n)) => {
            (S::Downloading(r.clone(), n.min(100)), E::None)
        }
        (S::Downloading(r, _), Event::Downloaded) => (S::Ready(r.clone()), E::None),
        (S::Ready(r) | S::Blocked(r, true), Event::Restart) => {
            if busy {
                (S::Blocked(r.clone(), true), E::None)
            } else {
                (state.clone(), E::Restart)
            }
        }
        (_, Event::Failed(message)) => (S::Error(message), E::None),
        _ => (state.clone(), E::None),
    }
}

impl State {
    pub fn label(&self) -> String {
        match self {
            Self::Inactive => crate::strings::ABOUT_UPDATES_UNAVAILABLE.into(),
            Self::Idle => crate::strings::UPDATE_IDLE.into(),
            Self::Checking => crate::strings::UPDATE_CHECKING.into(),
            Self::Available(r) => crate::strings::UPDATE_AVAILABLE.replace("{version}", &r.version),
            Self::Downloading(r, n) => crate::strings::UPDATE_PROGRESS
                .replace("{version}", &r.version)
                .replace("{percent}", &n.to_string()),
            Self::Ready(r) => crate::strings::UPDATE_READY.replace("{version}", &r.version),
            Self::Blocked(_, true) => crate::strings::UPDATE_BLOCKED_RESTART.into(),
            Self::Blocked(_, false) => crate::strings::UPDATE_BLOCKED_DOWNLOAD.into(),
            Self::Error(message) => message.clone(),
        }
    }

    pub fn release(&self) -> Option<&Release> {
        match self {
            Self::Available(r) | Self::Downloading(r, _) | Self::Ready(r) | Self::Blocked(r, _) => {
                Some(r)
            }
            _ => None,
        }
    }
}

/// Optional packaging metadata, followed by language sections in Markdown.
pub fn release_notes(markdown: &str) -> (String, Option<String>) {
    let published = markdown
        .lines()
        .find_map(|line| line.strip_prefix("Release-Date: "))
        .map(str::to_string);
    let section = |heading: &str| {
        markdown.split_once(heading).map(|(_, text)| {
            text.split("\n## ")
                .next()
                .unwrap_or(text)
                .trim()
                .to_string()
        })
    };
    let notes = section("## da\n")
        .or_else(|| section("## en\n"))
        .unwrap_or_else(|| {
            markdown
                .lines()
                .filter(|line| !line.starts_with("Release-Date: "))
                .collect::<Vec<_>>()
                .join("\n")
        });
    (notes, published)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn release() -> Release {
        Release {
            version: "0.5.1".into(),
            ..Release::default()
        }
    }
    #[test]
    fn disabled_means_no_network_even_for_manual_check() {
        for state in [
            State::Idle,
            State::Available(release()),
            State::Error("offline".into()),
        ] {
            assert_eq!(
                transition(&state, Event::Check, false, false).1,
                Effect::None
            );
            assert_eq!(
                transition(&state, Event::Download, false, false).1,
                Effect::None
            );
        }
        assert!(!Preferences {
            enabled: false,
            ..Preferences::default()
        }
        .due(u64::MAX));
    }
    #[test]
    fn cadence_survives_serialization_and_clock_changes() {
        let prefs = Preferences {
            last_check: Some(100),
            ..Preferences::default()
        };
        let saved = serde_json::to_string(&prefs).unwrap();
        let restored: Preferences = serde_json::from_str(&saved).unwrap();
        assert!(!restored.due(99));
        assert!(!restored.due(86499));
        assert!(restored.due(86500));
        assert!(!Preferences {
            weekly: true,
            ..restored
        }
        .due(86500));
        assert!(serde_json::from_str::<Preferences>("broken").is_err());
        assert!(serde_json::from_str::<Preferences>("{}").is_err());
    }
    #[test]
    fn recording_and_dictation_block_download_and_restart() {
        let r = release();
        for (recording, dictating) in [(true, false), (false, true), (true, true)] {
            let busy = recording || dictating;
            assert_eq!(
                transition(&State::Available(r.clone()), Event::Download, true, busy),
                (State::Blocked(r.clone(), false), Effect::None)
            );
            assert_eq!(
                transition(&State::Ready(r.clone()), Event::Restart, true, busy).1,
                Effect::None
            );
        }
        assert_eq!(
            transition(
                &State::Blocked(r.clone(), false),
                Event::Download,
                true,
                false
            )
            .1,
            Effect::Download
        );
        assert_eq!(
            transition(&State::Ready(r), Event::Restart, true, false).1,
            Effect::Restart
        );
    }
    #[test]
    fn happy_path_and_failures() {
        let (checking, effect) = transition(&State::Idle, Event::Check, true, false);
        assert_eq!(effect, Effect::Check);
        assert_eq!(
            transition(&checking, Event::Found(None), true, false).0,
            State::Idle
        );
        let available = transition(&checking, Event::Found(Some(release())), true, false).0;
        let downloading = transition(&available, Event::Download, true, false).0;
        assert_eq!(
            transition(&downloading, Event::Progress(255), true, false).0,
            State::Downloading(release(), 100)
        );
        assert_eq!(
            transition(&downloading, Event::Downloaded, true, false).0,
            State::Ready(release())
        );
        for message in ["offline", "invalid signature", "access denied", "disk full"] {
            assert_eq!(
                transition(&downloading, Event::Failed(message.into()), true, false).0,
                State::Error(message.into())
            );
        }
        assert_eq!(
            transition(&State::Inactive, Event::Check, true, false).1,
            Effect::None
        );
    }
    #[test]
    fn prefer_danish_and_preserve_english_fallback() {
        assert_eq!(
            release_notes("Release-Date: 2026-11-04\n## en\nEnglish\n## da\nDansk"),
            ("Dansk".into(), Some("2026-11-04".into()))
        );
        assert_eq!(release_notes("## en\nEnglish").0, "English");
        let r = Release {
            bytes: 41_900_000,
            published: Some("2026-11-04".into()),
            ..release()
        };
        assert_eq!(r.size_label(), "41,9 MB");
        assert_eq!(r.date_label(), "04.11.2026");
    }
}
