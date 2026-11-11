use anyhow::Result;
use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{Device, Stream, SupportedStreamConfig};
use log::{error, info, warn};
use std::sync::Arc;

use super::capture::{get_current_backend, AudioCaptureBackend};
use super::devices::{get_device_and_config, AudioDevice};
use super::pipeline::AudioCapture;
use super::recording_state::{DeviceType, RecordingState};
use super::recording_writer::StorageSender;

#[cfg(target_os = "macos")]
use super::capture::CoreAudioCapture;

/// Stream backend implementation
pub enum StreamBackend {
    /// CPAL-based stream (ScreenCaptureKit or default)
    Cpal(Stream),
    /// Core Audio direct implementation (macOS only)
    #[cfg(target_os = "macos")]
    CoreAudio {
        task: Option<tokio::task::JoinHandle<()>>,
    },
}

// SAFETY: While Stream doesn't implement Send, we ensure it's only accessed
// from the same thread context by using spawn_blocking for operations that cross thread boundaries
unsafe impl Send for StreamBackend {}

struct CaptureWorker {
    stopping: Arc<std::sync::atomic::AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl CaptureWorker {
    fn spawn<T: Copy + Default + Send + 'static>(
        capture: AudioCapture,
        sample_rate: u32,
        channels: usize,
        convert: fn(T) -> f32,
    ) -> Result<(
        super::callback_queue::Input<T>,
        ringbuf::HeapProd<cpal::StreamError>,
        Self,
    )> {
        use ringbuf::{
            traits::{Consumer, Split},
            HeapRb,
        };
        use std::sync::atomic::{AtomicBool, Ordering};
        // Half a second of bounded slack; a stalled worker never blocks CPAL.
        let capacity = sample_rate as usize * channels / 2;
        let (input, mut output) = super::callback_queue::queue::<T>(capacity, channels);
        let (errors, mut error_output) = HeapRb::<cpal::StreamError>::new(16).split();
        let stopping = Arc::new(AtomicBool::new(false));
        let stop = stopping.clone();
        let thread = std::thread::Builder::new()
            .name("audio-dsp".into())
            .spawn(move || {
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let size = capacity;
                    let mut raw = vec![T::default(); size];
                    let mut converted = vec![0.0; size];
                    let mut last_stamp = super::callback_queue::CaptureStamp::default();
                    loop {
                        while let Some(error) = error_output.try_pop() {
                            capture.handle_stream_error(error);
                        }
                        if let Some((len, stamp, lost)) = output.pop_into(&mut raw) {
                            if lost > 0 {
                                let mut gap_end = stamp;
                                gap_end.position = stamp.position.saturating_sub(
                                    std::time::Duration::from_secs_f64(
                                        len as f64 / channels as f64 / sample_rate as f64,
                                    ),
                                );
                                capture.process_capture_loss(lost, gap_end);
                            }
                            last_stamp = stamp;
                            for (dst, src) in converted[..len].iter_mut().zip(&raw[..len]) {
                                *dst = convert(*src);
                            }
                            capture.process_audio_data_at(&converted[..len], Some(stamp));
                        } else if stop.load(Ordering::Acquire) {
                            break;
                        } else {
                            std::thread::sleep(std::time::Duration::from_millis(2));
                        }
                    }
                    let trailing = output.take_trailing_loss();
                    if trailing > 0 {
                        last_stamp.position += std::time::Duration::from_secs_f64(
                            trailing as f64 / channels as f64 / sample_rate as f64,
                        );
                        capture.process_capture_loss(trailing, last_stamp);
                    }
                    let dropped = output.dropped_samples();
                    if dropped != 0 {
                        warn!(
                            "Audio callback queue for {} dropped {} samples",
                            capture.device_name(),
                            dropped
                        );
                    }
                    #[cfg(debug_assertions)]
                    info!(
                        "Audio callback p99 <= {} us (debug histogram, {} callbacks, device {})",
                        output.callback_p99_us(),
                        output.callback_count(),
                        capture.device_name()
                    );
                    capture.flush_tail();
                }));
                if outcome.is_err() {
                    error!("Audio DSP worker panicked; stopping recording");
                    capture.worker_failed();
                }
            })?;
        Ok((
            input,
            errors,
            CaptureWorker {
                stopping,
                thread: Some(thread),
            },
        ))
    }
}

impl Drop for CaptureWorker {
    fn drop(&mut self) {
        self.stopping
            .store(true, std::sync::atomic::Ordering::Release);
        if let Some(thread) = self.thread.take() {
            if thread.join().is_err() {
                error!("Audio DSP worker panicked");
            }
        }
    }
}

/// Simplified audio stream wrapper with multi-backend support
pub struct AudioStream {
    #[cfg(target_os = "windows")]
    pub(super) dropped_generation_chunks: Arc<std::sync::atomic::AtomicU64>,
    system_generation: Option<(Arc<RecordingState>, u64)>,
    device: Arc<AudioDevice>,
    backend: StreamBackend,
    /// Kept to flush the resampler tail after the callbacks stopped.
    capture: Option<AudioCapture>,
    worker: Option<CaptureWorker>,
}

// SAFETY: AudioStream contains StreamBackend which we've marked as Send
unsafe impl Send for AudioStream {}

impl AudioStream {
    /// Create a new audio stream for the given device
    pub async fn create(
        device: Arc<AudioDevice>,
        state: Arc<RecordingState>,
        device_type: DeviceType,
        recording_sender: Option<StorageSender>,
    ) -> Result<Self> {
        // Get current backend from global config
        let backend_type = get_current_backend();
        Self::create_with_backend(device, state, device_type, recording_sender, backend_type).await
    }

    /// Create a new audio stream with explicit backend selection
    pub async fn create_with_backend(
        device: Arc<AudioDevice>,
        state: Arc<RecordingState>,
        device_type: DeviceType,
        recording_sender: Option<StorageSender>,
        backend_type: AudioCaptureBackend,
    ) -> Result<Self> {
        info!(
            "🎵 Stream: Creating audio stream for device: {} with backend: {:?}, device_type: {:?}",
            device.name, backend_type, device_type
        );

        // For system audio devices, use the selected backend
        // For microphone devices, always use CPAL
        #[cfg(target_os = "macos")]
        let use_core_audio =
            device_type == DeviceType::System && backend_type == AudioCaptureBackend::CoreAudio;

        #[cfg(not(target_os = "macos"))]
        let use_core_audio = false;

        #[cfg(target_os = "macos")]
        info!(
            "🎵 Stream: use_core_audio = {}, device_type == System: {}, backend == CoreAudio: {}",
            use_core_audio,
            device_type == DeviceType::System,
            backend_type == AudioCaptureBackend::CoreAudio
        );

        #[cfg(not(target_os = "macos"))]
        info!(
            "🎵 Stream: use_core_audio = {}, device_type == System: {}",
            use_core_audio,
            device_type == DeviceType::System
        );

        #[cfg(target_os = "macos")]
        if use_core_audio {
            info!("🎵 Stream: Using Core Audio backend (cidre) for system audio");
            return Self::create_core_audio_stream(device, state, device_type, recording_sender)
                .await;
        }

        // Default path: use CPAL
        #[cfg(target_os = "macos")]
        let backend_name = if backend_type == AudioCaptureBackend::ScreenCaptureKit {
            "ScreenCaptureKit"
        } else {
            "CPAL (default)"
        };

        #[cfg(not(target_os = "macos"))]
        let backend_name = "CPAL";

        info!(
            "🎵 Stream: Using CPAL backend ({}) for device: {}",
            backend_name, device.name
        );
        Self::create_cpal_stream(device, state, device_type, recording_sender).await
    }

    /// Create a CPAL-based stream (ScreenCaptureKit on macOS)
    async fn create_cpal_stream(
        device: Arc<AudioDevice>,
        state: Arc<RecordingState>,
        device_type: DeviceType,
        recording_sender: Option<StorageSender>,
    ) -> Result<Self> {
        info!("Creating CPAL stream for device: {}", device.name);

        // Get the underlying cpal device and config
        let (cpal_device, config) = get_device_and_config(&device).await?;

        info!(
            "Audio config - Sample rate: {}, Channels: {}, Format: {:?}",
            config.sample_rate().0,
            config.channels(),
            config.sample_format()
        );

        // Create audio capture processor
        let capture = AudioCapture::new(
            device.clone(),
            state.clone(),
            config.sample_rate().0,
            config.channels(),
            device_type,
            recording_sender,
        );

        // Build the appropriate stream based on sample format
        let (stream, worker) = Self::build_stream(&cpal_device, &config, capture.clone())?;

        // Start the stream
        stream.play()?;
        info!("CPAL stream started for device: {}", device.name);

        Ok(Self {
            #[cfg(target_os = "windows")]
            dropped_generation_chunks: capture.dropped_generation_chunks.clone(),
            system_generation: capture.system_generation.map(|id| (state.clone(), id)),
            device,
            backend: StreamBackend::Cpal(stream),
            capture: None,
            worker: Some(worker),
        })
    }

    /// Create a Core Audio stream (macOS only)
    #[cfg(target_os = "macos")]
    async fn create_core_audio_stream(
        device: Arc<AudioDevice>,
        state: Arc<RecordingState>,
        device_type: DeviceType,
        recording_sender: Option<StorageSender>,
    ) -> Result<Self> {
        info!(
            "🔊 Stream: Creating Core Audio stream for device: {}",
            device.name
        );

        // Create Core Audio capture
        info!("🔊 Stream: Calling CoreAudioCapture::new()...");
        let capture_impl = CoreAudioCapture::new().map_err(|e| {
            error!("❌ Stream: CoreAudioCapture::new() failed: {}", e);
            anyhow::anyhow!("Failed to create Core Audio capture: {}", e)
        })?;

        info!("✅ Stream: CoreAudioCapture created, calling stream()...");
        let core_stream = capture_impl.stream().map_err(|e| {
            error!("❌ Stream: capture_impl.stream() failed: {}", e);
            anyhow::anyhow!("Failed to create Core Audio stream: {}", e)
        })?;

        let sample_rate = core_stream.sample_rate();
        info!(
            "✅ Stream: Core Audio stream created with sample rate: {} Hz",
            sample_rate
        );

        // Create audio capture processor for pipeline integration
        // CRITICAL: Core Audio tap is MONO (with_mono_global_tap_excluding_processes)
        let capture = AudioCapture::new(
            device.clone(),
            state.clone(),
            sample_rate,
            1, // Core Audio tap is MONO (not stereo!)
            device_type,
            recording_sender,
        );

        // Spawn task to process Core Audio stream samples
        // The stream needs to be polled continuously to produce samples
        let device_name = device.name.clone();
        info!("🔊 Stream: Spawning tokio task to poll Core Audio stream...");
        let task = tokio::spawn({
            let capture = capture.clone();
            let mut stream = core_stream;

            async move {
                use futures_util::StreamExt;

                let mut buffer = Vec::new();
                let mut frame_count = 0;
                let frames_per_chunk = 1024; // Process in chunks of 1024 samples

                info!(
                    "✅ Stream: Core Audio processing task started for {}",
                    device_name
                );

                let mut _sample_count = 0u64;
                while let Some(sample) = stream.next().await {
                    _sample_count += 1;
                    // if _sample_count % 48000 == 0 {
                    //     info!("📊 Stream: Received {} samples from Core Audio stream", _sample_count);
                    // }

                    buffer.push(sample);
                    frame_count += 1;

                    // Process when we have enough samples
                    if frame_count >= frames_per_chunk {
                        capture.process_audio_data(&buffer);
                        buffer.clear();
                        frame_count = 0;
                    }
                }

                // Process any remaining samples
                if !buffer.is_empty() {
                    capture.process_audio_data(&buffer);
                }

                info!(
                    "⚠️ Stream: Core Audio processing task ended for {}",
                    device_name
                );
            }
        });

        info!(
            "✅ Stream: Core Audio stream fully initialized for device: {}",
            device.name
        );

        Ok(Self {
            #[cfg(target_os = "windows")]
            dropped_generation_chunks: capture.dropped_generation_chunks.clone(),
            system_generation: capture.system_generation.map(|id| (state.clone(), id)),
            device: device.clone(),
            backend: StreamBackend::CoreAudio { task: Some(task) },
            capture: Some(capture),
            worker: None,
        })
    }

    /// Build stream based on sample format. Conversion and DSP belong to
    /// the consumer thread, including for integer device formats.
    fn build_stream(
        device: &Device,
        config: &SupportedStreamConfig,
        capture: AudioCapture,
    ) -> Result<(Stream, CaptureWorker)> {
        match config.sample_format() {
            cpal::SampleFormat::F32 => Self::build_typed::<f32>(device, config, capture, |x| x),
            cpal::SampleFormat::I16 => {
                Self::build_typed::<i16>(device, config, capture, |x| x as f32 / i16::MAX as f32)
            }
            cpal::SampleFormat::I32 => {
                Self::build_typed::<i32>(device, config, capture, |x| x as f32 / i32::MAX as f32)
            }
            cpal::SampleFormat::I8 => {
                Self::build_typed::<i8>(device, config, capture, |x| x as f32 / i8::MAX as f32)
            }
            format => Err(anyhow::anyhow!("Unsupported sample format: {:?}", format)),
        }
    }

    fn build_typed<T: cpal::SizedSample + Copy + Default + Send + 'static>(
        device: &Device,
        config: &SupportedStreamConfig,
        capture: AudioCapture,
        convert: fn(T) -> f32,
    ) -> Result<(Stream, CaptureWorker)> {
        use ringbuf::traits::Producer;
        let frames = capture.clone();
        // Initialise the atomic frame clock before entering the callback.
        frames.note_callback_frames(false);
        let (mut input, mut errors, worker) = CaptureWorker::spawn(
            capture,
            config.sample_rate().0,
            config.channels() as usize,
            convert,
        )?;
        let stream = device.build_input_stream(
            &config.clone().into(),
            move |data: &[T], _: &cpal::InputCallbackInfo| {
                #[cfg(debug_assertions)]
                let started = std::time::Instant::now();
                frames.note_callback_frames(!data.is_empty());
                if let Some(stamp) = frames.callback_stamp() {
                    input.copy_from(data, stamp);
                }
                #[cfg(debug_assertions)]
                input.record_duration(started.elapsed());
            },
            move |error| {
                let _ = errors.try_push(error);
            },
            None,
        )?;
        Ok((stream, worker))
    }

    /// Get device info
    pub fn device(&self) -> &AudioDevice {
        &self.device
    }

    /// Stop the stream
    pub fn stop(self) -> Result<()> {
        info!("Stopping audio stream for device: {}", self.device.name);
        let AudioStream {
            device,
            backend,
            capture,
            worker,
            system_generation: _,
            #[cfg(target_os = "windows")]
                dropped_generation_chunks: _,
        } = self;

        match backend {
            StreamBackend::Cpal(stream) => {
                // CRITICAL: Pause the stream first to stop callbacks immediately
                // This ensures closures stop executing before we drop the stream,
                // allowing Arc references captured in callbacks to be released
                if let Err(e) = stream.pause() {
                    warn!("Failed to pause stream before drop: {}", e);
                }
                info!("Stream paused, now dropping to release callbacks");
                drop(stream);
            }
            #[cfg(target_os = "macos")]
            StreamBackend::CoreAudio { task } => {
                // Abort the processing task and wait briefly for cleanup
                if let Some(task_handle) = task {
                    info!("Aborting Core Audio task...");
                    task_handle.abort();
                    // Give the runtime a moment to clean up the aborted task
                    // This helps ensure Arc references in the closure are dropped
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    info!("Core Audio task aborted");
                }
            }
        }

        // Drop CPAL first, then drain and join DSP before the writer closes.
        drop(worker);

        // No more callbacks: the last samples in the resampler go to the track.
        if let Some(capture) = capture {
            capture.flush_tail();
        }

        // Explicitly drop self.device Arc reference
        drop(device);
        info!("Audio stream stopped and device reference dropped");
        Ok(())
    }
}

/// Audio stream manager for handling multiple streams
pub struct AudioStreamManager {
    microphone_stream: Option<AudioStream>,
    system_stream: Option<AudioStream>,
    state: Arc<RecordingState>,
}

// SAFETY: AudioStreamManager contains AudioStream which we've marked as Send
unsafe impl Send for AudioStreamManager {}

impl AudioStreamManager {
    pub fn new(state: Arc<RecordingState>) -> Self {
        Self {
            microphone_stream: None,
            system_stream: None,
            state,
        }
    }

    /// Start audio streams for the given devices
    pub async fn start_streams(
        &mut self,
        microphone_device: Option<Arc<AudioDevice>>,
        system_device: Option<Arc<AudioDevice>>,
        recording_sender: Option<StorageSender>,
    ) -> Result<()> {
        use super::capture::get_current_backend;
        let backend = get_current_backend();
        info!("🎙️ Starting audio streams with backend: {:?}", backend);

        // Start microphone stream
        if let Some(mic_device) = microphone_device {
            info!(
                "🎤 Creating microphone stream: {} (always uses CPAL)",
                mic_device.name
            );
            match AudioStream::create(
                mic_device.clone(),
                self.state.clone(),
                DeviceType::Microphone,
                recording_sender.clone(),
            )
            .await
            {
                Ok(stream) => {
                    self.state.set_microphone_device(mic_device);
                    self.microphone_stream = Some(stream);
                    info!("✅ Microphone stream created successfully");
                }
                Err(e) => {
                    error!("❌ Failed to create microphone stream: {}", e);
                    return Err(e);
                }
            }
        } else {
            info!("ℹ️ No microphone device specified, skipping microphone stream");
        }

        // Start system audio stream
        if let Some(sys_device) = system_device {
            info!(
                "🔊 Creating system audio stream: {} (backend: {:?})",
                sys_device.name, backend
            );
            match AudioStream::create(
                sys_device.clone(),
                self.state.clone(),
                DeviceType::System,
                recording_sender.clone(),
            )
            .await
            {
                Ok(stream) => {
                    self.state.set_system_device(sys_device);
                    self.set_system_stream(stream);
                    info!("✅ System audio stream created with {:?} backend", backend);
                }
                Err(e) => {
                    warn!("⚠️ Failed to create system audio stream: {}", e);
                    // Don't fail if only system audio fails
                }
            }
        } else {
            info!("ℹ️ No system device specified, skipping system audio stream");
        }

        // Ensure at least one stream was created
        if self.microphone_stream.is_none() && self.system_stream.is_none() {
            return Err(anyhow::anyhow!("No audio streams could be created"));
        }

        Ok(())
    }

    /// Stop all audio streams
    pub fn stop_streams(&mut self) -> Result<()> {
        info!("Stopping all audio streams");

        let mut errors = Vec::new();

        // Stop microphone stream
        if let Some(mic_stream) = self.microphone_stream.take() {
            if let Err(e) = mic_stream.stop() {
                error!("Failed to stop microphone stream: {}", e);
                errors.push(e);
            }
        }

        // Stop system stream
        if let Some(sys_stream) = self.system_stream.take() {
            if let Err(e) = sys_stream.stop() {
                error!("Failed to stop system stream: {}", e);
                errors.push(e);
            }
        }

        if !errors.is_empty() {
            Err(anyhow::anyhow!("Failed to stop some streams: {:?}", errors))
        } else {
            info!("All audio streams stopped successfully");
            Ok(())
        }
    }

    /// Take the microphone stream OUT of the manager, keeping system audio
    /// running. The caller stops/drops it OUTSIDE any lock — a cpal teardown
    /// of a dead BT device can stall and must not block a held mutex.
    pub fn take_mic_stream(&mut self) -> Option<AudioStream> {
        self.microphone_stream.take()
    }

    /// Set a new microphone stream (used after hot-swap creation).
    pub fn set_mic_stream(&mut self, stream: AudioStream) {
        self.microphone_stream = Some(stream);
    }

    /// Take the system audio stream OUT of the manager, keeping the
    /// microphone running (Windows default-device follower).
    pub fn take_system_stream(&mut self) -> Option<AudioStream> {
        if let Some((state, _)) = self
            .system_stream
            .as_ref()
            .and_then(|s| s.system_generation.as_ref())
        {
            state.activate_system_generation(0);
        }
        self.system_stream.take()
    }

    /// Set a new system audio stream (used after hot-swap creation).
    pub fn set_system_stream(&mut self, stream: AudioStream) {
        if let Some((state, id)) = &stream.system_generation {
            state.activate_system_generation(*id);
        }
        self.system_stream = Some(stream);
    }

    /// Get stream count
    pub fn active_stream_count(&self) -> usize {
        let mut count = 0;
        if self.microphone_stream.is_some() {
            count += 1;
        }
        if self.system_stream.is_some() {
            count += 1;
        }
        count
    }

    /// Check if any streams are active
    pub fn has_active_streams(&self) -> bool {
        self.microphone_stream.is_some() || self.system_stream.is_some()
    }
}

impl Drop for AudioStreamManager {
    fn drop(&mut self) {
        if let Err(e) = self.stop_streams() {
            error!("Error stopping streams during drop: {}", e);
        }
    }
}

#[cfg(test)]
#[allow(clippy::disallowed_methods)]
mod worker_tests {
    use super::super::{
        callback_queue::CaptureStamp,
        recording_writer::{spawn_writer, LogObserver},
    };
    use super::*;
    use std::time::Duration;

    #[test]
    fn callback_overflow_preserves_short_gap_and_reports_loss_in_manifest() {
        for rate in [48000, 16000] {
            for trailing in [false, true] {
                let dir = tempfile::tempdir().unwrap();
                let state = RecordingState::new();
                state.start_recording().unwrap();
                let (sender, writer) =
                    spawn_writer(dir.path().to_owned(), 48000, Arc::new(LogObserver)).unwrap();
                state.set_audio_sender(sender);
                let device = Arc::new(AudioDevice::new(
                    "Fake system audio".into(),
                    super::super::devices::DeviceType::Output,
                ));
                let capture =
                    AudioCapture::new(device, state.clone(), rate, 2, DeviceType::System, None);
                let (mut input, mut output) =
                    super::super::callback_queue::queue::<f32>(rate as usize, 2);
                let mut buffer = vec![0.0; rate as usize];
                let stamp = |seconds| CaptureStamp {
                    position: Duration::from_secs_f64(seconds),
                    muted: false,
                };
                assert!(input.copy_from(&vec![0.2; rate as usize], stamp(0.5)));
                assert!(!input.copy_from(&vec![0.4; rate as usize / 5], stamp(0.6)));
                let (n, at, lost) = output.pop_into(&mut buffer).unwrap();
                assert_eq!(lost, 0);
                capture.process_audio_data_at(&buffer[..n], Some(at));
                if trailing {
                    drop(input);
                    capture.process_capture_loss(output.take_trailing_loss(), stamp(0.6));
                } else {
                    assert!(input.copy_from(&vec![0.6; rate as usize / 5], stamp(0.7)));
                    let (n, at, lost) = output.pop_into(&mut buffer).unwrap();
                    capture.process_capture_loss(lost, stamp(0.6));
                    capture.process_audio_data_at(&buffer[..n], Some(at));
                    drop(input);
                }
                capture.flush_tail();
                state.close_audio_sender();
                let summary = writer.join().unwrap();
                assert_eq!(summary.lost_samples(), 4800);
                assert!(summary.storage_error.is_some());
                let track = &summary.tracks[0].1;
                assert_eq!(
                    track.offset_samples + track.written_samples,
                    if trailing { 28800 } else { 33600 }
                );
                let manifest: serde_json::Value =
                    serde_json::from_slice(&std::fs::read(dir.path().join("tracks.json")).unwrap())
                        .unwrap();
                assert_eq!(manifest["dropped_samples"], 4800);
                if rate == 48000 {
                    let wav = std::fs::read(dir.path().join("system_000.wav")).unwrap();
                    let samples: Vec<i16> = wav[44..]
                        .chunks_exact(2)
                        .map(|b| i16::from_le_bytes([b[0], b[1]]))
                        .collect();
                    assert!(samples[..24000].iter().all(|&s| s > 0));
                    assert!(samples[24000..28800].iter().all(|&s| s == 0));
                    if !trailing {
                        assert!(samples[28800..].iter().all(|&s| s > 0));
                    }
                }
            }
        }
    }

    #[test]
    fn worker_panic_stops_recording_instead_of_silently_filling_the_queue() {
        let state = RecordingState::new();
        state.start_recording().unwrap();
        let device = Arc::new(AudioDevice::new(
            "Fake device".into(),
            super::super::devices::DeviceType::Output,
        ));
        let capture = AudioCapture::new(device, state.clone(), 48000, 1, DeviceType::System, None);
        let (mut input, errors, worker) =
            CaptureWorker::spawn(capture, 48000, 1, |_: f32| -> f32 {
                panic!("fake DSP failure")
            })
            .unwrap();
        input.copy_from(&[0.1], CaptureStamp::default());
        drop(input);
        drop(errors);
        drop(worker);
        assert!(!state.is_recording());
        assert!(matches!(
            state.get_last_error(),
            Some(super::super::recording_state::AudioError::ChannelClosed)
        ));
    }

    #[test]
    fn worker_drains_capture_buffers_and_resampler_tail_before_writer_closes() {
        for rate in [48000, 16000] {
            let dir = tempfile::tempdir().unwrap();
            let state = RecordingState::new();
            state.start_recording().unwrap();
            let (sender, writer) =
                spawn_writer(dir.path().to_owned(), 48000, Arc::new(LogObserver)).unwrap();
            state.set_audio_sender(sender);
            let device = Arc::new(AudioDevice::new(
                "Fake system audio".into(),
                super::super::devices::DeviceType::Output,
            ));
            let capture =
                AudioCapture::new(device, state.clone(), rate, 1, DeviceType::System, None);
            let (mut input, errors, worker) =
                CaptureWorker::spawn(capture, rate, 1, |x: i16| x as f32 / i16::MAX as f32)
                    .unwrap();
            let samples = [1000i16; 1000];
            assert!(input.copy_from(
                &samples,
                CaptureStamp {
                    position: Duration::from_secs_f64(1000.0 / rate as f64),
                    muted: false,
                }
            ));
            // Already captured audio must survive a pause while DSP is queued.
            state.pause_recording().unwrap();
            drop(input);
            drop(errors);
            drop(worker);
            state.close_audio_sender();
            let summary = writer.join().unwrap();
            assert_eq!(summary.tracks.len(), 1);
            let track = &summary.tracks[0].1;
            // Rubato has an initial filter delay, represented by the track's
            // offset. The last sample must still land at the capture time.
            assert_eq!(
                track.offset_samples + track.written_samples,
                1000 * 48000 / rate as u64
            );
            assert!(track.written_samples > 0);
            assert_eq!(summary.lost_samples(), 0);
        }
    }
}
