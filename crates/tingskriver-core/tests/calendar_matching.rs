use chrono::{DateTime, Duration, Utc};
use tingskriver_core::calendar::{match_meeting, Attendee, Event};
fn at(s: &str) -> DateTime<Utc> {
    s.parse().unwrap()
}
fn event(id: &str, start: &str, end: &str) -> Event {
    Event {
        uid: id.into(),
        title: id.into(),
        attendees: vec![],
        start: at(start),
        end: at(end),
        all_day: false,
    }
}
#[test]
fn closest_start_wins_even_without_attendees() {
    let now = at("2026-02-01T10:15:00Z");
    let earlier = event("earlier", "2026-02-01T09:00:00Z", "2026-02-01T11:00:00Z");
    let exact = event("exact", "2026-02-01T10:00:00Z", "2026-02-01T10:30:00Z");
    let matched = match_meeting(now, now - Duration::minutes(15), &[earlier, exact]).unwrap();
    assert_eq!(matched.title, "exact");
    assert_eq!(matched.end, at("2026-02-01T10:30:00Z"));
}
#[test]
fn all_day_is_low_priority_and_expired_events_are_rejected() {
    let now = at("2026-02-01T10:15:00Z");
    let mut day = event("day", "2026-02-01T00:00:00Z", "2026-02-02T00:00:00Z");
    day.all_day = true;
    let mut meeting = event("meeting", "2026-02-01T10:00:00Z", "2026-02-01T11:00:00Z");
    meeting.attendees.push(Attendee {
        name: Some("Anna Eksempel".into()),
        email: "anna@example.test".into(),
        declined: false,
    });
    let matched = match_meeting(now, now - Duration::minutes(5), &[day.clone(), meeting]).unwrap();
    assert_eq!(matched.title, "meeting");
    assert_eq!(matched.attendees.len(), 1);
    assert_eq!(match_meeting(now, now, &[day]).unwrap().title, "day");
    assert!(match_meeting(
        now,
        now,
        &[event(
            "expired",
            "2026-02-01T09:00:00Z",
            "2026-02-01T10:15:00Z"
        )]
    )
    .is_none());
    assert!(match_meeting(now, now, &[]).is_none());
    assert!(match_meeting(now, now + Duration::seconds(1), &[]).is_none());
}
#[test]
fn ties_are_independent_of_candidate_order_and_early_starts_work() {
    let now = at("2026-02-01T09:55:00Z");
    let a = event("a", "2026-02-01T10:00:00Z", "2026-02-01T11:00:00Z");
    let b = event("b", "2026-02-01T10:00:00Z", "2026-02-01T11:00:00Z");
    assert_eq!(
        match_meeting(now, now, &[b.clone(), a.clone()])
            .unwrap()
            .uid,
        "a"
    );
    assert_eq!(match_meeting(now, now, &[a, b]).unwrap().uid, "a");
}
