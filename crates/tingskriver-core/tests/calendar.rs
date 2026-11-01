use chrono::{DateTime, Utc};
use tingskriver_core::calendar::{parse, CalendarConfig};

fn at(s: &str) -> DateTime<Utc> {
    s.parse().unwrap()
}
fn events(fixture: &str) -> Vec<tingskriver_core::calendar::Event> {
    parse(
        fixture,
        &CalendarConfig::default(),
        at("2026-01-01T00:00:00Z"),
        at("2027-01-01T00:00:00Z"),
    )
    .unwrap()
}
const GOOGLE: &str = include_str!("fixtures/calendar/google.ics");
const OUTLOOK: &str = include_str!("fixtures/calendar/outlook.ics");
const RULES: &str = include_str!("fixtures/calendar/rules.ics");
const EDGES: &str = include_str!("fixtures/calendar/edge-cases.ics");

#[test]
fn duration_distinguishes_nominal_days_from_elapsed_hours() {
    let e = events(EDGES);
    let hours = e.iter().find(|e| e.uid == "elapsed-duration").unwrap();
    assert_eq!(hours.end - hours.start, chrono::Duration::hours(2));
    let day = e.iter().find(|e| e.uid == "nominal-duration").unwrap();
    assert_eq!(day.end - day.start, chrono::Duration::hours(24));
}
#[test]
fn recurring_dtend_preserves_exact_duration_across_dst() {
    let e = events(EDGES);
    let e: Vec<_> = e.iter().filter(|e| e.uid == "exact-end").collect();
    assert_eq!(e.len(), 2);
    assert!(e
        .iter()
        .all(|e| e.end - e.start == chrono::Duration::hours(2)));
}

#[test]
fn daily_interval_and_month_31() {
    let e = events(EDGES);
    let dates: Vec<_> = e
        .iter()
        .filter(|e| e.uid == "daily-interval")
        .map(|e| e.start.date_naive().to_string())
        .collect();
    assert_eq!(dates, vec!["2026-01-01", "2026-01-03", "2026-01-05"]);
    let dates: Vec<_> = e
        .iter()
        .filter(|e| e.uid == "month-31")
        .map(|e| e.start.date_naive().to_string())
        .collect();
    assert_eq!(dates, vec!["2026-01-31", "2026-03-31", "2026-05-31"]);
}
#[test]
fn nonexistent_recurrence_time_is_skipped_without_consuming_count() {
    let e = events(EDGES);
    let e: Vec<_> = e.iter().filter(|e| e.uid == "dst-gap").collect();
    assert_eq!(e.len(), 3);
    assert_eq!(e[1].start, at("2026-03-30T00:30:00Z"));
    assert_eq!(e[2].start, at("2026-03-31T00:30:00Z"));
}
#[test]
fn all_day_dst_is_one_local_day_and_fold_uses_first_occurrence() {
    let e = events(EDGES);
    let day = e.iter().find(|e| e.uid == "all-day-dst").unwrap();
    assert_eq!(day.end - day.start, chrono::Duration::hours(23));
    assert_eq!(
        e.iter().find(|e| e.uid == "dst-fold").unwrap().start,
        at("2026-10-25T00:30:00Z")
    );
}
#[test]
fn declined_override_suppresses_original_and_replacement() {
    let c = CalendarConfig {
        own_addresses: vec!["me@example.test".into()],
        ..Default::default()
    };
    let e = parse(
        EDGES,
        &c,
        at("2026-01-01T00:00:00Z"),
        at("2027-01-01T00:00:00Z"),
    )
    .unwrap();
    assert_eq!(e.iter().filter(|e| e.uid == "decline-override").count(), 2);
    assert!(!e
        .iter()
        .any(|e| e.uid == "decline-override" && e.start == at("2026-01-02T09:00:00Z")));
}

#[test]
fn weekly_interval_and_multiple_weekdays() {
    let e = events(RULES);
    let dates: Vec<_> = e
        .iter()
        .filter(|e| e.uid == "weekly")
        .map(|e| e.start.date_naive().to_string())
        .collect();
    assert_eq!(
        dates,
        vec![
            "2026-01-05",
            "2026-01-07",
            "2026-01-19",
            "2026-01-21",
            "2026-02-02"
        ]
    );
}
#[test]
fn negative_monthdays_skip_invalid_dates() {
    let e = events(RULES);
    let dates: Vec<_> = e
        .iter()
        .filter(|e| e.uid == "month-end")
        .map(|e| e.start.date_naive().to_string())
        .collect();
    assert_eq!(dates, vec!["2026-01-31", "2026-02-28", "2026-03-31"]);
}
#[test]
fn utc_until_is_inclusive_across_dst() {
    let e = events(RULES);
    let e: Vec<_> = e.iter().filter(|e| e.uid == "until").collect();
    assert_eq!(e.len(), 3);
    assert_eq!(e[2].start, at("2026-03-30T07:00:00Z"));
}
#[test]
fn cancelled_override_without_dtstart_suppresses_occurrence() {
    let e = events(RULES);
    let e: Vec<_> = e.iter().filter(|e| e.uid == "cancel-instance").collect();
    assert_eq!(e.len(), 2);
    assert_eq!(e[1].start, at("2026-01-03T12:00:00Z"));
}
#[test]
fn another_attendees_decline_does_not_hide_meeting() {
    let c = CalendarConfig {
        own_addresses: vec!["me@example.test".into()],
        ..Default::default()
    };
    let e = parse(
        RULES,
        &c,
        at("2026-01-01T00:00:00Z"),
        at("2027-01-01T00:00:00Z"),
    )
    .unwrap();
    assert!(e.iter().any(|e| e.uid == "other-declined"));
}
#[test]
fn rdate_and_exdate_can_use_a_different_zone() {
    let e = events(RULES);
    let e: Vec<_> = e.iter().filter(|e| e.uid == "rdate-zone").collect();
    assert_eq!(e.len(), 1);
    assert_eq!(e[0].start, at("2026-04-02T07:00:00Z"));
}
#[test]
fn leap_year_recurrence_skips_invalid_days_without_consuming_count() {
    let e = parse(
        RULES,
        &CalendarConfig::default(),
        at("2024-01-01T00:00:00Z"),
        at("2035-01-01T00:00:00Z"),
    )
    .unwrap();
    let years: Vec<_> = e
        .iter()
        .filter(|e| e.uid == "yearly-leap")
        .map(|e| e.start.date_naive().to_string())
        .collect();
    assert_eq!(years, vec!["2024-02-29", "2028-02-29", "2032-02-29"]);
}
#[test]
fn custom_vtimezone_rules_are_used_for_unknown_ids() {
    let e = events(include_str!("fixtures/calendar/custom-zone.ics"));
    assert_eq!(e.len(), 3);
    assert_eq!(e[0].start, at("2026-03-22T07:00:00Z"));
    assert_eq!(e[1].start, at("2026-03-29T06:00:00Z"));
}
#[test]
fn unsupported_rules_and_malformed_input_fail_without_echoing_input() {
    for source in [
        GOOGLE.replace("20260201T090000", "secret"),
        OUTLOOK.replace("FREQ=WEEKLY;COUNT=3", "FREQ=HOURLY;COUNT=3"),
        OUTLOOK.replace("Europe/Copenhagen", "Unknown/secret"),
        OUTLOOK.replace("BYDAY=SU", "BYDAY=éM"),
        OUTLOOK.replace("BYDAY=SU", "BYDAY=-2147483648SU"),
    ] {
        let err = parse(
            &source,
            &CalendarConfig::default(),
            at("2026-01-01T00:00:00Z"),
            at("2027-01-01T00:00:00Z"),
        )
        .err()
        .unwrap();
        assert!(!format!("{err:?} {err}").contains("secret"));
    }
}

#[test]
fn google_utc_and_duration() {
    let e = events(GOOGLE);
    let e = e.iter().find(|e| e.uid == "google-utc").unwrap();
    assert_eq!(e.end - e.start, chrono::Duration::minutes(45));
}
#[test]
fn folded_and_escaped_title() {
    assert_eq!(
        events(GOOGLE)
            .iter()
            .find(|e| e.uid == "google-utc")
            .unwrap()
            .title,
        "Planning, notes; next\nweek"
    );
}
#[test]
fn attendees() {
    assert_eq!(
        events(GOOGLE)
            .iter()
            .find(|e| e.uid == "google-utc")
            .unwrap()
            .attendees[0]
            .email,
        "anna@example.test"
    );
}
#[test]
fn all_day_exclusive_end() {
    let e = events(GOOGLE);
    let e = e.iter().find(|e| e.all_day).unwrap();
    assert_eq!(e.end - e.start, chrono::Duration::days(2));
}
#[test]
fn cancellations_filtered() {
    assert!(!events(GOOGLE).iter().any(|e| e.title == "Cancelled"));
}
#[test]
fn declines_require_own_identity() {
    let mut c = CalendarConfig::default();
    c.own_addresses.push("ANNA@example.test".into());
    let e = parse(
        GOOGLE,
        &c,
        at("2026-01-01T00:00:00Z"),
        at("2027-01-01T00:00:00Z"),
    )
    .unwrap();
    assert!(!e.iter().any(|e| e.title == "Declined"));
    assert!(events(GOOGLE).iter().any(|e| e.title == "Declined"));
}
#[test]
fn spring_dst_preserves_local_hour() {
    let e = events(OUTLOOK);
    let e: Vec<_> = e.iter().filter(|e| e.title == "Spring").collect();
    assert_eq!(
        e.iter().map(|e| e.start.to_rfc3339()).collect::<Vec<_>>(),
        vec![
            "2026-03-22T08:00:00+00:00",
            "2026-03-29T07:00:00+00:00",
            "2026-04-05T07:00:00+00:00"
        ]
    );
}
#[test]
fn autumn_dst_preserves_local_hour() {
    let e = events(OUTLOOK);
    let e: Vec<_> = e.iter().filter(|e| e.title == "Autumn").collect();
    assert_eq!(e[0].start, at("2026-10-18T07:00:00Z"));
    assert_eq!(e[1].start, at("2026-10-25T08:00:00Z"));
}
#[test]
fn exclusions_and_additions() {
    let e = events(OUTLOOK);
    let e: Vec<_> = e.iter().filter(|e| e.title == "Daily").collect();
    assert_eq!(e.len(), 3);
    assert_eq!(e[1].start, at("2026-02-03T12:00:00Z"));
    assert_eq!(e[2].start, at("2026-02-10T12:00:00Z"));
}
#[test]
fn moved_instance_replaces_original() {
    let e = events(OUTLOOK);
    assert!(!e.iter().any(|e| e.start == at("2026-06-02T12:00:00Z")));
    assert!(e
        .iter()
        .any(|e| e.title == "Moved" && e.start == at("2026-06-03T14:00:00Z")));
}
#[test]
fn monthly_byday_and_yearly_bymonthday() {
    let e = events(OUTLOOK);
    assert_eq!(e.iter().filter(|e| e.title == "Monthly").count(), 3);
    assert!(e
        .iter()
        .any(|e| e.title == "Monthly" && e.start == at("2026-02-02T12:00:00Z")));
    assert!(e
        .iter()
        .any(|e| e.title == "Yearly" && e.start == at("2026-07-15T12:00:00Z")));
}
#[test]
fn floating_uses_configured_zone() {
    assert!(events(GOOGLE)
        .iter()
        .any(|e| e.title == "Floating" && e.start == at("2026-02-01T08:00:00Z")));
}
