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
        start: t(28),
        end: t(28) + Duration::hours(1),
    }
}

#[derive(Default)]
struct Fake {
    points: AtomicUsize,
    overviews: AtomicUsize,
}

#[async_trait]
impl Briefer for Fake {
    fn model_id(&self) -> String {
        "fake:1".into()
    }
    async fn key_points(&self, title: &str, minutes: &str) -> Result<Vec<String>, BriefError> {
        self.points.fetch_add(1, Ordering::SeqCst);
        Ok(vec![
            format!("{title}: {minutes}"),
            "to".into(),
            "tre".into(),
            "fire".into(),
        ])
    }
    async fn overview(&self, m: &[MeetingPoints]) -> Result<String, BriefError> {
        self.overviews.fetch_add(1, Ordering::SeqCst);
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
    sqlx::query(include_str!(
        "../migrations/20261105120000_add_briefing_cache.sql"
    ))
    .execute(&pool)
    .await
    .unwrap();
    let c = SqliteCache(pool);
    assert_eq!(c.get("k").await.unwrap(), None);
    c.put("k", "v").await.unwrap();
    c.put("k", "w").await.unwrap();
    assert_eq!(c.get("k").await.unwrap().as_deref(), Some("w"));
}
