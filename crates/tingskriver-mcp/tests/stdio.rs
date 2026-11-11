use serde_json::{json, Value};
use std::{process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::Command,
    time::timeout,
};

#[tokio::test]
async fn stdio_initializes_lists_tools_and_reports_closed_app() {
    let dir = tempfile::tempdir().unwrap();
    let db = tingskriver_core::database::manager::DatabaseManager::open_at(
        &dir.path().join("meeting_minutes.sqlite"),
        &dir.path().join("absent.db"),
        Duration::from_secs(1),
    )
    .await
    .unwrap();
    sqlx_fixture(&db).await;
    // The user-triggered probe works while sharing is disabled and never reads data.
    tingskriver_core::mcp_probe::test_connection(
        std::path::Path::new(env!("CARGO_BIN_EXE_tingskriver-mcp")),
        dir.path(),
    )
    .await
    .unwrap();
    assert!(tingskriver_core::mcp_audit::recent(dir.path())
        .await
        .unwrap()
        .is_empty());
    sqlx::query("UPDATE mcp_settings SET enabled=1,notes=1,summary=1,transcript=1,participants=1")
        .execute(db.pool())
        .await
        .unwrap();
    tingskriver_core::mcp::set_access(db.pool(), tingskriver_core::mcp::Access::All)
        .await
        .unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_tingskriver-mcp"))
        .arg("--data-dir")
        .arg(dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    async fn send(input: &mut tokio::process::ChildStdin, value: Value) {
        input
            .write_all(format!("{value}\n").as_bytes())
            .await
            .unwrap();
        input.flush().await.unwrap();
    }
    async fn receive(output: &mut BufReader<tokio::process::ChildStdout>) -> Value {
        let mut line = String::new();
        timeout(Duration::from_secs(10), output.read_line(&mut line))
            .await
            .unwrap()
            .unwrap();
        serde_json::from_str(&line).unwrap()
    }
    send(&mut input, json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"fixture","version":"1"}}})).await;
    assert!(receive(&mut output).await["result"]["capabilities"]["tools"].is_object());
    assert!(
        tingskriver_core::mcp_probe::running_servers(std::path::Path::new(env!(
            "CARGO_BIN_EXE_tingskriver-mcp"
        ))) >= 1
    );
    send(
        &mut input,
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
    )
    .await;
    send(
        &mut input,
        json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
    )
    .await;
    let tools = receive(&mut output).await;
    assert_eq!(tools["result"]["tools"].as_array().unwrap().len(), 10);
    send(&mut input, json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"list_meetings","arguments":{}}})).await;
    let meetings = receive(&mut output).await;
    assert_ne!(meetings["result"]["isError"], true);
    let meeting_text = meetings["result"]["content"][0]["text"].as_str().unwrap();
    assert!(meeting_text.contains("Fixture meeting"));
    assert!(!meeting_text.contains("SECRET"));
    send(&mut input, json!({"jsonrpc":"2.0","id":30,"method":"tools/call","params":{"name":"get_meeting","arguments":{"id":"fixture"}}})).await;
    let meeting = receive(&mut output).await;
    assert!(meeting["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("SECRET transcript"));
    tingskriver_core::mcp::set_access(db.pool(), tingskriver_core::mcp::Access::Metadata)
        .await
        .unwrap();
    send(&mut input, json!({"jsonrpc":"2.0","id":31,"method":"tools/call","params":{"name":"get_meeting","arguments":{"id":"fixture"}}})).await;
    let metadata = receive(&mut output).await;
    assert!(!metadata.to_string().contains("SECRET"));
    tingskriver_core::mcp::set_access(db.pool(), tingskriver_core::mcp::Access::All)
        .await
        .unwrap();
    send(&mut input, json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"start_recording","arguments":{}}})).await;
    let action = receive(&mut output).await;
    assert_eq!(action["result"]["isError"], true);
    assert!(action["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("Åbn Tingskriver"));
    send(&mut input, json!({"jsonrpc":"2.0","id":40,"method":"tools/call","params":{"name":"get_meeting","arguments":{"id":"PRIVATE rejected argument"}}})).await;
    assert_eq!(receive(&mut output).await["result"]["isError"], true);
    let audit = tingskriver_core::mcp_audit::recent(dir.path())
        .await
        .unwrap();
    assert_eq!(audit.len(), 5);
    assert_eq!(audit[0].meeting_id, None);
    assert!(!audit[0].success);
    assert!(audit
        .iter()
        .any(|entry| entry.meeting_id.as_deref() == Some("fixture")));
    assert!(audit
        .iter()
        .all(|entry| !format!("{entry:?}").contains("PRIVATE")
            && !format!("{entry:?}").contains("SECRET")));
    #[cfg(target_os = "linux")]
    {
        let pid = child.id().unwrap().to_string();
        let ports = Command::new("netstat")
            .args(["-anp"])
            .output()
            .await
            .expect("netstat is required for the no-listener check");
        assert!(ports.status.success());
        let output = String::from_utf8_lossy(&ports.stdout);
        assert!(
            !output
                .lines()
                .any(|line| (line.starts_with("tcp") || line.starts_with("udp"))
                    && line.contains(&format!("{pid}/"))),
            "MCP opened a network socket: {output}"
        );
    }
    drop(input);
    assert!(timeout(Duration::from_secs(10), child.wait())
        .await
        .unwrap()
        .unwrap()
        .success());
}

async fn sqlx_fixture(db: &tingskriver_core::database::manager::DatabaseManager) {
    sqlx::query("INSERT INTO meetings(id,title,created_at,updated_at,ai_shared) VALUES ('fixture','Fixture meeting','2026-11-01','2026-11-01',1)").execute(db.pool()).await.unwrap();
    sqlx::query("INSERT INTO transcripts(id,meeting_id,transcript,timestamp,speaker,audio_start_time) VALUES ('segment','fixture','SECRET transcript','00:01','Speaker 1',1.0)").execute(db.pool()).await.unwrap();
}
