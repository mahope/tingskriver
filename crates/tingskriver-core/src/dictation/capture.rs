//! Microphone capture for dictation.
//!
//! Opens its own shared-mode input stream on the default microphone, so a
//! meeting recording that is running at the same time keeps running. While
//! dictating, the meeting pipeline silences its microphone track (see
//! `is_dictating`), so dictation speech stays out of the meeting.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use super::logic::audio;

pub struct Recorder {
    stop_tx: mpsc::Sender<()>,
    thread: Option<JoinHandle<Vec<f32>>>,
    level: Arc<AtomicU32>,
    sample_rate: u32,
}

impl Recorder {
    /// Start recording. Returns once the stream runs (or failed to start).
    pub fn start() -> Result<Recorder, String> {
        let level = Arc::new(AtomicU32::new(0));
        let (stop_tx, stop_rx) = mpsc::channel::<()>();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<u32, String>>();
        let level_for_thread = level.clone();

        let thread = std::thread::Builder::new()
            .name("dictation-capture".into())
            .spawn(move || {
                let buffer = Arc::new(Mutex::new(Vec::<f32>::with_capacity(48_000 * 30)));
                let stream = match open_stream(buffer.clone(), level_for_thread) {
                    Ok((stream, rate)) => {
                        let _ = ready_tx.send(Ok(rate));
                        stream
                    }
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                        return Vec::new();
                    }
                };
                let _ = stop_rx.recv();
                drop(stream);
                let samples = std::mem::take(&mut *buffer.lock().unwrap_or_else(|p| p.into_inner()));
                samples
            })
            .map_err(|e| format!("could not start capture thread: {e}"))?;

        match ready_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(sample_rate)) => Ok(Recorder {
                stop_tx,
                thread: Some(thread),
                level,
                sample_rate,
            }),
            Ok(Err(e)) => Err(e),
            Err(_) => {
                let _ = stop_tx.send(());
                Err("Mikrofonen svarede ikke".to_string())
            }
        }
    }

    /// Current input level, 0.0..=1.0.
    pub fn level(&self) -> f32 {
        f32::from_bits(self.level.load(Ordering::Relaxed))
    }

    /// Stop and return mono samples at 16 kHz.
    pub fn stop(mut self) -> Vec<f32> {
        let _ = self.stop_tx.send(());
        let samples = self
            .thread
            .take()
            .and_then(|t| t.join().ok())
            .unwrap_or_default();
        audio::resample(&samples, self.sample_rate, audio::TARGET_RATE)
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        let _ = self.stop_tx.send(());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn open_stream(buffer: Arc<Mutex<Vec<f32>>>, level: Arc<AtomicU32>) -> Result<(cpal::Stream, u32), String> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or_else(|| "Ingen mikrofon fundet".to_string())?;
    let config = device
        .default_input_config()
        .map_err(|e| format!("Mikrofonen kan ikke bruges: {e}"))?;
    let channels = config.channels() as usize;
    let sample_rate = config.sample_rate().0;
    let sample_format = config.sample_format();
    let stream_config: cpal::StreamConfig = config.into();

    let push = move |mono: Vec<f32>| {
        level.store(audio::level(&mono).to_bits(), Ordering::Relaxed);
        if let Ok(mut buf) = buffer.lock() {
            buf.extend_from_slice(&mono);
        }
    };
    let on_error = |e: cpal::StreamError| log::warn!("Dictation microphone stream error: {e}");

    let stream = match sample_format {
        cpal::SampleFormat::F32 => device.build_input_stream(
            &stream_config,
            move |data: &[f32], _: &cpal::InputCallbackInfo| push(audio::downmix(data, channels)),
            on_error,
            None,
        ),
        cpal::SampleFormat::I16 => device.build_input_stream(
            &stream_config,
            move |data: &[i16], _: &cpal::InputCallbackInfo| {
                let f: Vec<f32> = data.iter().map(|&s| s as f32 / i16::MAX as f32).collect();
                push(audio::downmix(&f, channels))
            },
            on_error,
            None,
        ),
        cpal::SampleFormat::I32 => device.build_input_stream(
            &stream_config,
            move |data: &[i32], _: &cpal::InputCallbackInfo| {
                let f: Vec<f32> = data.iter().map(|&s| s as f32 / i32::MAX as f32).collect();
                push(audio::downmix(&f, channels))
            },
            on_error,
            None,
        ),
        cpal::SampleFormat::U16 => device.build_input_stream(
            &stream_config,
            move |data: &[u16], _: &cpal::InputCallbackInfo| {
                let f: Vec<f32> = data.iter().map(|&s| (s as f32 - 32768.0) / 32768.0).collect();
                push(audio::downmix(&f, channels))
            },
            on_error,
            None,
        ),
        other => return Err(format!("Mikrofonens lydformat understøttes ikke: {other:?}")),
    }
    .map_err(|e| format!("Kunne ikke åbne mikrofonen: {e}"))?;

    stream.play().map_err(|e| format!("Kunne ikke starte mikrofonen: {e}"))?;
    Ok((stream, sample_rate))
}
