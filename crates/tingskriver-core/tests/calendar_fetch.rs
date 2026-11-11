use chrono::{DateTime, Duration, Utc};
use std::{sync::Mutex, time::Duration as Timeout};
use tingskriver_core::calendar::{
    Calendar, CalendarConfig, CalendarError, IcsAddress, RefreshOutcome,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

static LOGS: Mutex<Vec<String>> = Mutex::new(Vec::new());
struct Logger;
impl log::Log for Logger {
    fn enabled(&self, m: &log::Metadata<'_>) -> bool {
        m.target().contains("calendar")
    }
    fn log(&self, r: &log::Record<'_>) {
        if self.enabled(r.metadata()) {
            LOGS.lock().unwrap().push(r.args().to_string());
        }
    }
    fn flush(&self) {}
}
static LOGGER: Logger = Logger;
fn now() -> DateTime<Utc> {
    "2026-02-01T09:55:00Z".parse().unwrap()
}
async fn reply(listener: &TcpListener, status: &str, headers: &str, body: &str) -> String {
    let (mut stream, _) = listener.accept().await.unwrap();
    let mut request = Vec::new();
    loop {
        let mut b = [0; 1024];
        let n = stream.read(&mut b).await.unwrap();
        assert!(n > 0);
        request.extend_from_slice(&b[..n]);
        if request.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
    }
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n{headers}\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes()).await.unwrap();
    String::from_utf8(request).unwrap()
}

#[tokio::test]
async fn etag_cadence_offline_cache_and_secret_safety() {
    let _ = log::set_logger(&LOGGER);
    log::set_max_level(log::LevelFilter::Info);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let secret = format!(
        "http://{}/private-calendar-token?key=synthetic-secret",
        listener.local_addr().unwrap()
    );
    let address = IcsAddress::new(secret.clone()).unwrap();
    assert!(!format!("{address:?} {address}").contains("synthetic-secret"));
    let config = CalendarConfig::default();
    assert!(!serde_json::to_string(&config).unwrap().contains(&secret));
    assert!(!format!("{:?}", IcsAddress::new(format!("invalid-{secret}"))).contains(&secret));
    assert!(!format!("{:?}", CalendarError::Fetch).contains(&secret));
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("calendar/events.json");
    let mut calendar = Calendar::open_at(
        address.clone(),
        config.clone(),
        path.clone(),
        Timeout::from_secs(1),
    )
    .await
    .unwrap();
    let (result, request) = tokio::join!(
        calendar.refresh_if_due(now()),
        reply(
            &listener,
            "200 OK",
            "ETag: \"v1\"\r\n",
            include_str!("fixtures/calendar/google.ics")
        )
    );
    assert_eq!(result, RefreshOutcome::Updated);
    assert!(!request.to_lowercase().contains("if-none-match"));
    let original = calendar.events_around(now());
    assert!(!original.is_empty());
    assert_eq!(
        calendar.refresh_if_due(now() + Duration::minutes(9)).await,
        RefreshOutcome::NotDue
    );
    let (result, request) = tokio::join!(
        calendar.refresh_if_due(now() + Duration::minutes(10)),
        reply(&listener, "304 Not Modified", "", "")
    );
    assert_eq!(result, RefreshOutcome::NotModified);
    assert!(request.to_lowercase().contains("if-none-match: \"v1\""));
    assert!(calendar.events_around(now()) == original);
    let (result, _) = tokio::join!(
        calendar.refresh_if_due(now() + Duration::minutes(20)),
        reply(&listener, "200 OK", "", "invalid private-calendar-token")
    );
    assert_eq!(result, RefreshOutcome::Cached);
    assert!(calendar.events_around(now()) == original);
    drop(listener);
    assert_eq!(
        calendar.refresh_if_due(now() + Duration::minutes(30)).await,
        RefreshOutcome::Cached
    );
    assert!(calendar.events_around(now()) == original);
    let mut reopened = Calendar::open_at(
        address.clone(),
        config.clone(),
        path.clone(),
        Timeout::from_millis(50),
    )
    .await
    .unwrap();
    assert!(reopened.events_around(now()) == original);
    assert_eq!(
        reopened.refresh_if_due(now() + Duration::hours(1)).await,
        RefreshOutcome::Cached
    );
    assert!(reopened.events_around(now()) == original);
    let disk = tokio::fs::read_to_string(&path).await.unwrap();
    let own_config = CalendarConfig {
        own_addresses: vec!["anna@example.test".into()],
        ..config
    };
    let with_identity =
        Calendar::open_at(address, own_config, path.clone(), Timeout::from_millis(50))
            .await
            .unwrap();
    assert!(!with_identity
        .events_around(now())
        .iter()
        .any(|e| e.title == "Declined"));
    assert!(!disk.contains("synthetic-secret"));
    assert!(!disk.contains("private-calendar-token"));
    let logs = LOGS.lock().unwrap();
    assert!(!logs.is_empty());
    assert!(logs.iter().all(|s| !s.contains("synthetic-secret")
        && !s.contains("private-calendar-token")
        && !s.contains("anna@example.test")));
}

#[tokio::test]
async fn timeout_is_bounded_and_keeps_cache() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address =
        IcsAddress::new(format!("http://{}/secret", listener.local_addr().unwrap())).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut calendar = Calendar::open_at(
        address,
        CalendarConfig::default(),
        dir.path().join("events.json"),
        Timeout::from_millis(100),
    )
    .await
    .unwrap();
    let (result, _) = tokio::join!(
        calendar.refresh_if_due(now()),
        reply(
            &listener,
            "200 OK",
            "",
            include_str!("fixtures/calendar/google.ics")
        )
    );
    assert_eq!(result, RefreshOutcome::Updated);
    let original = calendar.events_around(now());
    let stalled = async {
        let (_stream, _) = listener.accept().await.unwrap();
        tokio::time::sleep(Timeout::from_millis(500)).await;
    };
    let (result, _) = tokio::join!(
        tokio::time::timeout(
            Timeout::from_millis(400),
            calendar.refresh_if_due(now() + Duration::minutes(10))
        ),
        stalled
    );
    assert_eq!(result.unwrap(), RefreshOutcome::Cached);
    assert!(calendar.events_around(now()) == original);
}

#[tokio::test]
async fn timezone_change_discards_cache_and_refetches_without_etag() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address =
        IcsAddress::new(format!("http://{}/secret", listener.local_addr().unwrap())).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("events.json");
    let body = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nUID:floating\nDTSTART:20260201T090000\nDURATION:PT1H\nEND:VEVENT\nEND:VCALENDAR";
    let mut calendar = Calendar::open_at(
        address.clone(),
        CalendarConfig::default(),
        path.clone(),
        Timeout::from_secs(1),
    )
    .await
    .unwrap();
    let (result, _) = tokio::join!(
        calendar.refresh_if_due(now()),
        reply(&listener, "200 OK", "ETag: \"v1\"\r\n", body)
    );
    assert_eq!(result, RefreshOutcome::Updated);
    assert_eq!(
        calendar.events_around(now())[0].start,
        "2026-02-01T08:00:00Z".parse::<DateTime<Utc>>().unwrap()
    );
    let mut reopened = Calendar::open_at(
        address,
        CalendarConfig {
            timezone: "UTC".into(),
            ..CalendarConfig::default()
        },
        path,
        Timeout::from_secs(1),
    )
    .await
    .unwrap();
    assert!(reopened.events_around(now()).is_empty());
    let (result, request) = tokio::join!(
        reopened.refresh_if_due(now()),
        reply(&listener, "200 OK", "ETag: \"v1\"\r\n", body)
    );
    assert_eq!(result, RefreshOutcome::Updated);
    assert!(!request.to_lowercase().contains("if-none-match"));
    assert_eq!(
        reopened.events_around(now())[0].start,
        "2026-02-01T09:00:00Z".parse::<DateTime<Utc>>().unwrap()
    );
}

#[tokio::test]
async fn calendar_identity_is_resolved_from_current_config_offline() {
    use tingskriver_core::calendar::{
        auto_start_action, should_auto_start, AutoRecordMode, AutoStartAction, IgnoredEvents,
        OwnStatus,
    };
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = IcsAddress::new(format!(
        "http://{}/calendar",
        listener.local_addr().unwrap()
    ))
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("events.json");
    let config = CalendarConfig {
        own_addresses: vec!["alias@example.test".into()],
        ..Default::default()
    };
    let mut calendar = Calendar::open_at(
        address.clone(),
        config.clone(),
        path.clone(),
        Timeout::from_secs(1),
    )
    .await
    .unwrap();
    let body = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nUID:identity\nDTSTART:20260201T095500Z\nDURATION:PT1H\nURL:https://zoom.us/j/123\nORGANIZER:mailto:host@example.test\nATTENDEE;PARTSTAT=ACCEPTED:mailto:alias@example.test\nEND:VEVENT\nEND:VCALENDAR";
    let (result, _) = tokio::join!(
        calendar.refresh_if_due(now()),
        reply(&listener, "200 OK", "", body)
    );
    assert_eq!(result, RefreshOutcome::Updated);
    drop(listener);
    let disk = tokio::fs::read_to_string(&path).await.unwrap();
    assert!(disk.contains("ACCEPTED"));
    assert!(!disk.contains("own_status"));
    assert!(!disk.contains("is_self"));
    for (addresses, status, action) in [
        (
            vec!["alias@example.test".into()],
            OwnStatus::Accepted,
            AutoStartAction::Record,
        ),
        (vec![], OwnStatus::Unknown, AutoStartAction::Nothing),
        (
            vec!["other@example.test".into()],
            OwnStatus::Unknown,
            AutoStartAction::Nothing,
        ),
        (
            vec!["host@example.test".into()],
            OwnStatus::Organizer,
            AutoStartAction::Record,
        ),
        (
            vec!["alias@example.test".into()],
            OwnStatus::Accepted,
            AutoStartAction::Record,
        ),
    ] {
        let reopened = Calendar::open_at(
            address.clone(),
            CalendarConfig {
                own_addresses: addresses,
                ..config.clone()
            },
            path.clone(),
            Timeout::from_millis(50),
        )
        .await
        .unwrap();
        let events = reopened.events_around(now());
        let event = &events[0];
        assert!(event.own_status == status);
        assert_eq!(
            event.attendees.iter().filter(|a| a.is_self).count(),
            usize::from(status != OwnStatus::Unknown)
        );
        assert!(
            auto_start_action(
                AutoRecordMode::Record,
                should_auto_start(event, now(), &[], &IgnoredEvents::new()),
                false
            ) == action
        );
    }
}

#[tokio::test]
async fn calendar_v2_cache_refetches_identity_with_unchanged_etag() {
    use tingskriver_core::calendar::OwnStatus;

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = IcsAddress::new(format!(
        "http://{}/calendar",
        listener.local_addr().unwrap()
    ))
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("events.json");
    let config = CalendarConfig {
        own_addresses: vec!["alias@example.test".into()],
        ..Default::default()
    };
    let body = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nUID:migration\nDTSTART:20260201T095500Z\nDURATION:PT1H\nATTENDEE;PARTSTAT=ACCEPTED:mailto:alias@example.test\nEND:VEVENT\nEND:VCALENDAR";
    let legacy = serde_json::json!({
        "version": 2,
        "timezone": config.timezone,
        "etag": "\"v1\"",
        "fetched_at": now() - Duration::minutes(10),
        "range_start": now() - Duration::days(30),
        "range_end": now() + Duration::days(366),
        "events": [{
            "uid": "migration", "title": "", "start": now(),
            "end": now() + Duration::hours(1), "all_day": false,
            "own_status": "accepted",
            "attendees": [{"email": "alias@example.test", "is_self": true}]
        }]
    });
    tokio::fs::write(&path, serde_json::to_vec(&legacy).unwrap())
        .await
        .unwrap();
    let mut calendar = Calendar::open_at(
        address.clone(),
        config.clone(),
        path.clone(),
        Timeout::from_secs(1),
    )
    .await
    .unwrap();
    let server = async {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        while !request.windows(4).any(|w| w == b"\r\n\r\n") {
            let mut byte = [0];
            stream.read_exact(&mut byte).await.unwrap();
            request.extend_from_slice(&byte);
        }
        let request = String::from_utf8(request).unwrap();
        let (status, body) = if request.to_lowercase().contains("if-none-match") {
            ("304 Not Modified", "")
        } else {
            ("200 OK", body)
        };
        let response = format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\nETag: \"v1\"\r\nConnection: close\r\n\r\n{body}", body.len());
        stream.write_all(response.as_bytes()).await.unwrap();
        request
    };
    let (result, request) = tokio::join!(calendar.refresh_if_due(now()), server);
    assert_eq!(result, RefreshOutcome::Updated);
    assert!(!request.to_lowercase().contains("if-none-match"));
    assert!(calendar.events_around(now())[0].own_status == OwnStatus::Accepted);

    let mut reopened = Calendar::open_at(address, config, path, Timeout::from_secs(1))
        .await
        .unwrap();
    assert!(reopened.events_around(now())[0].own_status == OwnStatus::Accepted);
    let (result, request) = tokio::join!(
        reopened.refresh_if_due(now() + Duration::minutes(10)),
        reply(&listener, "304 Not Modified", "", "")
    );
    assert_eq!(result, RefreshOutcome::NotModified);
    assert!(request.to_lowercase().contains("if-none-match: \"v1\""));
    assert!(reopened.events_around(now())[0].own_status == OwnStatus::Accepted);
}
