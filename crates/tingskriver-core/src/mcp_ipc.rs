//! Bounded, local-only action channel. The MCP process never owns a write connection.
use crate::{
    delivery::Delivery,
    host::{HostExt, HostHandle},
    mcp::{Access, Reader},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

const LIMIT: usize = 64 * 1024;
const CLOSED: &str = "Åbn Tingskriver for at udføre handlinger. Læseværktøjerne virker uden appen.";
pub const PIPE: &str = r"\\.\pipe\tingskriver";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    StartRecording { title: Option<String> },
    StopRecording,
    RegenerateSummary { id: String, template: String },
    Handoff { id: String, target: String },
    AddNote { id: String, text: String },
}

pub fn socket_path(dir: &Path) -> PathBuf {
    dir.join("mcp-ipc").join("actions.sock")
}

async fn write_frame<S: AsyncWrite + Unpin, T: Serialize>(
    stream: &mut S,
    value: &T,
) -> Result<(), String> {
    let data = serde_json::to_vec(value).map_err(|_| "Ugyldig handling")?;
    if data.len() > LIMIT {
        return Err("Handlingen er for stor".into());
    }
    stream
        .write_u32(data.len() as u32)
        .await
        .map_err(|_| CLOSED)?;
    stream.write_all(&data).await.map_err(|_| CLOSED)?;
    stream.flush().await.map_err(|_| CLOSED.into())
}

async fn read_frame<S: AsyncRead + Unpin, T: for<'de> Deserialize<'de>>(
    stream: &mut S,
) -> Result<T, String> {
    let len = stream.read_u32().await.map_err(|_| CLOSED)? as usize;
    if len > LIMIT {
        return Err("Handlingen er for stor".into());
    }
    let mut data = vec![0; len];
    stream.read_exact(&mut data).await.map_err(|_| CLOSED)?;
    serde_json::from_slice(&data).map_err(|_| "Ugyldig handling".into())
}

pub async fn call(_dir: &Path, action: &Action) -> Result<Value, String> {
    let request = async {
        #[cfg(windows)]
        let mut stream = tokio::net::windows::named_pipe::ClientOptions::new()
            .open(PIPE)
            .map_err(|_| CLOSED)?;
        #[cfg(unix)]
        let mut stream = tokio::net::UnixStream::connect(socket_path(_dir))
            .await
            .map_err(|_| CLOSED)?;
        write_frame(&mut stream, action).await?;
        read_frame::<_, Result<Value, String>>(&mut stream).await?
    };
    tokio::time::timeout(Duration::from_secs(15), request)
        .await
        .map_err(|_| {
            "Tingskriver svarede ikke. Kontrollér appen før du gentager handlingen.".to_string()
        })?
}

async fn connection<S: AsyncRead + AsyncWrite + Unpin>(
    mut stream: S,
    host: HostHandle,
    delivery: &Delivery,
) {
    let request =
        tokio::time::timeout(Duration::from_secs(5), read_frame::<_, Action>(&mut stream)).await;
    let response = match request {
        Ok(Ok(action)) => execute(host, delivery, action).await,
        _ => Err("Ugyldig eller ufuldstændig handling".into()),
    };
    let _ = tokio::time::timeout(Duration::from_secs(5), write_frame(&mut stream, &response)).await;
}

pub async fn serve(host: HostHandle, delivery: Delivery) -> Result<(), String> {
    #[cfg(windows)]
    {
        use tokio::net::windows::named_pipe::ServerOptions;
        let mut listener = ServerOptions::new()
            .first_pipe_instance(true)
            .reject_remote_clients(true)
            .create(PIPE)
            .map_err(|_| "MCP-kanalen kunne ikke åbnes")?;
        loop {
            listener
                .connect()
                .await
                .map_err(|_| "MCP-kanalen lukkede")?;
            let connected = listener;
            listener = ServerOptions::new()
                .reject_remote_clients(true)
                .create(PIPE)
                .map_err(|_| "MCP-kanalen kunne ikke åbnes")?;
            connection(connected, host.clone(), &delivery).await;
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let path = socket_path(&host.app_data_dir()?);
        let parent = path.parent().ok_or("MCP-mappen mangler")?;
        std::fs::create_dir_all(parent).map_err(|_| "MCP-mappen kunne ikke oprettes")?;
        std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))
            .map_err(|_| "MCP-mappen kunne ikke beskyttes")?;
        if path.exists() {
            if tokio::net::UnixStream::connect(&path).await.is_ok() {
                return Err("MCP-kanalen er allerede åben".into());
            }
            std::fs::remove_file(&path).map_err(|_| "Den gamle MCP-kanal kunne ikke fjernes")?;
        }
        let listener =
            tokio::net::UnixListener::bind(path).map_err(|_| "MCP-kanalen kunne ikke åbnes")?;
        loop {
            let (stream, _) = listener.accept().await.map_err(|_| "MCP-kanalen lukkede")?;
            connection(stream, host.clone(), &delivery).await;
        }
    }
}

pub async fn execute(
    host: HostHandle,
    delivery: &Delivery,
    action: Action,
) -> Result<Value, String> {
    let database = host.database().ok_or("Mødedatabasen er ikke åben")?;
    let reader = Reader::new(database.pool().clone());
    let access = reader.access().await?;
    if matches!(access, Access::Off | Access::Metadata) {
        return Err("Handlinger kræver Alt eller et markeret møde.".into());
    }
    if matches!(action, Action::Handoff { .. }) {
        let settings = crate::mcp::settings(database.pool()).await?;
        if !(settings.notes && settings.summary && settings.transcript && settings.participants) {
            return Err(
                "Aflevering kræver tilladelse til alle mødele. Vælg dem i MCP-indstillingerne."
                    .into(),
            );
        }
    }
    let name = match &action {
        Action::StartRecording { .. } => "start_recording",
        Action::StopRecording => "stop_recording",
        Action::RegenerateSummary { id, .. }
        | Action::Handoff { id, .. }
        | Action::AddNote { id, .. } => {
            reader.authorize(id).await?;
            match &action {
                Action::RegenerateSummary { .. } => "regenerate_summary",
                Action::Handoff { .. } => "handoff",
                _ => "add_note",
            }
        }
    };
    if matches!(
        action,
        Action::StartRecording { .. } | Action::StopRecording
    ) && access != Access::All
    {
        return Err("Optagelseshandlinger kræver adgangsniveauet Alt.".into());
    }
    // Log operation names and outcomes, never notes, titles or transcript content.
    log::info!("MCP handling: {name}");
    host.emit("mcp-action", json!({"action":name,"status":"started"}))?;
    let result = perform(host.clone(), delivery, action).await;
    log::info!("MCP handling: {name}, success={}", result.is_ok());
    let _ = host.emit(
        "mcp-action",
        json!({"action":name,"status":if result.is_ok() {"completed"} else {"failed"}}),
    );
    result
}

async fn perform(host: HostHandle, delivery: &Delivery, action: Action) -> Result<Value, String> {
    let database = host.database().ok_or("Mødedatabasen er ikke åben")?;
    match action {
        Action::StartRecording { title } => {
            crate::audio::recording_commands::start_recording_with_meeting_name(host, title)
                .await?;
            Ok(json!({"started":true}))
        }
        Action::StopRecording => {
            crate::audio::recording_commands::stop_recording(
                host,
                crate::audio::recording_commands::RecordingArgs {
                    save_path: String::new(),
                },
            )
            .await?;
            Ok(json!({"stopped":true}))
        }
        Action::AddNote { id, text } => {
            if text.trim().is_empty() {
                return Err("Noten er tom".into());
            }
            let store = crate::notes::NotesStore::new(database.pool().clone());
            let mut doc = store
                .load(&id)
                .await
                .map_err(|_| "Noter kunne ikke læses")?;
            doc.blocks.extend(
                crate::notes::markdown::parse(&text)
                    .map_err(|_| "Noten er ugyldig")?
                    .blocks,
            );
            store
                .save(&id, &doc)
                .await
                .map_err(|_| "Noten kunne ikke gemmes")?;
            host.emit("mcp-notes-changed", json!({"id":id}))?;
            Ok(json!({"saved":true}))
        }
        Action::Handoff { id, target } => {
            let summary = crate::database::repositories::summary::SummaryProcessesRepository::get_summary_data(database.pool(),&id)
                .await.map_err(|_| "Referatet kunne ikke læses")?.and_then(|s| s.result).ok_or("Mødet har intet referat")?;
            let summary: Value =
                serde_json::from_str(&summary).map_err(|_| "Referatet kunne ikke læses")?;
            let markdown = summary
                .get("markdown")
                .and_then(Value::as_str)
                .ok_or("Mødet har intet referat")?;
            let bundle = crate::delivery::meeting::snapshot(database.pool(), &id, markdown.into())
                .await
                .map_err(|e| e.to_string())?;
            let job = delivery
                .enqueue(&target, bundle, true, false)
                .await
                .map_err(|e| e.to_string())?;
            Ok(json!({"job":job}))
        }
        Action::RegenerateSummary { id, template } => {
            crate::summary::templates::get_template(&template)
                .map_err(|_| "Skabelonen findes ikke")?;
            let config = crate::api::api::get_model_config(&database)
                .await?
                .ok_or("Vælg en referatmodel i Tingskriver først")?;
            let details = crate::database::repositories::meeting::MeetingsRepository::get_meeting(
                database.pool(),
                &id,
            )
            .await
            .map_err(|_| "Mødet kunne ikke læses")?
            .ok_or("Mødet findes ikke")?;
            let text = details
                .transcripts
                .into_iter()
                .map(|t| t.text)
                .collect::<Vec<_>>()
                .join("\n");
            let state = crate::state::AppState {
                db_manager: database,
            };
            crate::summary::commands::api_process_transcript(
                host,
                &state,
                text,
                config.provider,
                config.model,
                Some(id),
                None,
                None,
                None,
                Some(template),
                None,
                None,
            )
            .await?;
            Ok(json!({"queued":true}))
        }
    }
}
