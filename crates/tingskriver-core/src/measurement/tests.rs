use super::*;
use crate::providers::{
    fake_server::{FakeServer, Reply},
    model::{ChatOptions, ProviderKind},
    secrets::MemoryKeyStore,
    Provider, ProviderService, ProvidersConfig,
};
use std::sync::Arc;

fn sample(seconds: usize) -> Sample {
    Sample::from_samples(vec![0.0; 16_000 * seconds]).unwrap()
}

#[test]
fn candidates_ignore_blank_overrides_and_deduplicate_models() {
    let mut config = config(&[("chosen", "http://localhost", ProviderKind::OpenAiCompatible)]);
    let mut blank = ChainLink::new("chosen");
    blank.model = Some(" ".into());
    config.functions.insert(
        Function::Transcription,
        vec![
            blank,
            ChainLink::new("chosen").with_model("test-model"),
            ChainLink::new("chosen").with_model("second-model"),
            ChainLink::new("unknown").with_model("unknown-model"),
        ],
    );
    config.functions.insert(
        Function::Dictation,
        vec![ChainLink::new("chosen").with_model("second-model")],
    );
    let models: Vec<_> = candidates(&config).into_iter().map(|s| s.model).collect();
    assert_eq!(models, ["test-model", "second-model"]);
}

fn config(urls: &[(&str, &str, ProviderKind)]) -> ProvidersConfig {
    let providers = urls
        .iter()
        .map(|(id, url, kind)| Provider {
            id: id.to_string(),
            name: id.to_string(),
            kind: *kind,
            base_url: url.to_string(),
            transcription_model: Some("test-model".into()),
            chat_model: None,
            diarization: false,
            chat_options: ChatOptions::default(),
        })
        .collect();
    ProvidersConfig {
        providers,
        functions: Default::default(),
        ..ProvidersConfig::default()
    }
}

#[tokio::test]
async fn selected_models_only_receive_identical_audio_and_danish_language() {
    let chosen = FakeServer::start(vec![Reply::ok(r#"{"text":"RØD grød med fløde!"}"#)]).await;
    let other = FakeServer::start(vec![Reply::ok(r#"{"text":"should not run"}"#)]).await;
    let missing = FakeServer::start(vec![Reply::ok(r#"{"text":"no key"}"#)]).await;
    let config = config(&[
        ("chosen", &chosen.base_url, ProviderKind::OpenAiCompatible),
        ("other", &other.base_url, ProviderKind::OpenAiCompatible),
        ("missing", &missing.base_url, ProviderKind::SyvAi),
    ]);
    let dir = tempfile::tempdir().unwrap();
    let service = ProviderService::new(dir.path(), Arc::new(MemoryKeyStore::default()));
    let selected = vec![
        Selection::new("chosen", "one"),
        Selection::new("chosen", "two"),
        Selection::new("missing", "three"),
    ];
    let report = run(
        &config,
        &service,
        &selected,
        &sample(60),
        "Rød, grød med fløde.",
        dir.path(),
    )
    .await
    .unwrap();
    assert_eq!(report.results.len(), 3);
    for row in &report.results[..2] {
        assert_eq!(row.score.unwrap().errors(), 0);
        assert_eq!(row.wer, Some(0.0));
        assert!(row.latency_ms.is_some());
    }
    assert_eq!(report.results[2].status, Status::Skipped);
    assert!(report.results[2].message.contains("nøgle"));
    assert!(other.requests().is_empty());
    assert!(missing.requests().is_empty());
    let requests = chosen.requests();
    assert_eq!(requests.len(), 2);
    let payload = crate::dictation::logic::audio::wav_bytes(&vec![0.0; 960_000], 16_000);
    let payload = String::from_utf8_lossy(&payload);
    for request in requests {
        assert!(request.contains(payload.as_ref()));
        assert!(request.contains("name=\"language\"\r\n\r\nda"));
    }
    let path = report.save(dir.path()).unwrap();
    let restored: Report = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(restored.results[0].score, report.results[0].score);
}

#[tokio::test]
async fn different_selected_providers_share_audio_and_normalization() {
    let first = FakeServer::start(vec![Reply::ok(r#"{"text":"RØD-grød, med FLØDE!"}"#)]).await;
    let second = FakeServer::start(vec![Reply::ok(r#"{"text":"rød grød uden fløde"}"#)]).await;
    let config = config(&[
        ("first", &first.base_url, ProviderKind::OpenAiCompatible),
        ("second", &second.base_url, ProviderKind::OpenAiCompatible),
    ]);
    let dir = tempfile::tempdir().unwrap();
    let service = ProviderService::new(dir.path(), Arc::new(MemoryKeyStore::default()));
    // Nonzero, exactly representable PCM values expose accidental audio replacement.
    let sample = Sample::from_samples([0.25, -0.5, 0.75, -1.0].repeat(4_000)).unwrap();
    let report = run(
        &config,
        &service,
        &[
            Selection::new("first", "one"),
            Selection::new("second", "two"),
        ],
        &sample,
        "Rød, grød med fløde.",
        dir.path(),
    )
    .await
    .unwrap();
    assert_eq!(report.results[0].wer, Some(0.0));
    assert_eq!(report.results[1].wer, Some(0.25));
    assert_eq!(report.results[1].score.unwrap().substitutions, 1);
    let payload = String::from_utf8_lossy(&sample.wav);
    for (server, model) in [(&first, "one"), (&second, "two")] {
        let requests = server.requests();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].contains(payload.as_ref()));
        assert!(requests[0].contains(&format!("name=\"model\"\r\n\r\n{model}")));
        assert!(!requests[0].contains("name=\"prompt\""));
        assert!(!requests[0].contains("name=\"diarize\""));
        assert!(!requests[0].contains("Rød, grød med fløde."));
    }
    for (pcm, sample) in sample.wav[44..].chunks_exact(2).zip(&sample.samples) {
        assert_eq!(
            *sample,
            i16::from_le_bytes([pcm[0], pcm[1]]) as f32 / 32768.0
        );
    }
    let path = report.save(dir.path()).unwrap();
    let saved = std::fs::read_to_string(path).unwrap();
    for private in ["Rød", "FLØDE", "uden", &first.base_url, &second.base_url] {
        assert!(!saved.contains(private));
    }
    assert!(report.save(dir.path()).is_err());
}

#[tokio::test]
async fn empty_selection_and_invalid_reference_never_call_a_provider() {
    let server = FakeServer::start(vec![Reply::ok(r#"{"text":"test"}"#)]).await;
    let config = config(&[("chosen", &server.base_url, ProviderKind::OpenAiCompatible)]);
    let dir = tempfile::tempdir().unwrap();
    let service = ProviderService::new(dir.path(), Arc::new(MemoryKeyStore::default()));
    assert!(run(&config, &service, &[], &sample(1), "hej", dir.path())
        .await
        .is_err());
    assert!(run(
        &config,
        &service,
        &[Selection::new("chosen", "one")],
        &sample(1),
        "...",
        dir.path()
    )
    .await
    .is_err());
    assert!(server.requests().is_empty());
}

#[test]
fn duration_and_prices_are_validated() {
    assert!(Sample::from_samples(vec![]).is_err());
    assert!(Sample::from_samples(vec![f32::NAN]).is_err());
    assert!(Sample::from_samples(vec![0.0; 16_000 * 91]).is_err());
    assert_eq!(sample(60).duration_seconds(), 60.0);
    assert!(Rate::new(-1.0, "DKK").is_err());
    assert!(Rate::new(f64::NAN, "DKK").is_err());
    assert!(Rate::new(12.0, "").is_err());
    let rate = Rate::new(12.0, "DKK").unwrap();
    assert_eq!(rate.sample_cost(60.0), 0.2);
}

#[tokio::test]
async fn failed_provider_does_not_fall_back_or_follow_redirects() {
    let other = FakeServer::start(vec![Reply::ok(r#"{"text":"private"}"#)]).await;
    let redirect = FakeServer::start(vec![Reply::json("303 See Other", "").with_header(
        "Location",
        format!("{}/audio/transcriptions", other.base_url),
    )])
    .await;
    let config = config(&[
        (
            "redirect",
            &redirect.base_url,
            ProviderKind::OpenAiCompatible,
        ),
        ("other", &other.base_url, ProviderKind::OpenAiCompatible),
    ]);
    let dir = tempfile::tempdir().unwrap();
    let service = ProviderService::new(dir.path(), Arc::new(MemoryKeyStore::default()));
    let report = run(
        &config,
        &service,
        &[Selection::new("redirect", "test")],
        &sample(1),
        "hej",
        dir.path(),
    )
    .await
    .unwrap();
    assert_eq!(report.results[0].status, Status::Failed);
    assert!(report.results[0].wer.is_none());
    assert_eq!(redirect.requests().len(), 1);
    assert!(other.requests().is_empty());
}

#[tokio::test]
async fn validates_whole_selection_before_first_upload() {
    let server = FakeServer::start(vec![Reply::ok(r#"{"text":"test"}"#)]).await;
    let config = config(&[("chosen", &server.base_url, ProviderKind::OpenAiCompatible)]);
    let dir = tempfile::tempdir().unwrap();
    let service = ProviderService::new(dir.path(), Arc::new(MemoryKeyStore::default()));
    let selected = [
        Selection::new("chosen", "one"),
        Selection::new("unknown", "two"),
    ];
    assert!(
        run(&config, &service, &selected, &sample(1), "hej", dir.path())
            .await
            .is_err()
    );
    assert!(server.requests().is_empty());
}

#[test]
fn imports_sixty_seconds_and_rejects_invalid_or_oversized_audio() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("synthetic.wav");
    let audio = sample(60);
    std::fs::write(&path, &audio.wav).unwrap();
    assert_eq!(Sample::load(&path).unwrap().duration_seconds(), 60.0);
    std::fs::write(&path, b"not a WAV").unwrap();
    assert!(Sample::load(&path).is_err());
    let long = crate::dictation::logic::audio::wav_bytes(&vec![0.0; 16_000 * 91], 16_000);
    std::fs::write(&path, long).unwrap();
    assert!(Sample::load(&path).is_err());
}

#[tokio::test]
async fn absent_local_model_is_skipped_without_keys_or_network() {
    let config = ProvidersConfig::default();
    let dir = tempfile::tempdir().unwrap();
    let service = ProviderService::new(dir.path(), Arc::new(MemoryKeyStore::default()));
    let selected = [Selection::new("local", "tiny")];
    let report = run(&config, &service, &selected, &sample(1), "hej", dir.path())
        .await
        .unwrap();
    assert_eq!(report.results[0].status, Status::Skipped);
    assert!(report.results[0].message.contains("Installér"));
    assert!(report.results[0].score.is_none());
}
