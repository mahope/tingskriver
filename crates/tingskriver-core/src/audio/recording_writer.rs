//! The recording's disk writer: capture -> bounded channel -> one thread that
//! writes the microphone and system tracks with [`TrackWriter`].
//!
//! No VAD, no transcription model and no ffmpeg run here, so a crash in any of
//! them cannot stop the audio from reaching the disk. The capture side never
//! blocks: [`StorageSender::send`] uses `try_send`, a full queue is counted and
//! reported by the writer thread, and more than [`BACKLOG_WARNING`] of queued
//! audio raises a warning. Stop drops every sender; the thread drains the
//! channel to the last chunk, commits and returns a [`WriterSummary`].
//!
//! `tracks.json` next to the segments records each track's offset on the
//! shared recording clock, so the tracks can be mixed after a crash too.

#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crossbeam::channel::{bounded, Receiver, RecvTimeoutError, Sender, TrySendError};
use serde::{Deserialize, Serialize};

use super::track_writer::{
    StorageErrorKind, StorageEvent, TrackKind, TrackStorage, TrackSummary, TrackWriter,
    WavSegmentStorage,
};

/// Chunks the channel holds. Capture delivers roughly 100 chunks per second
/// per device, so this is about a minute of audio (23 MB of f32 at most).
pub const CHANNEL_CAPACITY: usize = 12_000;

/// Queued audio above this raises a backlog warning.
pub const BACKLOG_WARNING: Duration = Duration::from_secs(5);

/// Repeat interval of the backlog and queue-full warnings.
pub const WARNING_REPEAT: Duration = Duration::from_secs(10);

/// How often the thread wakes up without audio to commit and retry.
const IDLE_TICK: Duration = Duration::from_millis(100);

/// The queue-full error clears once no chunk has been dropped for this long.
const QUEUE_RECOVERY: Duration = Duration::from_secs(2);

pub const MANIFEST_FILE: &str = "tracks.json";

/// User message when the writer cannot keep up and capture had to drop audio.
pub const QUEUE_FULL_MESSAGE: &str = "Lyden gemmes ikke: lageret kan ikke følge med";
/// User message when the writer thread died.
pub const WRITER_STOPPED_MESSAGE: &str = "Lyden gemmes ikke: lagringen stoppede uventet";

/// Told about storage state changes, from the writer thread.
pub trait WriterObserver: Send + Sync + 'static {
    fn storage_failed(&self, message: &str, kind: StorageErrorKind);
    fn storage_recovered(&self);
    fn backlog(&self, queued_seconds: f64);
}

/// Observer that only logs (tests, callers without a UI).
pub struct LogObserver;

impl WriterObserver for LogObserver {
    fn storage_failed(&self, message: &str, _kind: StorageErrorKind) {
        log::error!("{message}");
    }
    fn storage_recovered(&self) {
        log::info!("Storage recovered");
    }
    fn backlog(&self, queued_seconds: f64) {
        log::warn!("{queued_seconds:.1}s of audio waiting for the disk");
    }
}

struct WriterMessage {
    kind: TrackKind,
    samples: Vec<f32>,
    end_position: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageSendError {
    /// The queue is full; the chunk was dropped and will be reported.
    Full,
    /// The writer is gone (stopped or crashed).
    Closed,
}

/// Capture-side handle. Cheap to clone; the writer stops when every clone is
/// dropped.
#[derive(Clone)]
pub struct StorageSender {
    tx: Sender<WriterMessage>,
    queued_samples: Arc<AtomicU64>,
    dropped_samples: Arc<AtomicU64>,
    sample_rate: u32,
}

impl StorageSender {
    /// Queue a chunk without blocking. `end_seconds` is the position of the
    /// chunk's last sample on the recording clock (pauses excluded).
    pub fn send(
        &self,
        kind: TrackKind,
        samples: Vec<f32>,
        end_seconds: Option<f64>,
    ) -> Result<(), StorageSendError> {
        let len = samples.len() as u64;
        let end_position =
            end_seconds.map(|s| (s.max(0.0) * self.sample_rate as f64).round() as u64);
        self.queued_samples.fetch_add(len, Ordering::Relaxed);
        match self.tx.try_send(WriterMessage {
            kind,
            samples,
            end_position,
        }) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => {
                self.queued_samples.fetch_sub(len, Ordering::Relaxed);
                self.dropped_samples.fetch_add(len, Ordering::Relaxed);
                Err(StorageSendError::Full)
            }
            Err(TrySendError::Disconnected(_)) => {
                self.queued_samples.fetch_sub(len, Ordering::Relaxed);
                Err(StorageSendError::Closed)
            }
        }
    }

    /// Audio queued for the writer, in seconds (both tracks together).
    pub fn queued_seconds(&self) -> f64 {
        self.queued_samples.load(Ordering::Relaxed) as f64 / self.sample_rate as f64
    }
}

/// `tracks.json`: what recovery needs to put the tracks back together.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TrackManifest {
    pub version: u32,
    pub sample_rate: u32,
    pub tracks: Vec<ManifestTrack>,
    /// True once the writer finished normally (stop, not a crash).
    #[serde(default)]
    pub complete: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ManifestTrack {
    pub kind: TrackKind,
    pub offset_samples: u64,
    /// Samples on disk at stop (absent after a crash).
    #[serde(default)]
    pub recorded_samples: Option<u64>,
    #[serde(default)]
    pub lost_samples: u64,
    #[serde(default)]
    pub segments: Vec<String>,
}

impl TrackManifest {
    fn track_mut(&mut self, kind: TrackKind) -> &mut ManifestTrack {
        if let Some(i) = self.tracks.iter().position(|t| t.kind == kind) {
            return &mut self.tracks[i];
        }
        self.tracks.push(ManifestTrack {
            kind,
            offset_samples: 0,
            recorded_samples: None,
            lost_samples: 0,
            segments: Vec::new(),
        });
        let last = self.tracks.len() - 1;
        &mut self.tracks[last]
    }
}

pub fn read_manifest(dir: &Path) -> io::Result<TrackManifest> {
    let text = std::fs::read_to_string(dir.join(MANIFEST_FILE))?;
    serde_json::from_str(&text).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

pub fn write_manifest(dir: &Path, manifest: &TrackManifest) -> io::Result<()> {
    let json = serde_json::to_string_pretty(manifest).map_err(io::Error::other)?;
    let tmp = dir.join(".tracks.json.tmp");
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, dir.join(MANIFEST_FILE))
}

/// What the writer left on disk.
#[derive(Debug, Clone, Default)]
pub struct WriterSummary {
    pub sample_rate: u32,
    pub tracks: Vec<(TrackKind, TrackSummary)>,
    /// Samples capture dropped because the queue was full.
    pub dropped_samples: u64,
    /// Storage error still active at stop.
    pub storage_error: Option<String>,
    /// The writer thread panicked; the files hold what was committed before.
    pub crashed: bool,
}

impl WriterSummary {
    pub fn lost_samples(&self) -> u64 {
        self.tracks.iter().map(|(_, t)| t.lost_samples).sum::<u64>() + self.dropped_samples
    }

    /// Length of the recording on the shared clock (longest track end).
    pub fn duration_seconds(&self) -> f64 {
        let end = self
            .tracks
            .iter()
            .map(|(_, t)| t.offset_samples + t.written_samples)
            .max()
            .unwrap_or(0);
        if self.sample_rate == 0 {
            return 0.0;
        }
        end as f64 / self.sample_rate as f64
    }
}

/// Owner side of the writer thread.
pub struct WriterHandle {
    thread: Option<JoinHandle<WriterSummary>>,
    status: Arc<Mutex<Option<String>>>,
}

impl WriterHandle {
    /// Wait until every sender is dropped and the channel is drained. Blocks;
    /// call from `spawn_blocking` in async code.
    pub fn join(mut self) -> Result<WriterSummary, String> {
        match self.thread.take() {
            Some(thread) => thread
                .join()
                .map_err(|_| WRITER_STOPPED_MESSAGE.to_string()),
            None => Err("writer already joined".to_string()),
        }
    }

    /// The thread has ended (normally only after stop).
    pub fn is_finished(&self) -> bool {
        self.thread.as_ref().map_or(true, |t| t.is_finished())
    }

    /// The storage error being shown to the user right now, if any.
    pub fn storage_error(&self) -> Option<String> {
        self.status
            .lock()
            .map(|s| s.clone())
            .unwrap_or_else(|e| e.into_inner().clone())
    }
}

/// Start the writer for a meeting's raw-track folder.
pub fn spawn_writer(
    dir: PathBuf,
    sample_rate: u32,
    observer: Arc<dyn WriterObserver>,
) -> io::Result<(StorageSender, WriterHandle)> {
    let storage_dir = dir.clone();
    spawn_writer_with(dir, sample_rate, observer, CHANNEL_CAPACITY, move |kind| {
        Box::new(WavSegmentStorage::new(
            storage_dir.clone(),
            kind,
            sample_rate,
        )) as Box<dyn TrackStorage>
    })
}

/// [`spawn_writer`] with an injectable storage (tests).
pub fn spawn_writer_with<F>(
    dir: PathBuf,
    sample_rate: u32,
    observer: Arc<dyn WriterObserver>,
    capacity: usize,
    make_storage: F,
) -> io::Result<(StorageSender, WriterHandle)>
where
    F: Fn(TrackKind) -> Box<dyn TrackStorage> + Send + 'static,
{
    let (tx, rx) = bounded(capacity);
    let queued = Arc::new(AtomicU64::new(0));
    let dropped = Arc::new(AtomicU64::new(0));
    let status = Arc::new(Mutex::new(None));
    let sender = StorageSender {
        tx,
        queued_samples: queued.clone(),
        dropped_samples: dropped.clone(),
        sample_rate,
    };
    let thread_status = status.clone();
    let thread = std::thread::Builder::new()
        .name("recording-writer".to_string())
        .spawn(move || {
            let crash_observer = observer.clone();
            let crash_status = thread_status.clone();
            let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                WriterLoop {
                    dir,
                    sample_rate,
                    observer,
                    rx,
                    queued,
                    dropped,
                    status: thread_status,
                    make_storage,
                    tracks: BTreeMap::new(),
                    manifest: TrackManifest {
                        version: 1,
                        sample_rate,
                        tracks: Vec::new(),
                        complete: false,
                    },
                    failing: false,
                    last_backlog_warning: None,
                    last_dropped: 0,
                    last_drop_warning: None,
                    queue_full_at: None,
                }
                .run()
            }));
            match run {
                Ok(summary) => summary,
                Err(_) => {
                    log::error!("Recording writer panicked");
                    set_status(&crash_status, Some(WRITER_STOPPED_MESSAGE.to_string()));
                    crash_observer.storage_failed(WRITER_STOPPED_MESSAGE, StorageErrorKind::Other);
                    WriterSummary {
                        sample_rate,
                        storage_error: Some(WRITER_STOPPED_MESSAGE.to_string()),
                        crashed: true,
                        ..WriterSummary::default()
                    }
                }
            }
        })?;
    Ok((
        sender,
        WriterHandle {
            thread: Some(thread),
            status,
        },
    ))
}

fn set_status(status: &Mutex<Option<String>>, value: Option<String>) {
    match status.lock() {
        Ok(mut s) => *s = value,
        Err(e) => *e.into_inner() = value,
    }
}

struct WriterLoop<F> {
    dir: PathBuf,
    sample_rate: u32,
    observer: Arc<dyn WriterObserver>,
    rx: Receiver<WriterMessage>,
    queued: Arc<AtomicU64>,
    dropped: Arc<AtomicU64>,
    status: Arc<Mutex<Option<String>>>,
    make_storage: F,
    tracks: BTreeMap<TrackKind, TrackWriter<Box<dyn TrackStorage>>>,
    manifest: TrackManifest,
    failing: bool,
    last_backlog_warning: Option<Instant>,
    last_dropped: u64,
    last_drop_warning: Option<Instant>,
    /// When a dropped chunk was last seen; `None` once the queue has recovered.
    queue_full_at: Option<Instant>,
}

impl<F> WriterLoop<F>
where
    F: Fn(TrackKind) -> Box<dyn TrackStorage>,
{
    fn run(mut self) -> WriterSummary {
        log::info!("Recording writer started in {}", self.dir.display());
        if let Err(e) = write_manifest(&self.dir, &self.manifest) {
            log::warn!("Could not write {MANIFEST_FILE}: {e}");
        }
        loop {
            match self.rx.recv_timeout(IDLE_TICK) {
                Ok(message) => self.accept(message),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            let now = Instant::now();
            let events: Vec<StorageEvent> = self
                .tracks
                .values_mut()
                .filter_map(|w| w.tick(now))
                .collect();
            for event in events {
                self.handle(event);
            }
            self.check_queue(now);
        }
        self.finish()
    }

    fn accept(&mut self, message: WriterMessage) {
        self.queued
            .fetch_sub(message.samples.len() as u64, Ordering::Relaxed);
        let sample_rate = self.sample_rate;
        let make_storage = &self.make_storage;
        let writer = self
            .tracks
            .entry(message.kind)
            .or_insert_with(|| TrackWriter::new(make_storage(message.kind), sample_rate));
        if let Some(offset) = writer.push(&message.samples, message.end_position) {
            log::info!(
                "{} track starts at {:.3}s on the recording clock",
                message.kind.file_stem(),
                offset as f64 / sample_rate as f64
            );
            self.manifest.track_mut(message.kind).offset_samples = offset;
            if let Err(e) = write_manifest(&self.dir, &self.manifest) {
                log::warn!("Could not write {MANIFEST_FILE}: {e}");
            }
        }
    }

    fn handle(&mut self, event: StorageEvent) {
        match event {
            StorageEvent::Failed { message, kind } => {
                if !self.failing {
                    self.failing = true;
                    set_status(&self.status, Some(message.clone()));
                    self.observer.storage_failed(&message, kind);
                }
            }
            StorageEvent::Recovered => {
                if self.failing && !self.tracks.values().any(|w| w.is_failing()) {
                    self.failing = false;
                    if self.queue_full_at.is_some() {
                        set_status(&self.status, Some(QUEUE_FULL_MESSAGE.to_string()));
                    } else {
                        set_status(&self.status, None);
                        self.observer.storage_recovered();
                    }
                }
            }
        }
    }

    fn check_queue(&mut self, now: Instant) {
        let due =
            |last: Option<Instant>| last.map_or(true, |t| now.duration_since(t) >= WARNING_REPEAT);
        let queued = self.queued.load(Ordering::Relaxed) as f64 / self.sample_rate as f64;
        if queued > BACKLOG_WARNING.as_secs_f64() && due(self.last_backlog_warning) {
            self.last_backlog_warning = Some(now);
            log::warn!("{queued:.1}s of audio waiting for the disk writer");
            self.observer.backlog(queued);
        }
        let dropped = self.dropped.load(Ordering::Relaxed);
        if dropped > self.last_dropped {
            self.last_dropped = dropped;
            self.queue_full_at = Some(now);
            if due(self.last_drop_warning) {
                log::error!(
                    "Storage queue full: {:.2}s of audio dropped so far",
                    dropped as f64 / self.sample_rate as f64
                );
                self.last_drop_warning = Some(now);
                if !self.failing {
                    set_status(&self.status, Some(QUEUE_FULL_MESSAGE.to_string()));
                    self.observer
                        .storage_failed(QUEUE_FULL_MESSAGE, StorageErrorKind::Other);
                }
            }
        } else if let Some(since) = self.queue_full_at {
            if now.duration_since(since) >= QUEUE_RECOVERY {
                log::info!("Storage queue has caught up; nothing dropped for {QUEUE_RECOVERY:?}");
                self.queue_full_at = None;
                // The next drop warns at once instead of waiting out the repeat.
                self.last_drop_warning = None;
                if !self.failing {
                    set_status(&self.status, None);
                    self.observer.storage_recovered();
                }
            }
        }
    }

    fn finish(mut self) -> WriterSummary {
        // Both tracks end at the same point on the recording clock.
        let end = self.tracks.values().filter_map(|w| w.end_position()).max();
        if let Some(end) = end {
            for writer in self.tracks.values_mut() {
                if let Some(track_end) = writer.end_position() {
                    writer.push_silence(end.saturating_sub(track_end));
                }
            }
        }
        let now = Instant::now();
        let mut tracks = Vec::new();
        let mut storage_error = None;
        let writers = std::mem::take(&mut self.tracks);
        for (kind, writer) in writers {
            let (summary, event) = writer.finish(now);
            if let Some(StorageEvent::Failed { message, .. }) = &event {
                storage_error = Some(message.clone());
            }
            if summary.lost_samples > 0 && storage_error.is_none() {
                storage_error = self.status.lock().ok().and_then(|s| s.clone());
            }
            let entry = self.manifest.track_mut(kind);
            entry.offset_samples = summary.offset_samples;
            entry.recorded_samples = Some(summary.written_samples);
            entry.lost_samples = summary.lost_samples;
            entry.segments = summary.segments.clone();
            tracks.push((kind, summary));
        }
        self.manifest.complete = true;
        if let Err(e) = write_manifest(&self.dir, &self.manifest) {
            log::warn!("Could not write {MANIFEST_FILE}: {e}");
        }
        let dropped = self.dropped.load(Ordering::Relaxed);
        if dropped > 0 && storage_error.is_none() {
            storage_error = Some(QUEUE_FULL_MESSAGE.to_string());
        }
        for (kind, t) in &tracks {
            log::info!(
                "{} track saved: {:.2}s from {:.3}s, {} segment(s), {:.2}s silence filled, {:.2}s lost",
                kind.file_stem(),
                t.written_samples as f64 / self.sample_rate as f64,
                t.offset_samples as f64 / self.sample_rate as f64,
                t.segments.len(),
                t.silence_samples as f64 / self.sample_rate as f64,
                t.lost_samples as f64 / self.sample_rate as f64,
            );
        }
        WriterSummary {
            sample_rate: self.sample_rate,
            tracks,
            dropped_samples: dropped,
            storage_error,
            crashed: false,
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::super::track_writer::tests::{MemoryStorage, DISK_FULL};
    use super::super::track_writer::{inspect_wav, segment_file_name};
    use super::*;
    use std::collections::HashMap;

    #[derive(Default)]
    struct RecordingObserver {
        failed: Mutex<Vec<String>>,
        recovered: Mutex<u32>,
        backlog: Mutex<Vec<f64>>,
    }

    impl WriterObserver for RecordingObserver {
        fn storage_failed(&self, message: &str, _kind: StorageErrorKind) {
            self.failed.lock().unwrap().push(message.to_string());
        }
        fn storage_recovered(&self) {
            *self.recovered.lock().unwrap() += 1;
        }
        fn backlog(&self, queued_seconds: f64) {
            self.backlog.lock().unwrap().push(queued_seconds);
        }
    }

    fn memory_writer(
        storages: &HashMap<TrackKind, MemoryStorage>,
        observer: Arc<dyn WriterObserver>,
        capacity: usize,
    ) -> (StorageSender, WriterHandle, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let storages = storages.clone();
        let (tx, handle) = spawn_writer_with(
            dir.path().to_path_buf(),
            1000,
            observer,
            capacity,
            move |kind| Box::new(storages[&kind].clone()) as Box<dyn TrackStorage>,
        )
        .unwrap();
        (tx, handle, dir)
    }

    fn storages(delay: Option<Duration>) -> HashMap<TrackKind, MemoryStorage> {
        TrackKind::ALL
            .iter()
            .map(|&k| {
                (
                    k,
                    MemoryStorage {
                        delay,
                        ..MemoryStorage::default()
                    },
                )
            })
            .collect()
    }

    /// N chunks + stop gives exactly N chunks on disk, also when every write
    /// is slow, and the last (short) chunk is there too.
    #[test]
    fn stop_keeps_every_chunk_even_with_slow_storage() {
        for delay in [None, Some(Duration::from_millis(30))] {
            let storages = storages(delay);
            let (tx, handle, _dir) =
                memory_writer(&storages, Arc::new(LogObserver), CHANNEL_CAPACITY);
            let n = 500;
            for i in 0..n {
                tx.send(
                    TrackKind::Microphone,
                    vec![(i % 100) as f32 / 200.0 + 0.01; 10],
                    None,
                )
                .unwrap();
            }
            // The last, incomplete window: 3 samples.
            tx.send(TrackKind::Microphone, vec![0.9; 3], None).unwrap();
            drop(tx);
            let summary = handle.join().unwrap();
            let mic = storages[&TrackKind::Microphone].samples.lock().unwrap();
            assert_eq!(mic.len(), n * 10 + 3, "delay {delay:?}");
            assert_eq!(&mic[mic.len() - 3..], &[f32_to(0.9); 3]);
            assert_eq!(summary.tracks.len(), 1);
            assert_eq!(summary.tracks[0].1.written_samples, (n * 10 + 3) as u64);
            assert_eq!(summary.lost_samples(), 0);
        }
    }

    fn f32_to(x: f32) -> i16 {
        super::super::track_writer::f32_to_i16(x)
    }

    #[test]
    fn tracks_end_on_the_same_sample() {
        let storages = storages(None);
        let (tx, handle, _dir) = memory_writer(&storages, Arc::new(LogObserver), CHANNEL_CAPACITY);
        // System starts 0.2 s later and stops 0.1 s earlier.
        for i in 0..100u32 {
            let end = (i + 1) as f64 * 0.01;
            tx.send(TrackKind::Microphone, vec![0.1; 10], Some(end))
                .unwrap();
            if (20..90).contains(&i) {
                tx.send(TrackKind::System, vec![0.1; 10], Some(end))
                    .unwrap();
            }
        }
        drop(tx);
        let summary = handle.join().unwrap();
        let ends: Vec<u64> = summary
            .tracks
            .iter()
            .map(|(_, t)| t.offset_samples + t.written_samples)
            .collect();
        assert_eq!(ends, vec![1000, 1000]);
        let system = summary
            .tracks
            .iter()
            .find(|(k, _)| *k == TrackKind::System)
            .unwrap();
        assert_eq!(system.1.offset_samples, 200);
        assert!((summary.duration_seconds() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn writes_wav_segments_and_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, handle) =
            spawn_writer(dir.path().to_path_buf(), 48000, Arc::new(LogObserver)).unwrap();
        for i in 0..200u32 {
            let end = (i + 1) as f64 * 0.01;
            tx.send(TrackKind::Microphone, vec![0.2; 480], Some(end))
                .unwrap();
            tx.send(TrackKind::System, vec![-0.2; 480], Some(end))
                .unwrap();
        }
        drop(tx);
        let summary = handle.join().unwrap();
        assert_eq!(summary.duration_seconds(), 2.0);
        for kind in TrackKind::ALL {
            let wav = inspect_wav(&dir.path().join(segment_file_name(kind, 0))).unwrap();
            assert_eq!(wav.samples(), 96_000);
            assert!(!wav.header_lags());
        }
        let manifest = read_manifest(dir.path()).unwrap();
        assert!(manifest.complete);
        assert_eq!(manifest.tracks.len(), 2);
        assert!(manifest
            .tracks
            .iter()
            .all(|t| t.recorded_samples == Some(96_000) && t.offset_samples == 0));
    }

    /// Stop-to-saved does not grow with the meeting: after five minutes of
    /// audio, stop only writes what was still queued or pending (at most a
    /// couple of seconds), into the real WAV files. Measured as data written
    /// after stop, because CI disks make wall-clock bounds flaky; the time is
    /// printed and logged in the app as "Stop-to-saved".
    #[test]
    fn stop_to_saved_is_fast_after_a_long_recording() {
        let dir = tempfile::tempdir().unwrap();
        let written = Arc::new(AtomicU64::new(0));
        let storage_dir = dir.path().to_path_buf();
        let counter = written.clone();
        let (tx, handle) = spawn_writer_with(
            dir.path().to_path_buf(),
            48000,
            Arc::new(LogObserver),
            CHANNEL_CAPACITY,
            move |kind| {
                Box::new(CountingStorage {
                    inner: WavSegmentStorage::new(storage_dir.clone(), kind, 48000),
                    written: counter.clone(),
                }) as Box<dyn TrackStorage>
            },
        )
        .unwrap();
        for i in 0..30_000u32 {
            let end = (i + 1) as f64 * 0.01;
            tx.send(TrackKind::Microphone, vec![0.2; 480], Some(end))
                .unwrap();
            tx.send(TrackKind::System, vec![-0.2; 480], Some(end))
                .unwrap();
            // Faster than real time, but never more than a second ahead of
            // the disk (as in a real recording).
            while tx.queued_seconds() > 1.0 {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        let before_stop = written.load(Ordering::SeqCst);
        let stop = Instant::now();
        drop(tx);
        let summary = handle.join().unwrap();
        eprintln!("stop-to-saved after 300 s of audio: {:?}", stop.elapsed());
        let after_stop = written.load(Ordering::SeqCst) - before_stop;
        // Queue (<= 1 s for both tracks) + one commit interval per track.
        assert!(
            after_stop <= 3 * 2 * 48000,
            "stop wrote {:.1}s of audio",
            after_stop as f64 / 96000.0
        );
        assert!((summary.duration_seconds() - 300.0).abs() < 1e-6);
        assert_eq!(summary.lost_samples(), 0);
        assert_eq!(written.load(Ordering::SeqCst), 2 * 300 * 48000);
    }

    struct CountingStorage {
        inner: WavSegmentStorage,
        written: Arc<AtomicU64>,
    }

    impl TrackStorage for CountingStorage {
        fn write(&mut self, samples: &[i16]) -> super::super::track_writer::WriteOutcome {
            let outcome = self.inner.write(samples);
            self.written
                .fetch_add(outcome.committed as u64, Ordering::SeqCst);
            outcome
        }
        fn segment_names(&self) -> Vec<String> {
            self.inner.segment_names()
        }
        fn sync(&mut self) -> io::Result<()> {
            self.inner.sync()
        }
    }

    #[test]
    fn storage_failure_and_recovery_are_reported_once() {
        let storages = storages(None);
        *storages[&TrackKind::Microphone].fail.lock().unwrap() = Some(DISK_FULL);
        let observer = Arc::new(RecordingObserver::default());
        let (tx, handle, _dir) = memory_writer(&storages, observer.clone(), CHANNEL_CAPACITY);
        for _ in 0..20 {
            tx.send(TrackKind::Microphone, vec![0.1; 10], None).unwrap();
            std::thread::sleep(Duration::from_millis(10));
        }
        // The failure reaches the observer well within 5 s.
        let deadline = Instant::now() + Duration::from_secs(5);
        while observer.failed.lock().unwrap().is_empty() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            *observer.failed.lock().unwrap(),
            vec!["Lyden gemmes ikke: disken er fuld".to_string()]
        );
        assert_eq!(
            handle.storage_error().as_deref(),
            Some("Lyden gemmes ikke: disken er fuld")
        );
        // Stop while the disk is still full: nothing on disk, the loss is counted.
        drop(tx);
        let summary = handle.join().unwrap();
        assert_eq!(summary.lost_samples(), 200);
        assert_eq!(
            summary.storage_error.as_deref(),
            Some("Lyden gemmes ikke: disken er fuld")
        );
    }

    #[test]
    fn full_queue_drops_without_blocking_and_is_reported() {
        let storages = storages(Some(Duration::from_millis(200)));
        let observer = Arc::new(RecordingObserver::default());
        let (tx, handle, _dir) = memory_writer(&storages, observer.clone(), 4);
        let started = Instant::now();
        let mut full = 0;
        for _ in 0..200 {
            if tx.send(TrackKind::System, vec![0.1; 1000], None) == Err(StorageSendError::Full) {
                full += 1;
            }
        }
        assert!(
            started.elapsed() < Duration::from_millis(100),
            "send never blocks"
        );
        assert!(full > 0);
        drop(tx);
        let summary = handle.join().unwrap();
        assert!(summary.dropped_samples > 0);
        assert!(observer
            .failed
            .lock()
            .unwrap()
            .iter()
            .any(|m| m == QUEUE_FULL_MESSAGE));
    }

    #[test]
    fn queue_full_error_clears_once_the_queue_catches_up() {
        let storages = storages(Some(Duration::from_millis(50)));
        let observer = Arc::new(RecordingObserver::default());
        let (tx, handle, _dir) = memory_writer(&storages, observer.clone(), 4);
        for _ in 0..50 {
            let _ = tx.send(TrackKind::System, vec![0.1; 1000], None);
        }
        let deadline = Instant::now() + Duration::from_secs(10);
        while *observer.recovered.lock().unwrap() == 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(observer
            .failed
            .lock()
            .unwrap()
            .iter()
            .any(|m| m == QUEUE_FULL_MESSAGE));
        assert_eq!(*observer.recovered.lock().unwrap(), 1, "recovery is reported");
        assert_eq!(handle.storage_error(), None, "the live state is clear again");
        // The rest of the meeting is saved; the loss still marks the result.
        tx.send(TrackKind::System, vec![0.1; 1000], None).unwrap();
        drop(tx);
        let summary = handle.join().unwrap();
        assert!(summary.dropped_samples > 0);
        assert_eq!(summary.storage_error.as_deref(), Some(QUEUE_FULL_MESSAGE));
    }

    #[test]
    fn backlog_over_five_seconds_warns() {
        // Slow storage and a large queue: 10 s of audio arrives at once.
        let storages = storages(Some(Duration::from_millis(300)));
        let observer = Arc::new(RecordingObserver::default());
        let (tx, handle, _dir) = memory_writer(&storages, observer.clone(), CHANNEL_CAPACITY);
        for _ in 0..1000 {
            tx.send(TrackKind::Microphone, vec![0.1; 10], None).unwrap();
        }
        assert!(tx.queued_seconds() > 5.0);
        let deadline = Instant::now() + Duration::from_secs(5);
        while observer.backlog.lock().unwrap().is_empty() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(observer
            .backlog
            .lock()
            .unwrap()
            .first()
            .is_some_and(|&s| s > 5.0));
        drop(tx);
        handle.join().unwrap();
    }

    /// A panic elsewhere (VAD, transcription) cannot touch the writer.
    #[test]
    fn a_panicking_task_next_to_the_writer_does_not_stop_it() {
        let storages = storages(None);
        let (tx, handle, _dir) = memory_writer(&storages, Arc::new(LogObserver), CHANNEL_CAPACITY);
        let vad = std::thread::spawn(|| panic!("simulated VAD abort"));
        for _ in 0..100 {
            tx.send(TrackKind::Microphone, vec![0.1; 10], None).unwrap();
        }
        assert!(vad.join().is_err());
        for _ in 0..100 {
            tx.send(TrackKind::Microphone, vec![0.1; 10], None).unwrap();
        }
        drop(tx);
        handle.join().unwrap();
        assert_eq!(
            storages[&TrackKind::Microphone]
                .samples
                .lock()
                .unwrap()
                .len(),
            2000
        );
    }

    #[test]
    fn a_dead_writer_closes_the_channel() {
        let dir = tempfile::tempdir().unwrap();
        let observer = Arc::new(RecordingObserver::default());
        let (tx, handle) =
            spawn_writer_with(dir.path().to_path_buf(), 1000, observer.clone(), 16, |_| {
                Box::new(PanickingStorage) as Box<dyn TrackStorage>
            })
            .unwrap();
        let _ = tx.send(TrackKind::Microphone, vec![0.1; 10], None);
        let deadline = Instant::now() + Duration::from_secs(5);
        while !handle.is_finished() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            tx.send(TrackKind::Microphone, vec![0.1; 10], None),
            Err(StorageSendError::Closed)
        );
        let summary = handle.join().unwrap();
        assert!(summary.crashed);
        assert_eq!(
            *observer.failed.lock().unwrap(),
            vec![WRITER_STOPPED_MESSAGE.to_string()]
        );
    }

    struct PanickingStorage;
    impl TrackStorage for PanickingStorage {
        fn write(&mut self, _samples: &[i16]) -> super::super::track_writer::WriteOutcome {
            panic!("simulated storage bug")
        }
        fn segment_names(&self) -> Vec<String> {
            Vec::new()
        }
    }
}
