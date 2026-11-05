//! User-triggered stdio handshake. It never requests meeting content.
use serde_json::{json, Value};
use std::{path::Path, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::Command,
};

/// A snapshot of clients' server processes, matched by installed executable.
/// Process paths are only compared locally, never logged or returned.
pub fn running_servers(exe: &Path) -> usize {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::new().with_exe(UpdateKind::Always),
    );
    let expected = exe.canonicalize().unwrap_or_else(|_| exe.to_path_buf());
    system
        .processes()
        .values()
        .filter(|process| {
            process.exe().is_some_and(|path| {
                path == expected
                    || (path.file_name() == expected.file_name()
                        && path.canonicalize().is_ok_and(|path| path == expected))
            })
        })
        .count()
}

pub async fn test_connection(exe: &Path, dir: &Path) -> Result<(), String> {
    let mut child = Command::new(exe)
        .arg("--data-dir")
        .arg(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| "MCP-serveren kunne ikke startes. Kontrollér installationen.")?;
    let result = tokio::time::timeout(Duration::from_secs(5), async {
        let mut input = child.stdin.take().ok_or(())?;
        let mut output = BufReader::new(child.stdout.take().ok_or(())?);
        for request in [
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"Tingskriver self-test","version":"1"}}}),
            json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
            json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
        ] {
            input.write_all(format!("{request}\n").as_bytes()).await.map_err(|_| ())?;
            input.flush().await.map_err(|_| ())?;
            let Some(id) = request.get("id") else { continue; };
            loop {
                let mut line = String::new();
                if output.read_line(&mut line).await.map_err(|_| ())? == 0 { return Err(()); }
                let response: Value = serde_json::from_str(&line).map_err(|_| ())?;
                if response.get("id") != Some(id) { continue; }
                if response.get("error").is_some() { return Err(()); }
                if id == 1 && !response["result"]["capabilities"]["tools"].is_object() { return Err(()); }
                if id == 2 {
                    let tools = response["result"]["tools"].as_array().ok_or(())?;
                    if tools.len() != crate::mcp_audit::TOOLS.len() || !crate::mcp_audit::TOOLS.iter().all(|name| tools.iter().any(|t| t["name"] == *name)) { return Err(()); }
                }
                break;
            }
        }
        Ok(())
    }).await;
    let _ = child.kill().await;
    let _ = child.wait().await;
    match result {
        Ok(Ok(())) => Ok(()),
        _ => Err("Forbindelsestesten mislykkedes eller tog for lang tid. Kontrollér installationen og prøv igen.".into()),
    }
}
