use super::{rules, *};

const MICROPHONE_SECONDS: i64 = 5;
const MISSING_SECONDS: i64 = 30;
const SILENCE_SECONDS: i64 = 300;

struct Candidate {
    source: Source,
    started_at: Timestamp,
    microphone_process: String,
    foreground: bool,
    stable: bool,
}

fn elapsed(now: Timestamp, since: Timestamp, seconds: i64) -> bool {
    now >= since && now.saturating_sub(since) >= seconds
}

fn empty(reason: Reason) -> Decision {
    Decision {
        source: Source::Unknown,
        confidence: Confidence::Low,
        signals: vec![],
        action: Action::None,
        reason,
    }
}

fn calendar_end(snapshot: &Snapshot) -> Option<Timestamp> {
    snapshot
        .calendar
        .iter()
        .filter(|event| {
            event.start < event.end && event.start <= snapshot.now && snapshot.now < event.end
        })
        .map(|event| event.end)
        .min()
}

fn browser_source(source: Source) -> bool {
    matches!(
        source,
        Source::TeamsBrowser | Source::MeetBrowser | Source::ZoomBrowser
    )
}

/// Make a decision using only the supplied snapshot and previous state.
/// The caller executes actions and supplies the resulting recording status.
#[must_use]
pub fn detect(snapshot: &Snapshot, previous: &DetectorState) -> (Decision, DetectorState) {
    if previous
        .last_snapshot_at
        .is_some_and(|time| snapshot.now < time)
    {
        return (empty(Reason::ClockMovedBackwards), previous.clone());
    }
    let end = calendar_end(snapshot);
    let candidate = snapshot
        .windows
        .iter()
        .filter_map(|window| {
            let source = rules::source(window)?;
            let process = rules::normalized(&window.process_name);
            snapshot
                .microphone_users
                .iter()
                .filter(|mic| {
                    rules::microphone_matches(
                        source,
                        &process,
                        &rules::normalized(&mic.process_name),
                    )
                })
                .map(|mic| Candidate {
                    source,
                    started_at: mic.started_at,
                    microphone_process: rules::normalized(&mic.process_name),
                    foreground: window.is_foreground,
                    stable: elapsed(snapshot.now, mic.started_at, MICROPHONE_SECONDS),
                })
                .max_by_key(|c| (c.stable, c.started_at, c.microphone_process.clone()))
        })
        .max_by_key(|c| {
            (
                c.stable,
                end.is_some() || !browser_source(c.source),
                c.foreground,
                std::cmp::Reverse(c.source),
                c.started_at,
                c.microphone_process.clone(),
            )
        });
    let mut state = previous.clone();
    state.last_snapshot_at = Some(snapshot.now);
    if !snapshot.recording_active {
        state.active = None;
    }
    let mut decision = empty(Reason::NoMeeting);
    if let Some(candidate) = candidate {
        decision.source = candidate.source;
        decision.signals.push(Signal::MeetingWindow);
        if candidate.foreground {
            decision.signals.push(Signal::ForegroundWindow);
        }
        if candidate.stable {
            decision.signals.insert(0, Signal::MatchingMicrophone);
            if end.is_some() {
                decision.signals.push(Signal::CalendarOverlap);
            }
            decision.confidence = if end.is_some() || !browser_source(candidate.source) {
                Confidence::High
            } else {
                Confidence::Medium
            };
            let mode = snapshot
                .start_modes
                .get(&candidate.source)
                .copied()
                .unwrap_or(StartMode::Automatic);
            (decision.action, decision.reason) = if snapshot.recording_active {
                (Action::None, Reason::AlreadyRecording)
            } else {
                match mode {
                    StartMode::Off => (Action::None, Reason::SourceDisabled),
                    StartMode::Ask => (Action::SuggestStart, Reason::MeetingDetected),
                    StartMode::Automatic if decision.confidence == Confidence::High => {
                        (Action::Start, Reason::MeetingDetected)
                    }
                    StartMode::Automatic => (Action::SuggestStart, Reason::MeetingDetected),
                }
            };
            let session = SessionIdentity {
                source: candidate.source,
                microphone_process: candidate.microphone_process,
                microphone_started_at: candidate.started_at,
            };
            if state.stopped_session.as_ref() == Some(&session) {
                decision.action = Action::None;
                decision.reason = Reason::MeetingAlreadyStopped;
            } else {
                let same = state
                    .active
                    .as_ref()
                    .is_some_and(|active| active.session == session);
                if !same {
                    state.active = Some(ActiveMeeting {
                        session,
                        observed_at: snapshot.now,
                        calendar_end: end,
                        signals_missing_since: None,
                    });
                    state.stopped_session = None;
                }
            }
        } else {
            decision.reason = Reason::MicrophoneWarmingUp;
        }
    }
    if snapshot.recording_active {
        if let Some(active) = state.active.as_mut() {
            let window_present = snapshot
                .windows
                .iter()
                .any(|w| rules::source(w) == Some(active.session.source));
            let mic_present = snapshot.microphone_users.iter().any(|mic| {
                let process = rules::normalized(&mic.process_name);
                process == active.session.microphone_process
                    && mic.started_at == active.session.microphone_started_at
                    && mic.started_at <= snapshot.now
            });
            if window_present || mic_present {
                active.signals_missing_since = None;
            } else {
                active.signals_missing_since.get_or_insert(snapshot.now);
            }
            let lost = active
                .signals_missing_since
                .is_some_and(|since| elapsed(snapshot.now, since, MISSING_SECONDS));
            let silent = active.calendar_end.is_some_and(|end| {
                let speech = snapshot
                    .last_speech_at
                    .unwrap_or(active.observed_at)
                    .max(active.observed_at);
                elapsed(snapshot.now, end, SILENCE_SECONDS)
                    && elapsed(snapshot.now, speech, SILENCE_SECONDS)
            });
            if lost || silent {
                if lost || decision.source != active.session.source {
                    decision = empty(Reason::NoMeeting);
                }
                decision.source = active.session.source;
                decision.action = Action::Stop;
                decision.reason = if lost {
                    Reason::SignalsLost
                } else {
                    Reason::CalendarEndedWithoutSpeech
                };
                state.stopped_session = Some(active.session.clone());
                state.active = None;
            }
        }
    }
    (decision, state)
}
