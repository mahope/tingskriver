use std::sync::Arc;
use tingskriver_core::delivery::*;
use tingskriver_core::providers::secrets::MemoryKeyStore;

fn bundle() -> Bundle {
    Bundle {
        sidecar: Sidecar {
            kilde: "tingskriver".into(),
            skemaversion: 1,
            titel: "Ugemøde".into(),
            tid: "2026-11-04T10:00:00Z".parse().unwrap(),
            talere: vec!["Taler 1".into()],
            opgaver: vec![Task {
                tekst: "Følg op".into(),
                ansvarlig: None,
            }],
        },
        transcript: "Transskript".into(),
        summary: "Referat".into(),
        notes: "Noter".into(),
    }
}

#[test]
fn channel_limits_count_utf16_and_do_not_split_unicode() {
    let mut b = bundle();
    b.summary = "😀æ\"\\\n".repeat(40000);
    for (format, limit, field) in [
        (WebhookFormat::Slack, 4000, "text"),
        (WebhookFormat::Discord, 2000, "content"),
        (WebhookFormat::Teams, 6000, "text"),
    ] {
        let value = webhook_payload(format, &b, "id");
        assert!(value[field].as_str().unwrap().encode_utf16().count() <= limit);
        assert!(serde_json::to_vec(&value).unwrap().len() < 28000);
        assert!(!value[field].as_str().unwrap().is_empty());
    }
}

#[test]
fn serialized_sidecar_matches_the_schema_fixture() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../../testsaet/delivery/sidecar-v1.json")).unwrap();
    assert_eq!(serde_json::to_value(bundle().sidecar).unwrap(), fixture);
    let mut invalid = bundle();
    invalid.sidecar.skemaversion = 2;
    assert!(invalid.validate().is_err());
}

#[tokio::test]
async fn network_is_blocked_after_enqueue_in_both_privacy_modes() {
    use tokio::{
        net::TcpListener,
        time::{timeout, Duration},
    };
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let dir = tempfile::tempdir().unwrap();
    let service = Delivery::open(dir.path(), Arc::new(MemoryKeyStore::default()))
        .await
        .unwrap();
    service
        .set_policy(Policy {
            local_only: false,
            private: false,
        })
        .await
        .unwrap();
    service
        .save_target(
            Target {
                id: "hook".into(),
                name: "Webhook".into(),
                destination: Destination::Webhook {
                    format: WebhookFormat::Json,
                },
            },
            Some(format!(
                "http://{}/synthetic-token",
                listener.local_addr().unwrap()
            )),
        )
        .await
        .unwrap();
    service
        .enqueue("hook", bundle(), true, false)
        .await
        .unwrap();
    for policy in [
        Policy {
            local_only: true,
            private: false,
        },
        Policy {
            local_only: false,
            private: true,
        },
    ] {
        service.set_policy(policy).await.unwrap();
        assert!(service.test_target("hook").await.is_err());
        service.run_due().await.unwrap();
        assert!(timeout(Duration::from_millis(50), listener.accept())
            .await
            .is_err());
        assert_eq!(service.jobs().await.unwrap()[0].attempts, 0);
    }
    let db = std::fs::read(dir.path().join("delivery.sqlite-wal")).unwrap();
    assert!(!String::from_utf8_lossy(&db).contains("synthetic-token"));
}

#[tokio::test]
async fn webhook_failures_retry_with_stable_id_and_do_not_leak_secrets() {
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let dir = tempfile::tempdir().unwrap();
    let service = Delivery::open(dir.path(), Arc::new(MemoryKeyStore::default()))
        .await
        .unwrap();
    service
        .set_policy(Policy {
            local_only: false,
            private: false,
        })
        .await
        .unwrap();
    service
        .save_target(
            Target {
                id: "hook".into(),
                name: "Webhook".into(),
                destination: Destination::Webhook {
                    format: WebhookFormat::Json,
                },
            },
            Some(format!(
                "http://{}/synthetic-token",
                listener.local_addr().unwrap()
            )),
        )
        .await
        .unwrap();
    let id = service
        .enqueue("hook", bundle(), true, false)
        .await
        .unwrap();
    for response in ["500 Internal Server Error", "200 OK"] {
        let receive = async {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = vec![0; 65536];
            let n = stream.read(&mut request).await.unwrap();
            let request = String::from_utf8_lossy(&request[..n]).to_string();
            assert!(request
                .to_lowercase()
                .contains(&format!("idempotency-key: {id}")));
            stream.write_all(format!("HTTP/1.1 {response}\r\nContent-Length: 16\r\nConnection: close\r\n\r\nsynthetic-secret").as_bytes()).await.unwrap();
        };
        let (sent, _) = tokio::join!(service.run_due(), receive);
        sent.unwrap();
        let jobs = service.jobs().await.unwrap();
        let status = &jobs[0];
        assert!(!serde_json::to_string(status).unwrap().contains("synthetic"));
        if response.starts_with("500") {
            assert_eq!(status.status, "pending");
            assert!(status.error.is_some());
            service.retry(&id).await.unwrap();
        } else {
            assert_eq!(status.status, "completed");
        }
    }
}

#[tokio::test]
async fn approval_target_and_privacy_are_required_even_for_test_delivery() {
    let dir = tempfile::tempdir().unwrap();
    let service = Delivery::open(dir.path(), Arc::new(MemoryKeyStore::default()))
        .await
        .unwrap();
    service
        .set_policy(Policy {
            local_only: false,
            private: false,
        })
        .await
        .unwrap();
    assert!(service
        .enqueue("missing", bundle(), true, false)
        .await
        .is_err());
    service
        .save_target(
            Target {
                id: "folder".into(),
                name: "Mappe".into(),
                destination: Destination::Folder {
                    path: dir.path().join("out"),
                },
            },
            None,
        )
        .await
        .unwrap();
    assert!(service
        .enqueue("folder", bundle(), false, false)
        .await
        .is_err());
    assert!(service
        .enqueue("folder", bundle(), true, true)
        .await
        .is_err());
    service
        .set_policy(Policy {
            local_only: true,
            private: false,
        })
        .await
        .unwrap();
    assert!(service.test_target("folder").await.is_err());
    assert!(service
        .enqueue("folder", bundle(), true, false)
        .await
        .is_err());
    assert!(!dir.path().join("out").exists());
}

#[tokio::test]
async fn failed_delivery_survives_reopen_and_retries_without_losing_payload() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("out");
    std::fs::write(&out, "not a directory").unwrap();
    let keys = Arc::new(MemoryKeyStore::default());
    let service = Delivery::open(dir.path(), keys.clone()).await.unwrap();
    service
        .set_policy(Policy {
            local_only: false,
            private: false,
        })
        .await
        .unwrap();
    service
        .save_target(
            Target {
                id: "folder".into(),
                name: "Mappe".into(),
                destination: Destination::Folder { path: out.clone() },
            },
            None,
        )
        .await
        .unwrap();
    let id = service
        .enqueue("folder", bundle(), true, false)
        .await
        .unwrap();
    service.run_due().await.unwrap();
    let job = service.jobs().await.unwrap().remove(0);
    assert_eq!(job.status, "pending");
    assert_eq!(job.attempts, 1);
    assert!(job.error.is_some());
    assert!(job.next_attempt > chrono::Utc::now().timestamp());
    drop(service);
    std::fs::remove_file(&out).unwrap();
    let service = Delivery::open(dir.path(), keys).await.unwrap();
    service.retry(&id).await.unwrap();
    service.run_due().await.unwrap();
    assert_eq!(service.jobs().await.unwrap()[0].status, "completed");
    for name in ["transcript.md", "summary.md", "notes.md", "sidecar.json"] {
        assert!(out.join(&id).join(name).is_file());
    }
    assert_eq!(
        std::fs::read_to_string(out.join(&id).join("summary.md")).unwrap(),
        "Referat"
    );
}
