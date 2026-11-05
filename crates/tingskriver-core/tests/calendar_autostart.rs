//! #201: the start decision, ignore list, link/status extraction and the four
//! ICS reconciliation regressions. Synthetic data only.
use chrono::{DateTime, Duration, Utc};
use tingskriver_core::calendar::{
    auto_start_action, find_meeting_link, parse, should_auto_start, should_remind, Attendee,
    AutoRecordMode, AutoStartAction, CalendarConfig, Event, IgnoredEvents, MeetingLink, MicHold,
    OwnStatus, Platform, SkipReason, StartDecision,
};

fn at(s: &str) -> DateTime<Utc> {
    s.parse().unwrap()
}
fn person(email: &str, declined: bool, is_self: bool) -> Attendee {
    Attendee {
        name: None,
        email: email.into(),
        declined,
        is_self,
    }
}
/// A meeting that passes every rule: link, accepted, one other attendee.
fn meeting(uid: &str, start: &str, end: &str) -> Event {
    Event {
        uid: uid.into(),
        title: uid.into(),
        attendees: vec![
            person("me@example.test", false, true),
            person("anna@example.test", false, false),
        ],
        start: at(start),
        end: at(end),
        meeting_link: Some(MeetingLink {
            platform: Platform::Teams,
            url: "https://teams.microsoft.com/l/meetup-join/x".into(),
        }),
        own_status: OwnStatus::Accepted,
        ..Event::default()
    }
}
fn decide(e: &Event, now: &str) -> StartDecision {
    should_auto_start(e, at(now), &[], &IgnoredEvents::new())
}
const SKIP_NONE: StartDecision = StartDecision::Start { early: false };

#[test]
fn accepted_meeting_with_link_starts_at_start_and_inside_grace() {
    let e = meeting("a", "2026-03-02T10:00:00Z", "2026-03-02T11:00:00Z");
    assert!(decide(&e, "2026-03-02T10:00:00Z") == SKIP_NONE);
    assert!(decide(&e, "2026-03-02T10:05:00Z") == SKIP_NONE);
    assert!(decide(&e, "2026-03-02T10:05:01Z") == StartDecision::Skip(SkipReason::TooLate));
    assert!(decide(&e, "2026-03-02T09:59:59Z") == StartDecision::Skip(SkipReason::TooEarly));
}
#[test]
fn organizer_starts_too() {
    let mut e = meeting("a", "2026-03-02T10:00:00Z", "2026-03-02T11:00:00Z");
    e.own_status = OwnStatus::Organizer;
    assert!(decide(&e, "2026-03-02T10:01:00Z") == SKIP_NONE);
}
#[test]
fn all_day_and_linkless_events_do_not_start() {
    let mut day = meeting("day", "2026-03-02T00:00:00Z", "2026-03-03T00:00:00Z");
    day.all_day = true;
    assert!(decide(&day, "2026-03-02T00:01:00Z") == StartDecision::Skip(SkipReason::AllDay));
    let mut lunch = meeting("lunch", "2026-03-02T11:00:00Z", "2026-03-02T12:00:00Z");
    lunch.meeting_link = None;
    assert!(
        decide(&lunch, "2026-03-02T11:00:00Z") == StartDecision::Skip(SkipReason::NoMeetingLink)
    );
}
#[test]
fn declined_tentative_unanswered_and_unknown_do_not_start() {
    for status in [
        OwnStatus::Declined,
        OwnStatus::Tentative,
        OwnStatus::NeedsAction,
        OwnStatus::Unknown,
    ] {
        let mut e = meeting("a", "2026-03-02T10:00:00Z", "2026-03-02T11:00:00Z");
        e.own_status = status;
        assert!(
            decide(&e, "2026-03-02T10:00:00Z") == StartDecision::Skip(SkipReason::NotAttending)
        );
    }
}
#[test]
fn alone_or_everyone_else_declined_does_not_start() {
    let mut alone = meeting("a", "2026-03-02T10:00:00Z", "2026-03-02T11:00:00Z");
    alone.attendees = vec![person("me@example.test", false, true)];
    assert!(
        decide(&alone, "2026-03-02T10:00:00Z") == StartDecision::Skip(SkipReason::AloneInCalendar)
    );
    alone.attendees.clear();
    assert!(
        decide(&alone, "2026-03-02T10:00:00Z") == StartDecision::Skip(SkipReason::AloneInCalendar)
    );
    let mut declined = meeting("b", "2026-03-02T10:00:00Z", "2026-03-02T11:00:00Z");
    declined.attendees = vec![
        person("me@example.test", false, true),
        person("anna@example.test", true, false),
        person("bo@example.test", true, false),
    ];
    assert!(
        decide(&declined, "2026-03-02T10:00:00Z")
            == StartDecision::Skip(SkipReason::EveryoneElseDeclined)
    );
    declined
        .attendees
        .push(person("cy@example.test", false, false));
    assert!(decide(&declined, "2026-03-02T10:00:00Z") == SKIP_NONE);
}
#[test]
fn ended_meeting_never_starts() {
    let e = meeting("a", "2026-03-02T10:00:00Z", "2026-03-02T10:03:00Z");
    assert!(decide(&e, "2026-03-02T10:03:00Z") == StartDecision::Skip(SkipReason::TooLate));
}

#[test]
fn fresh_mic_hold_before_start_is_an_early_join() {
    let e = meeting("b", "2026-03-02T10:00:00Z", "2026-03-02T11:00:00Z");
    let holds = [MicHold {
        since: at("2026-03-02T09:55:00Z"),
    }];
    let d = should_auto_start(
        &e,
        at("2026-03-02T09:56:00Z"),
        &holds,
        &IgnoredEvents::new(),
    );
    assert!(d == StartDecision::Start { early: true });
    // The hold has not begun yet at this time.
    let d = should_auto_start(
        &e,
        at("2026-03-02T09:54:00Z"),
        &holds,
        &IgnoredEvents::new(),
    );
    assert!(d == StartDecision::Skip(SkipReason::TooEarly));
}
#[test]
fn previous_call_holding_the_mic_is_not_an_early_join() {
    // Back-to-back: the call before ran 09:00-10:00 and the mic was taken at
    // 09:00, one hour before B. B must not start at 09:55.
    let b = meeting("b", "2026-03-02T10:00:00Z", "2026-03-02T11:00:00Z");
    let holds = [MicHold {
        since: at("2026-03-02T09:00:00Z"),
    }];
    let ignored = IgnoredEvents::new();
    let d = should_auto_start(&b, at("2026-03-02T09:55:00Z"), &holds, &ignored);
    assert!(d == StartDecision::Skip(SkipReason::TooEarly));
    // Exactly ten minutes before the start is still early; one second less is not.
    let edge = [MicHold {
        since: b.start - Duration::minutes(10),
    }];
    assert!(
        should_auto_start(&b, at("2026-03-02T09:55:00Z"), &edge, &ignored)
            == StartDecision::Start { early: true }
    );
    let old = [MicHold {
        since: b.start - Duration::minutes(10) - Duration::seconds(1),
    }];
    assert!(
        should_auto_start(&b, at("2026-03-02T09:55:00Z"), &old, &ignored)
            == StartDecision::Skip(SkipReason::TooEarly)
    );
    // At 10:00 the normal start still applies despite the old hold.
    assert!(should_auto_start(&b, at("2026-03-02T10:00:00Z"), &holds, &ignored) == SKIP_NONE);
}
#[test]
fn back_to_back_meetings_only_the_current_one_starts() {
    let a = meeting("a", "2026-03-02T09:00:00Z", "2026-03-02T10:00:00Z");
    let b = meeting("b", "2026-03-02T10:00:00Z", "2026-03-02T11:00:00Z");
    let holds = [MicHold {
        since: at("2026-03-02T09:00:00Z"),
    }];
    let ignored = IgnoredEvents::new();
    let now = at("2026-03-02T10:00:00Z");
    assert!(
        should_auto_start(&a, now, &holds, &ignored) == StartDecision::Skip(SkipReason::TooLate)
    );
    assert!(should_auto_start(&b, now, &holds, &ignored) == SKIP_NONE);
    // The recording for A is still running at 10:00: B is not started over it.
    assert!(auto_start_action(AutoRecordMode::Record, SKIP_NONE, true) == AutoStartAction::Nothing);
}

#[test]
fn ignored_event_and_series_stop_start_and_reminder() {
    let mut e = meeting("series", "2026-03-02T10:00:00Z", "2026-03-02T11:00:00Z");
    e.recurring = true;
    e.original_start = Some(e.start);
    let mut next = e.clone();
    next.start = at("2026-03-09T10:00:00Z");
    next.end = at("2026-03-09T11:00:00Z");
    next.original_start = Some(next.start);

    let mut ignored = IgnoredEvents::new();
    assert!(should_remind(
        AutoRecordMode::Ask,
        &e,
        at("2026-03-02T09:57:00Z"),
        &ignored
    ));
    ignored.ignore_event(&e);
    assert!(!should_remind(
        AutoRecordMode::Ask,
        &e,
        at("2026-03-02T09:57:00Z"),
        &ignored
    ));
    assert!(
        should_auto_start(&e, at("2026-03-02T10:00:00Z"), &[], &ignored)
            == StartDecision::Skip(SkipReason::Ignored)
    );
    // Only that occurrence: next week still starts.
    assert!(should_auto_start(&next, at("2026-03-09T10:00:00Z"), &[], &ignored) == SKIP_NONE);

    ignored.ignore_series(&e);
    assert!(!should_remind(
        AutoRecordMode::Ask,
        &next,
        at("2026-03-09T09:57:00Z"),
        &ignored
    ));
    assert!(
        should_auto_start(&next, at("2026-03-09T10:00:00Z"), &[], &ignored)
            == StartDecision::Skip(SkipReason::Ignored)
    );
    ignored.unignore_series(&e);
    ignored.unignore_event(&e);
    assert!(should_auto_start(&next, at("2026-03-09T10:00:00Z"), &[], &ignored) == SKIP_NONE);
}
#[test]
fn a_moved_occurrence_stays_ignored() {
    let mut e = meeting("series", "2026-03-02T10:00:00Z", "2026-03-02T11:00:00Z");
    e.original_start = Some(at("2026-03-02T10:00:00Z"));
    let mut ignored = IgnoredEvents::new();
    ignored.ignore_event(&e);
    e.start = at("2026-03-02T14:00:00Z");
    e.end = at("2026-03-02T15:00:00Z");
    assert!(ignored.is_event_ignored(&e));
}
#[test]
fn reminder_window_and_off() {
    let e = meeting("a", "2026-03-02T10:00:00Z", "2026-03-02T11:00:00Z");
    let none = IgnoredEvents::new();
    assert!(should_remind(
        AutoRecordMode::Ask,
        &e,
        at("2026-03-02T09:55:00Z"),
        &none
    ));
    assert!(!should_remind(
        AutoRecordMode::Ask,
        &e,
        at("2026-03-02T09:54:59Z"),
        &none
    ));
    assert!(!should_remind(
        AutoRecordMode::Ask,
        &e,
        at("2026-03-02T10:00:00Z"),
        &none
    ));
    assert!(!should_remind(
        AutoRecordMode::Off,
        &e,
        at("2026-03-02T09:57:00Z"),
        &none
    ));
    let mut linkless = e.clone();
    linkless.meeting_link = None;
    assert!(!should_remind(
        AutoRecordMode::Ask,
        &linkless,
        at("2026-03-02T09:57:00Z"),
        &none
    ));
}

#[test]
fn nothing_starts_unless_the_setting_allows_it() {
    let start = StartDecision::Start { early: false };
    let skip = StartDecision::Skip(SkipReason::NoMeetingLink);
    assert!(AutoRecordMode::default() == AutoRecordMode::Ask);
    assert!(auto_start_action(AutoRecordMode::Record, start, false) == AutoStartAction::Record);
    assert!(auto_start_action(AutoRecordMode::Ask, start, false) == AutoStartAction::Ask);
    assert!(auto_start_action(AutoRecordMode::Off, start, false) == AutoStartAction::Nothing);
    for mode in [
        AutoRecordMode::Record,
        AutoRecordMode::Ask,
        AutoRecordMode::Off,
    ] {
        assert!(auto_start_action(mode, skip, false) == AutoStartAction::Nothing);
        assert!(auto_start_action(mode, start, true) == AutoStartAction::Nothing);
    }
}

#[tokio::test]
async fn ignore_list_round_trips_and_fails_closed() {
    let dir = std::env::temp_dir().join(format!("tingskriver-ignored-{}", std::process::id()));
    let path = dir.join("calendar/ignored.json");
    assert!(IgnoredEvents::load_from(&path).await.unwrap().is_empty());
    let e = meeting("series", "2026-03-02T10:00:00Z", "2026-03-02T11:00:00Z");
    let mut list = IgnoredEvents::new();
    list.ignore_event(&e);
    list.ignore_series(&meeting(
        "other",
        "2026-03-02T10:00:00Z",
        "2026-03-02T11:00:00Z",
    ));
    list.save_to(&path).await.unwrap();
    let back = IgnoredEvents::load_from(&path).await.unwrap();
    assert!(back.is_event_ignored(&e));
    assert!(back.is_series_ignored(&meeting(
        "other",
        "2026-04-01T10:00:00Z",
        "2026-04-01T11:00:00Z"
    )));
    assert!(!back.is_ignored(&meeting(
        "third",
        "2026-03-02T10:00:00Z",
        "2026-03-02T11:00:00Z"
    )));
    std::fs::write(&path, b"{not json").unwrap();
    assert!(IgnoredEvents::load_from(&path).await.is_err());
    std::fs::remove_dir_all(&dir).unwrap();
}
#[test]
fn old_single_occurrences_are_pruned_but_series_stay() {
    let old = meeting("old", "2025-01-02T10:00:00Z", "2025-01-02T11:00:00Z");
    let mut list = IgnoredEvents::new();
    list.ignore_event(&old);
    list.ignore_series(&old);
    list.prune(at("2026-03-02T00:00:00Z"));
    assert!(!list.is_event_ignored(&old));
    assert!(list.is_series_ignored(&old));
}

#[test]
fn links_come_from_known_platforms_only() {
    let teams = "Join: <https://teams.microsoft.com/l/meetup-join/abc>.";
    assert!(find_meeting_link([teams]).unwrap().platform == Platform::Teams);
    let zoom = "Agenda https://docs.example.test/a and https://us02web.zoom.us/j/123?pwd=x, bye";
    let link = find_meeting_link([zoom]).unwrap();
    assert!(link.platform == Platform::Zoom && link.url == "https://us02web.zoom.us/j/123?pwd=x");
    assert!(
        find_meeting_link(["https://meet.google.com/abc-defg-hij"])
            .unwrap()
            .platform
            == Platform::GoogleMeet
    );
    assert!(
        find_meeting_link(["https://acme.webex.com/meet/x"])
            .unwrap()
            .platform
            == Platform::Webex
    );
    assert!(find_meeting_link(["https://docs.example.test/zoom.us", "Kantinen"]).is_none());
    assert!(find_meeting_link(["http://zoom.us/j/1"]).is_none());
    assert!(
        find_meeting_link(["https://evil-zoom.us/j/1", "https://zoom.us.evil.test/j/1"]).is_none()
    );
    assert!(find_meeting_link(["https://user:pw@zoom.us/j/1"]).is_none());
    // First text wins in the order given.
    let both = find_meeting_link(["https://meet.google.com/a", "https://zoom.us/j/1"]).unwrap();
    assert!(both.platform == Platform::GoogleMeet);
}

fn cfg() -> CalendarConfig {
    CalendarConfig {
        own_addresses: vec!["Me@Example.test".into()],
        ..Default::default()
    }
}
fn events(ics: &str) -> Vec<Event> {
    parse(
        ics,
        &cfg(),
        at("2026-03-01T00:00:00Z"),
        at("2026-04-01T00:00:00Z"),
    )
    .unwrap()
}
fn ics(body: &str) -> String {
    format!("BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//t//EN\r\n{body}END:VCALENDAR\r\n")
}

#[test]
fn parser_reads_link_and_own_status() {
    let feed = ics(
        "BEGIN:VEVENT\r\nUID:link@t\r\nDTSTART:20260302T100000Z\r\nDTEND:20260302T110000Z\r\n\
SUMMARY:Status\r\nORGANIZER:mailto:anna@example.test\r\n\
ATTENDEE;CN=Me;PARTSTAT=ACCEPTED:mailto:me@example.test\r\n\
ATTENDEE;PARTSTAT=NEEDS-ACTION:mailto:anna@example.test\r\n\
DESCRIPTION:Agenda\\nJoin https://us02web.zoom.us/j/123\\n\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:loc@t\r\nDTSTART:20260303T100000Z\r\nDTEND:20260303T110000Z\r\n\
ATTENDEE;PARTSTAT=TENTATIVE:mailto:me@example.test\r\n\
LOCATION:https://meet.google.com/abc-defg-hij\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:url@t\r\nDTSTART:20260304T100000Z\r\nDTEND:20260304T110000Z\r\n\
ORGANIZER:mailto:ME@example.test\r\nURL:https://teams.microsoft.com/l/meetup-join/x\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:none@t\r\nDTSTART:20260305T100000Z\r\nDTEND:20260305T110000Z\r\n\
ATTENDEE:mailto:anna@example.test\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:nopart@t\r\nDTSTART:20260306T100000Z\r\nDTEND:20260306T110000Z\r\n\
ATTENDEE:mailto:me@example.test\r\nEND:VEVENT\r\n",
    );
    let e = events(&feed);
    let get = |uid: &str| e.iter().find(|e| e.uid == uid).unwrap();
    let link = get("link@t");
    assert!(link.own_status == OwnStatus::Accepted);
    assert!(link.meeting_link.as_ref().unwrap().platform == Platform::Zoom);
    assert!(link.attendees.iter().filter(|a| a.is_self).count() == 1);
    let loc = get("loc@t");
    assert!(loc.own_status == OwnStatus::Tentative);
    assert!(loc.meeting_link.as_ref().unwrap().platform == Platform::GoogleMeet);
    let url = get("url@t");
    assert!(url.own_status == OwnStatus::Organizer);
    assert!(url.meeting_link.as_ref().unwrap().platform == Platform::Teams);
    assert!(get("none@t").own_status == OwnStatus::Unknown && get("none@t").meeting_link.is_none());
    assert!(get("nopart@t").own_status == OwnStatus::NeedsAction);
    // Without own_addresses nothing can be accepted.
    let anon = parse(
        &feed,
        &CalendarConfig::default(),
        at("2026-03-01T00:00:00Z"),
        at("2026-04-01T00:00:00Z"),
    )
    .unwrap();
    assert!(anon.iter().all(|e| e.own_status == OwnStatus::Unknown));
}

const WEEKLY: &str =
    "BEGIN:VEVENT\r\nUID:weekly@t\r\nDTSTART:20260302T100000Z\r\nDTEND:20260302T110000Z\r\n\
RRULE:FREQ=WEEKLY;COUNT=3\r\nSUMMARY:Ugentligt\r\n";

#[test]
fn regression_moved_occurrence_replaces_the_original_slot() {
    let feed = ics(&format!(
        "{WEEKLY}END:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:weekly@t\r\nRECURRENCE-ID:20260309T100000Z\r\nDTSTART:20260309T140000Z\r\nDTEND:20260309T150000Z\r\nSUMMARY:Ugentligt\r\nEND:VEVENT\r\n"
    ));
    let e = events(&feed);
    let starts: Vec<_> = e.iter().map(|e| e.start).collect();
    assert_eq!(
        starts,
        vec![
            at("2026-03-02T10:00:00Z"),
            at("2026-03-09T14:00:00Z"),
            at("2026-03-16T10:00:00Z")
        ]
    );
    let moved = &e[1];
    assert!(moved.recurring);
    assert_eq!(moved.original_start, Some(at("2026-03-09T10:00:00Z")));
}
#[test]
fn regression_cancelled_override_removes_only_that_occurrence() {
    let feed = ics(&format!(
        "{WEEKLY}END:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:weekly@t\r\nRECURRENCE-ID:20260309T100000Z\r\nSTATUS:CANCELLED\r\nEND:VEVENT\r\n"
    ));
    let starts: Vec<_> = events(&feed).iter().map(|e| e.start).collect();
    assert_eq!(
        starts,
        vec![at("2026-03-02T10:00:00Z"), at("2026-03-16T10:00:00Z")]
    );
}
#[test]
fn regression_duplicate_uid_is_rejected_not_merged() {
    let one = "BEGIN:VEVENT\r\nUID:dup@t\r\nDTSTART:20260302T100000Z\r\nDTEND:20260302T110000Z\r\nEND:VEVENT\r\n";
    let two = "BEGIN:VEVENT\r\nUID:dup@t\r\nDTSTART:20260303T100000Z\r\nDTEND:20260303T110000Z\r\nEND:VEVENT\r\n";
    assert!(parse(
        &ics(&format!("{one}{two}")),
        &cfg(),
        at("2026-03-01T00:00:00Z"),
        at("2026-04-01T00:00:00Z")
    )
    .is_err());
}
#[test]
fn regression_changed_uid_is_a_new_event_without_leftovers() {
    let before = ics("BEGIN:VEVENT\r\nUID:old@t\r\nDTSTART:20260302T100000Z\r\nDTEND:20260302T110000Z\r\nSUMMARY:Flyttet\r\nEND:VEVENT\r\n");
    let after = ics("BEGIN:VEVENT\r\nUID:new@t\r\nDTSTART:20260302T100000Z\r\nDTEND:20260302T110000Z\r\nSUMMARY:Flyttet\r\nEND:VEVENT\r\n");
    let old = events(&before);
    let new = events(&after);
    assert_eq!(old.len(), 1);
    assert_eq!(new.len(), 1);
    assert_eq!(new[0].uid, "new@t");
    // The replacement is a different event for the ignore list: an ignore on
    // the old UID does not silently cover it, and nothing is left from the old.
    let mut ignored = IgnoredEvents::new();
    ignored.ignore_series(&old[0]);
    assert!(!ignored.is_ignored(&new[0]));
}
