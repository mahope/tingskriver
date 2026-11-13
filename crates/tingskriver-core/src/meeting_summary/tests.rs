use super::*;
use crate::host::{AppHost, ListenerId};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

/// A host with a data folder and nothing else.
struct FakeHost(PathBuf);

impl AppHost for FakeHost {
    fn emit_json(&self, _: &str, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn database(&self) -> Option<DatabaseManager> {
        None
    }
    fn app_data_dir(&self) -> Result<PathBuf, String> {
        Ok(self.0.clone())
    }
    fn listen(&self, _: &str, _: Box<dyn Fn(&str) + Send + Sync>) -> ListenerId {
        0
    }
    fn unlisten(&self, _: ListenerId) {}
    fn read_setting(&self, _: &str, _: &str) -> Result<Option<serde_json::Value>, String> {
        Ok(None)
    }
    fn write_setting(&self, _: &str, _: &str, _: serde_json::Value) -> Result<(), String> {
        Ok(())
    }
}

async fn database() -> (tempfile::TempDir, DatabaseManager) {
    let dir = tempfile::tempdir().unwrap();
    let db = DatabaseManager::open_at(
        &dir.path().join("db.sqlite"),
        &dir.path().join("legacy.sqlite"),
        Duration::from_secs(5),
    )
    .await
    .map_err(|e| e.to_string())
    .unwrap();
    (dir, db)
}

async fn meeting(db: &DatabaseManager, id: &str, lines: &[&str]) {
    let pool = db.pool();
    sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES (?, 'Møde', '2026-11-01T10:00:00Z', '2026-11-01T10:00:00Z')")
        .bind(id)
        .execute(pool)
        .await
        .unwrap();
    for (index, line) in lines.iter().enumerate() {
        sqlx::query("INSERT INTO transcripts (id, meeting_id, transcript, timestamp, audio_start_time) VALUES (?, ?, ?, '2026-11-01T10:00:00Z', ?)")
            .bind(format!("{id}-t{index}"))
            .bind(id)
            .bind(*line)
            .bind(index as f64 * 65.0)
            .execute(pool)
            .await
            .unwrap();
    }
}

async fn process(db: &DatabaseManager, id: &str, status: &str, result: Option<&str>) {
    sqlx::query("INSERT INTO summary_processes (meeting_id, status, created_at, updated_at, start_time, result) VALUES (?, ?, '2026-11-01T10:00:00Z', '2026-11-01T10:00:00Z', '2026-11-01T10:00:00Z', ?)")
        .bind(id)
        .bind(status)
        .bind(result)
        .execute(db.pool())
        .await
        .unwrap();
}

async fn stored(db: &DatabaseManager, id: &str) -> serde_json::Value {
    let text: String =
        sqlx::query_scalar("SELECT result FROM summary_processes WHERE meeting_id = ?")
            .bind(id)
            .fetch_one(db.pool())
            .await
            .unwrap();
    serde_json::from_str(&text).unwrap()
}

#[test]
fn language_codes_are_normalised_like_tauri() {
    assert_eq!(normalise_language("pt-BR").as_deref(), Some("pt"));
    assert_eq!(normalise_language("en_GB").as_deref(), Some("en"));
    assert_eq!(normalise_language("ZH-TW").as_deref(), Some("zh-tw"));
    assert_eq!(normalise_language("xx"), None);
    assert_eq!(language_label("da"), "Dansk");
    assert_eq!(language_label("xx"), "xx");
}

#[test]
fn the_check_before_a_run_uses_the_tauri_texts() {
    use text::*;
    assert_eq!(
        preflight_message("ollama", "x", Some(Ok(0)), None),
        Some(SUMMARY_OLLAMA_NONE)
    );
    assert_eq!(
        preflight_message("ollama", "x", Some(Err("nede".into())), None),
        Some(SUMMARY_MODEL_CHECK_FAILED)
    );
    assert_eq!(preflight_message("ollama", "x", Some(Ok(2)), None), None);
    assert_eq!(
        preflight_message("builtin-ai", "", None, None),
        Some(SUMMARY_BUILTIN_NO_MODEL)
    );
    assert_eq!(
        preflight_message("builtin-ai", "gemma", None, Some(Ok(false))),
        Some(SUMMARY_BUILTIN_NOT_READY)
    );
    assert_eq!(
        preflight_message("builtin-ai", "gemma", None, Some(Ok(true))),
        None
    );
    assert_eq!(
        preflight_message("claude", "", None, None),
        Some(SUMMARY_NO_MODEL)
    );
    assert_eq!(preflight_message("claude", "sonnet", None, None), None);
}

#[test]
fn the_transcript_is_sent_with_times_and_speakers() {
    let row = |speaker: Option<&str>, text: &str, start: Option<f64>| Transcript {
        id: "1".into(),
        meeting_id: "m".into(),
        transcript: text.into(),
        speaker: speaker.map(Into::into),
        timestamp: "2026-11-01T10:00:00Z".into(),
        summary: None,
        action_items: None,
        key_points: None,
        audio_start_time: start,
        audio_end_time: None,
        duration: None,
    };
    let (payload, texts) = transcript_payload(&[
        row(Some("mic"), "Godmorgen", Some(3.7)),
        row(Some("Maria"), "Hej", Some(125.0)),
        row(None, "Gammel linje", None),
    ]);
    assert_eq!(
        payload,
        "[00:03] Godmorgen\n[02:05] Maria: Hej\n2026-11-01T10:00:00Z Gammel linje"
    );
    assert_eq!(texts, ["Godmorgen", "Hej", "Gammel linje"]);
}

#[test]
fn the_template_falls_back_to_standard_then_the_first() {
    let template = |id: &str| Template {
        id: id.into(),
        name: id.into(),
        description: String::new(),
    };
    let list = [template("daily_standup"), template("standard")];
    assert_eq!(pick_template(&list, "daily_standup"), "daily_standup");
    assert_eq!(pick_template(&list, "findes_ikke"), "standard");
    assert_eq!(pick_template(&[template("a")], "b"), "a");
    assert_eq!(pick_template(&[], "b"), "b");
}

#[test]
fn preferences_keep_the_tauri_rules_for_quick_picks() {
    let mut preferences = Preferences::default();
    for code in ["en", "de", "fr", "sv", "no", "da"] {
        preferences.add_recent(code);
    }
    assert_eq!(preferences.language_recents, ["da", "no", "sv", "fr", "de"]);
    preferences.add_recent("de");
    assert_eq!(preferences.language_recents[0], "de");
    preferences.add_recent("xx");
    assert_eq!(preferences.language_recents.len(), 5);
    preferences.toggle_pinned("sv");
    assert_eq!(preferences.language_default.as_deref(), Some("sv"));
    preferences.toggle_pinned("sv");
    assert_eq!(preferences.language_default, None);
    preferences.toggle_pinned("sv");
    preferences.remove_recent("sv");
    assert_eq!(preferences.language_default, None);
    assert!(!preferences.language_recents.contains(&"sv".to_string()));
}

#[test]
fn preferences_are_saved_and_a_broken_file_is_not_read_as_off() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(
        Preferences::load(dir.path()).unwrap(),
        Preferences::default()
    );
    let preferences = Preferences {
        auto_summary: true,
        language_recents: vec!["pt-BR".into(), "en".into(), "en".into()],
        language_default: Some("EN".into()),
    };
    preferences.save(dir.path()).unwrap();
    let loaded = Preferences::load(dir.path()).unwrap();
    assert!(loaded.auto_summary);
    assert_eq!(loaded.language_recents, ["pt", "en"]);
    assert_eq!(loaded.language_default.as_deref(), Some("en"));
    std::fs::write(dir.path().join(PREFERENCES_FILE), "{ikke json").unwrap();
    assert!(Preferences::load(dir.path()).is_err());
}

#[tokio::test]
async fn a_summary_cannot_start_without_a_transcript() {
    let (dir, db) = database().await;
    meeting(&db, "m1", &[]).await;
    let host: HostHandle = Arc::new(FakeHost(dir.path().to_path_buf()));
    let error = start(host, &db, "m1", "standard", None).await.unwrap_err();
    assert_eq!(error, text::SUMMARY_NO_TRANSCRIPT);
    assert!(!has_summary_process(db.pool(), "m1").await.unwrap());
}

#[tokio::test]
async fn a_summary_does_not_start_with_a_model_that_is_not_ready() {
    let (dir, db) = database().await;
    meeting(&db, "m1", &["Godmorgen alle sammen"]).await;
    save_model(
        db.pool(),
        &ModelChoice {
            provider: "builtin-ai".into(),
            model: String::new(),
            ollama_endpoint: None,
        },
    )
    .await
    .unwrap();
    let host: HostHandle = Arc::new(FakeHost(dir.path().to_path_buf()));
    let error = start(host, &db, "m1", "standard", None).await.unwrap_err();
    assert_eq!(error, text::SUMMARY_BUILTIN_NO_MODEL);
    assert!(!has_summary_process(db.pool(), "m1").await.unwrap());
}

#[tokio::test]
async fn the_model_is_saved_where_tauri_saves_it() {
    let (_dir, db) = database().await;
    assert_eq!(read_model(db.pool()).await.unwrap(), None);
    assert_eq!(effective_model(db.pool()).await.unwrap().provider, "ollama");
    let choice = ModelChoice {
        provider: "ollama".into(),
        model: "gemma3:1b".into(),
        ollama_endpoint: Some("http://127.0.0.1:11435".into()),
    };
    save_model(db.pool(), &choice).await.unwrap();
    assert_eq!(read_model(db.pool()).await.unwrap(), Some(choice));
    // A new provider keeps the address.
    save_model(
        db.pool(),
        &ModelChoice {
            provider: "groq".into(),
            model: "llama".into(),
            ollama_endpoint: None,
        },
    )
    .await
    .unwrap();
    let read = read_model(db.pool()).await.unwrap().unwrap();
    assert_eq!(
        (read.provider.as_str(), read.model.as_str()),
        ("groq", "llama")
    );
    assert_eq!(
        read.ollama_endpoint.as_deref(),
        Some("http://127.0.0.1:11435")
    );
    assert!(save_model(
        db.pool(),
        &ModelChoice {
            provider: "ukendt".into(),
            model: "x".into(),
            ollama_endpoint: None
        }
    )
    .await
    .is_err());
}

#[tokio::test]
async fn the_own_server_needs_an_address_and_a_model() {
    let (_dir, db) = database().await;
    assert!(save_custom_server(db.pool(), "", "m", "").await.is_err());
    assert!(save_custom_server(db.pool(), "localhost:8000", "m", "")
        .await
        .is_err());
    save_custom_server(db.pool(), "http://localhost:8000/v1", "mistral", "")
        .await
        .unwrap();
    let server = read_custom_server(db.pool()).await.unwrap();
    assert_eq!(server.endpoint, "http://localhost:8000/v1");
    assert_eq!(server.model, "mistral");
}

#[tokio::test]
async fn an_edited_summary_replaces_the_text_and_keeps_the_rest() {
    let (_dir, db) = database().await;
    meeting(&db, "m1", &["Hej"]).await;
    process(
        &db,
        "m1",
        "completed",
        Some(r##"{"markdown":"# Gammelt","summary_json":[{"type":"paragraph"}],"english_cache":{"markdown":"# Old"},"MeetingName":"Ugentligt"}"##),
    )
    .await;
    save_edit(&db, "m1", "# Nyt\n- punkt\n\n").await.unwrap();
    let result = stored(&db, "m1").await;
    assert_eq!(result["markdown"], "# Nyt\n- punkt");
    assert_eq!(result["MeetingName"], "Ugentligt");
    assert!(result.get("summary_json").is_none());
    assert!(result.get("english_cache").is_none());
    // What the meeting page reads is the new text.
    let document = crate::meeting_document::load(db.pool(), "m1")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(document.summary.markdown.as_deref(), Some("# Nyt\n- punkt"));
}

#[tokio::test]
async fn an_empty_or_running_summary_is_not_saved() {
    let (_dir, db) = database().await;
    meeting(&db, "m1", &["Hej"]).await;
    process(
        &db,
        "m1",
        "completed",
        Some(r##"{"markdown":"# Gammelt"}"##),
    )
    .await;
    assert_eq!(
        save_edit(&db, "m1", "  \n ").await.unwrap_err(),
        text::SUMMARY_EDIT_EMPTY
    );
    assert_eq!(stored(&db, "m1").await["markdown"], "# Gammelt");
    meeting(&db, "m2", &["Hej"]).await;
    process(&db, "m2", "PROCESSING", None).await;
    assert_eq!(
        save_edit(&db, "m2", "# Nyt").await.unwrap_err(),
        text::SUMMARY_EDIT_RUNNING
    );
    assert_eq!(
        save_edit(&db, "findes-ikke", "# Nyt").await.unwrap_err(),
        text::SUMMARY_SAVE_FAILED
    );
}

#[tokio::test]
async fn stopping_names_the_run_by_its_start_time() {
    let (_dir, db) = database().await;
    meeting(&db, "m1", &["Hej"]).await;
    assert_eq!(stop(&db, "m1").await.unwrap(), Stopped::NothingRunning);

    let started_at = Utc::now();
    SummaryProcessesRepository::create_or_reset_process(db.pool(), "m1", started_at)
        .await
        .unwrap();
    // The run is gone (the app was restarted): nothing to cancel.
    assert_eq!(stop(&db, "m1").await.unwrap(), Stopped::AlmostDone);

    let token = crate::summary::SummaryService::register_cancellation_token("m1", started_at);
    assert_eq!(stop(&db, "m1").await.unwrap(), Stopped::Cancelled);
    assert!(token.is_cancelled());
    let status: String =
        sqlx::query_scalar("SELECT status FROM summary_processes WHERE meeting_id = 'm1'")
            .fetch_one(db.pool())
            .await
            .unwrap();
    assert_eq!(status.to_lowercase(), "cancelled");
    assert_eq!(
        process_id(started_at),
        started_at.to_rfc3339_opts(SecondsFormat::Nanos, true)
    );
}

#[tokio::test]
async fn the_language_of_a_meeting_without_a_folder_is_not_stored() {
    let (_dir, db) = database().await;
    meeting(&db, "m1", &["Hej"]).await;
    let saved = save_language(&db, "m1", Some("sv")).await.unwrap();
    assert!(saved.local_only);
    assert_eq!(saved.language.as_deref(), Some("sv"));
    assert_eq!(read_language(&db, "m1").await.unwrap().language, None);
}

#[tokio::test]
async fn the_automatic_summary_follows_the_setting() {
    let (dir, db) = database().await;
    meeting(&db, "m1", &["Godmorgen alle sammen"]).await;
    let host: HostHandle = Arc::new(FakeHost(dir.path().to_path_buf()));
    // Off by default.
    assert_eq!(auto_start(host.clone(), &db, "m1").await, Auto::Skipped);
    Preferences {
        auto_summary: true,
        ..Preferences::default()
    }
    .save(dir.path())
    .unwrap();
    // On, but the meeting already has a summary.
    process(&db, "m1", "completed", Some(r##"{"markdown":"# A"}"##)).await;
    assert_eq!(auto_start(host.clone(), &db, "m1").await, Auto::Skipped);
    // On, and a model that cannot be used says why instead of failing silently.
    meeting(&db, "m2", &["Godmorgen alle sammen"]).await;
    save_model(
        db.pool(),
        &ModelChoice {
            provider: "builtin-ai".into(),
            model: String::new(),
            ollama_endpoint: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(
        auto_start(host.clone(), &db, "m2").await,
        Auto::Failed(text::SUMMARY_BUILTIN_NO_MODEL.to_string())
    );
    // A broken settings file never starts a run.
    std::fs::write(dir.path().join(PREFERENCES_FILE), "{").unwrap();
    assert_eq!(auto_start(host, &db, "m2").await, Auto::Skipped);
}

#[test]
fn a_second_automatic_start_for_the_same_meeting_is_refused_until_the_first_is_done() {
    let first = AutoClaim::take("claim-m1").expect("first claim");
    assert!(AutoClaim::take("claim-m1").is_none());
    assert!(AutoClaim::take("claim-m2").is_some());
    drop(first);
    assert!(AutoClaim::take("claim-m1").is_some());
}
