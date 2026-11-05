//! Whether a calendar event may start a recording, be offered one, or only
//! be reminded about. Pure and clock-injected: the host passes `now`, the
//! microphone holds it has observed and the ignore list. Nothing here starts
//! anything; [`auto_start_action`] only says what the user's setting allows.
use super::{Event, IgnoredEvents, OwnStatus};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

/// How long after the start a meeting can still be started automatically.
pub const GRACE: Duration = Duration::minutes(5);
/// A microphone held from this long before the start (or later) counts as
/// joining early. Held for longer, it is the previous call, not this one.
pub const EARLY_JOIN_WINDOW: Duration = Duration::minutes(10);
/// How long before the start a reminder is wanted.
pub const REMINDER_LEAD: Duration = Duration::minutes(5);
/// The user's choice for calendar meetings (and #59's meeting detection):
/// "optag / spørg / fra". New users start with `Ask`.
#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutoRecordMode {
    Record,
    #[default]
    Ask,
    Off,
}

/// The microphone has been held by another process since `since`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct MicHold {
    pub since: DateTime<Utc>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    Ignored,
    AllDay,
    NoMeetingLink,
    /// Not organizer or accepted: declined, tentative, unanswered, or the
    /// feed says nothing about the user.
    NotAttending,
    /// Nobody but the user is on the invitation.
    AloneInCalendar,
    EveryoneElseDeclined,
    TooEarly,
    /// More than [`GRACE`] after the start, or already over.
    TooLate,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum StartDecision {
    /// `early` is true when a fresh microphone hold started it before the start.
    Start {
        early: bool,
    },
    Skip(SkipReason),
}

/// What the host may do, given the user's setting.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AutoStartAction {
    Record,
    /// Show the question prompt; record only if the user says so.
    Ask,
    Nothing,
}

/// Everything about the event itself, without the clock.
fn eligibility(event: &Event, ignored: &IgnoredEvents) -> Option<SkipReason> {
    if ignored.is_ignored(event) {
        return Some(SkipReason::Ignored);
    }
    if event.all_day {
        return Some(SkipReason::AllDay);
    }
    if event.meeting_link.is_none() {
        return Some(SkipReason::NoMeetingLink);
    }
    if !matches!(event.own_status, OwnStatus::Organizer | OwnStatus::Accepted) {
        return Some(SkipReason::NotAttending);
    }
    let mut others = event.attendees.iter().filter(|a| !a.is_self).peekable();
    if others.peek().is_none() {
        return Some(SkipReason::AloneInCalendar);
    }
    if others.all(|a| a.declined) {
        return Some(SkipReason::EveryoneElseDeclined);
    }
    None
}

/// May this event start a recording at `now`? `mic_holds` are the holds the
/// host has seen on the microphone; only the time they began matters.
pub fn should_auto_start(
    event: &Event,
    now: DateTime<Utc>,
    mic_holds: &[MicHold],
    ignored: &IgnoredEvents,
) -> StartDecision {
    if let Some(reason) = eligibility(event, ignored) {
        return StartDecision::Skip(reason);
    }
    if now >= event.end || now > event.start + GRACE {
        return StartDecision::Skip(SkipReason::TooLate);
    }
    if now >= event.start {
        return StartDecision::Start { early: false };
    }
    // Before the start: only an early join counts. A hold that began before
    // the window belongs to the previous call, so back-to-back meetings do
    // not start early just because the last one is still open.
    let early = mic_holds
        .iter()
        .any(|h| h.since <= now && h.since >= event.start - EARLY_JOIN_WINDOW);
    if early {
        StartDecision::Start { early: true }
    } else {
        StartDecision::Skip(SkipReason::TooEarly)
    }
}

/// Turn the decision into an action. Nothing starts unless the setting allows
/// it, and nothing starts or is asked while another recording runs.
pub fn auto_start_action(
    mode: AutoRecordMode,
    decision: StartDecision,
    recording_active: bool,
) -> AutoStartAction {
    if recording_active || !matches!(decision, StartDecision::Start { .. }) {
        return AutoStartAction::Nothing;
    }
    match mode {
        AutoRecordMode::Record => AutoStartAction::Record,
        AutoRecordMode::Ask => AutoStartAction::Ask,
        AutoRecordMode::Off => AutoStartAction::Nothing,
    }
}

/// Should the "starter snart" reminder be shown now? Same event rules as the
/// start (so an ignored series gets neither), in the [`REMINDER_LEAD`] before
/// the start. `Off` means no prompts at all.
pub fn should_remind(
    mode: AutoRecordMode,
    event: &Event,
    now: DateTime<Utc>,
    ignored: &IgnoredEvents,
) -> bool {
    mode != AutoRecordMode::Off
        && eligibility(event, ignored).is_none()
        && now < event.start
        && now >= event.start - REMINDER_LEAD
}
