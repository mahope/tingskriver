//! Disk-based recovery, independent of webview storage. Call `scan` again
//! when opening recovery UI: the startup snapshot may predate a recording.
use std::path::{Component, Path};
use std::sync::Mutex;
use std::time::UNIX_EPOCH;

use crate::audio::finalize::{
    recover_raw_tracks, scan_raw_tracks, AudioSaveStatus, FinalizeOutcome, MIXED_FILE, RAW_DIR,
};
use crate::audio::track_writer::TrackKind;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Recording {
    pub id: String,
    pub duration_seconds: f64,
    /// Unix milliseconds, from the last written raw segment.
    pub last_written: u64,
    pub tracks: Vec<TrackKind>,
}

const PENDING: &str = "recovery-pending";

static STARTUP: Mutex<Vec<Recording>> = Mutex::new(Vec::new());

pub fn startup_recordings() -> Vec<Recording> {
    STARTUP.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Blocking, metadata/header-only scan. Does not repair or delete anything.
pub fn scan(root: &Path) -> anyhow::Result<Vec<Recording>> {
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    scan_entries(entries)
}

fn scan_entries(
    entries: impl Iterator<Item = std::io::Result<std::fs::DirEntry>>,
) -> anyhow::Result<Vec<Recording>> {
    let mut found = Vec::new();
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(e) => {
                log::warn!("Recovery scan could not read directory entry: {e}");
                continue;
            }
        };
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(e) => {
                log::warn!("Recovery scan could not read entry type: {e}");
                continue;
            }
        };
        if !file_type.is_dir()
            || (entry.path().join(MIXED_FILE).is_file()
                && !entry.path().join(RAW_DIR).join(PENDING).is_file())
        {
            continue;
        }
        let raw = entry.path().join(RAW_DIR);
        if !raw.is_dir() || crate::audio::recording_writer::has_live_writer(&raw) {
            continue;
        }
        let scanned = match scan_raw_tracks(&raw, false) {
            Ok(s) if s.has_audio() => s,
            Ok(_) => continue,
            Err(e) => {
                log::warn!("Recovery scan could not read raw tracks: {e}");
                continue;
            }
        };
        let last_written = scanned
            .tracks
            .iter()
            .flat_map(|t| &t.sources)
            .filter_map(|s| {
                if let crate::audio::finalize::Source::File { path, .. } = s {
                    path.metadata()
                        .ok()?
                        .modified()
                        .ok()?
                        .duration_since(UNIX_EPOCH)
                        .ok()
                        .map(|d| d.as_millis() as u64)
                } else {
                    None
                }
            })
            .max()
            .unwrap_or(0);
        found.push(Recording {
            id: entry.file_name().to_string_lossy().into_owned(),
            duration_seconds: scanned.duration_seconds(),
            last_written,
            tracks: scanned
                .tracks
                .iter()
                .filter(|t| t.segment_count > 0)
                .map(|t| t.kind)
                .collect(),
        });
    }
    found.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(found)
}

pub fn scan_at_startup(root: &Path) {
    match scan(root) {
        Ok(found) => *STARTUP.lock().unwrap_or_else(|e| e.into_inner()) = found,
        Err(e) => log::error!("Recovery startup scan failed: {e}"),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Restored {
    pub meeting_id: String,
    #[serde(flatten)]
    pub audio: FinalizeOutcome,
}

/// Restore audio and persist the meeting before allowing cleanup. Partial
/// audio keeps its originals. A retry uses the same database ID.
pub async fn restore(
    root: &Path,
    id: &str,
    pool: &sqlx::SqlitePool,
    ffmpeg: Option<&Path>,
) -> anyhow::Result<Restored> {
    let mut components = Path::new(id).components();
    anyhow::ensure!(
        matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none(),
        "Invalid recording ID"
    );
    let folder_string = root.join(id).to_string_lossy().into_owned();
    let root = root.canonicalize()?;
    let folder = root.join(id).canonicalize()?;
    anyhow::ensure!(
        folder.parent() == Some(root.as_path()),
        "Recording outside recordings folder"
    );
    // A stable ID also makes retries after DB failures and concurrent requests safe.
    let existing: Vec<(String, String)> =
        sqlx::query_as("SELECT id, folder_path FROM meetings WHERE folder_path IS NOT NULL")
            .fetch_all(pool)
            .await?;
    let meeting_id = existing
        .into_iter()
        .find(|(_, saved)| Path::new(saved).canonicalize().ok().as_ref() == Some(&folder))
        .map(|row| row.0)
        .unwrap_or_else(|| format!("recovered-{id}"));
    // If DB persistence fails after encoding, startup must still offer recovery.
    let pending = folder.join(RAW_DIR).join(PENDING);
    std::fs::write(&pending, b"")?;
    let folder_for_encode = folder.clone();
    let ffmpeg = ffmpeg.map(Path::to_path_buf);
    let audio = tokio::task::spawn_blocking(move || {
        recover_raw_tracks(&folder_for_encode, ffmpeg.as_deref())
    })
    .await?;
    anyhow::ensure!(audio.status != AudioSaveStatus::Failed, "{}", audio.message);
    let now = chrono::Utc::now();
    sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at, folder_path) VALUES (?, ?, ?, ?, ?) ON CONFLICT(id) DO NOTHING")
        .bind(&meeting_id).bind(id).bind(now).bind(now).bind(&folder_string).execute(pool).await?;
    let (saved_folder,): (String,) =
        sqlx::query_as("SELECT folder_path FROM meetings WHERE id = ?")
            .bind(&meeting_id)
            .fetch_one(pool)
            .await?;
    anyhow::ensure!(
        Path::new(&saved_folder).canonicalize()? == folder,
        "Recovery ID belongs to another recording"
    );
    std::fs::remove_file(pending)?;
    // Keep originals available until the caller acknowledges the restored meeting.
    Ok(Restored { meeting_id, audio })
}

/// User-triggered recovery follows the normal saved-file transcription path.
/// A transcription failure leaves the meeting and audio available for retry.
pub async fn restore_and_transcribe(
    app: crate::host::HostHandle,
    id: &str,
) -> anyhow::Result<Restored> {
    let db = app
        .database()
        .ok_or_else(|| anyhow::anyhow!("Database not initialized"))?;
    let root = crate::audio::recording_preferences::get_default_recordings_folder();
    restore_and_transcribe_with(
        &root,
        id,
        db.pool(),
        |meeting_id, folder, model, provider| {
            crate::audio::retranscription::start_retranscription(
                app, meeting_id, folder, None, model, provider,
            )
        },
    )
    .await
}

async fn restore_and_transcribe_with<F, Fut, T>(
    root: &Path,
    id: &str,
    pool: &sqlx::SqlitePool,
    transcribe: F,
) -> anyhow::Result<Restored>
where
    F: FnOnce(String, String, Option<String>, Option<String>) -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<T>>,
{
    let restored = restore(
        root,
        id,
        pool,
        crate::audio::ffmpeg::find_ffmpeg_path().as_deref(),
    )
    .await?;
    let folder = root.join(id).to_string_lossy().into_owned();
    let (provider, model) = transcription_settings(pool).await?;
    transcribe(restored.meeting_id.clone(), folder, model, provider).await?;
    Ok(restored)
}

async fn transcription_settings(
    pool: &sqlx::SqlitePool,
) -> anyhow::Result<(Option<String>, Option<String>)> {
    let settings: Option<(String, String)> =
        sqlx::query_as("SELECT provider, model FROM transcript_settings WHERE id = '1'")
            .fetch_optional(pool)
            .await?;
    Ok(match settings {
        Some((provider, model)) => (Some(provider), Some(model)),
        None => (None, None),
    })
}

#[cfg(test)]
mod tests;
