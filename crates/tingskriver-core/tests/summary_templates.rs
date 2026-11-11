use tingskriver_core::summary::templates::get_template;

#[test]
fn danish_templates_are_embedded_and_valid() {
    for (id, name) in [
        ("standard", "Standardreferat"),
        ("kunde", "Kundemøde"),
        ("one_to_one", "1:1"),
        ("intern_status", "Intern status"),
    ] {
        let template = get_template(id).unwrap();
        assert_eq!(template.name, name);
        template.validate().unwrap();
        assert!(template
            .sections
            .iter()
            .any(|s| s.title == "Aftaler og opgaver"));
    }
}

#[test]
fn calendar_rules_prioritize_one_to_one_then_external_domains() {
    use tingskriver_core::{
        calendar::{parse, CalendarConfig},
        summary::template_rules::select_template,
    };
    let config = CalendarConfig {
        own_addresses: vec!["self@example.test".into()],
        ..Default::default()
    };
    let events = parse(
        include_str!("fixtures/calendar/templates.ics"),
        &config,
        "2026-01-01T00:00:00Z".parse().unwrap(),
        "2027-01-01T00:00:00Z".parse().unwrap(),
    )
    .unwrap();
    for event in &events {
        let expected = match event.uid.as_str() {
            "one" => "one_to_one",
            "client" => "kunde",
            _ => "standard",
        };
        assert_eq!(select_template(Some(event), &config), expected);
    }
    assert_eq!(events.len(), 4);
    assert_eq!(select_template(None, &config), "standard");
    assert_eq!(
        select_template(
            Some(events.iter().find(|e| e.uid == "one").unwrap()),
            &CalendarConfig::default()
        ),
        "one_to_one"
    );
}

#[test]
fn calendar_domain_rules_do_not_guess_or_match_domain_suffixes() {
    use tingskriver_core::{
        calendar::{Attendee, CalendarConfig, Event},
        summary::template_rules::select_template,
    };
    let mut event = Event {
        uid: "domains".into(),
        title: "Status".into(),
        start: "2026-02-01T10:00:00Z".parse().unwrap(),
        end: "2026-02-01T11:00:00Z".parse().unwrap(),
        all_day: false,
        attendees: vec![Attendee {
            name: None,
            email: "guest@other.test".into(),
            declined: false,
        }],
    };
    assert_eq!(
        select_template(Some(&event), &CalendarConfig::default()),
        "standard"
    );
    let config = CalendarConfig {
        own_addresses: vec!["self@example.test".into(), "self@other.test".into()],
        ..Default::default()
    };
    assert_eq!(select_template(Some(&event), &config), "standard");
    event.attendees[0].email = "guest@notexample.test".into();
    assert_eq!(select_template(Some(&event), &config), "kunde");
    let bare = CalendarConfig {
        own_addresses: vec!["mailto:example.test".into()],
        ..Default::default()
    };
    event.attendees[0].email = "mailto:guest@EXAMPLE.TEST".into();
    assert_eq!(select_template(Some(&event), &bare), "standard");
    let public = CalendarConfig {
        own_addresses: vec!["self@gmail.com".into()],
        ..Default::default()
    };
    event.attendees[0].email = "guest@gmail.com".into();
    assert_eq!(select_template(Some(&event), &public), "kunde");
    event.attendees[0].email = "self@gmail.com".into();
    assert_eq!(select_template(Some(&event), &public), "standard");
    event.attendees[0].email = "invalid".into();
    assert_eq!(select_template(Some(&event), &config), "standard");
}

// Isolate the platform data directory in a subprocess rather than changing
// process-wide environment variables while other template tests are running.
#[tokio::test]
async fn custom_template_public_api_round_trip() {
    use tingskriver_core::summary::templates::{
        export_template, list_template_ids, list_templates, save_template,
    };
    if std::env::var_os("TINGSKRIVER_TEMPLATE_TEST_CHILD").is_none() {
        let dir = tempfile::tempdir().unwrap();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "custom_template_public_api_round_trip"])
            .env("TINGSKRIVER_TEMPLATE_TEST_CHILD", "1")
            .env("TINGSKRIVER_TEMPLATES_DIR", dir.path())
            .status()
            .unwrap();
        assert!(status.success());
        return;
    }
    use tingskriver_core::calendar::CalendarConfig;
    use tingskriver_core::summary::template_rules::{load_rules, save_rules, select_template};
    assert_eq!(
        select_template(None, &CalendarConfig::default()),
        "standard"
    );
    save_rules(r#"[{"when":"default","template":"intern_status"}]"#)
        .await
        .unwrap();
    assert_eq!(load_rules().unwrap().len(), 1);
    assert_eq!(
        select_template(None, &CalendarConfig::default()),
        "intern_status"
    );
    assert!(save_rules("[]").await.is_err());
    assert_eq!(
        select_template(None, &CalendarConfig::default()),
        "intern_status"
    );
    let original = export_template("standard").await.unwrap();
    let directory =
        std::path::PathBuf::from(std::env::var_os("TINGSKRIVER_TEMPLATES_DIR").unwrap());
    tokio::fs::create_dir_all(&directory).await.unwrap();
    for id in ["Mit referat", "Møde med æøå"] {
        tokio::fs::write(directory.join(format!("{id}.json")), &original)
            .await
            .unwrap();
        assert!(list_template_ids().contains(&id.to_string()));
        assert!(list_templates().iter().any(|(listed, _, _)| listed == id));
        let mut template = get_template(id).unwrap();
        template.name = id.into();
        save_template(id, &serde_json::to_string(&template).unwrap())
            .await
            .unwrap();
        assert_eq!(get_template(id).unwrap().name, id);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&export_template(id).await.unwrap()).unwrap(),
            serde_json::to_value(&template).unwrap()
        );
    }
    save_template("my_template", &original).await.unwrap();
    assert!(list_template_ids().contains(&"my_template".into()));
    let mut edited = get_template("my_template").unwrap();
    edited.name = "Mit eget referat".into();
    edited.sections[0].instruction = "Bevar detaljerne på dansk".into();
    let json = serde_json::to_string(&edited).unwrap();
    save_template("my_template", &json).await.unwrap();
    let exported = export_template("my_template").await.unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&exported).unwrap(),
        serde_json::to_value(&edited).unwrap()
    );
    save_template("standard", &json).await.unwrap();
    assert_eq!(get_template("standard").unwrap().name, edited.name);
    assert!(save_template("standard", "{}").await.is_err());
    assert_eq!(export_template("standard").await.unwrap(), exported);
}

// Core template selection is independent of the persisted notes document.
#[tokio::test]
async fn switching_template_after_meeting_preserves_saved_notes() {
    use tingskriver_core::notes::{markdown, NotesStore};
    use tingskriver_core::{calendar::CalendarConfig, summary::template_rules::select_template};
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    sqlx::query("INSERT INTO meetings(id,title,created_at,updated_at) VALUES ('completed','Synthetic',datetime('now'),datetime('now'))")
        .execute(&pool).await.unwrap();
    let store = NotesStore::new(pool);
    let notes = markdown::parse("# Noter\n\n- Bevar denne aftale").unwrap();
    store.save("completed", &notes).await.unwrap();
    let before = store.load("completed").await.unwrap();
    let mut selected = select_template(None, &CalendarConfig::default());
    assert_eq!(selected, "standard");
    get_template(&selected).unwrap();
    selected = "kunde".into();
    assert_eq!(get_template(&selected).unwrap().name, "Kundemøde");
    assert_eq!(store.load("completed").await.unwrap(), before);
    assert_eq!(before, notes);
}
