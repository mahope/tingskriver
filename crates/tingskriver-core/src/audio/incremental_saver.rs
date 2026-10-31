//! Crash recovery and cleanup of a meeting's raw tracks, as called by the
//! frontend's recovery dialog (Tauri commands of the same names).
//!
//! Recordings since the track writer keep WAV segments per track in
//! `.checkpoints/` (`track_writer`); recovery repairs and encodes them with
//! `finalize`. Folders from older versions hold 30 s mp4 checkpoints and are
//! still merged with FFmpeg's concat demuxer.

#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};

use log::{error, info, warn};
use serde::{Deserialize, Serialize};

use super::ffmpeg::find_ffmpeg_path;
use super::finalize::{
    finalize_raw_tracks, has_raw_tracks, read_verified_status, scan_raw_tracks, AudioSaveStatus,
    MIXED_FILE, RAW_DIR,
};

/// Audio recovery status for transcript recovery feature
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioRecoveryStatus {
    pub status: String, // "success" | "partial" | "failed" | "none"
    pub chunk_count: u32,
    pub estimated_duration_seconds: f64,
    pub audio_file_path: Option<String>,
    pub message: String,
}

impl AudioRecoveryStatus {
    fn none(message: &str) -> Self {
        Self {
            status: "none".to_string(),
            chunk_count: 0,
            estimated_duration_seconds: 0.0,
            audio_file_path: None,
            message: message.to_string(),
        }
    }
}

/// Recover the audio of a meeting folder after a crash: repair, encode and
/// verify the raw tracks. `partial` means some pieces were missing or
/// damaged and the rest was saved; the raw tracks are only deleted on a
/// complete (`success`) recovery.
pub async fn recover_audio_from_checkpoints(
    meeting_folder: String,
    _sample_rate: u32,
) -> Result<AudioRecoveryStatus, String> {
    info!("Starting audio recovery for folder: {}", meeting_folder);

    let folder_path = PathBuf::from(&meeting_folder);
    let checkpoints_dir = folder_path.join(RAW_DIR);

    if !checkpoints_dir.exists() {
        info!(
            "No checkpoints directory found at: {}",
            checkpoints_dir.display()
        );
        return Ok(AudioRecoveryStatus::none("No audio checkpoints found"));
    }

    if has_raw_tracks(&checkpoints_dir) {
        let folder = folder_path.clone();
        let outcome = tokio::task::spawn_blocking(move || {
            let ffmpeg = find_ffmpeg_path();
            finalize_raw_tracks(&folder, ffmpeg.as_deref())
        })
        .await
        .map_err(|e| format!("Audio recovery failed: {e}"))?;
        let status = match outcome.status {
            AudioSaveStatus::Ok => "success",
            AudioSaveStatus::Partial => "partial",
            AudioSaveStatus::Failed => "failed",
        };
        info!(
            "Audio recovery of {}: {} ({})",
            meeting_folder, status, outcome.message
        );
        return Ok(AudioRecoveryStatus {
            status: status.to_string(),
            chunk_count: outcome.segment_count as u32,
            estimated_duration_seconds: outcome.duration_seconds,
            audio_file_path: outcome.audio_file.map(|p| p.to_string_lossy().to_string()),
            message: outcome.message,
        });
    }

    recover_legacy_mp4_checkpoints(&folder_path, &checkpoints_dir).await
}

/// Folders from before the track writer: 30 s mp4 checkpoints merged with
/// FFmpeg's concat demuxer.
async fn recover_legacy_mp4_checkpoints(
    folder_path: &Path,
    checkpoints_dir: &Path,
) -> Result<AudioRecoveryStatus, String> {
    let mut checkpoint_files: Vec<_> = std::fs::read_dir(checkpoints_dir)
        .map_err(|e| format!("Failed to read checkpoints directory: {}", e))?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|s| s.to_str()) == Some("mp4"))
        .collect();

    if checkpoint_files.is_empty() {
        info!(
            "No checkpoint files found in: {}",
            checkpoints_dir.display()
        );
        return Ok(AudioRecoveryStatus::none("No audio checkpoint files found"));
    }

    // Sort by filename (audio_chunk_000.mp4, audio_chunk_001.mp4, etc.)
    checkpoint_files.sort();

    let chunk_count = checkpoint_files.len() as u32;
    let estimated_duration = (chunk_count as f64) * 30.0; // 30 seconds per chunk

    let concat_file_path = checkpoints_dir.join("concat_list.txt");
    let mut concat_content = String::new();
    for path in &checkpoint_files {
        let path = path
            .canonicalize()
            .map_err(|e| format!("Failed to canonicalize path: {}", e))?;
        concat_content.push_str(&format!("file '{}'\n", path.display()));
    }
    std::fs::write(&concat_file_path, concat_content)
        .map_err(|e| format!("Failed to write concat file: {}", e))?;

    let output_path = folder_path.join(MIXED_FILE);
    let output_path_str = output_path
        .to_str()
        .ok_or("Invalid output path")?
        .to_string();
    let concat_str = concat_file_path
        .to_str()
        .ok_or("Invalid concat path")?
        .to_string();

    let ffmpeg_path = find_ffmpeg_path()
        .ok_or_else(|| "FFmpeg not found. Please install FFmpeg to recover audio.".to_string())?;

    let mut command = std::process::Command::new(ffmpeg_path);
    command.args([
        "-nostats",
        "-loglevel",
        "error",
        "-y",
        "-f",
        "concat",
        "-safe",
        "0",
        "-i",
        &concat_str,
        "-c",
        "copy",
        &output_path_str,
    ]);

    // Hide console window on Windows
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    match command.output() {
        Ok(output) if output.status.success() => {
            let _ = std::fs::remove_file(concat_file_path);
            info!("Successfully recovered audio: {}", output_path_str);
            Ok(AudioRecoveryStatus {
                status: "success".to_string(),
                chunk_count,
                estimated_duration_seconds: estimated_duration,
                audio_file_path: Some(output_path_str),
                message: format!("Successfully recovered {} audio chunks", chunk_count),
            })
        }
        Ok(output) => {
            let error = String::from_utf8_lossy(&output.stderr);
            error!("FFmpeg recovery failed: {}", error);
            Ok(AudioRecoveryStatus {
                status: "failed".to_string(),
                chunk_count,
                estimated_duration_seconds: estimated_duration,
                audio_file_path: None,
                message: format!("FFmpeg failed: {}", error),
            })
        }
        Err(e) => {
            error!("Failed to run FFmpeg: {}", e);
            Ok(AudioRecoveryStatus {
                status: "failed".to_string(),
                chunk_count,
                estimated_duration_seconds: estimated_duration,
                audio_file_path: None,
                message: format!("Failed to run FFmpeg: {}", e),
            })
        }
    }
}

/// Delete the raw tracks after the frontend saved a recovered meeting. Only
/// done when the last finalize verified the final files (status `ok`);
/// otherwise the raw tracks stay and an error says why.
pub async fn cleanup_checkpoints(meeting_folder: String) -> Result<(), String> {
    info!("Cleaning up checkpoints for folder: {}", meeting_folder);

    let folder_path = PathBuf::from(&meeting_folder);
    let checkpoints_dir = folder_path.join(RAW_DIR);

    if !checkpoints_dir.exists() {
        info!("No checkpoints directory to clean up");
        return Ok(());
    }

    if has_raw_tracks(&checkpoints_dir) {
        match read_verified_status(&checkpoints_dir) {
            Some(AudioSaveStatus::Ok) => {}
            status => {
                warn!(
                    "Keeping raw tracks in {}: last finalize status {:?}",
                    checkpoints_dir.display(),
                    status
                );
                return Err(
                    "Rå-sporene bevares, fordi lyden ikke er samlet og verificeret.".to_string(),
                );
            }
        }
    } else if !folder_path.join(MIXED_FILE).exists() {
        // Legacy mp4 checkpoints without a merged file: keep them.
        return Err("Rå-sporene bevares, fordi der ikke findes en samlet lydfil.".to_string());
    }

    std::fs::remove_dir_all(&checkpoints_dir)
        .map_err(|e| format!("Failed to remove checkpoints directory: {}", e))?;
    info!("Successfully cleaned up checkpoints directory");
    Ok(())
}

/// Check if a meeting folder has audio to recover: WAV track segments, or
/// mp4 checkpoints from older versions.
pub async fn has_audio_checkpoints(meeting_folder: String) -> Result<bool, String> {
    let folder_path = PathBuf::from(&meeting_folder);
    let checkpoints_dir = folder_path.join(RAW_DIR);

    if !checkpoints_dir.exists() {
        return Ok(false);
    }

    if has_raw_tracks(&checkpoints_dir) {
        return Ok(scan_raw_tracks(&checkpoints_dir, false)
            .map(|s| s.has_audio())
            .unwrap_or(true));
    }

    let has_mp4_files = std::fs::read_dir(&checkpoints_dir)
        .map_err(|e| format!("Failed to read checkpoints directory: {}", e))?
        .filter_map(|entry| entry.ok())
        .any(|entry| entry.path().extension().and_then(|s| s.to_str()) == Some("mp4"));

    Ok(has_mp4_files)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::super::recording_writer::{spawn_writer, LogObserver};
    use super::super::track_writer::TrackKind;
    use super::*;
    use std::sync::Arc;

    fn record(folder: &Path) {
        let raw = folder.join(RAW_DIR);
        std::fs::create_dir_all(&raw).unwrap();
        let (tx, handle) = spawn_writer(raw, 48000, Arc::new(LogObserver)).unwrap();
        for i in 0..100u32 {
            tx.send(
                TrackKind::Microphone,
                vec![0.1; 480],
                Some((i + 1) as f64 * 0.01),
            )
            .unwrap();
        }
        drop(tx);
        handle.join().unwrap();
    }

    #[tokio::test]
    async fn cleanup_refuses_unverified_raw_tracks() {
        let dir = tempfile::tempdir().unwrap();
        record(dir.path());
        let folder = dir.path().to_string_lossy().to_string();
        assert!(has_audio_checkpoints(folder.clone()).await.unwrap());
        assert!(cleanup_checkpoints(folder.clone()).await.is_err());
        assert!(dir.path().join(RAW_DIR).exists());
    }

    #[tokio::test]
    async fn recovery_merges_raw_tracks() {
        if find_ffmpeg_path().is_none() {
            eprintln!("ffmpeg not available, skipping");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        record(dir.path());
        let folder = dir.path().to_string_lossy().to_string();
        let status = recover_audio_from_checkpoints(folder.clone(), 48000)
            .await
            .unwrap();
        assert_eq!(status.status, "success", "{}", status.message);
        assert!(dir.path().join(MIXED_FILE).exists());
        // Verified and deleted already; cleanup is a no-op.
        assert!(cleanup_checkpoints(folder).await.is_ok());
    }

    #[tokio::test]
    async fn empty_folder_has_nothing_to_recover() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().to_string_lossy().to_string();
        let status = recover_audio_from_checkpoints(folder.clone(), 48000)
            .await
            .unwrap();
        assert_eq!(status.status, "none");
        assert!(!has_audio_checkpoints(folder).await.unwrap());
    }
}
