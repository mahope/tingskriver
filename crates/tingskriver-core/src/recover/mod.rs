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
    let mut found = Vec::new();
    for entry in entries {
        let entry = entry?;
        if !entry.file_type()?.is_dir()
            || (entry.path().join(MIXED_FILE).is_file()
                && !entry.path().join(RAW_DIR).join(PENDING).is_file())
        {
            continue;
        }
        let raw = entry.path().join(RAW_DIR);
        if !raw.is_dir() {
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
    let root = root.canonicalize()?;
    let folder = root.join(id).canonicalize()?;
    anyhow::ensure!(
        folder.parent() == Some(root.as_path()),
        "Recording outside recordings folder"
    );
    let folder_string = folder.to_string_lossy().into_owned();
    // A stable ID also makes retries after DB failures and concurrent requests safe.
    let existing: Option<(String,)> =
        sqlx::query_as("SELECT id FROM meetings WHERE folder_path = ? LIMIT 1")
            .bind(&folder_string)
            .fetch_optional(pool)
            .await?;
    let meeting_id = existing
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
        saved_folder == folder_string,
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
    let restored = restore(
        &root,
        id,
        db.pool(),
        crate::audio::ffmpeg::find_ffmpeg_path().as_deref(),
    )
    .await?;
    let folder = root.join(id).to_string_lossy().into_owned();
    crate::audio::retranscription::start_retranscription(
        app,
        restored.meeting_id.clone(),
        folder,
        None,
        None,
        None,
    )
    .await?;
    Ok(restored)
}

#[cfg(test)]
mod tests;
