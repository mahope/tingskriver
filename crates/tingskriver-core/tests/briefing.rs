use async_trait::async_trait;
use chrono::{DateTime, Duration, TimeZone, Utc};
use std::sync::atomic::{AtomicUsize, Ordering};
use tingskriver_core::briefing::*;

fn t(day: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, day, 9, 0, 0).unwrap()
}

fn past(
    id: &str,
    title: &str,
    uid: Option<&str>,
    who: &[&str],
    day: u32,
    minutes: &str,
) -> PastMeeting {
    PastMeeting {
        id: id.into(),
        title: title.into(),
        series_uid: uid.map(Into::into),
        attendees: who.iter().map(|s| s.to_string()).collect(),
        started_at: t(day),
        minutes: minutes.into(),
    }
}

fn upcoming() -> Upcoming {
    Upcoming {
        series_uid: "serie-1@example.test".into(),
        title: "Ugentligt statusmøde".into(),
        attendees: vec!["a@example.test".into()],
        own_addresses: vec![],
        start: t(28),
        end: t(28) + Duration::hours(1),
    }
}

#[derive(Default)]
struct Fake {
    points: AtomicUsize,
    overviews: AtomicUsize,
    fail_overview: bool,
    empty_points: bool,
}

#[async_trait]
impl Briefer for Fake {
    fn model_id(&self) -> String {
        "fake:1".into()
    }
    async fn key_points(&self, title: &str, minutes: &str) -> Result<Vec<String>, BriefError> {
        self.points.fetch_add(1, Ordering::SeqCst);
        if self.empty_points {
            return Ok(vec![]);
        }
        Ok(vec![
            format!("{title}: {minutes}"),
            "to".into(),
            "tre".into(),
            "fire".into(),
        ])
    }
    async fn overview(&self, m: &[MeetingPoints]) -> Result<String, BriefError> {
        self.overviews.fetch_add(1, Ordering::SeqCst);
        if self.fail_overview {
            return Err(BriefError::Model("overview failed".into()));
        }
        Ok(format!("Samlet over {} møder", m.len()))
    }
}

fn history() -> Vec<PastMeeting> {
    let mut v = vec![
        past(
            "s1",
            "Statusmøde",
            Some("serie-1@example.test"),
            &[],
            1,
            "ref 1",
        ),
        past(
            "s2",
            "Statusmøde",
            Some("serie-1@example.test"),
            &[],
            8,
            "ref 2",
        ),
        past(
            "s3",
            "Statusmøde",
            Some("serie-1@example.test"),
            &[],
            15,
            "ref 3",
        ),
        past(
            "s4",
            "Statusmøde",
            Some("serie-1@example.test"),
            &[],
            22,
            "ref 4",
        ),
        past(
            "s5",
            "Statusmøde",
            Some("serie-1@example.test"),
            &[],
            24,
            "ref 5",
        ),
        past(
            "s6",
            "Statusmøde",
            Some("serie-1@example.test"),
            &[],
            25,
            "ref 6",
        ),
        past(
            "x",
            "Noget andet",
            Some("andet"),
            &["b@example.test"],
            26,
            "andet",
        ),
        past(
            "later",
            "Statusmøde",
            Some("serie-1@example.test"),
            &[],
            29,
            "senere",
        ),
        past(
            "empty",
            "Statusmøde",
            Some("serie-1@example.test"),
            &[],
            27,
            "  ",
        ),
    ];
    v.push(past(
        "title",
        "ugentligt  STATUSmøde",
        None,
        &[],
        27,
        "samme titel",
    ));
    v.push(past(
        "people",
        "Andet",
        None,
        &["A@example.test"],
        27,
        "delt deltager",
    ));
    v
}

#[test]
fn ranks_series_then_title_then_attendees_and_caps_at_five() {
    let r = rank(&upcoming(), &history());
    let ids: Vec<_> = r.iter().map(|(m, _)| m.id.as_str()).collect();
    assert_eq!(ids, ["s6", "s5", "s4", "s3", "s2"]);
    let mut few = history();
    few.retain(|m| !m.id.starts_with('s'));
    let ids: Vec<_> = rank(&upcoming(), &few)
        .iter()
        .map(|(m, r)| (m.id.clone(), *r))
        .collect();
    assert_eq!(
        ids,
        [
            ("title".to_string(), Relation::SameTitle),
            ("people".to_string(), Relation::SharedAttendees)
        ]
    );
}

#[tokio::test]
async fn brief_from_five_latest_with_three_points_each() {
    let f = Fake::default();
    let b = build_brief(&upcoming(), &history(), &f, &InMemoryCache::default())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(b.meetings.len(), 5);
    assert!(b.meetings.iter().all(|m| m.points.len() == 3));
    assert_eq!(b.overview, "Samlet over 5 møder");
    assert_eq!(f.points.load(Ordering::SeqCst), 5);
}

#[tokio::test]
async fn unchanged_minutes_hit_cache_without_model_calls() {
    let f = Fake::default();
    let c = InMemoryCache::default();
    let first = build_brief(&upcoming(), &history(), &f, &c).await.unwrap();
    let second = build_brief(&upcoming(), &history(), &f, &c).await.unwrap();
    assert_eq!(first, second);
    assert_eq!(f.points.load(Ordering::SeqCst), 5);
    assert_eq!(f.overviews.load(Ordering::SeqCst), 1);
    // Changed minutes for one meeting: only that one is recomputed.
    let mut h = history();
    h.iter_mut().find(|m| m.id == "s6").unwrap().minutes = "rettet".into();
    build_brief(&upcoming(), &h, &f, &c).await.unwrap();
    assert_eq!(f.points.load(Ordering::SeqCst), 6);
    assert_eq!(f.overviews.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn no_history_gives_no_brief_and_no_model_call() {
    let f = Fake::default();
    let only_later: Vec<_> = history()
        .into_iter()
        .filter(|m| m.id == "later" || m.id == "x")
        .collect();
    assert!(
        build_brief(&upcoming(), &only_later, &f, &InMemoryCache::default())
            .await
            .unwrap()
            .is_none()
    );
    assert!(build_brief(&upcoming(), &[], &f, &InMemoryCache::default())
        .await
        .unwrap()
        .is_none());
    assert_eq!(
        f.points.load(Ordering::SeqCst) + f.overviews.load(Ordering::SeqCst),
        0
    );
}

#[test]
fn visible_until_five_minutes_after_end() {
    let u = upcoming();
    assert!(visible(u.start - Duration::minutes(30), &u));
    assert!(visible(u.end + Duration::minutes(5), &u));
    assert!(!visible(u.end + Duration::minutes(6), &u));
}

#[tokio::test]
async fn sqlite_cache_roundtrip_and_migration() {
    let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    sqlx::query(
        "INSERT INTO meetings (id, title, created_at, updated_at) VALUES ('m', 'Test', '', '')",
    )
    .execute(&pool)
    .await
    .unwrap();
    let c = SqliteCache(pool);
    assert_eq!(c.get("k").await.unwrap(), None);
    c.put("k", "v", &["m"]).await.unwrap();
    c.put("k", "w", &["m"]).await.unwrap();
    assert_eq!(c.get("k").await.unwrap().as_deref(), Some("w"));
}

/// Fails for one named minutes text, otherwise like `Fake`.
struct Flaky {
    bad: String,
    all: bool,
}

#[async_trait]
impl Briefer for Flaky {
    fn model_id(&self) -> String {
        "flaky:1".into()
    }
    async fn key_points(&self, _title: &str, minutes: &str) -> Result<Vec<String>, BriefError> {
        if self.all || minutes.contains(&self.bad) {
            return Err(BriefError::Model("tidsgrænsen blev nået".into()));
        }
        Ok(vec!["et".into()])
    }
    async fn overview(&self, m: &[MeetingPoints]) -> Result<String, BriefError> {
        Ok(format!("Samlet over {} møder", m.len()))
    }
}

#[tokio::test]
async fn one_failing_meeting_is_skipped() {
    let h = history();
    let bad = h.iter().find(|m| m.id == "s6").unwrap().minutes.clone();
    let f = Flaky { bad, all: false };
    let b = build_brief(&upcoming(), &h, &f, &InMemoryCache::default())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(b.meetings.len(), 4);
}

#[tokio::test]
async fn all_meetings_failing_is_an_error() {
    let f = Flaky {
        bad: String::new(),
        all: true,
    };
    let r = build_brief(&upcoming(), &history(), &f, &InMemoryCache::default()).await;
    assert!(matches!(r, Err(BriefError::Model(_))));
}

#[tokio::test]
async fn failed_overview_keeps_points_and_is_retried() {
    let f = Fake {
        fail_overview: true,
        ..Fake::default()
    };
    let cache = InMemoryCache::default();
    for _ in 0..2 {
        let brief = build_brief(&upcoming(), &history(), &f, &cache)
            .await
            .unwrap()
            .unwrap();
        assert!(brief.overview.is_empty());
        assert_eq!(brief.meetings.len(), 5);
    }
    assert_eq!(f.points.load(Ordering::SeqCst), 5);
    assert_eq!(f.overviews.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn empty_points_are_retried() {
    let f = Fake {
        empty_points: true,
        ..Fake::default()
    };
    let cache = InMemoryCache::default();
    for _ in 0..2 {
        assert!(build_brief(&upcoming(), &history(), &f, &cache)
            .await
            .unwrap()
            .is_none());
    }
    assert_eq!(f.points.load(Ordering::SeqCst), 10);
}

#[tokio::test]
async fn own_addresses_do_not_match_unrelated_one_off_meetings() {
    let mut next = upcoming();
    next.own_addresses = vec![" ME@example.test ".into(), "alias@example.test".into()];
    next.attendees
        .extend(["me@example.test".into(), "ALIAS@example.test".into()]);
    let past = vec![past(
        "unrelated",
        "Other topic",
        None,
        &[
            "me@example.test",
            "alias@example.test",
            "other@example.test",
        ],
        27,
        "minutes",
    )];
    let fake = Fake::default();
    assert!(build_brief(&next, &past, &fake, &InMemoryCache::default())
        .await
        .unwrap()
        .is_none());
    assert_eq!(fake.points.load(Ordering::SeqCst), 0);
    next.attendees.push("other@example.test".into());
    assert_eq!(rank(&next, &past)[0].1, Relation::SharedAttendees);
}

#[tokio::test]
async fn identical_minutes_from_different_meetings_have_separate_cache_entries() {
    let fake = Fake::default();
    let cache = InMemoryCache::default();
    let mut past = vec![history()[0].clone()];
    build_brief(&upcoming(), &past, &fake, &cache)
        .await
        .unwrap();
    past[0].id = "different".into();
    build_brief(&upcoming(), &past, &fake, &cache)
        .await
        .unwrap();
    assert_eq!(fake.points.load(Ordering::SeqCst), 2);
    assert_eq!(fake.overviews.load(Ordering::SeqCst), 2);
}
