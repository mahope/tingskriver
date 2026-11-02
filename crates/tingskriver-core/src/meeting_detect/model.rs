use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// UTC Unix seconds, supplied by the caller; no clock is read by the detector.
pub type Timestamp = i64;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub now: Timestamp,
    pub microphone_users: Vec<MicrophoneUser>,
    pub windows: Vec<Window>,
    pub calendar: Vec<CalendarEvent>,
    pub dictation_active: bool,
    pub recording_active: bool,
    pub last_speech_at: Option<Timestamp>,
    #[serde(default)]
    pub start_modes: BTreeMap<Source, StartMode>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MicrophoneUser {
    pub process_name: String,
    pub started_at: Timestamp,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Window {
    pub process_name: String,
    pub title: String,
    pub is_browser: bool,
    pub is_foreground: bool,
}

/// Independent of calendar collection and parsing (issue #32).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalendarEvent {
    pub title: String,
    pub start: Timestamp,
    pub end: Timestamp,
    pub attendee_count: Option<u32>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    TeamsApp,
    TeamsBrowser,
    MeetBrowser,
    MeetApp,
    ZoomBrowser,
    ZoomApp,
    #[default]
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StartMode {
    Automatic,
    Ask,
    Off,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    Low,
    Medium,
    High,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    None,
    SuggestStart,
    Start,
    Stop,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Signal {
    MatchingMicrophone,
    MeetingWindow,
    ForegroundWindow,
    CalendarOverlap,
}

/// Fixed codes only: never titles, process strings or calendar content.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    NoMeeting,
    MicrophoneWarmingUp,
    MeetingDetected,
    AlreadyRecording,
    SourceDisabled,
    SignalsLost,
    CalendarEndedWithoutSpeech,
    ClockMovedBackwards,
    MeetingAlreadyStopped,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Decision {
    pub source: Source,
    pub confidence: Confidence,
    pub signals: Vec<Signal>,
    pub action: Action,
    pub reason: Reason,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DetectorState {
    pub last_snapshot_at: Option<Timestamp>,
    pub active: Option<ActiveMeeting>,
    pub stopped_session: Option<SessionIdentity>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionIdentity {
    pub source: Source,
    pub microphone_process: String,
    pub microphone_started_at: Timestamp,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActiveMeeting {
    pub session: SessionIdentity,
    pub observed_at: Timestamp,
    pub calendar_end: Option<Timestamp>,
    pub signals_missing_since: Option<Timestamp>,
}
