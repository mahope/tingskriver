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

#[tokio::test(flavor = "current_thread")]
async fn credentials_run_off_the_async_executor() {
    use std::sync::Mutex;
    use tingskriver_core::providers::secrets::{ApiKey, KeyStore};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    struct ThreadCheckedKeys {
        inner: MemoryKeyStore,
        calls: Mutex<Vec<(&'static str, std::thread::ThreadId)>>,
    }
    impl KeyStore for ThreadCheckedKeys {
        fn get(&self, target: &str) -> Result<Option<ApiKey>, String> {
            self.calls
                .lock()
                .unwrap()
                .push(("get", std::thread::current().id()));
            self.inner.get(target)
        }
        fn set(&self, target: &str, key: &ApiKey) -> Result<(), String> {
            self.calls
                .lock()
                .unwrap()
                .push(("set", std::thread::current().id()));
            self.inner.set(target, key)
        }
        fn delete(&self, target: &str) -> Result<(), String> {
            self.calls
                .lock()
                .unwrap()
                .push(("delete", std::thread::current().id()));
            self.inner.delete(target)
        }
    }

    let executor = std::thread::current().id();
    let keys = Arc::new(ThreadCheckedKeys {
        inner: MemoryKeyStore::default(),
        calls: Mutex::new(Vec::new()),
    });
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let dir = tempfile::tempdir().unwrap();
    let service = Delivery::open(&dir.path().join("queue"), keys.clone())
        .await
        .unwrap();
    service
        .set_policy(Policy {
            local_only: false,
            private: false,
        })
        .await
        .unwrap();
    let target = Target {
        id: "hook".into(),
        name: "Webhook".into(),
        destination: Destination::Webhook {
            format: WebhookFormat::Json,
        },
    };
    service
        .save_target(
            target.clone(),
            Some(format!(
                "http://{}/synthetic",
                listener.local_addr().unwrap()
            )),
        )
        .await
        .unwrap();
    service.save_target(target, None).await.unwrap();
    service.test_target("hook").await.unwrap();
    let receive = async {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = vec![0; 65536];
        stream.read(&mut request).await.unwrap();
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
    };
    let (sent, _) = tokio::join!(service.run_due(), receive);
    sent.unwrap();
    assert_eq!(service.jobs().await.unwrap()[0].status, "completed");
    service.delete_target("hook").await.unwrap();
    let calls = keys.calls.lock().unwrap();
    assert_eq!(
        calls
            .iter()
            .map(|(operation, _)| *operation)
            .collect::<Vec<_>>(),
        ["set", "get", "get", "delete"]
    );
    assert!(calls.iter().all(|(_, thread)| *thread != executor));
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

async fn database(dir: &std::path::Path) -> sqlx::SqlitePool {
    sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            sqlx::sqlite::SqliteConnectOptions::new().filename(dir.join("delivery.sqlite")),
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn interrupted_jobs_recover_and_exhausted_jobs_stay_visible_until_manual_retry() {
    let dir = tempfile::tempdir().unwrap();
    let keys = Arc::new(MemoryKeyStore::default());
    let service = Delivery::open(dir.path(), keys.clone()).await.unwrap();
    service
        .set_policy(Policy {
            local_only: false,
            private: false,
        })
        .await
        .unwrap();
    let out = dir.path().join("not-directory");
    std::fs::write(&out, "blocked").unwrap();
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
    let id = service.test_target("folder").await.unwrap();
    let pool = database(dir.path()).await;
    sqlx::query("UPDATE delivery_jobs SET status = 'running', attempts = 4")
        .execute(&pool)
        .await
        .unwrap();
    drop(service);
    let service = Delivery::open(dir.path(), keys).await.unwrap();
    assert_eq!(service.jobs().await.unwrap()[0].status, "pending");
    service.run_due().await.unwrap();
    assert_eq!(service.jobs().await.unwrap()[0].status, "failed");
    assert_eq!(service.jobs().await.unwrap()[0].attempts, 5);
    service.run_due().await.unwrap();
    assert_eq!(service.jobs().await.unwrap()[0].attempts, 5);
    let payload: String = sqlx::query_scalar("SELECT bundle FROM delivery_jobs")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(payload.contains("Test fra Tingskriver"));
    std::fs::remove_file(&out).unwrap();
    service.retry(&id).await.unwrap();
    service.run_due().await.unwrap();
    assert_eq!(service.jobs().await.unwrap()[0].status, "completed");
    assert!(service.retry(&id).await.is_err());
}

#[tokio::test]
async fn pending_approval_cannot_be_redirected_by_editing_the_target_or_its_secret() {
    use tingskriver_core::providers::secrets::KeyStore;
    let dir = tempfile::tempdir().unwrap();
    let keys = Arc::new(MemoryKeyStore::default());
    let service = Delivery::open(dir.path(), keys.clone()).await.unwrap();
    service
        .set_policy(Policy {
            local_only: false,
            private: false,
        })
        .await
        .unwrap();
    let target = Target {
        id: "hook".into(),
        name: "Webhook".into(),
        destination: Destination::Webhook {
            format: WebhookFormat::Json,
        },
    };
    service
        .save_target(
            target.clone(),
            Some("https://example.invalid/original-synthetic-token".into()),
        )
        .await
        .unwrap();
    service.test_target("hook").await.unwrap();
    assert!(service
        .save_target(
            target,
            Some("https://example.invalid/replacement-synthetic-token".into())
        )
        .await
        .is_err());
    assert_eq!(
        keys.get("Tingskriver/delivery/hook")
            .unwrap()
            .unwrap()
            .expose(),
        "https://example.invalid/original-synthetic-token"
    );
    service.delete_target("hook").await.unwrap();
    service.run_due().await.unwrap();
    assert_eq!(service.jobs().await.unwrap()[0].status, "pending");
    assert!(service.jobs().await.unwrap()[0].error.is_some());
}

#[test]
fn ssh_options_and_batch_injection_are_rejected() {
    for (host, user, remote_dir) in [
        ("-oProxyCommand=oops", "upload", "/upload"),
        ("host", "-bad", "/upload"),
        ("host", "upload", "/upload\n!command"),
        ("host", "upload", "/upload/../elsewhere"),
        ("host", "upload", "relative"),
    ] {
        let target = Target {
            id: "ssh".into(),
            name: "SSH".into(),
            destination: Destination::Ssh {
                host: host.into(),
                port: 22,
                user: user.into(),
                key_path: std::env::temp_dir().join("id_ed25519"),
                remote_dir: remote_dir.into(),
            },
        };
        assert!(target.validate().is_err());
    }
}

#[tokio::test]
async fn corrupted_policy_fails_closed() {
    let dir = tempfile::tempdir().unwrap();
    let service = Delivery::open(dir.path(), Arc::new(MemoryKeyStore::default()))
        .await
        .unwrap();
    let pool = database(dir.path()).await;
    sqlx::query("INSERT INTO delivery_settings VALUES ('policy', '{}')")
        .execute(&pool)
        .await
        .unwrap();
    assert!(service.policy().await.is_err());
    assert!(service.run_due().await.is_err());
    assert!(service.test_target("any").await.is_err());
}

#[tokio::test]
async fn meeting_adapter_exports_saved_notes_and_the_approved_summary() {
    use tingskriver_core::notes::{markdown, NotesStore};
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES ('meeting', 'Ugemøde', '2026-11-04T10:00:00Z', '2026-11-04T10:00:00Z')").execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO transcripts (id, meeting_id, transcript, timestamp) VALUES ('first', 'meeting', 'Syntetisk transskript', '10:01')").execute(&pool).await.unwrap();
    NotesStore::new(pool.clone())
        .save("meeting", &markdown::parse("Syntetiske noter").unwrap())
        .await
        .unwrap();
    let b = meeting::snapshot(&pool, "meeting", "Netop godkendt referat".into())
        .await
        .unwrap();
    assert_eq!(b.summary, "Netop godkendt referat");
    assert!(b.transcript.contains("Syntetisk transskript"));
    assert!(b.notes.contains("Syntetiske noter"));
    assert_eq!(b.sidecar.titel, "Ugemøde");
    assert_eq!(b.sidecar.tid.to_rfc3339(), "2026-11-04T10:00:00+00:00");
    assert!(meeting::snapshot(&pool, "missing", "Referat".into())
        .await
        .is_err());
    assert!(meeting::snapshot(&pool, "meeting", " ".into())
        .await
        .is_err());
}

#[tokio::test]
async fn privacy_change_between_attempts_stops_the_rest_of_the_batch() {
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
        sync::oneshot,
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
                "http://{}/synthetic",
                listener.local_addr().unwrap()
            )),
        )
        .await
        .unwrap();
    service.test_target("hook").await.unwrap();
    service.test_target("hook").await.unwrap();
    let (seen, started) = oneshot::channel();
    let (waiting, queued) = oneshot::channel();
    let receiver = async {
        let (mut first, _) = listener.accept().await.unwrap();
        let mut buf = [0; 8192];
        let received = first.read(&mut buf).await.unwrap();
        assert!(received > 0);
        seen.send(()).unwrap();
        queued.await.unwrap();
        first
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        drop(first);
        if let Ok(Ok((mut second, _))) =
            timeout(Duration::from_millis(200), listener.accept()).await
        {
            let received = second.read(&mut buf).await.unwrap();
            assert!(received > 0);
            second
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .await
                .unwrap();
        }
    };
    let policy = async {
        started.await.unwrap();
        let setting = service.set_policy(Policy {
            local_only: false,
            private: true,
        });
        tokio::pin!(setting);
        tokio::select! {
            biased;
            _ = &mut setting => panic!("policy must wait for the in-flight request"),
            _ = tokio::task::yield_now() => {}
        }
        waiting.send(()).unwrap();
        setting.await.unwrap();
    };
    let (sent, _, _) = tokio::join!(service.run_due(), receiver, policy);
    sent.unwrap();
    let jobs = service.jobs().await.unwrap();
    assert_eq!(jobs.iter().filter(|j| j.status == "completed").count(), 1);
    assert_eq!(
        jobs.iter()
            .filter(|j| j.status == "pending" && j.attempts == 0)
            .count(),
        1
    );
}

#[tokio::test(flavor = "current_thread")]
async fn signed_webhook_carries_verifiable_headers_and_secret_stays_in_keystore() {
    use tingskriver_core::delivery::signing;
    use tingskriver_core::providers::secrets::KeyStore;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    const SECRET: &str = "whsec_synthetic_signing_secret";
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let dir = tempfile::tempdir().unwrap();
    let keys = Arc::new(MemoryKeyStore::default());
    let service = Delivery::open(&dir.path().join("queue"), keys.clone())
        .await
        .unwrap();
    service
        .set_policy(Policy {
            local_only: false,
            private: false,
        })
        .await
        .unwrap();
    let target = Target {
        id: "hook".into(),
        name: "Webhook".into(),
        destination: Destination::Webhook {
            format: WebhookFormat::Json,
        },
    };
    service
        .save_target(
            target,
            Some(format!(
                "http://{}/synthetic",
                listener.local_addr().unwrap()
            )),
        )
        .await
        .unwrap();
    assert!(!service.has_signing_secret("hook").await.unwrap());
    service
        .set_signing_secret("hook", Some(SECRET.into()))
        .await
        .unwrap();
    assert!(service.has_signing_secret("hook").await.unwrap());
    service.test_target("hook").await.unwrap();

    let receive = async {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        let mut chunk = vec![0; 65536];
        let split = loop {
            let n = stream.read(&mut chunk).await.unwrap();
            request.extend_from_slice(&chunk[..n]);
            if let Some(i) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                break i + 4;
            }
        };
        let head = String::from_utf8_lossy(&request[..split]).to_string();
        let length: usize = head
            .lines()
            .find_map(|l| {
                l.to_ascii_lowercase()
                    .strip_prefix("content-length:")
                    .map(|v| v.trim().parse().unwrap())
            })
            .unwrap();
        while request.len() < split + length {
            let n = stream.read(&mut chunk).await.unwrap();
            request.extend_from_slice(&chunk[..n]);
        }
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        let header = |name: &str| {
            head.lines()
                .find_map(|l| {
                    let (k, v) = l.split_once(':')?;
                    k.eq_ignore_ascii_case(name).then(|| v.trim().to_string())
                })
                .unwrap()
        };
        (
            header(signing::TIMESTAMP_HEADER),
            header(signing::SIGNATURE_HEADER),
            request[split..split + length].to_vec(),
        )
    };
    let (sent, (timestamp, signature, body)) = tokio::join!(service.run_due(), receive);
    sent.unwrap();
    let timestamp: i64 = timestamp.parse().unwrap();
    let now = chrono::Utc::now().timestamp();
    assert_eq!(
        signing::verify(SECRET.as_bytes(), timestamp, &body, &signature, now, 300),
        Ok(())
    );
    let mut tampered = body.clone();
    tampered.push(b' ');
    assert!(signing::verify(
        SECRET.as_bytes(),
        timestamp,
        &tampered,
        &signature,
        now,
        300
    )
    .is_err());
    assert!(signing::verify(
        SECRET.as_bytes(),
        timestamp,
        &body,
        &signature,
        now + 3600,
        300
    )
    .is_err());

    // The secret is in the key store only: not in the queue database or status.
    assert!(keys.targets().iter().any(|t| t.ends_with("/signing")));
    assert!(keys
        .get("Tingskriver/delivery/hook/signing")
        .unwrap()
        .is_some());
    let status = format!("{:?}", service.jobs().await.unwrap());
    assert!(!status.contains(SECRET));
    for entry in std::fs::read_dir(dir.path().join("queue")).unwrap() {
        let bytes = std::fs::read(entry.unwrap().path()).unwrap();
        assert!(!bytes.windows(SECRET.len()).any(|w| w == SECRET.as_bytes()));
    }

    // Clearing the secret removes it from the key store; deleting the target too.
    service.set_signing_secret("hook", None).await.unwrap();
    assert!(!service.has_signing_secret("hook").await.unwrap());
    assert!(keys
        .get("Tingskriver/delivery/hook/signing")
        .unwrap()
        .is_none());
    service
        .set_signing_secret("hook", Some(SECRET.into()))
        .await
        .unwrap();
    service.delete_target("hook").await.unwrap();
    assert!(!service.has_signing_secret("hook").await.unwrap());
    assert!(keys
        .get("Tingskriver/delivery/hook/signing")
        .unwrap()
        .is_none());
}

#[tokio::test(flavor = "current_thread")]
async fn signing_secret_requires_webhook_target_and_no_pending_jobs() {
    let dir = tempfile::tempdir().unwrap();
    let service = Delivery::open(
        &dir.path().join("queue"),
        Arc::new(MemoryKeyStore::default()),
    )
    .await
    .unwrap();
    assert_eq!(
        service
            .set_signing_secret("missing", Some("abc".into()))
            .await,
        Err(Error::Missing)
    );
    let folder = Target {
        id: "folder".into(),
        name: "Mappe".into(),
        destination: Destination::Folder {
            path: dir.path().join("out"),
        },
    };
    service.save_target(folder, None).await.unwrap();
    assert_eq!(
        service
            .set_signing_secret("folder", Some("abc".into()))
            .await,
        Err(Error::Invalid)
    );
    service
        .save_target(
            Target {
                id: "hook".into(),
                name: "Webhook".into(),
                destination: Destination::Webhook {
                    format: WebhookFormat::Json,
                },
            },
            Some("http://127.0.0.1:9/synthetic".into()),
        )
        .await
        .unwrap();
    service
        .set_policy(Policy {
            local_only: false,
            private: false,
        })
        .await
        .unwrap();
    let job = service.test_target("hook").await.unwrap();
    let pool = sqlx::SqlitePool::connect(&format!(
        "sqlite:{}",
        dir.path().join("queue/delivery.sqlite").display()
    ))
    .await
    .unwrap();
    for status in ["pending", "running"] {
        sqlx::query("UPDATE delivery_jobs SET status = ? WHERE id = ?")
            .bind(status)
            .bind(&job)
            .execute(&pool)
            .await
            .unwrap();
        for secret in [Some("abc".into()), None] {
            assert_eq!(
                service.set_signing_secret("hook", secret).await,
                Err(Error::Invalid)
            );
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn changing_webhook_to_folder_removes_signing_secret() {
    use tingskriver_core::providers::secrets::KeyStore;
    let dir = tempfile::tempdir().unwrap();
    let keys = Arc::new(MemoryKeyStore::default());
    let service = Delivery::open(&dir.path().join("queue"), keys.clone())
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
            Some("http://127.0.0.1:9/synthetic".into()),
        )
        .await
        .unwrap();
    service
        .set_signing_secret("hook", Some("abc".into()))
        .await
        .unwrap();
    service
        .save_target(
            Target {
                id: "hook".into(),
                name: "Folder".into(),
                destination: Destination::Folder {
                    path: dir.path().join("out"),
                },
            },
            None,
        )
        .await
        .unwrap();
    assert!(!service.has_signing_secret("hook").await.unwrap());
    assert!(keys
        .get("Tingskriver/delivery/hook/signing")
        .unwrap()
        .is_none());
}

#[tokio::test(flavor = "current_thread")]
async fn signing_secret_with_surrounding_whitespace_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let service = Delivery::open(
        &dir.path().join("queue"),
        Arc::new(MemoryKeyStore::default()),
    )
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
            Some("http://127.0.0.1:9/synthetic".into()),
        )
        .await
        .unwrap();
    for bad in [" abc", "abc ", "abc\n", "  "] {
        assert_eq!(
            service.set_signing_secret("hook", Some(bad.into())).await,
            Err(Error::Invalid)
        );
    }
    assert!(!service.has_signing_secret("hook").await.unwrap());
}

#[tokio::test(flavor = "current_thread")]
async fn signing_credential_failures_keep_targets_closed_and_present() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use tingskriver_core::providers::secrets::{ApiKey, KeyStore};

    #[derive(Default)]
    struct FailingKeys {
        inner: MemoryKeyStore,
        fail: AtomicBool,
    }
    impl KeyStore for FailingKeys {
        fn get(&self, name: &str) -> Result<Option<ApiKey>, String> {
            self.inner.get(name)
        }
        fn set(&self, name: &str, key: &ApiKey) -> Result<(), String> {
            if self.fail.load(Ordering::SeqCst) {
                return Err("synthetic failure".into());
            }
            self.inner.set(name, key)
        }
        fn delete(&self, name: &str) -> Result<(), String> {
            if self.fail.load(Ordering::SeqCst) {
                return Err("synthetic failure".into());
            }
            self.inner.delete(name)
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let keys = Arc::new(FailingKeys::default());
    let service = Delivery::open(&dir.path().join("queue"), keys.clone())
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
            Some("http://127.0.0.1:9/synthetic".into()),
        )
        .await
        .unwrap();
    keys.fail.store(true, Ordering::SeqCst);
    assert_eq!(
        service.set_signing_secret("hook", Some("abc".into())).await,
        Err(Error::Credential)
    );
    assert!(service.has_signing_secret("hook").await.unwrap());
    assert!(keys
        .get("Tingskriver/delivery/hook/signing")
        .unwrap()
        .is_none());
    assert_eq!(service.delete_target("hook").await, Err(Error::Credential));
    assert_eq!(service.targets().await.unwrap().len(), 1);
    assert!(service.has_signing_secret("hook").await.unwrap());
    keys.fail.store(false, Ordering::SeqCst);
    service
        .set_signing_secret("hook", Some("abc".into()))
        .await
        .unwrap();
    keys.fail.store(true, Ordering::SeqCst);
    assert_eq!(service.delete_target("hook").await, Err(Error::Credential));
    assert_eq!(service.targets().await.unwrap().len(), 1);
    assert!(service.has_signing_secret("hook").await.unwrap());
    assert!(keys
        .get("Tingskriver/delivery/hook/signing")
        .unwrap()
        .is_some());
}
