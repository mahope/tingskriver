#![allow(clippy::disallowed_methods)]
use super::*;
use tokio::io::AsyncWriteExt;

#[tokio::test]
async fn twenty_monitor_respawns_keep_at_most_one_of_each_loop() {
    let manager = SidecarManager::with_binary(PathBuf::from("unused-fake-helper"));
    for _ in 0..20 {
        manager.start_monitors().await;
        tokio::task::yield_now().await;
        assert_eq!(manager.health_live.load(Ordering::SeqCst), 1);
        assert_eq!(manager.idle_live.load(Ordering::SeqCst), 1);
    }
    manager.monitors.lock().await.token.cancel();
    manager.stop_monitors().await;
    assert_eq!(manager.health_live.load(Ordering::SeqCst), 0);
    assert_eq!(manager.idle_live.load(Ordering::SeqCst), 0);
}

#[cfg(unix)]
#[tokio::test]
async fn twenty_process_respawns_keep_one_health_and_idle_loop() {
    use std::os::unix::fs::PermissionsExt;
    let directory = tempfile::tempdir().unwrap();
    let binary = directory.path().join("fake-helper");
    std::fs::write(
        &binary,
        "#!/bin/sh\nwhile IFS= read -r line; do\nprintf '%s\\n' '{\"type\":\"pong\"}'\ndone\n",
    )
    .unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
    let manager = SidecarManager::with_binary(binary);
    for iteration in 0..20 {
        manager
            .ensure_running(PathBuf::from(format!("fake-model-{iteration}")))
            .await
            .unwrap();
        tokio::task::yield_now().await;
        assert_eq!(manager.health_live.load(Ordering::SeqCst), 1);
        assert_eq!(manager.idle_live.load(Ordering::SeqCst), 1);
    }
    manager.shutdown().await.unwrap();
    manager.stop_monitors().await;
    assert_eq!(manager.health_live.load(Ordering::SeqCst), 0);
    assert_eq!(manager.idle_live.load(Ordering::SeqCst), 0);
    assert!(manager.child_process.lock().await.is_none());
}

#[tokio::test]
async fn real_health_ping_skips_busy_generation_without_waiting() {
    let manager = SidecarManager::with_binary(PathBuf::from("unused-fake-helper"));
    let (client, helper) = tokio::io::duplex(4096);
    let (reader, writer) = tokio::io::split(client);
    *manager.transport.lock().await = Some(Transport::new(reader, writer));
    let fake = tokio::spawn(async move {
        let mut helper = BufReader::new(helper);
        let mut line = String::new();
        helper.read_line(&mut line).await.unwrap();
        tokio::time::sleep(Duration::from_millis(30)).await;
        helper
            .get_mut()
            .write_all(b"{\"type\":\"response\",\"text\":\"generated\"}\n")
            .await
            .unwrap();
    });
    let generation = manager.send_request(r#"{"type":"generate"}"#.into(), Duration::from_secs(1));
    let ping = async {
        tokio::task::yield_now().await;
        tokio::time::timeout(Duration::from_millis(10), manager.send_ping())
            .await
            .unwrap()
            .unwrap();
    };
    let (response, ()) = tokio::join!(generation, ping);
    assert!(response.unwrap().contains("generated"));
    fake.await.unwrap();
}

#[tokio::test]
async fn generation_queued_behind_ping_receives_its_own_response() {
    let manager = SidecarManager::with_binary(PathBuf::from("unused-fake-helper"));
    let (client, helper) = tokio::io::duplex(4096);
    let (reader, writer) = tokio::io::split(client);
    *manager.transport.lock().await = Some(Transport::new(reader, writer));
    let (ping_started, received_ping) = tokio::sync::oneshot::channel();
    let fake = tokio::spawn(async move {
        let mut helper = BufReader::new(helper);
        let mut line = String::new();
        helper.read_line(&mut line).await.unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&line).unwrap()["type"],
            "ping"
        );
        ping_started.send(()).unwrap();
        tokio::time::sleep(Duration::from_millis(10)).await;
        helper
            .get_mut()
            .write_all(b"{\"type\":\"pong\"}\n")
            .await
            .unwrap();
        line.clear();
        helper.read_line(&mut line).await.unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&line).unwrap()["type"],
            "generate"
        );
        helper
            .get_mut()
            .write_all(b"{\"type\":\"response\",\"text\":\"generated\"}\n")
            .await
            .unwrap();
    });
    let generation = async {
        received_ping.await.unwrap();
        manager
            .send_request(r#"{"type":"generate"}"#.into(), Duration::from_secs(1))
            .await
    };
    let (ping, generation) = tokio::join!(manager.send_ping(), generation);
    ping.unwrap();
    assert!(generation.unwrap().contains("generated"));
    fake.await.unwrap();
}

#[tokio::test]
async fn ping_during_generation_never_steals_response_1000_iterations() {
    let (client, helper) = tokio::io::duplex(4096);
    let (reader, writer) = tokio::io::split(client);
    let transport = Transport::new(reader, writer);
    let manager = SidecarManager::with_binary(PathBuf::from("unused-fake-helper"));
    *manager.transport.lock().await = Some(transport);
    let (started, mut received) = tokio::sync::mpsc::unbounded_channel();
    let (resume, mut released) = tokio::sync::mpsc::unbounded_channel();
    let (reader, mut writer) = tokio::io::split(helper);
    let fake = tokio::spawn(async move {
        let mut lines = BufReader::new(reader).lines();
        let mut seed = 20_u64;
        while let Some(line) = lines.next_line().await.unwrap() {
            let request: serde_json::Value = serde_json::from_str(&line).unwrap();
            if request["type"] == "generate" {
                started.send(()).unwrap();
                released.recv().await.unwrap();
            }
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            tokio::time::sleep(Duration::from_micros(seed % 100)).await;
            let response = if request["type"] == "ping" {
                serde_json::json!({"type":"pong"})
            } else {
                serde_json::json!({"type":"response", "text":request["iteration"]})
            };
            writer
                .write_all(format!("{response}\n").as_bytes())
                .await
                .unwrap();
        }
    });
    for iteration in 0..1000 {
        let request = serde_json::json!({"type":"generate", "iteration":iteration}).to_string();
        let generation = manager.send_request(request, Duration::from_secs(1));
        let ping = async {
            received.recv().await.unwrap();
            tokio::time::timeout(Duration::from_millis(100), manager.send_ping())
                .await
                .unwrap()
                .unwrap();
            resume.send(()).unwrap();
        };
        let (response, ()) = tokio::join!(generation, ping);
        let response: serde_json::Value = serde_json::from_str(&response.unwrap()).unwrap();
        assert_eq!(response["text"], iteration);
        manager.send_ping().await.unwrap();
    }
    manager.shutdown().await.unwrap();
    fake.await.unwrap();
}

#[tokio::test]
async fn queued_request_timeout_preserves_active_generation() {
    let manager = SidecarManager::with_binary(PathBuf::from("unused-fake-helper"));
    let (client, helper) = tokio::io::duplex(4096);
    let (reader, writer) = tokio::io::split(client);
    *manager.transport.lock().await = Some(Transport::new(reader, writer));
    let (started, received) = tokio::sync::oneshot::channel();
    let (resume, released) = tokio::sync::oneshot::channel();
    let fake = tokio::spawn(async move {
        let mut helper = BufReader::new(helper);
        let mut line = String::new();
        helper.read_line(&mut line).await.unwrap();
        started.send(()).unwrap();
        released.await.unwrap();
        helper
            .get_mut()
            .write_all(b"{\"type\":\"response\",\"text\":\"active\"}\n")
            .await
            .unwrap();
        line.clear();
        helper.read_line(&mut line).await.unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&line).unwrap()["type"],
            "ping"
        );
        helper
            .get_mut()
            .write_all(b"{\"type\":\"pong\"}\n")
            .await
            .unwrap();
    });
    let active = manager.send_request(r#"{"type":"generate"}"#.into(), Duration::from_secs(2));
    let queued = async {
        received.await.unwrap();
        let result = manager
            .send_request(r#"{"type":"generate"}"#.into(), Duration::from_millis(10))
            .await;
        resume.send(()).unwrap();
        assert!(result.unwrap_err().to_string().contains("timed out"));
    };
    let (response, ()) = tokio::join!(active, queued);
    assert!(response.unwrap().contains("active"));
    assert!(manager.transport.lock().await.is_some());
    manager.send_ping().await.unwrap();
    fake.await.unwrap();
}

#[tokio::test]
async fn active_request_timeout_stops_helper_transport() {
    let manager = SidecarManager::with_binary(PathBuf::from("unused-fake-helper"));
    let (client, helper) = tokio::io::duplex(4096);
    let (reader, writer) = tokio::io::split(client);
    *manager.transport.lock().await = Some(Transport::new(reader, writer));
    let result = manager
        .send_request(r#"{"type":"generate"}"#.into(), Duration::from_millis(10))
        .await;
    assert!(result.unwrap_err().to_string().contains("timed out"));
    assert!(manager.transport.lock().await.is_none());
    let mut helper = BufReader::new(helper);
    let mut line = String::new();
    helper.read_line(&mut line).await.unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&line).unwrap()["type"],
        "generate"
    );
    line.clear();
    assert_eq!(helper.read_line(&mut line).await.unwrap(), 0);
}
