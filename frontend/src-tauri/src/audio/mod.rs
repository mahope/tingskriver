//! Audio Tauri commands and recording orchestration; capture, mixing,
//! saving and transcription live in `tingskriver_core::audio`.

pub use tingskriver_core::audio::*;

pub mod import;
pub mod incremental_saver;
pub mod permissions;
pub mod recording_commands;
pub mod recording_preferences;
pub mod retranscription;
pub mod system_audio_commands;

pub use system_audio_commands::{
    start_system_audio_capture_command, list_system_audio_devices_command,
    check_system_audio_permissions_command, start_system_audio_monitoring,
    stop_system_audio_monitoring, get_system_audio_monitoring_status,
    init_system_audio_state
};
pub use recording_commands::{
    start_recording, start_recording_with_devices, stop_recording,
    is_recording, get_transcription_status, RecordingArgs, TranscriptionStatus, TranscriptUpdate
};
