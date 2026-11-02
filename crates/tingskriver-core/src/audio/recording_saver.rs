//! Meeting folder, metadata and the disk writer of one recording.
//!
//! Start creates the folder and starts the writer (`recording_writer`); the
//! capture side sends to it through `RecordingState`. Stop waits for the
//! writer to drain its queue (every chunk sent before stop is on disk when
//! `stop_and_save` returns), writes the metadata and reports the result.
//! Encoding to the final files runs afterwards in the background
//! (`finalize`), so "stopped" means "the audio is saved" within seconds,
//! however long the meeting was.

#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use anyhow::Result;
use log::{error, info, warn};
use serde::{Deserialize, Serialize};

use crate::host::{HostExt, HostHandle};

use super::audio_processing::create_meeting_folder;
use super::finalize::{finalize_raw_tracks, AudioSaveStatus, MIXED_FILE, RAW_DIR};
use super::recording_writer::{spawn_writer, StorageSender, WriterHandle, WriterObserver};
use super::track_writer::StorageErrorKind;

/// Sample rate of the recording tracks.
pub const RECORDING_SAMPLE_RATE: u32 = 48000;

/// Structured transcript segment for JSON export
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptSegment {
    pub id: String,
    pub text: String,
    pub audio_start_time: f64, // Seconds from recording start
    pub audio_end_time: f64,   // Seconds from recording start
    pub duration: f64,         // Segment duration in seconds
    pub display_time: String,  // Formatted time for display like "[02:15]"
    pub confidence: f32,
    pub sequence_id: u64,
}

/// Meeting metadata structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeetingMetadata {
    pub version: String,
    pub meeting_id: Option<String>,
    pub meeting_name: Option<String>,
    pub created_at: String,
    pub completed_at: Option<String>,
    pub duration_seconds: Option<f64>,
    pub devices: DeviceInfo,
    pub audio_file: String,
    pub transcript_file: String,
    pub sample_rate: u32,
    pub status: String, // "recording", "completed", "error"
    /// "ok", "delvis" or "fejlet" once known (see `finalize`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub microphone: Option<String>,
    pub system_audio: Option<String>,
}

/// What stop reports to the UI.
#[derive(Debug, Clone, Serialize)]
pub struct SaveResult {
    pub status: AudioSaveStatus,
    /// Danish message for the user.
    pub message: String,
    pub meeting_folder: Option<PathBuf>,
    pub duration_seconds: f64,
}

/// Sends the writer's storage events to the UI.
struct HostObserver {
    app: HostHandle,
}

impl WriterObserver for HostObserver {
    fn storage_failed(&self, message: &str, kind: StorageErrorKind) {
        if let Err(e) = self.app.emit(
            "recording-storage-error",
            serde_json::json!({ "message": message, "kind": kind }),
        ) {
            warn!("Failed to emit recording-storage-error: {e}");
        }
    }

    fn storage_recovered(&self) {
        if let Err(e) = self.app.emit(
            "recording-storage-recovered",
            serde_json::json!({ "message": "Lyden gemmes igen." }),
        ) {
            warn!("Failed to emit recording-storage-recovered: {e}");
        }
    }

    fn backlog(&self, queued_seconds: f64) {
        if let Err(e) = self.app.emit(
            "recording-storage-backlog",
            serde_json::json!({
                "queued_seconds": queued_seconds,
                "message": format!("Disken er langsom: {queued_seconds:.0} s lyd venter på at blive gemt."),
            }),
        ) {
            warn!("Failed to emit recording-storage-backlog: {e}");
        }
    }
}

/// Folder, metadata and writer of the current recording.
pub struct RecordingSaver {
    meeting_folder: Option<PathBuf>,
    meeting_name: Option<String>,
    metadata: Option<MeetingMetadata>,
    transcript_segments: Arc<Mutex<Vec<TranscriptSegment>>>,
    writer: Option<WriterHandle>,
}

impl RecordingSaver {
    pub fn new() -> Self {
        Self {
            meeting_folder: None,
            meeting_name: None,
            metadata: None,
            transcript_segments: Arc::new(Mutex::new(Vec::new())),
            writer: None,
        }
    }

    /// Set the meeting name for this recording session
    pub fn set_meeting_name(&mut self, name: Option<String>) {
        self.meeting_name = name;
    }

    /// Set device information in metadata
    pub fn set_device_info(&mut self, mic_name: Option<String>, sys_name: Option<String>) {
        if let Some(ref mut metadata) = self.metadata {
            metadata.devices.microphone =
                mic_name.map(|n| super::device_follow::repair_double_utf8(&n));
            metadata.devices.system_audio =
                sys_name.map(|n| super::device_follow::repair_double_utf8(&n));

            // Write updated metadata to disk if folder exists
            if let Some(folder) = &self.meeting_folder {
                let metadata_clone = metadata.clone();
                if let Err(e) = self.write_metadata(folder, &metadata_clone) {
                    warn!("Failed to update metadata with device info: {}", e);
                }
            }
        }
    }

    /// Keep a transcript segment in memory (upserts on `sequence_id`). Nothing
    /// is written per segment; transcripts.json is written once at stop.
    pub fn add_transcript_segment(&self, segment: TranscriptSegment) {
        let mut segments = self
            .transcript_segments
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(existing) = segments
            .iter_mut()
            .find(|s| s.sequence_id == segment.sequence_id)
        {
            *existing = segment;
        } else {
            segments.push(segment);
        }
    }

    /// Legacy method for backward compatibility - converts text to basic segment
    pub fn add_transcript_chunk(&self, text: String) {
        let segment = TranscriptSegment {
            id: format!("seg_{}", chrono::Utc::now().timestamp_millis()),
            text,
            audio_start_time: 0.0,
            audio_end_time: 0.0,
            duration: 0.0,
            display_time: "[00:00]".to_string(),
            confidence: 1.0,
            sequence_id: 0,
        };
        self.add_transcript_segment(segment);
    }

    /// Create the meeting folder and start the disk writer. Returns the
    /// sender the capture side uses. Fails (and the recording does not start)
    /// if the folder cannot be created: a recording that cannot be saved is
    /// not started.
    ///
    /// The old `auto_save = false` mode ("transcripts only") is gone: the
    /// transcript is made from the saved audio after the meeting, so the
    /// audio is always written.
    pub fn start_accumulation(&mut self, app: &HostHandle) -> Result<StorageSender> {
        let name = self.meeting_name.clone().unwrap_or_else(|| {
            format!(
                "Meeting {}",
                chrono::Local::now().format("%Y-%m-%d_%H-%M-%S")
            )
        });
        self.initialize_meeting_folder(&name)?;
        let folder = self
            .meeting_folder
            .clone()
            .ok_or_else(|| anyhow::anyhow!("meeting folder missing after initialization"))?;
        let observer = Arc::new(HostObserver { app: app.clone() });
        let (sender, writer) = spawn_writer(folder.join(RAW_DIR), RECORDING_SAMPLE_RATE, observer)
            .map_err(|e| anyhow::anyhow!("Lyden kan ikke gemmes: {e}"))?;
        self.writer = Some(writer);
        info!(
            "Recording writer started for meeting '{}' in {}",
            name,
            folder.display()
        );
        Ok(sender)
    }

    /// Initialize meeting folder structure and metadata
    fn initialize_meeting_folder(&mut self, meeting_name: &str) -> Result<()> {
        // Load preferences to get base recordings folder
        let base_folder = super::recording_preferences::get_default_recordings_folder();

        let meeting_folder =
            create_meeting_folder(&base_folder, meeting_name, true).map_err(|e| {
                anyhow::anyhow!("Lyden kan ikke gemmes: mappen kunne ikke oprettes ({e})")
            })?;

        // Create initial metadata
        let metadata = MeetingMetadata {
            version: "1.0".to_string(),
            meeting_id: None, // Will be set by backend
            meeting_name: Some(meeting_name.to_string()),
            created_at: chrono::Utc::now().to_rfc3339(),
            completed_at: None,
            duration_seconds: None,
            devices: DeviceInfo {
                microphone: None,
                system_audio: None,
            },
            audio_file: MIXED_FILE.to_string(),
            transcript_file: "transcripts.json".to_string(),
            sample_rate: RECORDING_SAMPLE_RATE,
            status: "recording".to_string(),
            audio_status: None,
        };

        // Write initial metadata.json
        self.write_metadata(&meeting_folder, &metadata)?;

        self.meeting_folder = Some(meeting_folder);
        self.metadata = Some(metadata);

        Ok(())
    }

    /// Write metadata.json to disk (atomic write with temp file)
    fn write_metadata(&self, folder: &Path, metadata: &MeetingMetadata) -> Result<()> {
        let metadata_path = folder.join("metadata.json");
        let temp_path = folder.join(".metadata.json.tmp");

        let json_string = serde_json::to_string_pretty(metadata)?;
        std::fs::write(&temp_path, json_string)?;
        std::fs::rename(&temp_path, &metadata_path)?; // Atomic

        Ok(())
    }

    /// Write transcripts.json to disk (atomic write with temp file)
    fn write_transcripts_json(&self, folder: &Path) -> Result<()> {
        let segments_clone = self.get_transcript_segments();
        let transcript_path = folder.join("transcripts.json");
        let temp_path = folder.join(".transcripts.json.tmp");

        let json = serde_json::json!({
            "version": "1.0",
            "segments": segments_clone,
            "last_updated": chrono::Utc::now().to_rfc3339(),
            "total_segments": segments_clone.len()
        });
        let json_string = serde_json::to_string_pretty(&json)?;
        std::fs::write(&temp_path, &json_string)?;
        std::fs::rename(&temp_path, &transcript_path)?;
        Ok(())
    }

    /// (segment files written so far, sample rate). Kept for the old stats API.
    pub fn get_stats(&self) -> (usize, u32) {
        (0, RECORDING_SAMPLE_RATE)
    }

    /// The storage error being shown to the user right now ("Lyden gemmes
    /// ikke: disken er fuld"), if any.
    pub fn storage_error(&self) -> Option<String> {
        self.writer.as_ref().and_then(|w| w.storage_error())
    }

    /// Wait for the writer to drain (the capture side must have dropped its
    /// senders: `RecordingState::close_audio_sender`), write the metadata,
    /// emit `recording-saved`, and start encoding the final files in the
    /// background (`recording-audio-finalized` when done).
    pub async fn stop_and_save(
        &mut self,
        app: &HostHandle,
        recording_duration: Option<f64>,
    ) -> Result<Option<SaveResult>, String> {
        info!("Stopping recording saver");
        let started = Instant::now();

        let Some(writer) = self.writer.take() else {
            info!("No recording writer was started - nothing to save");
            return Ok(None);
        };

        // Every chunk sent before stop is drained and committed here; there is
        // no fixed wait and no flag that cuts the queue short.
        let summary = match tokio::task::spawn_blocking(move || writer.join()).await {
            Ok(Ok(summary)) => summary,
            Ok(Err(e)) => {
                error!("Recording writer failed: {e}");
                return Err(e);
            }
            Err(e) => {
                error!("Waiting for the recording writer failed: {e}");
                return Err(format!("Waiting for the recording writer failed: {e}"));
            }
        };
        let audio_duration = summary.duration_seconds();
        info!(
            "Recording writer drained in {} ms: {:.1}s of audio on disk",
            started.elapsed().as_millis(),
            audio_duration
        );

        let lost = summary.lost_samples();
        let (status, message) = if summary.tracks.is_empty() {
            (
                AudioSaveStatus::Failed,
                "Der blev ikke optaget nogen lyd.".to_string(),
            )
        } else if summary.crashed || lost > 0 || summary.storage_error.is_some() {
            let lost_seconds = lost as f64 / RECORDING_SAMPLE_RATE as f64;
            let reason = summary
                .storage_error
                .clone()
                .unwrap_or_else(|| "lagringen fejlede".to_string());
            (
                AudioSaveStatus::Partial,
                format!("Lyden er delvist gemt ({lost_seconds:.1} s mangler): {reason}"),
            )
        } else {
            (AudioSaveStatus::Ok, "Lyden er gemt.".to_string())
        };

        if let Some(folder) = &self.meeting_folder {
            if let Err(e) = self.write_transcripts_json(folder) {
                warn!("Failed to write transcripts.json: {e}");
            }
        }

        // Update metadata to completed status with the length of the saved audio
        if let (Some(folder), Some(mut metadata)) = (&self.meeting_folder, self.metadata.clone()) {
            metadata.status = "completed".to_string();
            metadata.completed_at = Some(chrono::Utc::now().to_rfc3339());
            if let Some(clock) = recording_duration {
                if (clock - audio_duration).abs() > 1.0 {
                    warn!("Recording clock says {:.1}s but the saved audio is {:.1}s; using the audio length", clock, audio_duration);
                }
            }
            metadata.duration_seconds = Some(audio_duration)
                .filter(|d| *d > 0.0)
                .or(recording_duration);
            if let Err(e) = self.write_metadata(folder, &metadata) {
                warn!("Failed to update metadata to completed: {e}");
            }
        }

        let save_event = serde_json::json!({
            "audio_file": self.meeting_folder.as_ref().map(|f| f.join(MIXED_FILE).to_string_lossy().to_string()),
            "transcript_file": self.meeting_folder.as_ref()
                .map(|f| f.join("transcripts.json").to_string_lossy().to_string()),
            "meeting_name": self.meeting_name,
            "meeting_folder": self.meeting_folder.as_ref()
                .map(|f| f.to_string_lossy().to_string()),
            "status": status,
            "message": message,
            "duration_seconds": audio_duration,
        });
        if let Err(e) = app.emit("recording-saved", &save_event) {
            warn!("Failed to emit recording-saved event: {}", e);
        }

        if let Some(folder) = self.meeting_folder.clone() {
            spawn_finalize(app.clone(), folder);
        }

        // Clean up transcript segments
        self.transcript_segments
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();

        Ok(Some(SaveResult {
            status,
            message,
            meeting_folder: self.meeting_folder.clone(),
            duration_seconds: audio_duration,
        }))
    }

    /// Get the meeting folder path (for passing to backend)
    pub fn get_meeting_folder(&self) -> Option<&PathBuf> {
        self.meeting_folder.as_ref()
    }

    /// Get accumulated transcript segments (for reload sync)
    pub fn get_transcript_segments(&self) -> Vec<TranscriptSegment> {
        self.transcript_segments
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Get meeting name (for reload sync)
    pub fn get_meeting_name(&self) -> Option<String> {
        self.meeting_name.clone()
    }
}

/// Encode and verify the final files after stop, off the stop path.
fn spawn_finalize(app: HostHandle, folder: PathBuf) {
    tokio::spawn(async move {
        let job_folder = folder.clone();
        let outcome = tokio::task::spawn_blocking(move || {
            let ffmpeg = super::ffmpeg::find_ffmpeg_path();
            finalize_raw_tracks(&job_folder, ffmpeg.as_deref())
        })
        .await;
        let payload = match outcome {
            Ok(outcome) => serde_json::json!({
                "meeting_folder": folder.to_string_lossy(),
                "status": outcome.status,
                "message": outcome.message,
                "audio_file": outcome.audio_file.map(|p| p.to_string_lossy().to_string()),
                "track_files": outcome.track_files.iter().map(|p| p.to_string_lossy().to_string()).collect::<Vec<_>>(),
                "raw_kept": outcome.raw_kept,
            }),
            Err(e) => {
                error!("Finalizing {} panicked: {e}", folder.display());
                serde_json::json!({
                    "meeting_folder": folder.to_string_lossy(),
                    "status": AudioSaveStatus::Failed,
                    "message": super::finalize::KEPT_MESSAGE,
                    "raw_kept": true,
                })
            }
        };
        if let Err(e) = app.emit("recording-audio-finalized", &payload) {
            warn!("Failed to emit recording-audio-finalized: {e}");
        }
    });
}

impl Default for RecordingSaver {
    fn default() -> Self {
        Self::new()
    }
}
