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
