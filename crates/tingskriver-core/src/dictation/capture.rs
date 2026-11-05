//! Microphone capture for dictation.
//!
//! Opens its own shared-mode input stream on the microphone chosen for
//! dictation (the Windows default when none is chosen), so a meeting
//! recording that is running at the same time keeps running and meetings keep
//! their own device. While dictating, the meeting pipeline silences its
//! microphone track (see `is_dictating`), so dictation speech stays out of the
//! meeting.
//!
//! Two modes:
//! - on demand (default): the stream opens at the shortcut and closes at the
//!   stop, so Windows' microphone indicator is only on while dictating;
//! - keep open (setting): one stream stays open between dictations and only
//!   collects samples while a dictation runs. It is rebuilt at the next start
//!   if it died or the microphone or channel changed.
//!
//! A recording is *ready* when the first non-empty buffer has arrived, which
//! is what the pill, the start sound and `start_ms` wait for.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use super::logic::audio::{self, Channel};

/// What dictation records from.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CaptureOptions {
    /// Name of the input device; empty is the Windows default.
    pub microphone: String,
    pub channel: Channel,
    /// Keep the stream open between dictations.
    pub keep_open: bool,
}

/// Samples and signals shared between the audio callback and the recorder.
struct Shared {
    /// Samples are only collected while armed.
    armed: AtomicBool,
    buffer: Mutex<Vec<f32>>,
    level: AtomicU32,
    got_audio: AtomicBool,
    first_audio_at: Mutex<Option<Instant>>,
    /// The stream reported an error and must be rebuilt.
    dead: AtomicBool,
}

impl Shared {
    fn new(armed: bool) -> Arc<Shared> {
        Arc::new(Shared {
            armed: AtomicBool::new(armed),
            buffer: Mutex::new(Vec::with_capacity(48_000 * 30)),
            level: AtomicU32::new(0),
            got_audio: AtomicBool::new(false),
            first_audio_at: Mutex::new(None),
            dead: AtomicBool::new(false),
        })
    }

    fn push(&self, mono: Vec<f32>) {
        if !self.armed.load(Ordering::Acquire) || mono.is_empty() {
            return;
        }
        self.level
            .store(audio::level(&mono).to_bits(), Ordering::Relaxed);
        if let Ok(mut buf) = self.buffer.lock() {
            buf.extend_from_slice(&mono);
        }
        if !self.got_audio.swap(true, Ordering::AcqRel) {
            if let Ok(mut at) = self.first_audio_at.lock() {
                *at = Some(Instant::now());
            }
        }
    }

    fn arm(&self) {
        if let Ok(mut buf) = self.buffer.lock() {
            buf.clear();
        }
        self.level.store(0, Ordering::Relaxed);
        self.got_audio.store(false, Ordering::Release);
        if let Ok(mut at) = self.first_audio_at.lock() {
            *at = None;
        }
        self.armed.store(true, Ordering::Release);
    }

    fn take(&self) -> Vec<f32> {
        self.armed.store(false, Ordering::Release);
        self.level.store(0, Ordering::Relaxed);
        std::mem::take(&mut *self.buffer.lock().unwrap_or_else(|p| p.into_inner()))
    }
}

/// An open input stream on its own thread (the stream is not `Send`).
struct StreamHandle {
    stop_tx: mpsc::Sender<()>,
    thread: Option<JoinHandle<()>>,
    shared: Arc<Shared>,
    sample_rate: u32,
}

impl StreamHandle {
    fn open(options: &CaptureOptions, armed: bool) -> Result<StreamHandle, String> {
        let shared = Shared::new(armed);
        let (stop_tx, stop_rx) = mpsc::channel::<()>();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<u32, String>>();
        let for_thread = shared.clone();
        let options = options.clone();
        let thread = std::thread::Builder::new()
            .name("dictation-capture".into())
            .spawn(move || {
                let stream = match open_stream(&options, for_thread) {
                    Ok((stream, rate)) => {
                        let _ = ready_tx.send(Ok(rate));
                        stream
                    }
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                        return;
                    }
                };
                let _ = stop_rx.recv();
                drop(stream);
            })
            .map_err(|e| format!("could not start capture thread: {e}"))?;

        let mut handle = StreamHandle {
            stop_tx,
            thread: Some(thread),
            shared,
            sample_rate: 0,
        };
        match ready_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(rate)) => {
                handle.sample_rate = rate;
                Ok(handle)
            }
            Ok(Err(e)) => Err(e),
            Err(_) => Err("Mikrofonen svarede ikke".to_string()),
        }
    }
}

impl Drop for StreamHandle {
    fn drop(&mut self) {
        let _ = self.stop_tx.send(());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The stream kept open between dictations, with what it was opened for.
struct Kept {
    options: CaptureOptions,
    handle: StreamHandle,
}

static KEPT: Mutex<Option<Kept>> = Mutex::new(None);

/// Whether the settings want the stream kept open. Switching it off during a
/// dictation cannot close the stream then, so the recording closes it when it ends.
static KEEP_WANTED: AtomicBool = AtomicBool::new(false);

fn kept() -> std::sync::MutexGuard<'static, Option<Kept>> {
    KEPT.lock().unwrap_or_else(|p| p.into_inner())
}

/// Open the kept-open stream now (settings saved, app started), so the first
/// dictation does not pay for opening the microphone. Does nothing while a
/// dictation is using the stream; the next start rebuilds it if needed.
pub fn warm_up(options: &CaptureOptions) {
    if !options.keep_open || super::service::is_dictating() {
        return;
    }
    KEEP_WANTED.store(true, Ordering::Release);
    let mut slot = kept();
    if let Err(e) = ensure_kept(&mut slot, options) {
        log::warn!("Dictation microphone could not be kept open: {e}");
    }
}

/// Close the kept-open stream (keep open switched off), so the microphone
/// indicator goes out.
pub fn release() {
    KEEP_WANTED.store(false, Ordering::Release);
    if super::service::is_dictating() {
        return;
    }
    kept().take();
}

fn ensure_kept(slot: &mut Option<Kept>, options: &CaptureOptions) -> Result<(), String> {
    let usable = slot
        .as_ref()
        .is_some_and(|k| k.options == *options && !k.handle.shared.dead.load(Ordering::Acquire));
    if !usable {
        // Close the old stream first: two streams on one device can conflict.
        slot.take();
        let handle = StreamHandle::open(options, false)?;
        *slot = Some(Kept {
            options: options.clone(),
            handle,
        });
    }
    Ok(())
}

pub struct Recorder {
    shared: Arc<Shared>,
    /// The stream this recording owns (on demand). Kept-open streams live in [`KEPT`].
    owned: Option<StreamHandle>,
    sample_rate: u32,
    opened_at: Instant,
}

impl Recorder {
    /// Start recording. Returns once the stream runs (or failed to start);
    /// audio may take longer, see [`Recorder::first_audio_at`].
    pub fn start(options: &CaptureOptions) -> Result<Recorder, String> {
        if options.keep_open {
            KEEP_WANTED.store(true, Ordering::Release);
            let mut slot = kept();
            ensure_kept(&mut slot, options)?;
            let handle = &slot.as_ref().ok_or("Mikrofonen er ikke åben")?.handle;
            handle.shared.arm();
            return Ok(Recorder {
                shared: handle.shared.clone(),
                owned: None,
                sample_rate: handle.sample_rate,
                opened_at: Instant::now(),
            });
        }
        // Switched off while a kept-open stream still exists: close it, the
        // microphone indicator should only be on while dictating.
        kept().take();
        let handle = StreamHandle::open(options, true)?;
        Ok(Recorder {
            shared: handle.shared.clone(),
            sample_rate: handle.sample_rate,
            owned: Some(handle),
            opened_at: Instant::now(),
        })
    }

    /// Current input level, 0.0..=1.0.
    pub fn level(&self) -> f32 {
        f32::from_bits(self.shared.level.load(Ordering::Relaxed))
    }

    /// When the first non-empty buffer arrived; `None` until then.
    pub fn first_audio_at(&self) -> Option<Instant> {
        if !self.shared.got_audio.load(Ordering::Acquire) {
            return None;
        }
        *self.shared.first_audio_at.lock().ok()?
    }

    /// Time since the stream started.
    pub fn age(&self) -> Duration {
        self.opened_at.elapsed()
    }

    /// Stop and return mono samples at 16 kHz.
    pub fn stop(mut self) -> Vec<f32> {
        self.finish()
    }

    fn finish(&mut self) -> Vec<f32> {
        // Closing an owned stream also stops the callback, so the buffer is final.
        drop(self.owned.take());
        let samples = self.shared.take();
        audio::resample(&samples, self.sample_rate, audio::TARGET_RATE)
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        drop(self.owned.take());
        self.shared.armed.store(false, Ordering::Release);
        // Keep open was switched off during this dictation: close the stream now.
        if !KEEP_WANTED.load(Ordering::Acquire) {
            kept().take();
        }
    }
}

/// Names of the input devices, for the settings page.
pub fn input_device_names() -> Vec<String> {
    let host = cpal::default_host();
    let mut names: Vec<String> = host
        .input_devices()
        .map(|devices| devices.filter_map(|d| d.name().ok()).collect())
        .unwrap_or_default();
    names.sort();
    names.dedup();
    names
}

/// The chosen microphone, or the default when none is chosen or the chosen one
/// is not connected (dictation should still work; the log says which).
fn pick_device(host: &cpal::Host, name: &str) -> Result<cpal::Device, String> {
    let name = name.trim();
    if !name.is_empty() {
        let found = host
            .input_devices()
            .ok()
            .and_then(|mut devices| devices.find(|d| d.name().is_ok_and(|n| n == name)));
        match found {
            Some(device) => return Ok(device),
            None => log::warn!("Dictation microphone not found, using the default"),
        }
    }
    host.default_input_device()
        .ok_or_else(|| "Ingen mikrofon fundet".to_string())
}

fn open_stream(
    options: &CaptureOptions,
    shared: Arc<Shared>,
) -> Result<(cpal::Stream, u32), String> {
    let host = cpal::default_host();
    let device = pick_device(&host, &options.microphone)?;
    let config = device
        .default_input_config()
        .map_err(|e| format!("Mikrofonen kan ikke bruges: {e}"))?;
    let channels = config.channels() as usize;
    let channel = options.channel;
    let sample_rate = config.sample_rate().0;
    let sample_format = config.sample_format();
    let stream_config: cpal::StreamConfig = config.into();

    let for_error = shared.clone();
    let push = move |mono: Vec<f32>| shared.push(mono);
    let on_error = move |e: cpal::StreamError| {
        log::warn!("Dictation microphone stream error: {e}");
        for_error.dead.store(true, Ordering::Release);
    };

    let stream = match sample_format {
        cpal::SampleFormat::F32 => device.build_input_stream(
            &stream_config,
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                push(audio::pick_channel(data, channels, channel))
            },
            on_error,
            None,
        ),
        cpal::SampleFormat::I16 => device.build_input_stream(
            &stream_config,
            move |data: &[i16], _: &cpal::InputCallbackInfo| {
                let f: Vec<f32> = data.iter().map(|&s| s as f32 / i16::MAX as f32).collect();
                push(audio::pick_channel(&f, channels, channel))
            },
            on_error,
            None,
        ),
        cpal::SampleFormat::I32 => device.build_input_stream(
            &stream_config,
            move |data: &[i32], _: &cpal::InputCallbackInfo| {
                let f: Vec<f32> = data.iter().map(|&s| s as f32 / i32::MAX as f32).collect();
                push(audio::pick_channel(&f, channels, channel))
            },
            on_error,
            None,
        ),
        cpal::SampleFormat::U16 => device.build_input_stream(
            &stream_config,
            move |data: &[u16], _: &cpal::InputCallbackInfo| {
                let f: Vec<f32> = data
                    .iter()
                    .map(|&s| (s as f32 - 32768.0) / 32768.0)
                    .collect();
                push(audio::pick_channel(&f, channels, channel))
            },
            on_error,
            None,
        ),
        other => {
            return Err(format!(
                "Mikrofonens lydformat understøttes ikke: {other:?}"
            ))
        }
    }
    .map_err(|e| format!("Kunne ikke åbne mikrofonen: {e}"))?;

    stream
        .play()
        .map_err(|e| format!("Kunne ikke starte mikrofonen: {e}"))?;
    Ok((stream, sample_rate))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_is_collected_until_armed_and_ready_waits_for_the_first_buffer() {
        let shared = Shared::new(false);
        shared.push(vec![0.5; 100]);
        assert!(shared.take().is_empty());
        assert!(!shared.got_audio.load(Ordering::Acquire));

        shared.arm();
        shared.push(Vec::new());
        assert!(
            !shared.got_audio.load(Ordering::Acquire),
            "an empty buffer is not audio"
        );
        shared.push(vec![0.5; 100]);
        shared.push(vec![0.5; 100]);
        assert!(shared.got_audio.load(Ordering::Acquire));
        assert!(shared.first_audio_at.lock().unwrap().is_some());
        assert_eq!(shared.take().len(), 200);

        // Arming again forgets the previous dictation's audio.
        shared.arm();
        assert!(!shared.got_audio.load(Ordering::Acquire));
        assert!(shared.first_audio_at.lock().unwrap().is_none());
        assert!(shared.take().is_empty());
    }
}
