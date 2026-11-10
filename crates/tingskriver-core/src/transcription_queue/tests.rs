use super::*;
use crate::providers::{
    client::TranscriptSegment, model::*, secrets::MemoryKeyStore, ProviderService, Transcription,
};
use std::{sync::Arc, time::Duration};

use crate::providers::fake_server;

fn service(dir: &std::path::Path) -> ProviderService {
    ProviderService::new(dir, Arc::new(MemoryKeyStore::default()))
}

fn remote_config(url: &str) -> ProvidersConfig {
    let mut provider = custom_provider("fake", "Fake", url);
    provider.transcription_model = Some("test".into());
    provider.diarization = true;
    let mut config = ProvidersConfig::default();
    config.upsert_provider(provider);
    config.functions.insert(
        Function::Transcription,
        vec![ChainLink::new("fake").with_timeout_ms(100)],
    );
    config
}

#[tokio::test]
async fn hotwords_reach_only_the_called_link_as_a_prompt() {
    use fake_server::{FakeServer, Reply};
    let body = r#"{"text":"Hej","segments":[{"start":0,"end":1,"text":"Hej"}]}"#;
    let server = FakeServer::start(vec![Reply::ok(body)]).await;
    let unused = FakeServer::start(vec![Reply::ok(body)]).await;
    let dir = tempfile::tempdir().unwrap();
    let mic = dir.path().join("mic.wav");
    std::fs::write(&mic, [0, 1]).unwrap();
    let mut spec = JobSpec::new("meeting", mic, None);
    spec.hotwords = crate::hotwords::Hotwords {
        participant_names: vec!["Ægir Ø".into()],
        dictionary_words: vec!["Åle".into(), "Aale".into(), "x".repeat(225)],
    };
    let mut config = remote_config(&server.base_url);
    let mut reserve = custom_provider("reserve", "Reserve", &unused.base_url);
    reserve.transcription_model = Some("test".into());
    config.upsert_provider(reserve);
    config
        .functions
        .get_mut(&Function::Transcription)
        .unwrap()
        .push(ChainLink::new("reserve"));
    let queue = Queue::open_memory().await.unwrap();
    queue.enqueue(&spec, 0).await.unwrap();
    queue
        .run_once_at(
            &service(dir.path()),
            &config,
            |_, _| async { panic!("remote only") },
            |_| {},
            0,
        )
        .await
        .unwrap();
    assert_eq!(queue.statuses().await.unwrap()[0].status, "completed");
    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].contains("name=\"prompt\"\r\n\r\nÆgir Ø, Åle, Aale\r\n"));
    assert!(!requests[0].contains(&"x".repeat(225)));
    assert!(!requests[0].contains("name=\"hotwords\""));
    assert!(unused.requests().is_empty());
}

#[test]
fn old_jobs_without_hotwords_still_deserialize() {
    let spec = JobSpec::new("meeting", "mic.wav".into(), None);
    let mut value = serde_json::to_value(spec).unwrap();
    value.as_object_mut().unwrap().remove("hotwords");
    let restored: JobSpec = serde_json::from_value(value).unwrap();
    assert_eq!(restored.hotwords.prompt(224), None);
}

#[tokio::test]
async fn interrupted_job_survives_reopen_and_rerun_preserves_versions() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("queue.sqlite");
    let queue = Queue::open(&path).await.unwrap();
    let spec = JobSpec::new("meeting-1", dir.path().join("mic.wav"), None);
    let id = queue.enqueue(&spec, 0).await.unwrap();
    let claimed = queue.claim(0).await.unwrap().unwrap();
    assert_eq!(claimed.id, id);
    assert!(queue.claim(0).await.unwrap().is_none());
    drop(queue);
    let queue = Queue::open(&path).await.unwrap();
    assert_eq!(queue.recover_interrupted().await.unwrap(), 1);
    let claimed = queue.claim(0).await.unwrap().unwrap();
    assert_eq!(claimed.attempts, 2);
    queue
        .complete(&claimed, &serde_json::json!({"text":"første"}))
        .await
        .unwrap();
    let mut again = spec.clone();
    again.language = "en".into();
    let second = queue.enqueue(&again, 1).await.unwrap();
    assert_ne!(second, id);
    let claimed = queue.claim(1).await.unwrap().unwrap();
    queue
        .complete(&claimed, &serde_json::json!({"text":"second"}))
        .await
        .unwrap();
    let versions = queue.versions("meeting-1").await.unwrap();
    assert_eq!(versions.len(), 2);
    assert_eq!(versions[0].result["text"], "første");
    assert_eq!(versions[1].spec.language, "en");
}

#[tokio::test]
async fn fake_server_failures_back_off_and_end_in_failed_status() {
    use fake_server::{FakeServer, Reply};
    for reply in [
        Reply::json("500 Internal Server Error", "{}"),
        Reply::json("429 Too Many Requests", "{}").with_header("retry-after", "120"),
        Reply::json("503 Service Unavailable", "{}").with_header("retry-after", "120"),
        Reply::Hang,
        Reply::HangAfterHeaders,
    ] {
        let rate_limited = matches!(
            &reply,
            Reply::Respond {
                status: "429 Too Many Requests" | "503 Service Unavailable",
                ..
            }
        );
        let server = FakeServer::start(vec![reply]).await;
        let dir = tempfile::tempdir().unwrap();
        let mic = dir.path().join("mic.wav");
        std::fs::write(&mic, [0, 1, 2]).unwrap();
        let queue = Queue::open_memory().await.unwrap();
        queue
            .enqueue(&JobSpec::new("m", mic, None), 0)
            .await
            .unwrap();
        let mut now = 0;
        let mut previous = 0;
        for attempt in 1..=MAX_ATTEMPTS {
            let mut events = Vec::new();
            assert!(queue
                .run_once_at(
                    &service(dir.path()),
                    &remote_config(&server.base_url),
                    |_, _| async { panic!("remote only") },
                    |e| events.push(e),
                    now
                )
                .await
                .unwrap());
            let last = events.last().unwrap();
            assert_eq!(last.attempts, attempt);
            if attempt == MAX_ATTEMPTS {
                assert_eq!(last.status, "failed");
                assert!(last.next_attempt.is_none());
            } else {
                assert_eq!(last.status, "pending");
                let due = last.next_attempt.unwrap();
                let delay = due - now;
                assert!(delay >= previous);
                assert!(delay >= if rate_limited { 120 } else { 5 });
                if !rate_limited {
                    let base = 5 * (1 << (attempt - 1));
                    assert!(delay >= base && delay <= base + base / 4 + 1);
                    assert!(delay > previous);
                }
                previous = delay;
                assert!(queue.claim(due - 1).await.unwrap().is_none());
                now = due;
            }
        }
        assert_eq!(server.requests().len(), MAX_ATTEMPTS as usize);
        assert_eq!(queue.statuses().await.unwrap()[0].status, "failed");
        assert!(queue.versions("m").await.unwrap().is_empty());
    }
}

#[tokio::test]
async fn no_key_uses_local_in_danish_and_merges_speakers() {
    let dir = tempfile::tempdir().unwrap();
    let mic = dir.path().join("mic.wav");
    std::fs::write(&mic, [1, 2, 3]).unwrap();
    let queue = Queue::open_memory().await.unwrap();
    let mut spec = JobSpec::new("m", mic.clone(), Some(mic));
    spec.microphone_label = "Testbruger".into();
    spec.offset_ms = 2000;
    queue.enqueue(&spec, 0).await.unwrap();
    let mut calls = 0;
    queue
        .run_once_at(
            &service(dir.path()),
            &ProvidersConfig::default(),
            |link, request| {
                assert_eq!(link.provider_id, LOCAL_ID);
                assert_eq!(request.language.as_deref(), Some("da"));
                assert_eq!(request.diarize, calls == 1);
                calls += 1;
                async {
                    Ok(Transcription {
                        text: "Det er dansk tekst.".into(),
                        segments: vec![TranscriptSegment {
                            start: Some(0.0),
                            end: Some(1.0),
                            text: "Det er dansk tekst.".into(),
                            speaker: None,
                        }],
                        ..Default::default()
                    })
                }
            },
            |_| {},
            0,
        )
        .await
        .unwrap();
    assert_eq!(calls, 2);
    let versions = queue.versions("m").await.unwrap();
    let segments = &versions[0].result["segments"];
    assert_eq!(segments[0]["speaker"], "Testbruger");
    assert_eq!(segments[0]["text"], "Det er dansk tekst.");
    assert_eq!(segments[0]["startMs"], 2000);
    assert_eq!(segments[1]["speakersMissing"], true);
}

#[tokio::test]
async fn remote_system_speakers_stay_separate_and_requests_do_not_translate() {
    use fake_server::{FakeServer, Reply};
    let server = FakeServer::start(vec![
        Reply::ok(r#"{"text":"mig igen","segments":[{"start":1,"end":2,"text":"mig"},{"start":3,"end":4,"text":"igen"}]}"#),
        Reply::ok(r#"{"text":"andre","segments":[{"start":0,"end":1,"text":"en","speaker":"a"},{"start":2,"end":3,"text":"to","speaker":"b"}]}"#),
    ]).await;
    let dir = tempfile::tempdir().unwrap();
    let mic = dir.path().join("mic.wav");
    std::fs::write(&mic, [1]).unwrap();
    let queue = Queue::open_memory().await.unwrap();
    queue
        .enqueue(&JobSpec::new("m", mic.clone(), Some(mic)), 0)
        .await
        .unwrap();
    queue
        .run_once_at(
            &service(dir.path()),
            &remote_config(&server.base_url),
            |_, _| async { panic!("remote only") },
            |_| {},
            0,
        )
        .await
        .unwrap();
    let versions = queue.versions("m").await.unwrap();
    let segments = &versions[0].result["segments"];
    assert_eq!(segments[0]["speaker"], "Taler 1");
    assert_eq!(segments[1]["speaker"], "Mig");
    assert_eq!(segments[2]["speaker"], "Taler 2");
    assert_eq!(segments[3]["speaker"], "Mig");
    let requests = server.requests();
    for request in &requests {
        assert!(request.starts_with("POST /v1/audio/transcriptions"));
        assert!(request.contains("name=\"language\"\r\n\r\nda"));
        assert!(request.contains("name=\"response_format\"\r\n\r\nverbose_json"));
        assert!(!request.contains("name=\"prompt\""));
    }
    assert!(!requests[0].contains("name=\"diarize\""));
    assert!(requests[1].contains("name=\"diarize\"\r\n\r\ntrue"));
}

#[tokio::test]
async fn successful_fallback_preserves_provider_cooldown_across_tracks_and_restart() {
    use fake_server::{FakeServer, Reply};
    let server = FakeServer::start(vec![
        Reply::json("429 Too Many Requests", "{}").with_header("retry-after", "120")
    ])
    .await;
    let dir = tempfile::tempdir().unwrap();
    let mic = dir.path().join("mic.wav");
    std::fs::write(&mic, [1]).unwrap();
    let db = dir.path().join("queue.sqlite");
    let mut config = remote_config(&server.base_url);
    config
        .functions
        .get_mut(&Function::Transcription)
        .unwrap()
        .push(ChainLink::new(LOCAL_ID));
    for now in [0, 1, 121] {
        let queue = Queue::open(&db).await.unwrap();
        queue
            .enqueue(&JobSpec::new("m", mic.clone(), Some(mic.clone())), now)
            .await
            .unwrap();
        queue
            .run_once_at(
                &service(dir.path()),
                &config,
                |_, _| async { Ok(Transcription::default()) },
                |_| {},
                now,
            )
            .await
            .unwrap();
        assert_eq!(server.requests().len(), if now < 120 { 1 } else { 2 });
    }
}

#[tokio::test]
async fn untimed_remote_text_uses_timestamped_reserve() {
    use fake_server::{FakeServer, Reply};
    let server = FakeServer::start(vec![Reply::ok(r#"{"text":"untimed"}"#)]).await;
    let dir = tempfile::tempdir().unwrap();
    let mic = dir.path().join("mic.wav");
    std::fs::write(&mic, [1]).unwrap();
    let mut config = remote_config(&server.base_url);
    config
        .functions
        .get_mut(&Function::Transcription)
        .unwrap()
        .push(ChainLink::new(LOCAL_ID));
    let queue = Queue::open_memory().await.unwrap();
    queue
        .enqueue(&JobSpec::new("m", mic, None), 0)
        .await
        .unwrap();
    queue
        .run_once_at(
            &service(dir.path()),
            &config,
            |_, _| async {
                Ok(Transcription {
                    text: "Senere svar".into(),
                    segments: vec![TranscriptSegment {
                        start: Some(5.0),
                        end: Some(6.0),
                        text: "Senere svar".into(),
                        speaker: None,
                    }],
                    ..Default::default()
                })
            },
            |_| {},
            0,
        )
        .await
        .unwrap();
    let versions = queue.versions("m").await.unwrap();
    assert_eq!(versions[0].result["segments"][0]["startMs"], 5000);
    assert_eq!(versions[0].result["tracks"][0]["providerId"], LOCAL_ID);
}

#[test]
fn killed_process_resumes_committed_claim() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("queue.sqlite");
    let ready = dir.path().join("ready");
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "transcription_queue::tests::crash_child",
            "--nocapture",
        ])
        .env("TINGSKRIVER_QUEUE_CRASH_DB", &db)
        .env("TINGSKRIVER_QUEUE_CRASH_READY", &ready)
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    while !ready.exists() {
        if std::time::Instant::now() > deadline || child.try_wait().unwrap().is_some() {
            let _ = child.kill();
            let _ = child.wait();
            panic!("child did not commit its claim");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    child.kill().unwrap();
    assert!(!child.wait().unwrap().success());
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let queue = Queue::open(&db).await.unwrap();
        assert_eq!(queue.recover_interrupted().await.unwrap(), 1);
        queue
            .run_once_at(
                &service(dir.path()),
                &ProvidersConfig::default(),
                |_, _| async {
                    Ok(Transcription {
                        text: "Genoptaget".into(),
                        segments: vec![TranscriptSegment {
                            start: Some(0.0),
                            end: Some(1.0),
                            text: "Genoptaget".into(),
                            speaker: None,
                        }],
                        ..Default::default()
                    })
                },
                |_| {},
                0,
            )
            .await
            .unwrap();
        let status = queue.statuses().await.unwrap();
        assert_eq!(status[0].status, "completed");
        assert_eq!(status[0].attempts, 2);
        assert_eq!(queue.versions("crashed").await.unwrap().len(), 1);
    });
}

#[tokio::test]
async fn crash_child() {
    let Some(db) = std::env::var_os("TINGSKRIVER_QUEUE_CRASH_DB") else {
        return;
    };
    let queue = Queue::open(std::path::Path::new(&db)).await.unwrap();
    let mic = std::path::Path::new(&db).with_extension("wav");
    std::fs::write(&mic, [1]).unwrap();
    queue
        .enqueue(&JobSpec::new("crashed", mic, None), 0)
        .await
        .unwrap();
    queue.claim(0).await.unwrap().unwrap();
    std::fs::write(
        std::env::var_os("TINGSKRIVER_QUEUE_CRASH_READY").unwrap(),
        "ready",
    )
    .unwrap();
    std::future::pending::<()>().await;
}

#[tokio::test]
async fn pending_backoff_survives_reopen_and_stale_attempt_cannot_complete() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("queue.sqlite");
    let queue = Queue::open(&db).await.unwrap();
    queue
        .enqueue(&JobSpec::new("m", "mic.wav".into(), None), 0)
        .await
        .unwrap();
    let old = queue.claim(0).await.unwrap().unwrap();
    let due = queue.fail(&old, Some(120), 0).await.unwrap().unwrap();
    assert!(queue.complete(&old, &serde_json::json!({})).await.is_err());
    drop(queue);
    let queue = Queue::open(&db).await.unwrap();
    assert_eq!(queue.recover_interrupted().await.unwrap(), 0);
    assert!(queue.claim(due - 1).await.unwrap().is_none());
    let (a, b) = tokio::join!(queue.claim(due), queue.claim(due));
    assert_eq!(
        usize::from(a.unwrap().is_some()) + usize::from(b.unwrap().is_some()),
        1
    );
    assert!(queue.complete(&old, &serde_json::json!({})).await.is_err());
}

#[tokio::test]
async fn local_adapter_decodes_wav_and_passes_explicit_language() {
    use crate::audio::transcription::{
        TranscriptResult, TranscriptionError, TranscriptionProvider,
    };
    struct Local;
    #[async_trait::async_trait]
    impl TranscriptionProvider for Local {
        async fn transcribe(
            &self,
            audio: Vec<f32>,
            language: Option<String>,
        ) -> Result<TranscriptResult, TranscriptionError> {
            assert_eq!(language.as_deref(), Some("da"));
            assert!(audio.len() < 32000);
            Ok(TranscriptResult {
                text: "Dansk møde".into(),
                confidence: None,
                is_partial: false,
            })
        }
        async fn is_model_loaded(&self) -> bool {
            true
        }
        async fn get_current_model(&self) -> Option<String> {
            Some("test".into())
        }
        fn provider_name(&self) -> &'static str {
            "test"
        }
    }
    let mut wav = Vec::new();
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(160036_u32).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16_u32.to_le_bytes());
    wav.extend_from_slice(&1_u16.to_le_bytes());
    wav.extend_from_slice(&1_u16.to_le_bytes());
    wav.extend_from_slice(&16000_u32.to_le_bytes());
    wav.extend_from_slice(&32000_u32.to_le_bytes());
    wav.extend_from_slice(&2_u16.to_le_bytes());
    wav.extend_from_slice(&16_u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&160000_u32.to_le_bytes());
    for i in 0..80000 {
        let sample = if (16000..32000).contains(&i) || (48000..64000).contains(&i) {
            (10000.0 * (i as f32 * 0.17).sin()) as i16
        } else {
            0
        };
        wav.extend_from_slice(&sample.to_le_bytes());
    }
    let result = super::worker::transcribe_local_with_vad(
        Arc::new(Local),
        crate::providers::TranscriptionRequest {
            wav,
            language: Some("da".into()),
            ..Default::default()
        },
        crate::vad_trim::TrimConfig {
            backend: crate::vad_trim::BackendPreference::Energy,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(result.text, "Dansk møde Dansk møde");
    assert_eq!(result.segments.len(), 2);
    assert!(result.segments[0].start.unwrap() > 0.5);
    assert!(result.segments[0].end.unwrap() < 2.5);
    assert!(result.segments[1].start.unwrap() > 2.5);
    let dir = tempfile::tempdir().unwrap();
    let mic = dir.path().join("mic.wav");
    std::fs::write(&mic, [1]).unwrap();
    let queue = Queue::open_memory().await.unwrap();
    queue
        .enqueue(&JobSpec::new("m", mic.clone(), Some(mic)), 0)
        .await
        .unwrap();
    queue
        .run_once_at(
            &service(dir.path()),
            &ProvidersConfig::default(),
            |_, request| {
                let transcript = if request.diarize {
                    Transcription {
                        text: "Først. Igen.".into(),
                        segments: vec![
                            TranscriptSegment {
                                start: Some(0.0),
                                end: Some(0.5),
                                text: "Først.".into(),
                                speaker: None,
                            },
                            TranscriptSegment {
                                start: Some(2.5),
                                end: Some(2.7),
                                text: "Igen.".into(),
                                speaker: None,
                            },
                        ],
                        ..Default::default()
                    }
                } else {
                    result.clone()
                };
                async move { Ok(transcript) }
            },
            |_| {},
            0,
        )
        .await
        .unwrap();
    let versions = queue.versions("m").await.unwrap();
    let merged = versions[0].result["segments"].as_array().unwrap();
    let sources: Vec<_> = merged
        .iter()
        .map(|s| s["source"].as_str().unwrap())
        .collect();
    assert_eq!(sources, ["system", "microphone", "system", "microphone"]);
}

#[tokio::test]
async fn rerun_uses_provider_and_language_override_without_replacing_success() {
    let dir = tempfile::tempdir().unwrap();
    let mic = dir.path().join("mic.wav");
    std::fs::write(&mic, [1]).unwrap();
    let queue = Queue::open_memory().await.unwrap();
    let mut spec = JobSpec::new("m", mic, None);
    let mut local = ProvidersConfig::default();
    local
        .functions
        .insert(Function::Transcription, vec![ChainLink::new(LOCAL_ID)]);
    spec.providers = Some(local);
    // The per-job override must win over this unusable default chain.
    let default = remote_config("http://127.0.0.1:1/v1");
    for (now, language) in [(0, "da"), (1, "en")] {
        spec.language = language.into();
        queue.enqueue(&spec, now).await.unwrap();
        queue
            .run_once_at(
                &service(dir.path()),
                &default,
                |link, request| {
                    assert_eq!(link.provider_id, LOCAL_ID);
                    assert_eq!(request.language.as_deref(), Some(language));
                    async move {
                        Ok(Transcription {
                            text: language.into(),
                            segments: vec![TranscriptSegment {
                                start: Some(0.0),
                                end: Some(1.0),
                                text: language.into(),
                                speaker: None,
                            }],
                            ..Default::default()
                        })
                    }
                },
                |_| {},
                now,
            )
            .await
            .unwrap();
    }
    queue.enqueue(&spec, 2).await.unwrap();
    queue
        .run_once_at(
            &service(dir.path()),
            &default,
            |_, _| async { Err(crate::providers::ProviderError::local("injected")) },
            |_| {},
            2,
        )
        .await
        .unwrap();
    let versions = queue.versions("m").await.unwrap();
    assert_eq!(versions.len(), 2);
    assert_eq!(versions[0].result["segments"][0]["text"], "da");
    assert_eq!(versions[1].result["segments"][0]["text"], "en");
}

#[tokio::test]
async fn retries_increase_and_retry_after_is_a_lower_bound() {
    let queue = Queue::open_memory().await.unwrap();
    queue
        .enqueue(&JobSpec::new("m", "mic.wav".into(), None), 0)
        .await
        .unwrap();
    let mut now = 0;
    let mut previous = 0;
    for attempt in 1..=MAX_ATTEMPTS {
        let job = queue.claim(now).await.unwrap().unwrap();
        assert_eq!(job.attempts, attempt);
        let next = queue.fail(&job, Some(60), now).await.unwrap();
        if attempt == MAX_ATTEMPTS {
            assert!(next.is_none());
        } else {
            let due = next.unwrap();
            assert!(due - now >= 60);
            assert!(due - now >= previous);
            previous = due - now;
            assert!(queue.claim(due - 1).await.unwrap().is_none());
            now = due;
        }
    }
    assert!(queue.claim(i64::MAX).await.unwrap().is_none());
}
