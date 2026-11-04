use tauri::{AppHandle, Listener};
use tauri_plugin_notification::NotificationExt;
use tingskriver_core::{
    host::HostExt,
    mcp::{Access, Reader},
};

#[derive(serde::Serialize)]
pub struct Status {
    access: Access,
    claude: String,
    codex: String,
}

#[tauri::command]
pub async fn mcp_status(app: AppHandle) -> Result<Status, String> {
    let db = crate::core_bridge::host(&app).expect_database();
    let exe = std::env::current_exe().map_err(|_| "Programstien kunne ikke findes")?;
    let exe = exe
        .parent()
        .ok_or("Programstien kunne ikke findes")?
        .join(if cfg!(windows) {
            "tingskriver-mcp.exe"
        } else {
            "tingskriver-mcp"
        });
    let quoted = serde_json::to_string(&exe.to_string_lossy())
        .map_err(|_| "Programstien kunne ikke læses")?;
    Ok(Status {
        access: Reader::new(db.pool().clone()).access().await?,
        claude: format!("claude mcp add tingskriver -- \"{}\"", exe.display()),
        codex: format!("[mcp_servers.tingskriver]\ncommand = {quoted}\n"),
    })
}

#[tauri::command]
pub async fn mcp_set_access(app: AppHandle, access: Access) -> Result<(), String> {
    let db = crate::core_bridge::host(&app).expect_database();
    tingskriver_core::mcp::set_access(db.pool(), access).await
}

#[tauri::command]
pub async fn mcp_meeting_shared(app: AppHandle, id: String) -> Result<bool, String> {
    let host = crate::core_bridge::host(&app);
    let db = host.expect_database();
    sqlx::query_scalar("SELECT ai_shared FROM meetings WHERE id=?")
        .bind(id)
        .fetch_one(db.pool())
        .await
        .map_err(|_| "Deling kunne ikke læses".into())
}

#[tauri::command]
pub async fn mcp_set_meeting_shared(
    app: AppHandle,
    id: String,
    shared: bool,
) -> Result<(), String> {
    let db = crate::core_bridge::host(&app).expect_database();
    tingskriver_core::mcp::set_shared(db.pool(), &id, shared).await
}

pub async fn start(app: AppHandle) {
    let notify_app = app.clone();
    app.listen("mcp-action", move |event| {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(event.payload()) {
            if value["status"] == "started" {
                let action = value["action"].as_str().unwrap_or("handling");
                let _ = notify_app
                    .notification()
                    .builder()
                    .title("AI-værktøj bruger Tingskriver")
                    .body(tingskriver_core::strings::mcp_action(action))
                    .show();
            }
        }
    });
    match crate::delivery::service(&app).await {
        Ok(delivery) => {
            if tingskriver_core::mcp_ipc::serve(crate::core_bridge::host(&app), delivery.clone())
                .await
                .is_err()
            {
                log::warn!("MCP-kanalen kunne ikke startes");
            }
        }
        Err(_) => log::warn!("MCP kunne ikke åbne afleveringslaget"),
    }
}
