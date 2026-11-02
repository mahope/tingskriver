use std::sync::Arc;
use crate::host::HostHandle;
use tokio::sync::mpsc;
use anyhow::Result;
use log::{debug, error, info, warn};
#[cfg(target_os = "macos")]
use std::time::Duration;
#[cfg(target_os = "macos")]
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use super::devices::AudioDevice;

use super::recording_state::RecordingState;
use super::stream::AudioStreamManager;
use super::recording_saver::{RecordingSaver, SaveResult};
use super::device_monitor::{AudioDeviceMonitor, DeviceEvent};

/// Stream manager type enumeration
pub enum StreamManagerType {
    Standard(AudioStreamManager),
}

// ============================================================================
// macOS Core Audio "pre-wake" — ported from feat/audio-device-handling
// (commit 8bf6af8b, with refinements from d266feac + 7107adc7 + a10f8e55).
// ============================================================================
//
// When a macOS Mac has a Bluetooth device as the active audio output, the
// built-in audio hardware unit — and, crucially, the BT audio connection
// itself — can sit in a low-power state where registered IO procs do not
// fire even though `AudioDeviceStart` returned `noErr`. The symptom is a
// 60–90 second "silent" period at recording start (or during a disconnect
// fallback) where the mic is technically capturing but no samples reach
// the pipeline, until something nudges Core Audio back to life.
//
// The workaround, empirically validated by Sujith in the audio-device-
// handling branch: briefly play 300 ms of digital silence through the
// output device by name. The name-based enumeration is itself part of the
// fix ("enumeration wake" — it forces the HAL to bring the device out of
// its idle power state). The subsequent `stream.play()` on the output
// activates the hardware unit fully, at which point the built-in mic's
// IO proc starts firing normally. Total cost: ~300 ms added to recording
// start, ~150 ms added to the disconnect-fallback hot-swap path.

#[cfg(target_os = "macos")]
const AUDIO_WAKE_DURATION: Duration = Duration::from_millis(300);

#[cfg(target_os = "macos")]
const AUDIO_WAKE_SWAP_DURATION: Duration = Duration::from_millis(150);

/// Synchronous implementation of the wake. Must be called from a blocking
/// thread (not directly from async code) because it sleeps 300 ms and does
/// sync cpal I/O. The async wrapper below routes to it via spawn_blocking.
#[cfg(target_os = "macos")]
fn wake_audio_connection_sync(speaker_device_name: &str) -> Result<()> {
    info!("[AUDIO_WAKE] Waking audio via speaker: '{}'", speaker_device_name);

    let host = cpal::default_host();

    let output_device = host.output_devices()?
        .find(|d| d.name().ok().as_deref() == Some(speaker_device_name))
        .ok_or_else(|| anyhow::anyhow!("Output device '{}' not found", speaker_device_name))?;

    let config = output_device.default_output_config()?;
    info!("[AUDIO_WAKE] Output config: {} Hz, {} channels",
          config.sample_rate().0, config.channels());

    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => {
            output_device.build_output_stream(
                &config.into(),
                |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    for sample in data.iter_mut() { *sample = 0.0; }
                },
                |err| error!("[AUDIO_WAKE] Output stream error: {}", err),
                None,
            )?
        }
        cpal::SampleFormat::I16 => {
            // BT HFP mode frequently picks I16 for the output config — without
            // this branch the wake would error out exactly when we need it most.
            output_device.build_output_stream(
                &config.into(),
                |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
                    for sample in data.iter_mut() { *sample = 0; }
                },
                |err| error!("[AUDIO_WAKE] Output stream error: {}", err),
                None,
            )?
        }
        format => {
            return Err(anyhow::anyhow!("Unsupported sample format: {:?}", format));
        }
    };

    stream.play()?;
    info!("[AUDIO_WAKE] Playing silence for {} ms to wake audio connection...", AUDIO_WAKE_DURATION.as_millis());
    std::thread::sleep(AUDIO_WAKE_DURATION);
    drop(stream);
    info!("[AUDIO_WAKE] Audio wake completed");

    Ok(())
}

/// Async wrapper around `wake_audio_connection_sync`. Used at recording start
/// so the 300 ms sleep doesn't block the tokio runtime. Non-fatal — the caller
/// should log the error and proceed if this fails; recording will still work,
/// it just may have the 60-90s silent startup on BT.
#[cfg(target_os = "macos")]
pub(super) async fn wake_audio_connection(speaker_device_name: &str) -> Result<()> {
    let name = speaker_device_name.to_string();
    tokio::task::spawn_blocking(move || {
        wake_audio_connection_sync(&name)
    }).await.map_err(|e| anyhow::anyhow!("Join error: {}", e))?
}

/// Public async wake for use by the disconnect-fallback hot-swap path in
/// `recording_commands.rs::trigger_mic_fallback_to_default`.
///
/// Differences from the start-time wake:
///   - Shorter 150 ms sleep to minimize fallback latency (the window of no
///     audio between primary death and secondary-taking-over)
///   - Falls back to the system default output if the named device isn't
///     found (e.g. the original BT output just disappeared, and we're
///     waking the MacBook speakers instead)
///   - Still handles both F32 and I16 sample formats for robustness
#[cfg(target_os = "macos")]
pub async fn wake_audio_connection_for_swap(speaker_device_name: &str) -> Result<()> {
    let name = speaker_device_name.to_string();
    tokio::task::spawn_blocking(move || {
        info!("[AUDIO_WAKE] Hot-swap wake via speaker: '{}'", name);
        let host = cpal::default_host();
        // Try the specified device first, fall back to default output. During
        // a BT disconnect the original output device name may no longer
        // enumerate, so the fallback is what actually runs in practice.
        let (output_device, config) = host.output_devices()?
            .find(|d| d.name().ok().as_deref() == Some(&*name))
            .and_then(|d| d.default_output_config().ok().map(|c| (d, c)))
            .or_else(|| {
                info!("[AUDIO_WAKE] '{}' not found or config failed — using default output device", name);
                host.default_output_device()
                    .and_then(|d| d.default_output_config().ok().map(|c| (d, c)))
            })
            .ok_or_else(|| anyhow::anyhow!("No output device available for wake"))?;
        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => {
                output_device.build_output_stream(
                    &config.into(),
                    |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                        for sample in data.iter_mut() { *sample = 0.0; }
                    },
                    |err| error!("[AUDIO_WAKE] Hot-swap wake error: {}", err),
                    None,
                )?
            }
            cpal::SampleFormat::I16 => {
                output_device.build_output_stream(
                    &config.into(),
                    |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
                        for sample in data.iter_mut() { *sample = 0; }
                    },
                    |err| error!("[AUDIO_WAKE] Hot-swap wake error: {}", err),
                    None,
                )?
            }
            format => {
                return Err(anyhow::anyhow!("Unsupported sample format for wake: {:?}", format));
            }
        };
        stream.play()?;
        std::thread::sleep(AUDIO_WAKE_SWAP_DURATION);
        drop(stream);
        info!("[AUDIO_WAKE] Hot-swap wake completed");
        Ok(())
    }).await.map_err(|e| anyhow::anyhow!("Join error: {}", e))?
}

/// Simplified recording manager that coordinates all audio components
pub struct RecordingManager {
    state: Arc<RecordingState>,
    stream_manager: AudioStreamManager,
    recording_saver: RecordingSaver,
    device_monitor: Option<AudioDeviceMonitor>,
    device_event_receiver: Option<mpsc::UnboundedReceiver<DeviceEvent>>,
}

// SAFETY: RecordingManager contains types that we've marked as Send
unsafe impl Send for RecordingManager {}

impl RecordingManager {
    /// Create a new recording manager
    pub fn new() -> Self {
        let state = RecordingState::new();
        let stream_manager = AudioStreamManager::new(state.clone());
        let (device_monitor, device_event_receiver) = AudioDeviceMonitor::new();

        Self {
            state,
            stream_manager,
            recording_saver: RecordingSaver::new(),
            device_monitor: Some(device_monitor),
            device_event_receiver: Some(device_event_receiver),
        }
    }

    // Remove app handle storage for now - will be passed directly when saving

    /// Start a recording: meeting folder and disk writer first, then the
    /// capture streams. No transcription model or VAD is involved; the
    /// transcript is made from the saved files after the meeting.
    pub(crate) async fn start_recording(
        &mut self,
        app: &HostHandle,
        microphone_device: Option<Arc<AudioDevice>>,
        system_device: Option<Arc<AudioDevice>>,
    ) -> Result<()> {
        info!("Starting recording manager");

        self.state.start_recording()?;

        let sender = match self.recording_saver.start_accumulation(app) {
            Ok(sender) => sender,
            Err(e) => {
                self.state.stop_recording();
                return Err(e);
            }
        };
        self.state.set_audio_sender(sender);
        self.recording_saver.set_device_info(
            microphone_device.as_ref().map(|d| d.name.clone()),
            system_device.as_ref().map(|d| d.name.clone())
        );

        if let Err(e) = self.stream_manager.start_streams(microphone_device.clone(), system_device.clone(), None).await {
            // Closing the sender lets the writer finish on its own.
            self.state.stop_recording();
            return Err(e);
        }

        // Start device monitoring for disconnect/reconnect detection
        if let Some(ref mut monitor) = self.device_monitor {
            if let Err(e) = monitor.start_monitoring(microphone_device, system_device) {
                warn!("Failed to start device monitoring: {}", e);
            } else {
                info!("✅ Device monitoring started");
            }
        }

        info!("Recording manager started successfully with {} active streams",
               self.stream_manager.active_stream_count());

        Ok(())
    }

    /// Stop capture and close the writer's queue: streams first (their last
    /// callbacks and resampler tails still reach the writer), then the
    /// sender, so the writer drains everything and finishes.
    async fn stop_capture(&mut self) {
        if let Some(ref mut monitor) = self.device_monitor {
            monitor.stop_monitoring().await;
        }

        if let Err(e) = self.stream_manager.stop_streams() {
            error!("Error stopping audio streams: {}", e);
        }

        self.state.stop_recording();
    }

    /// Stop capture only (the writer drains in the background until
    /// `save_recording_only` collects it).
    pub async fn stop_streams_only(&mut self) -> Result<()> {
        info!("Stopping recording streams only");
        self.stop_capture().await;
        debug!("Recording streams stopped successfully");
        Ok(())
    }

    /// Stop capture; kept under its old name for the stop command. There is
    /// no pipeline to flush any more: every chunk goes straight to the writer.
    pub async fn stop_streams_and_force_flush(&mut self) -> Result<()> {
        info!("Stopping recording streams");
        self.stop_capture().await;
        info!("✅ Recording streams stopped");
        Ok(())
    }

    /// Wait for the writer, write metadata and report the save status.
    pub async fn save_recording_only(&mut self, app: &HostHandle) -> std::result::Result<Option<SaveResult>, String> {
        let recording_duration = self.state.get_active_recording_duration();
        info!("Recording duration from state: {:?}s", recording_duration);

        let result = self.recording_saver.stop_and_save(app, recording_duration).await;
        match &result {
            Ok(Some(saved)) => info!("Recording saved ({}): {}", saved.status.as_str(), saved.message),
            Ok(None) => debug!("Recording not saved (no writer was started)"),
            Err(e) => error!("Failed to save recording: {}", e),
        }
        self.state.cleanup();
        result
    }

    /// Stop recording and save
    pub async fn stop_recording(&mut self, app: &HostHandle) -> Result<()> {
        info!("Stopping recording manager");
        self.stop_capture().await;
        if let Err(e) = self.save_recording_only(app).await {
            error!("Failed to save recording: {}", e);
        }
        info!("Recording manager stopped");
        Ok(())
    }

    /// Get recording stats from the saver
    pub fn get_recording_stats(&self) -> (usize, u32) {
        self.recording_saver.get_stats()
    }

    /// Check if currently recording
    pub fn is_recording(&self) -> bool {
        self.state.is_recording()
    }

    /// Pause the current recording session
    pub fn pause_recording(&self) -> Result<()> {
        info!("Pausing recording");
        self.state.pause_recording()
    }

    /// Resume the current recording session
    pub fn resume_recording(&self) -> Result<()> {
        info!("Resuming recording");
        self.state.resume_recording()
    }

    /// Check if recording is currently paused
    pub fn is_paused(&self) -> bool {
        self.state.is_paused()
    }

    /// Check if recording is active (recording and not paused)
    pub fn is_active(&self) -> bool {
        self.state.is_active()
    }

    /// Get recording statistics
    pub fn get_stats(&self) -> super::recording_state::RecordingStats {
        self.state.get_stats()
    }

    /// Get recording duration
    pub fn get_recording_duration(&self) -> Option<f64> {
        self.state.get_recording_duration()
    }

    /// Get active recording duration (excluding pauses)
    pub fn get_active_recording_duration(&self) -> Option<f64> {
        self.state.get_active_recording_duration()
    }

    /// Get total pause duration
    pub fn get_total_pause_duration(&self) -> f64 {
        self.state.get_total_pause_duration()
    }

    /// Get current pause duration if paused
    pub fn get_current_pause_duration(&self) -> Option<f64> {
        self.state.get_current_pause_duration()
    }

    /// Get error information
    pub fn get_error_info(&self) -> (u32, Option<super::recording_state::AudioError>) {
        (self.state.get_error_count(), self.state.get_last_error())
    }

    /// Get active stream count
    pub fn active_stream_count(&self) -> usize {
        self.stream_manager.active_stream_count()
    }

    /// Set error callback for handling errors
    pub fn set_error_callback<F>(&self, callback: F)
    where
        F: Fn(&super::recording_state::AudioError) + Send + Sync + 'static,
    {
        self.state.set_error_callback(callback);
    }

    /// Check if there's a fatal error
    pub fn has_fatal_error(&self) -> bool {
        self.state.has_fatal_error()
    }

    /// Set the meeting name for this recording session
    pub fn set_meeting_name(&mut self, name: Option<String>) {
        self.recording_saver.set_meeting_name(name);
    }

    /// Add a structured transcript segment to be saved later
    pub fn add_transcript_segment(&self, segment: super::recording_saver::TranscriptSegment) {
        self.recording_saver.add_transcript_segment(segment);
    }

    /// Add a transcript chunk to be saved later (legacy method)
    pub fn add_transcript_chunk(&self, text: String) {
        self.recording_saver.add_transcript_chunk(text);
    }

    /// Get accumulated transcript segments from current recording session
    /// Used for syncing frontend state after page reload during active recording
    pub fn get_transcript_segments(&self) -> Vec<super::recording_saver::TranscriptSegment> {
        self.recording_saver.get_transcript_segments()
    }

    /// Get meeting name from current recording session
    /// Used for syncing frontend state after page reload during active recording
    pub fn get_meeting_name(&self) -> Option<String> {
        self.recording_saver.get_meeting_name()
    }

    /// The storage error being shown to the user right now, if any.
    pub fn storage_error(&self) -> Option<String> {
        self.recording_saver.storage_error()
    }

    /// Cleanup all resources without saving
    pub async fn cleanup_without_save(&mut self) {
        if self.is_recording() {
            debug!("Stopping recording without saving during cleanup");
            self.stop_capture().await;
        }
        self.state.cleanup();
    }

    pub fn get_meeting_folder(&self) -> Option<std::path::PathBuf> {
        self.recording_saver.get_meeting_folder().map(|p| p.clone())
    }

    /// Take ownership of the device event receiver for use by a background processor.
    pub fn take_device_event_receiver(&mut self) -> Option<mpsc::UnboundedReceiver<DeviceEvent>> {
        self.device_event_receiver.take()
    }

    /// Take the mic stream OUT for hot-swap (Phase 1) so the caller can tear it
    /// down without holding RECORDING_MANAGER. System audio continues uninterrupted.
    pub fn take_mic_stream_for_swap(&mut self) -> Option<super::stream::AudioStream> {
        self.stream_manager.take_mic_stream()
    }

    /// Set new mic stream and update device state after hot-swap (Phase 3).
    pub fn set_mic_stream_after_swap(
        &mut self,
        stream: super::stream::AudioStream,
        device: Arc<AudioDevice>,
        system_name: Option<String>,
    ) {
        self.stream_manager.set_mic_stream(stream);
        // Notify the device monitor so it tracks the new mic (and updated
        // system output) instead of the dead ones (M8 fix). BT devices like
        // AirPods are often both mic AND speaker — when they disconnect, both
        // monitor entries go stale. `system_name` is the *current* default
        // output, resolved lock-free in Phase 2 by the caller (do_mic_swap)
        // so a CoreAudio stall can't happen under the RECORDING_MANAGER lock;
        // `state.get_system_device()` still holds the pre-swap
        // reference (e.g. AirPods) because we don't mutate system_device
        // during a mic-only hot-swap, which would leave the monitor tracking
        // the dead name and spamming "missing for N checks".
        if let Some(ref monitor) = self.device_monitor {
            monitor.notify_mic_swapped(device.name.clone(), system_name);
        }
        self.state.set_microphone_device(device);
    }

    /// Take the system audio stream OUT for hot-swap so the caller can tear it
    /// down without holding RECORDING_MANAGER. The microphone keeps running.
    pub fn take_system_stream_for_swap(&mut self) -> Option<super::stream::AudioStream> {
        self.stream_manager.take_system_stream()
    }

    /// Set new system audio stream and update device state after hot-swap.
    pub fn set_system_stream_after_swap(
        &mut self,
        stream: super::stream::AudioStream,
        device: Arc<AudioDevice>,
    ) {
        self.stream_manager.set_system_stream(stream);
        // Keep the device monitor tracking the live devices (see
        // set_mic_stream_after_swap).
        if let (Some(monitor), Some(mic)) = (self.device_monitor.as_ref(), self.state.get_microphone_device()) {
            monitor.notify_mic_swapped(mic.name.clone(), Some(device.name.clone()));
        }
        self.state.set_system_device(device);
    }

    /// Get reference to recording state for external access
    pub fn get_state(&self) -> &Arc<RecordingState> {
        &self.state
    }
}

impl Default for RecordingManager {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for RecordingManager {
    fn drop(&mut self) {
        // Note: Can't call async cleanup in Drop, but streams have their own Drop implementations
        self.state.cleanup();
    }
}

#[cfg(all(test, target_os = "macos"))]
mod wake_tests {
    use super::*;

    #[test]
    fn wake_fails_gracefully_for_unknown_device() {
        // Nonexistent device name fails at the `find` step — no audio hardware needed.
        // Asserts the wake is non-fatal (returns Err, does not panic).
        assert!(wake_audio_connection_sync("__no_such_device__").is_err());
    }
}
