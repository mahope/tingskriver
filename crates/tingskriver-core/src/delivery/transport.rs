use super::*;
use crate::providers::secrets::ApiKey;
use std::{path::Path, process::Stdio, time::Duration};
use tokio::io::AsyncWriteExt;

pub(super) fn validate_url(value: &str) -> Result<(), Error> {
    let url = reqwest::Url::parse(value).map_err(|_| Error::Invalid)?;
    let loopback = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    if (url.scheme() != "https" && !(url.scheme() == "http" && loopback))
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(Error::Invalid);
    }
    Ok(())
}

#[allow(clippy::disallowed_methods)] // Only called on blocking workers.
fn write_bundle(dir: &Path, bundle: &Bundle) -> Result<(), Error> {
    std::fs::create_dir_all(dir).map_err(|_| Error::Transport)?;
    for (name, text) in [
        ("transcript.md", bundle.transcript.clone()),
        ("summary.md", bundle.summary.clone()),
        ("notes.md", bundle.notes.clone()),
        (
            "sidecar.json",
            serde_json::to_string_pretty(&bundle.sidecar).map_err(|_| Error::Invalid)?,
        ),
    ] {
        std::fs::write(dir.join(name), text).map_err(|_| Error::Transport)?;
    }
    Ok(())
}

#[allow(clippy::disallowed_methods)] // Complete atomic folder export on one blocking worker.
fn send_folder(path: &Path, bundle: &Bundle, id: &str) -> Result<(), Error> {
    std::fs::create_dir_all(path).map_err(|_| Error::Transport)?;
    let final_dir = path.join(id);
    // A retry after a crash sees the same complete immutable bundle.
    if final_dir.is_dir() {
        return Ok(());
    }
    let temp = tempfile::Builder::new()
        .prefix(".tingskriver-")
        .tempdir_in(path)
        .map_err(|_| Error::Transport)?;
    write_bundle(temp.path(), bundle)?;
    std::fs::rename(temp.path(), final_dir).map_err(|_| Error::Transport)?;
    Ok(())
}

// TempDir removes files synchronously, including when an SSH future is cancelled.
struct StagedBundle(Option<tempfile::TempDir>);

impl StagedBundle {
    fn path(&self) -> &Path {
        self.0.as_ref().expect("staged bundle is live").path()
    }
}

impl Drop for StagedBundle {
    fn drop(&mut self) {
        if let Some(temp) = self.0.take() {
            tokio::task::spawn_blocking(move || drop(temp));
        }
    }
}

pub(super) async fn send(
    destination: &Destination,
    bundle: &Bundle,
    id: &str,
    url: Option<&ApiKey>,
) -> Result<(), Error> {
    if !safe_id(id) {
        return Err(Error::Invalid);
    }
    match destination {
        Destination::Folder { path } => {
            let path = path.clone();
            let bundle = bundle.clone();
            let id = id.to_owned();
            tokio::task::spawn_blocking(move || send_folder(&path, &bundle, &id))
                .await
                .map_err(|_| Error::Transport)?
        }
        Destination::Webhook { format } => {
            let url = url.ok_or(Error::Credential)?.expose();
            validate_url(url)?;
            let response = reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .map_err(|_| Error::Transport)?
                .post(url)
                .header("Idempotency-Key", id)
                .json(&webhook_payload(*format, bundle, id))
                .send()
                .await
                .map_err(|_| Error::Transport)?;
            if response.status().is_success() {
                Ok(())
            } else {
                Err(Error::Transport)
            }
        }
        Destination::Ssh {
            host,
            port,
            user,
            key_path,
            remote_dir,
        } => {
            let bundle = bundle.clone();
            let temp = tokio::task::spawn_blocking(move || {
                let temp = tempfile::tempdir().map_err(|_| Error::Transport)?;
                write_bundle(temp.path(), &bundle)?;
                Ok::<_, Error>(StagedBundle(Some(temp)))
            })
            .await
            .map_err(|_| Error::Transport)??;
            let mut batch = String::new();
            for name in ["transcript.md", "summary.md", "notes.md", "sidecar.json"] {
                let local = temp
                    .path()
                    .join(name)
                    .to_string_lossy()
                    .replace('\\', "/")
                    .replace('"', "\\\"");
                if local.contains(['\r', '\n']) {
                    return Err(Error::Invalid);
                }
                batch.push_str(&format!(
                    "put \"{local}\" \"{}/{id}-{name}\"\n",
                    remote_dir.trim_end_matches('/')
                ));
            }
            let mut child = tokio::process::Command::new("sftp")
                .args([
                    "-b",
                    "-",
                    "-oBatchMode=yes",
                    "-oStrictHostKeyChecking=yes",
                    "-oIdentitiesOnly=yes",
                    "-oConnectTimeout=10",
                    "-P",
                    &port.to_string(),
                    "-i",
                ])
                .arg(key_path)
                .arg(format!("{user}@{host}"))
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .kill_on_drop(true)
                .spawn()
                .map_err(|_| Error::Transport)?;
            let operation = async {
                let mut input = child.stdin.take().ok_or(Error::Transport)?;
                input
                    .write_all(batch.as_bytes())
                    .await
                    .map_err(|_| Error::Transport)?;
                drop(input);
                let status = child.wait().await.map_err(|_| Error::Transport)?;
                if status.success() {
                    Ok(())
                } else {
                    Err(Error::Transport)
                }
            };
            tokio::time::timeout(Duration::from_secs(60), operation)
                .await
                .map_err(|_| Error::Transport)?
        }
    }
}
