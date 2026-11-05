//! Verified model downloads shared by the Whisper and Parakeet engines.
//!
//! The pinned catalogue lives in `src/model_manifest.json`: for every model it has the
//! source revision, the licence, and the exact size and SHA-256 of every file.
//!
//! A file is fetched into `<name>.part`, resumed with `Range` after a cancel,
//! a stall or a restart, checked against its SHA-256, and only then renamed
//! over the final name. A file that fails the check is deleted, so a wrong or
//! partial download can never become an active model, and an existing model is
//! never touched before its replacement has been verified. If a source fails,
//! the next source in the manifest (the reserve mirror) is tried.
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

use anyhow::anyhow;
use async_trait::async_trait;
use once_cell::sync::Lazy;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::fs;
use tokio::io::{AsyncWriteExt, BufWriter};
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

use crate::strings;

/// A download is aborted when no bytes arrive for this long.
pub const STALL_TIMEOUT: Duration = Duration::from_secs(60);

const MANIFEST_JSON: &str = include_str!("model_manifest.json");

#[derive(Debug, Clone, Deserialize)]
pub struct Manifest {
    pub schema: u32,
    pub models: Vec<ModelEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModelEntry {
    pub id: String,
    pub engine: String,
    pub license: String,
    /// Commit of the upstream repository the hashes were computed from.
    pub revision: String,
    /// Tried in order: the first is the pinned main source, the rest are reserve mirrors.
    pub sources: Vec<Source>,
    pub files: Vec<FileEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Source {
    pub name: String,
    pub base_url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FileEntry {
    /// File name on disk.
    pub name: String,
    /// File name at the sources when it differs from the name on disk.
    #[serde(default)]
    pub remote_name: Option<String>,
    pub size: u64,
    pub sha256: String,
}

impl ModelEntry {
    pub fn total_bytes(&self) -> u64 {
        self.files.iter().map(|file| file.size).sum()
    }
}

static MANIFEST: Lazy<Manifest> = Lazy::new(|| {
    serde_json::from_str(MANIFEST_JSON)
        .expect("src/model_manifest.json is valid (checked by a test)")
});

pub fn manifest() -> &'static Manifest {
    &MANIFEST
}

pub fn entry(engine: &str, id: &str) -> Option<&'static ModelEntry> {
    MANIFEST
        .models
        .iter()
        .find(|model| model.engine == engine && model.id == id)
}

#[derive(Debug, thiserror::Error)]
pub enum TransferError {
    #[error("Download cancelled by user")]
    Cancelled,
    /// A downloaded file did not match its SHA-256; it has been deleted.
    #[error("{}", strings::model_hash_mismatch(.file))]
    HashMismatch { file: String },
    /// An installed file does not match its SHA-256; it is left in place.
    #[error("{}", strings::model_installed_mismatch(.file))]
    InstalledMismatch { file: String },
    #[error("{0}")]
    Failed(String),
}

impl TransferError {
    fn failed(message: impl Into<String>) -> Self {
        Self::Failed(message.into())
    }
}

/// Receives the confirmed bytes on disk for the file being fetched.
#[async_trait]
pub trait ProgressSink: Send {
    async fn bytes(&mut self, file_bytes: u64);
}

pub struct NoProgress;

#[async_trait]
impl ProgressSink for NoProgress {
    async fn bytes(&mut self, _file_bytes: u64) {}
}

#[derive(Debug, Clone)]
pub struct TransferOptions {
    pub stall_timeout: Duration,
}

impl Default for TransferOptions {
    fn default() -> Self {
        Self {
            stall_timeout: STALL_TIMEOUT,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Fetched {
    /// Name of the source that delivered the file (or the existing file was already verified).
    pub source: String,
    /// True when a verified file was already in place and nothing was downloaded.
    pub already_installed: bool,
}

/// HTTP client for model downloads: a bounded connect, no total deadline (the
/// files are gigabytes); a stalled transfer is caught by [`STALL_TIMEOUT`].
pub fn client() -> anyhow::Result<reqwest::Client> {
    reqwest::Client::builder()
        .tcp_nodelay(true)
        .pool_max_idle_per_host(1)
        .connect_timeout(crate::http::TIMEOUT)
        .user_agent(concat!("Tingskriver/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|error| anyhow!("Failed to create HTTP client: {}", error))
}

pub fn part_path(dir: &Path, name: &str) -> PathBuf {
    dir.join(format!("{name}.part"))
}

fn verified_marker_path(dir: &Path, name: &str) -> PathBuf {
    dir.join(format!("{name}.verified"))
}

/// Fetch one file into `dir`, trying each source in turn. See the module docs.
pub async fn fetch_file(
    client: &reqwest::Client,
    dir: &Path,
    file: &FileEntry,
    sources: &[Source],
    cancel: &CancellationToken,
    options: &TransferOptions,
    sink: &mut dyn ProgressSink,
) -> Result<Fetched, TransferError> {
    fs::create_dir_all(dir).await.map_err(|error| {
        TransferError::failed(format!("Failed to create model directory: {error}"))
    })?;
    let final_path = dir.join(&file.name);

    // A verified file already in place is kept as it is.
    if let Ok(metadata) = fs::metadata(&final_path).await {
        if metadata.len() == file.size && verify_installed(dir, file, cancel).await.is_ok() {
            sink.bytes(file.size).await;
            return Ok(Fetched {
                source: "installed".to_string(),
                already_installed: true,
            });
        }
        if cancel.is_cancelled() {
            return Err(TransferError::Cancelled);
        }
    }

    let mut hash_mismatch = false;
    let mut last_error = String::from("no sources configured");
    for source in sources {
        if cancel.is_cancelled() {
            return Err(TransferError::Cancelled);
        }
        match fetch_from_source(client, dir, file, source, cancel, options, sink).await {
            Ok(()) => {
                log::info!(
                    "Model file {} delivered by source '{}'",
                    file.name,
                    source.name
                );
                return Ok(Fetched {
                    source: source.name.clone(),
                    already_installed: false,
                });
            }
            Err(TransferError::Cancelled) => return Err(TransferError::Cancelled),
            Err(error) => {
                if matches!(error, TransferError::HashMismatch { .. }) {
                    hash_mismatch = true;
                }
                log::warn!(
                    "Source '{}' failed for model file {}: {}",
                    source.name,
                    file.name,
                    error
                );
                last_error = error.to_string();
            }
        }
    }

    if hash_mismatch {
        return Err(TransferError::HashMismatch {
            file: file.name.clone(),
        });
    }
    Err(TransferError::failed(format!(
        "{} ({})",
        strings::model_download_failed(&file.name),
        last_error
    )))
}

async fn fetch_from_source(
    client: &reqwest::Client,
    dir: &Path,
    file: &FileEntry,
    source: &Source,
    cancel: &CancellationToken,
    options: &TransferOptions,
    sink: &mut dyn ProgressSink,
) -> Result<(), TransferError> {
    let part = part_path(dir, &file.name);
    let final_path = dir.join(&file.name);
    let url = format!(
        "{}/{}",
        source.base_url.trim_end_matches('/'),
        file.remote_name.as_deref().unwrap_or(&file.name)
    );

    let mut have = match fs::metadata(&part).await {
        Ok(metadata) => metadata.len(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
        Err(error) => {
            return Err(TransferError::failed(format!(
                "Failed to read {} metadata: {error}",
                file.name
            )));
        }
    };
    if have > file.size {
        remove_quietly(&part).await;
        have = 0;
    }

    if have < file.size {
        transfer(client, &url, &part, file, have, cancel, options, sink).await?;
    } else {
        sink.bytes(file.size).await;
    }

    let stored = fs::metadata(&part)
        .await
        .map_err(|error| {
            TransferError::failed(format!(
                "Failed to read {} after download: {error}",
                file.name
            ))
        })?
        .len();
    if stored != file.size {
        return Err(TransferError::failed(format!(
            "{} stored {} bytes, expected exactly {} bytes",
            file.name, stored, file.size
        )));
    }

    let digest = sha256_of(&part, cancel).await?;
    if !digest.eq_ignore_ascii_case(&file.sha256) {
        remove_quietly(&part).await;
        return Err(TransferError::HashMismatch {
            file: file.name.clone(),
        });
    }
    if cancel.is_cancelled() {
        return Err(TransferError::Cancelled);
    }

    // The old marker belongs to the file that is about to be replaced.
    remove_quietly(&verified_marker_path(dir, &file.name)).await;
    fs::rename(&part, &final_path).await.map_err(|error| {
        TransferError::failed(format!("Failed to activate {}: {error}", file.name))
    })?;
    write_verified_marker(dir, file, &digest).await;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn transfer(
    client: &reqwest::Client,
    url: &str,
    part: &Path,
    file: &FileEntry,
    have: u64,
    cancel: &CancellationToken,
    options: &TransferOptions,
    sink: &mut dyn ProgressSink,
) -> Result<(), TransferError> {
    use futures_util::StreamExt;

    let range_start = (have > 0).then_some(have);
    let mut response = send(client, url, range_start, cancel, options).await?;
    let mut append = false;
    match range_start {
        Some(start) => match response.status() {
            reqwest::StatusCode::PARTIAL_CONTENT => {
                validate_partial(&response, start, file.size)?;
                append = true;
            }
            reqwest::StatusCode::OK => validate_full(&response, file.size)?,
            reqwest::StatusCode::RANGE_NOT_SATISFIABLE => {
                validate_unsatisfied(&response, file.size)?;
                remove_quietly(part).await;
                response = send(client, url, None, cancel, options).await?;
                validate_full(&response, file.size)?;
            }
            status => {
                return Err(TransferError::failed(format!(
                    "Download failed for {} with status {}",
                    file.name, status
                )));
            }
        },
        None => match response.status() {
            reqwest::StatusCode::OK => validate_full(&response, file.size)?,
            reqwest::StatusCode::PARTIAL_CONTENT => validate_partial(&response, 0, file.size)?,
            status => {
                return Err(TransferError::failed(format!(
                    "Download failed for {} with status {}",
                    file.name, status
                )));
            }
        },
    }

    if cancel.is_cancelled() {
        return Err(TransferError::Cancelled);
    }
    let handle = if append {
        fs::OpenOptions::new().append(true).open(part).await
    } else {
        fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(part)
            .await
    }
    .map_err(|error| {
        TransferError::failed(format!("Failed to open {}: {error}", part.display()))
    })?;
    let mut writer = BufWriter::with_capacity(8 * 1024 * 1024, handle);
    let mut written = if append { have } else { 0 };
    sink.bytes(written).await;

    let mut stream = response.bytes_stream();
    loop {
        let next = tokio::select! {
            biased;
            _ = cancel.cancelled() => {
                writer.flush().await.map_err(|error| {
                    TransferError::failed(format!("Failed to preserve {} during cancellation: {error}", file.name))
                })?;
                return Err(TransferError::Cancelled);
            }
            next = timeout(options.stall_timeout, stream.next()) => next,
        };
        let chunk = match next {
            Err(_) => {
                let _ = writer.flush().await;
                return Err(TransferError::failed(format!(
                    "Download timeout for {}: no data received for {} seconds",
                    file.name,
                    options.stall_timeout.as_secs()
                )));
            }
            Ok(None) => break,
            Ok(Some(Err(error))) => {
                let _ = writer.flush().await;
                return Err(TransferError::failed(format!(
                    "Download stream failed for {}: {error}",
                    file.name
                )));
            }
            Ok(Some(Ok(chunk))) => chunk,
        };

        let next_written = written
            .checked_add(chunk.len() as u64)
            .ok_or_else(|| TransferError::failed(format!("{} size overflow", file.name)))?;
        if next_written > file.size {
            let _ = writer.flush().await;
            return Err(TransferError::failed(format!(
                "{} response exceeds its exact {} byte size",
                file.name, file.size
            )));
        }
        writer.write_all(&chunk).await.map_err(|error| {
            TransferError::failed(format!("Failed to write {}: {error}", file.name))
        })?;
        written = next_written;
        sink.bytes(written).await;
    }
    writer.flush().await.map_err(|error| {
        TransferError::failed(format!("Failed to flush {}: {error}", file.name))
    })?;
    if written != file.size {
        return Err(TransferError::failed(format!(
            "{} ended after {} bytes, expected exactly {} bytes",
            file.name, written, file.size
        )));
    }
    Ok(())
}

async fn send(
    client: &reqwest::Client,
    url: &str,
    range_start: Option<u64>,
    cancel: &CancellationToken,
    options: &TransferOptions,
) -> Result<reqwest::Response, TransferError> {
    let mut request = client.get(url);
    if let Some(start) = range_start {
        request = request.header(reqwest::header::RANGE, format!("bytes={start}-"));
    }
    tokio::select! {
        biased;
        _ = cancel.cancelled() => Err(TransferError::Cancelled),
        response = timeout(options.stall_timeout, request.send()) => match response {
            Err(_) => Err(TransferError::failed(format!(
                "No response from {} within {} seconds", url, options.stall_timeout.as_secs()
            ))),
            Ok(Err(error)) => Err(TransferError::failed(format!("Failed to start download for {url}: {error}"))),
            Ok(Ok(response)) => Ok(response),
        },
    }
}

async fn remove_quietly(path: &Path) {
    match fs::remove_file(path).await {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => log::warn!("Failed to remove {}: {}", path.display(), error),
    }
}

/// SHA-256 of a file as lowercase hex, computed on a blocking worker that
/// stops early when the token is cancelled.
#[allow(clippy::disallowed_methods)]
pub async fn sha256_of(path: &Path, cancel: &CancellationToken) -> Result<String, TransferError> {
    use std::io::Read;

    let path = path.to_path_buf();
    let cancel = cancel.clone();
    tokio::task::spawn_blocking(move || {
        let mut file = std::fs::File::open(&path).map_err(|error| {
            TransferError::failed(format!("Failed to open {}: {error}", path.display()))
        })?;
        let mut hasher = Sha256::new();
        let mut buffer = vec![0u8; 1024 * 1024];
        loop {
            if cancel.is_cancelled() {
                return Err(TransferError::Cancelled);
            }
            let read = file.read(&mut buffer).map_err(|error| {
                TransferError::failed(format!("Failed to read {}: {error}", path.display()))
            })?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
        }
        Ok(hex(&hasher.finalize()))
    })
    .await
    .map_err(|error| TransferError::failed(format!("Hash task failed: {error}")))?
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// `<size> <mtime-ns> <sha256>` of a file that has passed the hash check.
async fn marker_for(path: &Path, sha256: &str) -> Option<String> {
    let metadata = fs::metadata(path).await.ok()?;
    let modified = metadata.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
    Some(format!(
        "{} {} {}",
        metadata.len(),
        modified.as_nanos(),
        sha256.to_ascii_lowercase()
    ))
}

async fn write_verified_marker(dir: &Path, file: &FileEntry, sha256: &str) {
    if let Some(marker) = marker_for(&dir.join(&file.name), sha256).await {
        if let Err(error) = fs::write(verified_marker_path(dir, &file.name), marker).await {
            log::debug!(
                "Could not write verification marker for {}: {}",
                file.name,
                error
            );
        }
    }
}

/// Check an installed file against the manifest. The SHA-256 is only
/// recomputed when the file's size or modification time differs from the last
/// successful check, so loading a verified model stays cheap. A mismatch
/// leaves the file in place; the caller decides what to show.
pub async fn verify_installed(
    dir: &Path,
    file: &FileEntry,
    cancel: &CancellationToken,
) -> Result<(), TransferError> {
    let path = dir.join(&file.name);
    let metadata = fs::metadata(&path).await.map_err(|error| {
        TransferError::failed(format!("Failed to read {} metadata: {error}", file.name))
    })?;
    if metadata.len() != file.size {
        return Err(TransferError::InstalledMismatch {
            file: file.name.clone(),
        });
    }
    if let Some(expected) = marker_for(&path, &file.sha256).await {
        if fs::read_to_string(verified_marker_path(dir, &file.name))
            .await
            .ok()
            .as_deref()
            == Some(expected.as_str())
        {
            return Ok(());
        }
    }
    let digest = sha256_of(&path, cancel).await?;
    if !digest.eq_ignore_ascii_case(&file.sha256) {
        remove_quietly(&verified_marker_path(dir, &file.name)).await;
        return Err(TransferError::InstalledMismatch {
            file: file.name.clone(),
        });
    }
    write_verified_marker(dir, file, &digest).await;
    Ok(())
}

/// Remove the partial download and verification marker next to a model file.
pub async fn remove_sidecars(dir: &Path, name: &str) {
    remove_quietly(&part_path(dir, name)).await;
    remove_quietly(&verified_marker_path(dir, name)).await;
}

enum ContentRange {
    Range { start: u64, end: u64, total: u64 },
    Unsatisfied { total: u64 },
}

fn parse_content_range(
    value: &reqwest::header::HeaderValue,
) -> Result<ContentRange, TransferError> {
    let bad = |what: &str| TransferError::failed(format!("Invalid Content-Range: {what}"));
    let value = value.to_str().map_err(|_| bad("not valid text"))?;
    let value = value
        .strip_prefix("bytes ")
        .ok_or_else(|| bad("must use bytes"))?;
    if let Some(total) = value.strip_prefix("*/") {
        return total
            .parse()
            .map(|total| ContentRange::Unsatisfied { total })
            .map_err(|_| bad("unsatisfied total"));
    }
    let (range, total) = value.split_once('/').ok_or_else(|| bad("malformed"))?;
    let (start, end) = range
        .split_once('-')
        .ok_or_else(|| bad("malformed range"))?;
    let start: u64 = start.parse().map_err(|_| bad("start"))?;
    let end: u64 = end.parse().map_err(|_| bad("end"))?;
    let total: u64 = total.parse().map_err(|_| bad("total"))?;
    if start > end {
        return Err(bad("start exceeds end"));
    }
    Ok(ContentRange::Range { start, end, total })
}

fn declared_content_length(response: &reqwest::Response) -> Result<Option<u64>, TransferError> {
    response
        .headers()
        .get(reqwest::header::CONTENT_LENGTH)
        .map(|value| {
            value
                .to_str()
                .ok()
                .and_then(|text| text.parse().ok())
                .ok_or_else(|| TransferError::failed("Invalid Content-Length header"))
        })
        .transpose()
}

fn validate_full(response: &reqwest::Response, size: u64) -> Result<(), TransferError> {
    if response.status() != reqwest::StatusCode::OK {
        return Err(TransferError::failed(format!(
            "Expected full 200 response, received {}",
            response.status()
        )));
    }
    if let Some(length) = declared_content_length(response)? {
        if length != size {
            return Err(TransferError::failed(format!(
                "Full response declared {length} bytes, expected {size}"
            )));
        }
    }
    Ok(())
}

fn validate_partial(
    response: &reqwest::Response,
    expected_start: u64,
    size: u64,
) -> Result<(), TransferError> {
    if response.status() != reqwest::StatusCode::PARTIAL_CONTENT {
        return Err(TransferError::failed(format!(
            "Expected partial 206 response, received {}",
            response.status()
        )));
    }
    let header = response
        .headers()
        .get(reqwest::header::CONTENT_RANGE)
        .ok_or_else(|| TransferError::failed("Partial response is missing Content-Range"))?;
    let ContentRange::Range { start, end, total } = parse_content_range(header)? else {
        return Err(TransferError::failed(
            "Partial response has an unsatisfied Content-Range",
        ));
    };
    if start != expected_start || end != size - 1 || total != size {
        return Err(TransferError::failed(format!(
            "Partial response range {start}-{end} / {total} does not match {expected_start}-{} / {size}",
            size - 1
        )));
    }
    if let Some(length) = declared_content_length(response)? {
        if length != end - start + 1 {
            return Err(TransferError::failed(format!(
                "Partial response declared {length} bytes, expected {}",
                end - start + 1
            )));
        }
    }
    Ok(())
}

fn validate_unsatisfied(response: &reqwest::Response, size: u64) -> Result<(), TransferError> {
    if response.status() != reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
        return Err(TransferError::failed(format!(
            "Expected range-not-satisfiable 416 response, received {}",
            response.status()
        )));
    }
    let header = response
        .headers()
        .get(reqwest::header::CONTENT_RANGE)
        .ok_or_else(|| TransferError::failed("416 response is missing Content-Range"))?;
    match parse_content_range(header)? {
        ContentRange::Unsatisfied { total } if total == size => Ok(()),
        ContentRange::Unsatisfied { total } => Err(TransferError::failed(format!(
            "416 response reports {total} total bytes, expected {size}"
        ))),
        ContentRange::Range { .. } => Err(TransferError::failed(
            "416 response has a satisfied Content-Range",
        )),
    }
}

#[cfg(test)]
pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

#[cfg(test)]
#[allow(clippy::disallowed_methods)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tempfile::tempdir;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    const GOOD: &[u8] = b"0123456789abcdefghij";

    fn spec(name: &str, body: &[u8]) -> FileEntry {
        FileEntry {
            name: name.to_string(),
            remote_name: None,
            size: body.len() as u64,
            sha256: sha256_hex(body),
        }
    }

    fn source(name: &str, base_url: &str) -> Source {
        Source {
            name: name.to_string(),
            base_url: base_url.to_string(),
        }
    }

    #[derive(Clone, Copy)]
    enum Mode {
        /// Serves `body` (Range supported).
        Serve,
        /// Serves a body of the right size with one byte changed.
        Tampered,
        /// Always answers 404.
        NotFound,
        /// Sends the headers and half the body, then goes silent.
        StallAfterHalf,
    }

    struct TestServer {
        base_url: String,
        requests: Arc<Mutex<Vec<(String, Option<String>)>>>,
    }

    /// One connection per request, any number of requests, until the test ends.
    async fn serve(mode: Mode, body: &'static [u8]) -> TestServer {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let base_url = format!("http://{}", listener.local_addr().expect("addr"));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&requests);
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                let log = Arc::clone(&log);
                tokio::spawn(async move {
                    let mut request = Vec::new();
                    let mut buffer = [0u8; 1024];
                    while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                        match socket.read(&mut buffer).await {
                            Ok(0) | Err(_) => return,
                            Ok(read) => request.extend_from_slice(&buffer[..read]),
                        }
                    }
                    let text = String::from_utf8_lossy(&request).to_string();
                    let path = text.split_whitespace().nth(1).unwrap_or("").to_string();
                    let range = text.lines().find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("range")
                            .then(|| value.trim().to_string())
                    });
                    log.lock().unwrap().push((path, range.clone()));

                    let mut data = body.to_vec();
                    if matches!(mode, Mode::Tampered) {
                        data[3] ^= 0x01;
                    }
                    let start = range
                        .as_deref()
                        .and_then(|r| r.strip_prefix("bytes="))
                        .and_then(|r| r.strip_suffix('-'))
                        .and_then(|r| r.parse::<usize>().ok());
                    if matches!(mode, Mode::NotFound) {
                        let _ = socket
                            .write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                            .await;
                        return;
                    }
                    let (head, payload) = match start {
                        Some(start) if start >= data.len() => (
                            format!(
                                "HTTP/1.1 416 Range Not Satisfiable\r\nContent-Range: bytes */{}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                                data.len()
                            ),
                            Vec::new(),
                        ),
                        Some(start) => (
                            format!(
                                "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {}-{}/{}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                                start,
                                data.len() - 1,
                                data.len(),
                                data.len() - start
                            ),
                            data[start..].to_vec(),
                        ),
                        None => (
                            format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", data.len()),
                            data,
                        ),
                    };
                    let _ = socket.write_all(head.as_bytes()).await;
                    if matches!(mode, Mode::StallAfterHalf) {
                        let _ = socket.write_all(&payload[..payload.len() / 2]).await;
                        let _ = socket.flush().await;
                        tokio::time::sleep(Duration::from_secs(30)).await;
                        return;
                    }
                    let _ = socket.write_all(&payload).await;
                });
            }
        });
        TestServer { base_url, requests }
    }

    fn fast() -> TransferOptions {
        TransferOptions {
            stall_timeout: Duration::from_millis(300),
        }
    }

    async fn fetch(
        dir: &Path,
        file: &FileEntry,
        sources: &[Source],
        cancel: &CancellationToken,
    ) -> Result<Fetched, TransferError> {
        fetch_file(
            &client().unwrap(),
            dir,
            file,
            sources,
            cancel,
            &fast(),
            &mut NoProgress,
        )
        .await
    }

    #[test]
    fn manifest_pins_every_downloadable_model() {
        let manifest = manifest();
        assert_eq!(manifest.schema, 1);
        let hex64 = |text: &str| {
            text.len() == 64 && text.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f'))
        };
        for model in &manifest.models {
            assert!(!model.license.is_empty(), "{} has no license", model.id);
            assert!(
                model.revision.len() == 40 && model.revision.chars().all(|c| c.is_ascii_hexdigit()),
                "{} has no pinned revision",
                model.id
            );
            assert!(
                !model.sources.is_empty() && !model.files.is_empty(),
                "{}",
                model.id
            );
            assert!(
                model.sources[0].base_url.contains(&model.revision),
                "{} main source is not pinned",
                model.id
            );
            for source in &model.sources {
                assert!(
                    source.base_url.starts_with("https://"),
                    "{}",
                    source.base_url
                );
            }
            for file in &model.files {
                assert!(
                    file.size > 0 && hex64(&file.sha256),
                    "{}/{}",
                    model.id,
                    file.name
                );
                assert!(!file.name.contains(['/', '\\']), "{}", file.name);
            }
        }
        // Everything the engines can download today.
        for (name, ..) in crate::config::WHISPER_MODEL_CATALOG {
            assert!(
                entry("whisper", name).is_some(),
                "whisper:{name} is missing from the manifest"
            );
        }
        for name in ["parakeet-tdt-0.6b-v3-int8", "parakeet-tdt-0.6b-v2-int8"] {
            assert_eq!(
                entry("parakeet", name).map(|m| m.files.len()),
                Some(4),
                "{name}"
            );
        }
    }

    #[tokio::test]
    async fn good_download_is_verified_then_activated_without_part_file() {
        let server = serve(Mode::Serve, GOOD).await;
        let dir = tempdir().unwrap();
        let file = spec("model.bin", GOOD);
        let fetched = fetch(
            dir.path(),
            &file,
            &[source("main", &server.base_url)],
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(fetched.source, "main");
        assert_eq!(std::fs::read(dir.path().join("model.bin")).unwrap(), GOOD);
        assert!(!part_path(dir.path(), "model.bin").exists());
        verify_installed(dir.path(), &file, &CancellationToken::new())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn changed_byte_is_rejected_deleted_and_keeps_the_old_model() {
        let server = serve(Mode::Tampered, GOOD).await;
        let dir = tempdir().unwrap();
        // An older, differently-hashed file is in place (e.g. a model from before the manifest).
        let old = b"an older model that still loads";
        std::fs::write(dir.path().join("model.bin"), old).unwrap();
        let file = spec("model.bin", GOOD);

        let error = fetch(
            dir.path(),
            &file,
            &[source("main", &server.base_url)],
            &CancellationToken::new(),
        )
        .await
        .unwrap_err();

        assert!(
            matches!(error, TransferError::HashMismatch { .. }),
            "{error}"
        );
        assert!(
            error.to_string().contains("model.bin") && error.to_string().contains("slettet"),
            "{error}"
        );
        assert!(
            !part_path(dir.path(), "model.bin").exists(),
            "the bad download must be deleted"
        );
        assert_eq!(
            std::fs::read(dir.path().join("model.bin")).unwrap(),
            old,
            "old model must survive"
        );
    }

    #[tokio::test]
    async fn verified_installed_file_is_not_downloaded_again() {
        let server = serve(Mode::Tampered, GOOD).await;
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("model.bin"), GOOD).unwrap();
        let file = spec("model.bin", GOOD);
        let fetched = fetch(
            dir.path(),
            &file,
            &[source("main", &server.base_url)],
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        assert!(fetched.already_installed);
        assert!(
            server.requests.lock().unwrap().is_empty(),
            "no request for a verified file"
        );
    }

    #[tokio::test]
    async fn failing_primary_falls_back_to_the_mirror_and_reports_which_one_delivered() {
        let primary = serve(Mode::NotFound, GOOD).await;
        let mirror = serve(Mode::Serve, GOOD).await;
        let dir = tempdir().unwrap();
        let file = spec("model.bin", GOOD);
        let fetched = fetch(
            dir.path(),
            &file,
            &[
                source("main", &primary.base_url),
                source("mirror", &mirror.base_url),
            ],
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(fetched.source, "mirror");
        assert_eq!(primary.requests.lock().unwrap().len(), 1);
        assert_eq!(std::fs::read(dir.path().join("model.bin")).unwrap(), GOOD);
    }

    #[tokio::test]
    async fn tampered_primary_falls_back_to_a_good_mirror() {
        let primary = serve(Mode::Tampered, GOOD).await;
        let mirror = serve(Mode::Serve, GOOD).await;
        let dir = tempdir().unwrap();
        let fetched = fetch(
            dir.path(),
            &spec("model.bin", GOOD),
            &[
                source("main", &primary.base_url),
                source("mirror", &mirror.base_url),
            ],
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(fetched.source, "mirror");
        assert_eq!(std::fs::read(dir.path().join("model.bin")).unwrap(), GOOD);
    }

    #[tokio::test]
    async fn all_sources_failing_gives_a_danish_error_and_no_files() {
        let primary = serve(Mode::NotFound, GOOD).await;
        let dir = tempdir().unwrap();
        let error = fetch(
            dir.path(),
            &spec("model.bin", GOOD),
            &[source("main", &primary.base_url)],
            &CancellationToken::new(),
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("kunne ikke hentes"), "{error}");
        assert!(!dir.path().join("model.bin").exists());
    }

    struct CancelAt {
        token: CancellationToken,
        at: u64,
    }

    #[async_trait]
    impl ProgressSink for CancelAt {
        async fn bytes(&mut self, file_bytes: u64) {
            if file_bytes >= self.at {
                self.token.cancel();
            }
        }
    }

    #[tokio::test]
    async fn cancelling_midway_keeps_the_part_file_and_a_new_run_resumes_it() {
        let server = serve(Mode::Serve, GOOD).await;
        let dir = tempdir().unwrap();
        let file = spec("model.bin", GOOD);
        // Seed the situation after a cancel / app restart: a partial file on disk.
        std::fs::write(part_path(dir.path(), "model.bin"), &GOOD[..8]).unwrap();

        let fetched = fetch(
            dir.path(),
            &file,
            &[source("main", &server.base_url)],
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(fetched.source, "main");
        let requests = server.requests.lock().unwrap().clone();
        assert_eq!(
            requests,
            vec![("/model.bin".to_string(), Some("bytes=8-".to_string()))]
        );
        assert_eq!(std::fs::read(dir.path().join("model.bin")).unwrap(), GOOD);
        assert!(!part_path(dir.path(), "model.bin").exists());
    }

    #[tokio::test]
    async fn real_cancellation_leaves_a_resumable_part_file() {
        let server = serve(Mode::Serve, GOOD).await;
        let dir = tempdir().unwrap();
        let file = spec("model.bin", GOOD);
        let cancel = CancellationToken::new();
        let mut sink = CancelAt {
            token: cancel.clone(),
            at: 5,
        };
        let error = fetch_file(
            &client().unwrap(),
            dir.path(),
            &file,
            &[source("main", &server.base_url)],
            &cancel,
            &fast(),
            &mut sink,
        )
        .await
        .unwrap_err();
        assert!(matches!(error, TransferError::Cancelled), "{error}");
        assert!(
            !dir.path().join("model.bin").exists(),
            "a cancelled download is never activated"
        );
        let kept = std::fs::metadata(part_path(dir.path(), "model.bin"))
            .map(|m| m.len())
            .unwrap_or(0);
        assert!(kept <= file.size);

        // Whatever was kept is resumed, and the end result is verified.
        fetch(
            dir.path(),
            &file,
            &[source("main", &server.base_url)],
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(std::fs::read(dir.path().join("model.bin")).unwrap(), GOOD);
    }

    #[tokio::test]
    async fn stalled_transfer_times_out_and_keeps_what_arrived_for_resume() {
        let server = serve(Mode::StallAfterHalf, GOOD).await;
        let dir = tempdir().unwrap();
        let file = spec("model.bin", GOOD);
        let started = std::time::Instant::now();
        let error = fetch(
            dir.path(),
            &file,
            &[source("main", &server.base_url)],
            &CancellationToken::new(),
        )
        .await
        .unwrap_err();
        assert!(started.elapsed() < Duration::from_secs(10));
        assert!(error.to_string().contains("kunne ikke hentes"), "{error}");
        assert!(error.to_string().contains("no data received"), "{error}");
        assert_eq!(
            std::fs::read(part_path(dir.path(), "model.bin")).unwrap(),
            &GOOD[..GOOD.len() / 2]
        );
        assert!(!dir.path().join("model.bin").exists());
    }

    #[tokio::test]
    async fn oversized_part_file_is_restarted_from_zero() {
        let server = serve(Mode::Serve, GOOD).await;
        let dir = tempdir().unwrap();
        std::fs::write(part_path(dir.path(), "model.bin"), [7u8; 99]).unwrap();
        fetch(
            dir.path(),
            &spec("model.bin", GOOD),
            &[source("main", &server.base_url)],
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(server.requests.lock().unwrap()[0].1, None);
        assert_eq!(std::fs::read(dir.path().join("model.bin")).unwrap(), GOOD);
    }

    #[tokio::test]
    async fn complete_part_with_wrong_content_is_deleted_without_a_request() {
        let dir = tempdir().unwrap();
        std::fs::write(part_path(dir.path(), "model.bin"), vec![b'x'; GOOD.len()]).unwrap();
        let error = fetch(
            dir.path(),
            &spec("model.bin", GOOD),
            &[source("main", "http://127.0.0.1:9")],
            &CancellationToken::new(),
        )
        .await
        .unwrap_err();
        assert!(
            matches!(error, TransferError::HashMismatch { .. }),
            "{error}"
        );
        assert!(!part_path(dir.path(), "model.bin").exists());
    }

    #[tokio::test]
    async fn installed_file_is_hashed_once_then_trusted_until_it_changes() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("model.bin");
        let file = spec("model.bin", GOOD);
        std::fs::write(&path, GOOD).unwrap();
        verify_installed(dir.path(), &file, &CancellationToken::new())
            .await
            .unwrap();
        assert!(verified_marker_path(dir.path(), "model.bin").exists());

        // Same size, different content, new modification time: the marker no longer applies.
        let mut changed = GOOD.to_vec();
        changed[0] ^= 1;
        std::fs::write(&path, &changed).unwrap();
        let later = std::time::SystemTime::now() + Duration::from_secs(5);
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(later)
            .unwrap();
        let error = verify_installed(dir.path(), &file, &CancellationToken::new())
            .await
            .unwrap_err();
        assert!(
            matches!(error, TransferError::InstalledMismatch { .. }),
            "{error}"
        );
        assert!(
            path.exists(),
            "an installed file is never deleted by the check"
        );
    }
}
