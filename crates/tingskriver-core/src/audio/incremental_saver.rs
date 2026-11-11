//! Crash recovery and cleanup of a meeting's raw tracks, as called by the
//! frontend's recovery dialog (Tauri commands of the same names).
//!
//! Recordings since the track writer keep WAV segments per track in
//! `.checkpoints/` (`track_writer`); recovery repairs and encodes them with
//! `finalize`. Folders from older versions hold 30 s mp4 checkpoints and are
//! still merged with FFmpeg's concat demuxer.

#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};

use log::{error, info};
use serde::{Deserialize, Serialize};

use super::ffmpeg::find_ffmpeg_path;
use super::finalize::{
    decoded_duration_seconds, finish_with_cleanup, has_raw_tracks, recover_raw_tracks,
    scan_raw_tracks, verify_saved_outputs, AudioSaveStatus, FinalizeOutcome, FolderGuard,
    KEPT_MESSAGE, MAX_DURATION_DEVIATION, MIXED_FILE, RAW_DIR, VERIFIED_MARKER,
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
/// complete (`success`) recovery and the frontend's database save.
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
            recover_raw_tracks(&folder, ffmpeg.as_deref())
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

    tokio::task::spawn_blocking(move || {
        recover_legacy_mp4_checkpoints(&folder_path, &checkpoints_dir)
    })
    .await
    .map_err(|e| format!("{KEPT_MESSAGE} ({e})"))?
}

/// Folders from before the track writer: 30 s mp4 checkpoints merged with
/// FFmpeg's concat demuxer.
#[allow(clippy::disallowed_methods)] // Synchronous disk worker.
fn recover_legacy_mp4_checkpoints(
    folder_path: &Path,
    checkpoints_dir: &Path,
) -> Result<AudioRecoveryStatus, String> {
    let _guard = FolderGuard::acquire(folder_path).map_err(|e| e.to_string())?;
    match std::fs::remove_file(checkpoints_dir.join(VERIFIED_MARKER)) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("{KEPT_MESSAGE} ({e})")),
    }
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

    let original_count = checkpoint_files.len();
    let mut estimated_duration = 0.0;
    checkpoint_files.retain(|path| match decoded_duration_seconds(path) {
        Ok(duration) => {
            estimated_duration += duration;
            true
        }
        Err(e) => {
            log::warn!("Skipping damaged checkpoint {}: {e}", path.display());
            false
        }
    });
    if checkpoint_files.is_empty() {
        return Ok(AudioRecoveryStatus {
            status: "failed".to_string(),
            chunk_count: 0,
            estimated_duration_seconds: 0.0,
            audio_file_path: None,
            message: KEPT_MESSAGE.to_string(),
        });
    }
    let chunk_count = checkpoint_files.len() as u32;
    let partial = checkpoint_files.len() != original_count;

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
            let verified = decoded_duration_seconds(&output_path).is_ok_and(|duration| {
                (duration - estimated_duration).abs() <= MAX_DURATION_DEVIATION
            });
            if !verified {
                return Ok(AudioRecoveryStatus {
                    status: "failed".to_string(),
                    chunk_count,
                    estimated_duration_seconds: estimated_duration,
                    audio_file_path: None,
                    message: KEPT_MESSAGE.to_string(),
                });
            }
            let message = if partial {
                "Lyden er delvist gemt. Originalen er gemt.".to_string()
            } else {
                format!("Successfully recovered {} audio chunks", chunk_count)
            };
            finish_with_cleanup(
                checkpoints_dir,
                FinalizeOutcome {
                    status: if partial {
                        AudioSaveStatus::Partial
                    } else {
                        AudioSaveStatus::Ok
                    },
                    message: message.clone(),
                    audio_file: Some(output_path),
                    track_files: Vec::new(),
                    duration_seconds: estimated_duration,
                    segment_count: chunk_count as usize,
                    raw_kept: true,
                    problems: Vec::new(),
                },
                false,
            );
            info!("Successfully recovered audio: {}", output_path_str);
            Ok(AudioRecoveryStatus {
                status: if partial { "partial" } else { "success" }.to_string(),
                chunk_count,
                estimated_duration_seconds: estimated_duration,
                audio_file_path: Some(output_path_str),
                message,
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

    #[allow(clippy::disallowed_methods)] // Cleanup runs on the blocking pool.
    tokio::task::spawn_blocking(move || {
        let _guard = FolderGuard::acquire(&folder_path).map_err(|e| e.to_string())?;
        verify_saved_outputs(&checkpoints_dir).map_err(|e| format!("{KEPT_MESSAGE} ({e})"))?;
        std::fs::remove_dir_all(&checkpoints_dir)
            .map_err(|e| format!("Failed to remove checkpoints directory: {}", e))?;
        info!("Successfully cleaned up checkpoints directory");
        Ok(())
    })
    .await
    .map_err(|e| format!("{KEPT_MESSAGE} ({e})"))?
}

/// Check if a meeting folder has audio to recover: WAV track segments, or
/// mp4 checkpoints from older versions.
pub async fn has_audio_checkpoints(meeting_folder: String) -> Result<bool, String> {
    tokio::task::spawn_blocking(move || has_audio_checkpoints_blocking(meeting_folder))
        .await
        .map_err(|e| format!("Checkpoint scan task failed: {e}"))?
}

#[allow(clippy::disallowed_methods)] // Scan runs on the blocking pool.
fn has_audio_checkpoints_blocking(meeting_folder: String) -> Result<bool, String> {
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
#[allow(clippy::disallowed_methods)]
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
    async fn cleanup_refuses_unverified_legacy_output() {
        let dir = tempfile::tempdir().unwrap();
        let raw = dir.path().join(RAW_DIR);
        std::fs::create_dir_all(&raw).unwrap();
        std::fs::write(raw.join("audio_chunk_000.mp4"), b"original").unwrap();
        std::fs::write(dir.path().join(MIXED_FILE), b"").unwrap();
        assert!(
            cleanup_checkpoints(dir.path().to_string_lossy().into_owned())
                .await
                .is_err()
        );
        assert!(raw.exists());
    }

    #[tokio::test]
    async fn legacy_recovery_skips_truncated_last_file() {
        let Some(ffmpeg) = find_ffmpeg_path() else {
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let raw = dir.path().join(RAW_DIR);
        std::fs::create_dir_all(&raw).unwrap();
        let first = raw.join("audio_chunk_000.mp4");
        let mut sent = false;
        super::super::encode::encode_pcm_s16(&ffmpeg, 48000, "96k", &first, |block| {
            if sent {
                return Ok(false);
            }
            block.extend((0..48000).map(|i| ((i as f32 * 0.05).sin() * 10000.0) as i16));
            sent = true;
            Ok(true)
        })
        .unwrap();
        let last = raw.join("audio_chunk_001.mp4");
        std::fs::copy(&first, &last).unwrap();
        let file = std::fs::OpenOptions::new().write(true).open(&last).unwrap();
        file.set_len(file.metadata().unwrap().len() / 2).unwrap();
        let folder = dir.path().to_string_lossy().into_owned();
        let outcome = recover_audio_from_checkpoints(folder.clone(), 48000)
            .await
            .unwrap();
        assert_eq!(outcome.status, "partial", "{}", outcome.message);
        assert_eq!(outcome.chunk_count, 1);
        assert!((outcome.estimated_duration_seconds - 1.0).abs() < 0.1);
        assert!(outcome.audio_file_path.is_some());
        assert!(cleanup_checkpoints(folder).await.is_err());
        assert!(raw.exists());
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
        assert!(dir.path().join(RAW_DIR).exists());
        let retry = recover_audio_from_checkpoints(folder.clone(), 48000)
            .await
            .unwrap();
        assert_eq!(retry.status, "success");
        assert!(has_audio_checkpoints(folder.clone()).await.unwrap());
        assert!(cleanup_checkpoints(folder).await.is_ok());
        assert!(!dir.path().join(RAW_DIR).exists());
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

    #[tokio::test]
    async fn cleanup_rechecks_verified_files_before_deleting_originals() {
        if find_ffmpeg_path().is_none() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        record(dir.path());
        let folder = dir.path().to_string_lossy().into_owned();
        let outcome = recover_audio_from_checkpoints(folder.clone(), 48000)
            .await
            .unwrap();
        assert_eq!(outcome.status, "success", "{}", outcome.message);
        std::fs::write(dir.path().join("microphone.m4a"), b"").unwrap();
        assert!(cleanup_checkpoints(folder).await.is_err());
        assert!(dir.path().join(RAW_DIR).exists());
    }

    #[tokio::test]
    async fn failed_retry_invalidates_previous_verification() {
        if find_ffmpeg_path().is_none() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        record(dir.path());
        let folder = dir.path().to_string_lossy().into_owned();
        assert_eq!(
            recover_audio_from_checkpoints(folder.clone(), 48000)
                .await
                .unwrap()
                .status,
            "success"
        );
        std::fs::write(
            dir.path().join(RAW_DIR).join("microphone_000.wav"),
            b"broken",
        )
        .unwrap();
        assert_eq!(
            recover_audio_from_checkpoints(folder.clone(), 48000)
                .await
                .unwrap()
                .status,
            "failed"
        );
        assert!(cleanup_checkpoints(folder).await.is_err());
        assert!(dir.path().join(RAW_DIR).exists());
    }

    #[tokio::test]
    async fn cleanup_refuses_while_finalization_is_running() {
        let dir = tempfile::tempdir().unwrap();
        record(dir.path());
        let _guard = FolderGuard::acquire(dir.path()).unwrap();
        let error = cleanup_checkpoints(dir.path().to_string_lossy().into_owned())
            .await
            .unwrap_err();
        assert_eq!(error, "Lyden samles allerede.");
        assert!(dir.path().join(RAW_DIR).exists());
    }

    #[tokio::test]
    async fn cleanup_refuses_folder_alias_while_finalization_is_running() {
        let dir = tempfile::tempdir().unwrap();
        record(dir.path());
        let _guard = FolderGuard::acquire(dir.path()).unwrap();
        let alias = dir.path().join(RAW_DIR).join("..");
        let error = cleanup_checkpoints(alias.to_string_lossy().into_owned())
            .await
            .unwrap_err();
        assert_eq!(error, "Lyden samles allerede.");
        assert!(dir.path().join(RAW_DIR).exists());
    }

    #[tokio::test]
    async fn empty_legacy_retry_invalidates_previous_verification() {
        let dir = tempfile::tempdir().unwrap();
        let raw = dir.path().join(RAW_DIR);
        std::fs::create_dir_all(&raw).unwrap();
        let marker = serde_json::json!({
            "status": "ok", "message": "Lyden er gemt.",
            "audio_file": dir.path().join(MIXED_FILE), "track_files": [],
            "duration_seconds": 1.0, "segment_count": 1,
            "raw_kept": true, "problems": []
        });
        std::fs::write(raw.join(VERIFIED_MARKER), marker.to_string()).unwrap();
        assert_eq!(
            super::super::finalize::read_verified_status(&raw),
            Some(AudioSaveStatus::Ok)
        );
        let folder = dir.path().to_string_lossy().into_owned();
        assert_eq!(
            recover_audio_from_checkpoints(folder, 48000)
                .await
                .unwrap()
                .status,
            "none"
        );
        assert_eq!(super::super::finalize::read_verified_status(&raw), None);
    }
}
