//! Contract tests against a fake OpenAI-compatible server, chain fallback,
//! key handling and the migration of the old settings.

use std::sync::{Arc, Mutex, Once};
use std::time::Duration;

use super::chain;
use super::client::{self, ChatMessage, ResolvedLink, Transcription, TranscriptionRequest};
use super::error::ProviderError;
use super::fake_server::{FakeServer, Reply};
use super::migrate;
use super::model::*;
use super::secrets::{ApiKey, KeyStore, MemoryKeyStore};
use super::service::ProviderService;
use crate::dictation::settings::DictationSettings;

// -- log capture ---------------------------------------------------------

static LINES: Mutex<Vec<String>> = Mutex::new(Vec::new());

struct Capture;

impl log::Log for Capture {
    fn enabled(&self, _: &log::Metadata) -> bool {
        true
    }

    fn log(&self, record: &log::Record) {
        if let Ok(mut lines) = LINES.lock() {
            lines.push(format!(
                "{} {} {}",
                record.level(),
                record.target(),
                record.args()
            ));
        }
    }

    fn flush(&self) {}
}

fn capture_logs() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        log::set_logger(&Capture).expect("no other logger in core tests");
        log::set_max_level(log::LevelFilter::Trace);
    });
}

fn logs() -> Vec<String> {
    LINES.lock().unwrap().clone()
}

// -- helpers -------------------------------------------------------------

fn link(id: &str, base_url: &str, key: Option<&str>) -> ResolvedLink {
    ResolvedLink {
        function: Function::Dictation,
        provider_id: id.to_string(),
        provider_name: format!("Udbyder {id}"),
        kind: ProviderKind::OpenAiCompatible,
        base_url: base_url.to_string(),
        model: "test-model".to_string(),
        key: key.and_then(ApiKey::new),
        timeout: Some(Duration::from_millis(1500)),
        diarization: false,
        chat_options: ChatOptions::default(),
    }
}

fn wav_request() -> TranscriptionRequest {
    TranscriptionRequest {
        wav: vec![1, 2, 3, 4],
        file_name: "diktering.wav".to_string(),
        language: Some("da".to_string()),
        prompt: Some("Mahoje, Syv.ai".to_string()),
        diarize: false,
    }
}

fn http() -> reqwest::Client {
    client::http_client()
}

fn field(request: &str, name: &str, value: &str) -> bool {
    request.contains(&format!("name=\"{name}\"\r\n\r\n{value}\r\n"))
}

async fn no_local(
    _: ResolvedLink,
    _: TranscriptionRequest,
) -> Result<Transcription, ProviderError> {
    Err(ProviderError::local("no local model in this test"))
}

// -- contract: transcription ------------------------------------------------

#[tokio::test]
async fn transcription_sends_openai_compatible_multipart() {
    let server = FakeServer::start(vec![Reply::ok(r#"{"text":" Hej Mads. "}"#)]).await;
    let result = client::transcribe(
        &http(),
        &link("a", &server.base_url, Some("sk-contract-1")),
        &wav_request(),
    )
    .await
    .unwrap();
    assert_eq!(result.text, "Hej Mads.");
    assert!(!result.speakers_missing);

    let request = &server.requests()[0];
    assert!(request.starts_with("POST /v1/audio/transcriptions "));
    assert!(request
        .to_lowercase()
        .contains("authorization: bearer sk-contract-1"));
    for (name, value) in [
        ("model", "test-model"),
        ("language", "da"),
        ("prompt", "Mahoje, Syv.ai"),
        ("response_format", "json"),
    ] {
        assert!(field(request, name, value), "missing field {name}");
    }
    assert!(!request.contains("name=\"diarize\""));
    assert!(request.contains("filename=\"diktering.wav\""));
}

#[tokio::test]
async fn diarization_only_where_supported_and_missing_speakers_are_flagged() {
    let body = r#"{"text":"Hej. Ja.","segments":[{"start":0.0,"end":1.0,"text":"Hej.","speaker":"A"},{"start":1.0,"end":2.0,"text":"Ja.","speaker":1}]}"#;
    let server =
        FakeServer::start(vec![Reply::ok(body), Reply::ok(r#"{"text":"Hej. Ja."}"#)]).await;
    let request = TranscriptionRequest {
        diarize: true,
        ..wav_request()
    };

    let mut syv = link("syv", &server.base_url, Some("k"));
    syv.diarization = true;
    let with = client::transcribe(&http(), &syv, &request).await.unwrap();
    assert!(!with.speakers_missing);
    assert_eq!(with.segments[0].speaker.as_deref(), Some("A"));
    assert_eq!(with.segments[1].speaker.as_deref(), Some("1"));

    let odin = link("odin", &server.base_url, Some("k"));
    let without = client::transcribe(&http(), &odin, &request).await.unwrap();
    assert!(
        without.speakers_missing,
        "a link without diarization marks speakers as missing"
    );

    let requests = server.requests();
    assert!(field(&requests[0], "diarize", "true"));
    assert!(field(&requests[0], "response_format", "verbose_json"));
    assert!(!requests[1].contains("name=\"diarize\""));
}

// -- contract: error classes -----------------------------------------------

#[tokio::test]
async fn http_401_404_429_are_classified_without_the_key() {
    let key = "sk-contract-secret-401";
    let cases = [
        (
            Reply::json(
                "401 Unauthorized",
                format!(r#"{{"error":"bad key {key}"}}"#),
            ),
            ProviderError::Unauthorized { status: 401 },
        ),
        (Reply::json("404 Not Found", "{}"), ProviderError::NotFound),
        (
            Reply::json("429 Too Many Requests", "{}").with_header("retry-after", "17"),
            ProviderError::RateLimited {
                retry_after_secs: Some(17),
            },
        ),
        (
            Reply::json("429 Too Many Requests", "{}"),
            ProviderError::RateLimited {
                retry_after_secs: None,
            },
        ),
        (
            Reply::json("503 Service Unavailable", "{}"),
            ProviderError::Http { status: 503 },
        ),
    ];
    for (reply, expected) in cases {
        let server = FakeServer::start(vec![reply]).await;
        let error = client::transcribe(
            &http(),
            &link("a", &server.base_url, Some(key)),
            &wav_request(),
        )
        .await
        .unwrap_err();
        assert_eq!(error, expected);
        let shown = format!("{error:?} {}", error.message("Syv.ai"));
        assert!(!shown.contains(key), "{shown}");
    }
}

#[tokio::test]
async fn danish_messages_for_401_404_429() {
    assert!(ProviderError::Unauthorized { status: 401 }
        .message("OdinCore")
        .contains("Nøglen mangler eller er ugyldig"));
    assert!(ProviderError::NotFound
        .message("OdinCore")
        .contains("kender ikke adressen (404)"));
    assert!(ProviderError::RateLimited {
        retry_after_secs: Some(17)
    }
    .message("OdinCore")
    .contains("Prøv igen om 17 sekunder"));
}

#[tokio::test]
async fn retry_after_as_http_date() {
    let when = (chrono::Utc::now() + chrono::Duration::seconds(60)).to_rfc2822();
    let server = FakeServer::start(vec![
        Reply::json("429 Too Many Requests", "{}").with_header("retry-after", when)
    ])
    .await;
    let error = client::chat(
        &http(),
        &link("a", &server.base_url, Some("k")),
        &[ChatMessage::user("hej")],
    )
    .await
    .unwrap_err();
    match error {
        ProviderError::RateLimited {
            retry_after_secs: Some(secs),
        } => assert!((55..=60).contains(&secs), "{secs}"),
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn time_limit_on_a_server_that_never_answers() {
    let server = FakeServer::start(vec![Reply::Hang]).await;
    let mut l = link("a", &server.base_url, Some("k"));
    l.timeout = Some(Duration::from_millis(300));
    let started = std::time::Instant::now();
    let error = client::transcribe(&http(), &l, &wav_request())
        .await
        .unwrap_err();
    assert_eq!(error, ProviderError::Timeout { after_ms: 300 });
    assert!(started.elapsed() < Duration::from_secs(3));
    assert!(error.message("Syv.ai").contains("tidsgrænsen på 300 ms"));
}

#[tokio::test]
async fn time_limit_on_a_connection_that_hangs_mid_body() {
    let server = FakeServer::start(vec![Reply::HangAfterHeaders]).await;
    let mut l = link("a", &server.base_url, Some("k"));
    l.timeout = Some(Duration::from_millis(300));
    let started = std::time::Instant::now();
    let error = client::transcribe(&http(), &l, &wav_request())
        .await
        .unwrap_err();
    assert_eq!(error, ProviderError::Timeout { after_ms: 300 });
    assert!(started.elapsed() < Duration::from_secs(3));
}

#[tokio::test]
async fn unreachable_server() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    // Windows retries a refused connection for about 2 s before giving up.
    let mut closed = link("a", &format!("http://127.0.0.1:{port}/v1"), Some("k"));
    closed.timeout = Some(Duration::from_secs(9));
    let error = client::transcribe(&http(), &closed, &wav_request())
        .await
        .unwrap_err();
    assert!(
        matches!(error, ProviderError::Unreachable { .. }),
        "{error:?}"
    );
}

#[tokio::test]
async fn bad_json_is_a_bad_response() {
    let server = FakeServer::start(vec![Reply::ok("<html>proxy</html>")]).await;
    let error = client::chat(
        &http(),
        &link("a", &server.base_url, Some("k")),
        &[ChatMessage::user("hej")],
    )
    .await
    .unwrap_err();
    assert!(
        matches!(error, ProviderError::BadResponse { .. }),
        "{error:?}"
    );
}

// -- contract: chat and test call ------------------------------------------

#[tokio::test]
async fn chat_sends_messages_and_options() {
    let server = FakeServer::start(vec![Reply::ok(
        r#"{"choices":[{"message":{"role":"assistant","content":"Referat."}}]}"#,
    )])
    .await;
    let mut l = link("a", &server.base_url, Some("sk-chat"));
    l.chat_options = ChatOptions {
        max_tokens: Some(256),
        temperature: Some(0.5),
        top_p: None,
    };
    let text = client::chat(
        &http(),
        &l,
        &[
            ChatMessage::system("Du er kort."),
            ChatMessage::user("Opsummer."),
        ],
    )
    .await
    .unwrap();
    assert_eq!(text, "Referat.");

    let request = &server.requests()[0];
    assert!(request.starts_with("POST /v1/chat/completions "));
    let body: serde_json::Value =
        serde_json::from_str(&request[request.find("\r\n\r\n").unwrap() + 4..]).unwrap();
    assert_eq!(body["model"], "test-model");
    assert_eq!(body["messages"][0]["role"], "system");
    assert_eq!(body["messages"][1]["content"], "Opsummer.");
    assert_eq!(body["max_tokens"], 256);
    assert_eq!(body["temperature"], 0.5);
    assert!(body.get("top_p").is_none());
    assert_eq!(body["stream"], false);
}

#[tokio::test]
async fn test_call_measures_latency_and_counts_models() {
    let server = FakeServer::start(vec![Reply::ok(
        r#"{"object":"list","data":[{"id":"a"},{"id":"b"}]}"#,
    )])
    .await;
    let report = client::test(&http(), &link("a", &server.base_url, Some("k")))
        .await
        .unwrap();
    assert_eq!(report.models, 2);
    assert!(report.latency_ms < 5_000);
    assert!(server.requests()[0].starts_with("GET /v1/models "));
}

#[tokio::test]
async fn service_test_button_classifies_errors() {
    let dir = tempfile::tempdir().unwrap();
    let keys = Arc::new(MemoryKeyStore::default());
    let service = ProviderService::new(dir.path(), keys.clone());
    let server = FakeServer::start(vec![Reply::json("401 Unauthorized", "{}")]).await;
    let mut config = ProvidersConfig::default();
    config.provider_mut(ODINCORE_ID).unwrap().base_url = server.base_url.clone();

    let missing = service.test(&config, ODINCORE_ID).await.unwrap_err();
    assert_eq!(missing.error, ProviderError::MissingKey);
    assert!(server.requests().is_empty(), "no call without a key");

    service.set_key(ODINCORE_ID, "odin-secret-key").unwrap();
    let rejected = service.test(&config, ODINCORE_ID).await.unwrap_err();
    assert_eq!(rejected.error, ProviderError::Unauthorized { status: 401 });
    assert_eq!(
        rejected.message,
        "OdinCore afviste API-nøglen (401). Nøglen mangler eller er ugyldig."
    );
    assert!(!rejected.to_string().contains("odin-secret-key"));
}

// -- chain -----------------------------------------------------------------

#[tokio::test]
async fn first_link_fails_second_delivers_and_the_source_is_logged() {
    capture_logs();
    let first = FakeServer::start(vec![Reply::json("500 Internal Server Error", "{}")]).await;
    let second = FakeServer::start(vec![Reply::ok(r#"{"text":"Fra reserven."}"#)]).await;
    let links = vec![
        link("chain-first", &first.base_url, Some("sk-chain-first-key")),
        link(
            "chain-second",
            &second.base_url,
            Some("sk-chain-second-key"),
        ),
    ];

    let delivered = chain::transcribe(Function::Dictation, links, wav_request(), no_local)
        .await
        .unwrap();
    assert_eq!(delivered.value.text, "Fra reserven.");
    assert_eq!(delivered.provider_id, "chain-second");
    assert_eq!(
        delivered.delivered_by(),
        "leveret af Udbyder chain-second (test-model)"
    );
    assert!(delivered.fell_back());
    assert_eq!(delivered.attempts.len(), 1);
    assert_eq!(
        delivered.attempts[0].error,
        ProviderError::Http { status: 500 }
    );
    assert_eq!(first.requests().len(), 1);
    assert_eq!(second.requests().len(), 1);

    let logs = logs();
    assert!(
        logs.iter()
            .any(|l| l.contains("chain-first") && l.contains("failed")),
        "{logs:#?}"
    );
    assert!(
        logs.iter()
            .any(|l| l.contains("delivered by reserve chain-second")),
        "{logs:#?}"
    );
    assert!(!logs
        .iter()
        .any(|l| l.contains("sk-chain-first-key") || l.contains("sk-chain-second-key")));
}

#[tokio::test]
async fn hanging_first_link_falls_back_after_its_time_limit() {
    let first = FakeServer::start(vec![Reply::Hang]).await;
    let second = FakeServer::start(vec![Reply::ok(r#"{"text":"ok"}"#)]).await;
    let mut slow = link("slow", &first.base_url, Some("k"));
    slow.timeout = Some(Duration::from_millis(200));
    let started = std::time::Instant::now();
    let delivered = chain::transcribe(
        Function::Dictation,
        vec![slow, link("fast", &second.base_url, Some("k"))],
        wav_request(),
        no_local,
    )
    .await
    .unwrap();
    assert_eq!(delivered.provider_id, "fast");
    assert_eq!(
        delivered.attempts[0].error,
        ProviderError::Timeout { after_ms: 200 }
    );
    assert!(started.elapsed() < Duration::from_secs(3));
}

#[tokio::test]
async fn missing_key_is_skipped_without_a_call_and_local_reserve_runs() {
    let server = FakeServer::start(vec![Reply::ok(r#"{"text":"remote"}"#)]).await;
    let mut syv = link(SYV_ID, &server.base_url, None);
    syv.kind = ProviderKind::SyvAi;
    let mut local = link(LOCAL_ID, "", None);
    local.kind = ProviderKind::Local;
    local.model = "parakeet-tdt-0.6b-v3-int8".to_string();
    local.timeout = None;

    let request = TranscriptionRequest {
        diarize: true,
        ..wav_request()
    };
    let delivered = chain::transcribe(
        Function::Transcription,
        vec![syv, local],
        request,
        |link, _req| async move {
            Ok(Transcription {
                text: format!("lokal {}", link.model),
                ..Default::default()
            })
        },
    )
    .await
    .unwrap();
    assert_eq!(delivered.value.text, "lokal parakeet-tdt-0.6b-v3-int8");
    assert!(
        delivered.value.speakers_missing,
        "the local model has no diarization"
    );
    assert_eq!(delivered.attempts[0].error, ProviderError::MissingKey);
    assert!(server.requests().is_empty());
}

#[tokio::test]
async fn all_links_failing_explains_each_in_danish() {
    let server = FakeServer::start(vec![Reply::json("401 Unauthorized", "{}")]).await;
    let failed = chain::chat(
        Function::Summary,
        vec![link("x", &server.base_url, Some("k"))],
        vec![ChatMessage::user("hej")],
        |_, _| async { Err(ProviderError::local("ikke brugt")) },
    )
    .await
    .unwrap_err();
    assert_eq!(failed.attempts.len(), 1);
    assert_eq!(
        failed.to_string(),
        "Referat lykkedes ikke. Udbyder x afviste API-nøglen (401). Nøglen mangler eller er ugyldig."
    );

    let off = chain::chat(
        Function::DictationPolish,
        Vec::new(),
        vec![],
        |_, _| async { Ok(String::new()) },
    )
    .await
    .unwrap_err();
    assert!(off.is_off());
    assert!(off.to_string().contains("slået fra"));
}

#[tokio::test]
async fn local_links_get_no_time_limit_unless_configured() {
    let mut local = link(LOCAL_ID, "", None);
    local.kind = ProviderKind::Local;
    local.timeout = None;
    assert_eq!(local.attempt_timeout(), None);
    local.timeout = Some(Duration::from_millis(50));
    let delivered = chain::chat(
        Function::DictationPolish,
        vec![local],
        vec![],
        |_, _| async {
            tokio::time::sleep(Duration::from_millis(500)).await;
            Ok("for sent".to_string())
        },
    )
    .await;
    assert_eq!(
        delivered.unwrap_err().attempts[0].error,
        ProviderError::Timeout { after_ms: 50 }
    );
}

// -- service -----------------------------------------------------------------

#[tokio::test]
async fn resolve_fills_models_and_keys_and_config_file_has_no_key() {
    let dir = tempfile::tempdir().unwrap();
    let keys = Arc::new(MemoryKeyStore::default());
    let service = ProviderService::new(dir.path(), keys.clone());
    let mut config = ProvidersConfig::default();
    config.functions.insert(
        Function::Summary,
        vec![ChainLink::new(ODINCORE_ID), ChainLink::new(LOCAL_ID)],
    );
    service.save(&config).unwrap();
    service.set_key(ODINCORE_ID, "odin-file-secret").unwrap();
    service.set_key(SYV_ID, "syv-file-secret").unwrap();

    let saved = std::fs::read_to_string(service.path()).unwrap();
    assert!(!saved.contains("odin-file-secret") && !saved.contains("syv-file-secret"));
    assert_eq!(
        keys.targets(),
        vec!["Tingskriver/syv.ai", "Tingskriver/udbyder/odincore"]
    );

    let loaded = service.load().unwrap().unwrap();
    assert_eq!(loaded, config);
    let links = service.resolve(&loaded, Function::Summary);
    assert_eq!(links.len(), 2);
    assert_eq!(links[0].model, "odin-2");
    assert_eq!(links[0].base_url, "https://api.odincore.ai/v1");
    assert_eq!(
        links[0].key.as_ref().map(ApiKey::expose),
        Some("odin-file-secret")
    );
    assert!(links[1].key.is_none());
    assert!(!format!("{links:?}").contains("odin-file-secret"));

    let dictation = service.resolve(&loaded, Function::Dictation);
    assert_eq!(dictation[0].model, "syv-transcribe");
    assert_eq!(dictation[1].model, crate::config::DEFAULT_PARAKEET_MODEL);
}

#[tokio::test]
async fn corrupt_config_fails_closed() {
    let dir = tempfile::tempdir().unwrap();
    let service = ProviderService::new(dir.path(), Arc::new(MemoryKeyStore::default()));
    std::fs::write(service.path(), "{ ikke json").unwrap();
    assert!(service.load().unwrap_err().contains("ødelagte"));
    assert!(service.load_or_migrate(None, None).await.is_err());
}

#[tokio::test]
async fn without_a_database_defaults_are_used_and_not_saved() {
    let dir = tempfile::tempdir().unwrap();
    let service = ProviderService::new(dir.path(), Arc::new(MemoryKeyStore::default()));
    let config = service.load_or_migrate(None, None).await.unwrap();
    assert_eq!(config, ProvidersConfig::default());
    assert!(!service.path().exists());
}

#[tokio::test]
async fn invalid_config_is_not_saved() {
    let dir = tempfile::tempdir().unwrap();
    let service = ProviderService::new(dir.path(), Arc::new(MemoryKeyStore::default()));
    let mut config = ProvidersConfig::default();
    config
        .functions
        .insert(Function::Summary, vec![ChainLink::new("findes-ikke")]);
    assert!(service.save(&config).is_err());
    assert!(!service.path().exists());
}

// -- migration ---------------------------------------------------------------

async fn legacy_pool(dir: &std::path::Path) -> sqlx::SqlitePool {
    let options = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(dir.join("meeting_minutes.sqlite"))
        .create_if_missing(true);
    let pool = sqlx::SqlitePool::connect_with(options).await.unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    sqlx::raw_sql(include_str!("fixtures/legacy_settings.sql"))
        .execute(&pool)
        .await
        .unwrap();
    pool
}

const FIXTURE_KEYS: [&str; 9] = [
    "gsk-fixture-groq",
    "sk-fixture-openai",
    "sk-ant-fixture-claude",
    "ol-fixture-ollama",
    "sk-or-fixture-router",
    "cu-fixture-custom",
    "gm-fixture-gemini",
    "dg-fixture-deepgram",
    "sk-fixture-openai-transcription",
];

fn mads_dictation() -> DictationSettings {
    DictationSettings {
        fallback_after_ms: 1800,
        polish_enabled: true,
        polish_provider: "ollama".to_string(),
        polish_model: "qwen2.5:3b".to_string(),
        polish_endpoint: "http://localhost:11434".to_string(),
        ..DictationSettings::default()
    }
}

#[tokio::test]
async fn legacy_settings_migrate_without_loss() {
    let dir = tempfile::tempdir().unwrap();
    let pool = legacy_pool(dir.path()).await;
    let keys = Arc::new(MemoryKeyStore::default());
    let service = ProviderService::new(dir.path(), keys.clone());

    let config = service
        .load_or_migrate(Some(&pool), Some(&mads_dictation()))
        .await
        .unwrap();
    config.validate().unwrap();

    // Every key is in the key store, under the target the old code reads.
    let stored: Vec<String> = keys
        .targets()
        .iter()
        .map(|t| keys.get(t).unwrap().unwrap().expose().to_string())
        .collect();
    for key in FIXTURE_KEYS {
        assert!(stored.iter().any(|s| s == key), "lost {key}");
    }
    let get = |target: &str| keys.get(target).unwrap().map(|k| k.expose().to_string());
    for (table, old, key) in [
        (migrate::LegacyTable::Summary, "openai", "sk-fixture-openai"),
        (
            migrate::LegacyTable::Summary,
            "claude",
            "sk-ant-fixture-claude",
        ),
        (migrate::LegacyTable::Summary, "groq", "gsk-fixture-groq"),
        (migrate::LegacyTable::Summary, "ollama", "ol-fixture-ollama"),
        (
            migrate::LegacyTable::Summary,
            "openrouter",
            "sk-or-fixture-router",
        ),
        (migrate::LegacyTable::Summary, "gemini", "gm-fixture-gemini"),
        (
            migrate::LegacyTable::Summary,
            "custom-openai",
            "cu-fixture-custom",
        ),
        (
            migrate::LegacyTable::Transcript,
            "openai",
            "sk-fixture-openai-transcription",
        ),
        (migrate::LegacyTable::Transcript, "groq", "gsk-fixture-groq"),
        (
            migrate::LegacyTable::Transcript,
            "deepgram",
            "dg-fixture-deepgram",
        ),
    ] {
        let found = migrate::legacy_read_targets(table, old)
            .iter()
            .find_map(|t| get(t));
        assert_eq!(found.as_deref(), Some(key), "{table:?} {old}");
    }

    // The old columns no longer hold keys, and neither does providers.json.
    let row: (Option<String>, Option<String>, Option<String>, Option<String>) =
        sqlx::query_as("SELECT openaiApiKey, anthropicApiKey, geminiApiKey, customOpenAIConfig FROM settings WHERE id = '1'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        (row.0.as_deref(), row.1.as_deref(), row.2.as_deref()),
        (None, None, None)
    );
    let custom = row.3.unwrap();
    assert!(!custom.contains("cu-fixture-custom"));
    assert!(
        custom.contains("llama-3-70b"),
        "the rest of the custom config stays"
    );
    let t: (Option<String>, Option<String>, Option<String>) = sqlx::query_as(
        "SELECT openaiApiKey, groqApiKey, deepgramApiKey FROM transcript_settings WHERE id = '1'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(t, (None, None, None));
    let file = std::fs::read_to_string(service.path()).unwrap();
    for key in FIXTURE_KEYS {
        assert!(!file.contains(key), "{key} in providers.json");
    }

    // Providers and chains carry the old choices.
    let ids = |f| {
        config
            .chain(f)
            .iter()
            .map(|l| l.provider.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(ids(Function::Summary), ["openai", "local"]);
    assert_eq!(
        config.chain(Function::Summary)[0].model.as_deref(),
        Some("gpt-4o-2024-11-20")
    );
    assert_eq!(ids(Function::Assistant), ids(Function::Summary));
    assert_eq!(
        ids(Function::Transcription),
        ["openai-transcription", "local"]
    );
    assert_eq!(
        config.chain(Function::Transcription)[0].model.as_deref(),
        Some("whisper-1")
    );
    assert_eq!(ids(Function::Dictation), ["syv-ai", "local"]);
    assert_eq!(config.chain(Function::Dictation)[0].timeout_ms, Some(1800));
    assert_eq!(ids(Function::DictationPolish), ["ollama-polish"]);
    assert_eq!(
        config.chain(Function::DictationPolish)[0].model.as_deref(),
        Some("qwen2.5:3b")
    );

    assert_eq!(
        config.provider("ollama").unwrap().base_url,
        "http://gpu-box:11434/v1"
    );
    assert_eq!(
        config.provider("ollama-polish").unwrap().base_url,
        "http://localhost:11434/v1"
    );
    assert_eq!(
        config.provider("groq").unwrap().base_url,
        migrate::GROQ_BASE_URL
    );
    assert_eq!(
        config.provider("groq").unwrap().kind,
        ProviderKind::OpenAiCompatible
    );
    assert_eq!(
        config.provider("openrouter").unwrap().base_url,
        migrate::OPENROUTER_BASE_URL
    );
    let custom = config.provider("custom-openai").unwrap();
    assert_eq!(custom.base_url, "http://localhost:8000/v1");
    assert_eq!(custom.chat_model.as_deref(), Some("llama-3-70b"));
    assert_eq!(custom.chat_options.max_tokens, Some(2048));
    assert_eq!(custom.chat_options.top_p, Some(0.9));
    assert!(
        config.provider("anthropic").is_none(),
        "the Claude path is gone"
    );

    // The second start reads the saved file and does not migrate again.
    let again = service.load_or_migrate(Some(&pool), None).await.unwrap();
    assert_eq!(again, config);
}

#[tokio::test]
async fn migration_keeps_old_keys_when_the_key_store_refuses() {
    let dir = tempfile::tempdir().unwrap();
    let pool = legacy_pool(dir.path()).await;
    let keys = Arc::new(MemoryKeyStore::failing());
    let service = ProviderService::new(dir.path(), keys);

    let config = service.load_or_migrate(Some(&pool), None).await.unwrap();
    assert_eq!(config.chain(Function::Summary)[0].provider, "openai");
    assert!(
        !service.path().exists(),
        "the migration runs again next time"
    );
    let still: Option<String> =
        sqlx::query_scalar("SELECT openaiApiKey FROM settings WHERE id = '1'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(still.as_deref(), Some("sk-fixture-openai"));
}

#[test]
fn migration_plan_for_local_and_claude_users() {
    let legacy = migrate::LegacySettings {
        has_summary_row: true,
        summary_provider: Some("claude".into()),
        summary_model: Some("claude-3-5-sonnet".into()),
        has_transcript_row: true,
        transcript_provider: Some("parakeet".into()),
        transcript_model: Some("parakeet-tdt-0.6b-v3-int8".into()),
        ..Default::default()
    };
    let dictation = DictationSettings {
        engine: "local".into(),
        ..DictationSettings::default()
    };
    let plan = migrate::plan(&legacy, Some(&dictation));
    plan.config.validate().unwrap();
    let chain = |f| plan.config.chain(f).to_vec();
    assert_eq!(chain(Function::Summary), vec![ChainLink::new(LOCAL_ID)]);
    assert_eq!(
        chain(Function::Transcription),
        vec![ChainLink::new(LOCAL_ID).with_model("parakeet-tdt-0.6b-v3-int8")],
        "meetings are not switched to Syv.ai behind the user's back"
    );
    assert_eq!(chain(Function::Dictation), vec![ChainLink::new(LOCAL_ID)]);
    assert!(chain(Function::DictationPolish).is_empty());
    assert!(plan.notes.iter().any(|n| n.contains("Claude")));
    assert!(plan.keys.is_empty());
}

#[test]
fn migration_of_a_custom_syv_endpoint_and_builtin_summary() {
    let legacy = migrate::LegacySettings {
        has_summary_row: true,
        summary_provider: Some("builtin-ai".into()),
        summary_model: Some("gemma3:1b".into()),
        ..Default::default()
    };
    let dictation = DictationSettings {
        syv_endpoint: "https://eu.syv.ai/v1/".into(),
        syv_model: "syv-fast".into(),
        polish_enabled: true,
        polish_model: "gemma3:1b".into(),
        ..DictationSettings::default()
    };
    let plan = migrate::plan(&legacy, Some(&dictation));
    plan.config.validate().unwrap();
    assert_eq!(
        plan.config.provider(SYV_ID).unwrap().base_url,
        "https://eu.syv.ai/v1"
    );
    assert_eq!(
        plan.config.chain(Function::Dictation)[0].model.as_deref(),
        Some("syv-fast")
    );
    assert_eq!(
        plan.config.chain(Function::Summary),
        [ChainLink::new(LOCAL_ID).with_model("gemma3:1b")]
    );
    assert_eq!(
        plan.config.chain(Function::DictationPolish),
        [ChainLink::new(LOCAL_ID)
            .with_model("gemma3:1b")
            .with_timeout_ms(2000)]
    );
}

/// A database with the old tables and the given rows, without the fixture.
async fn pool_with(dir: &std::path::Path, sql: &str) -> sqlx::SqlitePool {
    let options = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(dir.join("meeting_minutes.sqlite"))
        .create_if_missing(true);
    let pool = sqlx::SqlitePool::connect_with(options).await.unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    sqlx::raw_sql(sql).execute(&pool).await.unwrap();
    pool
}

#[tokio::test]
async fn keys_the_old_code_already_stored_are_migrated() {
    let target = |id: &str| key_target(id);
    let get = |keys: &MemoryKeyStore, t: &str| keys.get(t).unwrap().map(|k| k.expose().to_string());

    // Local summary, OpenAI transcription; the transcription key was saved
    // by the old settings screen after this change, so its column is empty.
    let dir = tempfile::tempdir().unwrap();
    let pool = pool_with(
        dir.path(),
        "INSERT INTO settings (id, provider, model, whisperModel) VALUES ('1', 'builtin-ai', 'gemma3:1b', 'large-v3');
         INSERT INTO transcript_settings (id, provider, model) VALUES ('1', 'openai', 'whisper-1');",
    )
    .await;
    let keys = Arc::new(MemoryKeyStore::default());
    let old_key = ApiKey::new("sk-only-in-store").unwrap();
    keys.set(&target("openai-transcription"), &old_key).unwrap();
    let service = ProviderService::new(dir.path(), keys.clone());
    let config = service.load_or_migrate(Some(&pool), None).await.unwrap();
    let first = &config.chain(Function::Transcription)[0];
    assert_eq!(
        get(&keys, &target(&first.provider)).as_deref(),
        Some("sk-only-in-store"),
        "the transcription chain finds the key"
    );

    // Different summary and transcription keys, both already in the store:
    // neither overwrites the other.
    let dir = tempfile::tempdir().unwrap();
    let pool = pool_with(
        dir.path(),
        "INSERT INTO settings (id, provider, model, whisperModel) VALUES ('1', 'openai', 'gpt-4o', 'large-v3');
         INSERT INTO transcript_settings (id, provider, model) VALUES ('1', 'openai', 'whisper-1');",
    )
    .await;
    let keys = Arc::new(MemoryKeyStore::default());
    keys.set(&target(OPENAI_ID), &ApiKey::new("sk-summary").unwrap())
        .unwrap();
    keys.set(
        &target("openai-transcription"),
        &ApiKey::new("sk-transcription").unwrap(),
    )
    .unwrap();
    let service = ProviderService::new(dir.path(), keys.clone());
    let config = service.load_or_migrate(Some(&pool), None).await.unwrap();
    assert_eq!(config.chain(Function::Summary)[0].provider, OPENAI_ID);
    assert_eq!(
        get(&keys, &target(OPENAI_ID)).as_deref(),
        Some("sk-summary")
    );
    let first = &config.chain(Function::Transcription)[0];
    assert_eq!(first.provider, "openai-transcription");
    assert_eq!(
        get(&keys, &target(&first.provider)).as_deref(),
        Some("sk-transcription")
    );
}

#[test]
fn a_shared_key_stays_readable_for_transcription_after_the_summary_key_is_deleted() {
    for (old, column) in [("openai", "openaiApiKey"), ("groq", "groqApiKey")] {
        let key = ApiKey::new("sk-shared").unwrap();
        let legacy = migrate::LegacySettings {
            has_summary_row: true,
            summary_provider: Some(old.into()),
            summary_model: Some("m".into()),
            summary_keys: [(column.to_string(), key.clone())].into(),
            has_transcript_row: true,
            transcript_provider: Some(old.into()),
            transcript_keys: [(column.to_string(), key.clone())].into(),
            ..Default::default()
        };
        let plan = migrate::plan(&legacy, None);
        plan.config.validate().unwrap();
        let store = MemoryKeyStore::default();
        for (target, key) in &plan.keys {
            store.set(target, key).unwrap();
        }
        // What SettingsRepository::delete_api_key deletes for the summary key
        for target in migrate::legacy_read_targets(migrate::LegacyTable::Summary, old) {
            store.delete(&target).unwrap();
        }
        let found = migrate::legacy_read_targets(migrate::LegacyTable::Transcript, old)
            .iter()
            .find_map(|t| store.get(t).unwrap());
        assert_eq!(
            found.map(|k| k.expose().to_string()).as_deref(),
            Some("sk-shared"),
            "{old}"
        );
    }
}

#[test]
fn one_invalid_old_value_does_not_discard_the_migration() {
    let legacy = migrate::LegacySettings {
        has_summary_row: true,
        summary_provider: Some("groq".into()),
        summary_model: Some("llama-3.3-70b".into()),
        ollama_endpoint: Some("gpu-box:11434".into()),
        custom_openai: Some(migrate::LegacyCustomOpenAi {
            endpoint: String::new(),
            model: "llama".into(),
            ..Default::default()
        }),
        has_transcript_row: true,
        transcript_provider: Some("parakeet".into()),
        transcript_model: Some("parakeet-tdt-0.6b-v3-int8".into()),
        ..Default::default()
    };
    // Dictation uses Syv.ai; meetings must stay local.
    let plan = migrate::plan(&legacy, Some(&DictationSettings::default()));
    plan.config.validate().unwrap();
    assert!(plan.config.provider("ollama").is_none());
    assert!(plan.config.provider("custom-openai").is_none());
    assert_eq!(plan.config.chain(Function::Summary)[0].provider, "groq");
    assert_eq!(
        plan.config.chain(Function::Transcription),
        [ChainLink::new(LOCAL_ID).with_model("parakeet-tdt-0.6b-v3-int8")]
    );
    assert!(plan.notes.iter().any(|n| n.contains("Ollama")));
    assert!(plan.notes.iter().any(|n| n.contains("Brugerdefineret")));

    // A broken link is dropped, the rest of its chain stays.
    let legacy = migrate::LegacySettings {
        has_summary_row: true,
        summary_provider: Some("custom-openai".into()),
        custom_openai: Some(migrate::LegacyCustomOpenAi {
            endpoint: "ftp://server".into(),
            model: "llama".into(),
            ..Default::default()
        }),
        ..Default::default()
    };
    let plan = migrate::plan(&legacy, None);
    plan.config.validate().unwrap();
    assert_eq!(
        plan.config.chain(Function::Summary),
        [ChainLink::new(LOCAL_ID)]
    );
}

#[tokio::test]
async fn read_legacy_fails_on_database_errors_but_not_on_missing_tables() {
    let empty = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
    assert!(migrate::read_legacy(&empty).await.unwrap().is_empty());
    empty.close().await;
    assert!(migrate::read_legacy(&empty).await.is_err());
}

#[test]
fn no_old_odincore_domain_in_presets() {
    let json = serde_json::to_string(&ProvidersConfig::default()).unwrap();
    assert!(json.contains("odincore.ai"));
    assert!(!json.contains(&["odincore", "dk"].join(".")));
}

#[test]
fn no_log_statement_formats_a_key() {
    let sources = [
        ("chain.rs", include_str!("chain.rs")),
        ("client.rs", include_str!("client.rs")),
        ("migrate.rs", include_str!("migrate.rs")),
        ("service.rs", include_str!("service.rs")),
        ("secrets.rs", include_str!("secrets.rs")),
        ("mod.rs", include_str!("mod.rs")),
    ];
    for (file, source) in sources {
        for statement in source.split("log::").skip(1) {
            let statement = statement.split(';').next().unwrap_or_default();
            assert!(
                !statement.contains("expose") && !statement.contains("api_key"),
                "{file}: log statement may print a key: log::{statement}"
            );
        }
    }
}

#[cfg(not(windows))]
struct TestHost(std::path::PathBuf);

#[cfg(not(windows))]
impl crate::host::AppHost for TestHost {
    fn emit_json(&self, _: &str, _: &str) -> Result<(), String> {
        Ok(())
    }

    fn database(&self) -> Option<crate::database::manager::DatabaseManager> {
        None
    }

    fn app_data_dir(&self) -> Result<std::path::PathBuf, String> {
        Ok(self.0.clone())
    }

    fn listen(&self, _: &str, _: Box<dyn Fn(&str) + Send + Sync>) -> crate::host::ListenerId {
        0
    }

    fn unlisten(&self, _: crate::host::ListenerId) {}

    fn read_setting(&self, _: &str, _: &str) -> Result<Option<serde_json::Value>, String> {
        Ok(None)
    }

    fn write_setting(&self, _: &str, _: &str, _: serde_json::Value) -> Result<(), String> {
        Ok(())
    }
}

#[cfg(not(windows))]
#[tokio::test]
async fn public_api_falls_back_to_the_local_model_without_a_syv_key() {
    let dir = tempfile::tempdir().unwrap();
    let host: crate::host::HostHandle = Arc::new(TestHost(dir.path().to_path_buf()));

    let links = super::chain_for(&host, Function::Dictation).await.unwrap();
    assert_eq!(
        links
            .iter()
            .map(|l| l.provider_id.as_str())
            .collect::<Vec<_>>(),
        [SYV_ID, LOCAL_ID]
    );

    let delivered = super::transcribe(
        &host,
        Function::Dictation,
        wav_request(),
        |link, _| async move {
            Ok(Transcription {
                text: format!("lokal {}", link.model),
                ..Default::default()
            })
        },
    )
    .await
    .unwrap();
    assert_eq!(delivered.provider_id, LOCAL_ID);
    assert_eq!(delivered.attempts[0].error, ProviderError::MissingKey);

    let off = super::chat(
        &host,
        Function::DictationPolish,
        vec![ChatMessage::user("hej")],
        |_, _| async { Ok(String::new()) },
    )
    .await
    .unwrap_err();
    assert!(off.to_string().contains("slået fra"));
    assert!(!dir.path().join(super::service::CONFIG_FILE).exists());
}
