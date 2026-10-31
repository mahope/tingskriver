//! One recording track (microphone or system) written continuously to disk.
//!
//! Samples are kept as 16-bit PCM in WAV segments (`microphone_000.wav`,
//! `microphone_001.wav`, ...). Every commit hands the new samples and then
//! the header sizes to the OS, so a killed or crashed process loses at most
//! [`COMMIT_INTERVAL`]; the files are synced to the disk every
//! [`SYNC_INTERVAL`] and at stop (power loss). A header that lags the data is
//! repaired on recovery (`audio::finalize`).
//!
//! [`TrackWriter`] is the timeline logic on top of a [`TrackStorage`]: gap
//! filling against the shared recording clock, bounded buffering while the
//! storage fails, and at most one write attempt per [`RETRY_INTERVAL`]. It
//! takes the current time as an argument, so tests drive it with a fake clock.

#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::collections::VecDeque;
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// How often buffered samples are written and synced. A crash loses at most
/// this much audio (plus what is still in the channel).
pub const COMMIT_INTERVAL: Duration = Duration::from_secs(1);

/// How often the files are flushed to the disk itself (`sync_data`). Not on
/// every commit: a slow disk can take hundreds of milliseconds per sync.
pub const SYNC_INTERVAL: Duration = Duration::from_secs(5);

/// While the storage fails, at most one write attempt per this interval.
pub const RETRY_INTERVAL: Duration = Duration::from_secs(10);

/// A track that lags the shared recording clock by more than this, for at
/// least [`GAP_CONFIRM`], gets silence for the missing time, so both tracks
/// stay on the same time axis across device outages. Larger than callback
/// jitter, smaller than anything audible as drift.
pub const GAP_FILL_THRESHOLD: Duration = Duration::from_millis(250);

/// How long a lag must persist before it counts as a gap. A stalled device
/// that then delivers its backlog in a burst lags only briefly; filling that
/// with silence would push the track ahead of the other for good.
pub const GAP_CONFIRM: Duration = Duration::from_secs(1);

/// While the storage fails, at most this much audio is kept in memory per
/// track (16-bit: 2.9 MB at 48 kHz). Older audio is replaced by silence of the
/// same length, so the timeline stays intact and memory stays flat.
pub const MAX_PENDING: Duration = Duration::from_secs(30);

/// Length of one WAV segment file.
pub const SEGMENT_LENGTH: Duration = Duration::from_secs(10 * 60);

pub const WAV_HEADER_LEN: u64 = 44;

/// Largest single storage write, in seconds of audio.
const MAX_WRITE_SECONDS: u64 = 10;

/// Which track a writer holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TrackKind {
    Microphone,
    System,
}

impl TrackKind {
    pub const ALL: [TrackKind; 2] = [TrackKind::Microphone, TrackKind::System];

    /// File name stem of the raw segments and the encoded track.
    pub fn file_stem(self) -> &'static str {
        match self {
            TrackKind::Microphone => "microphone",
            TrackKind::System => "system",
        }
    }
}

/// Name of segment `index` of a track: `microphone_000.wav`.
pub fn segment_file_name(kind: TrackKind, index: u32) -> String {
    format!("{}_{:03}.wav", kind.file_stem(), index)
}

/// Result of a storage write: how many samples are durable, and the error
/// that stopped the rest.
#[derive(Debug)]
pub struct WriteOutcome {
    pub committed: usize,
    pub error: Option<io::Error>,
}

impl WriteOutcome {
    fn ok(committed: usize) -> Self {
        Self {
            committed,
            error: None,
        }
    }
}

/// Where a track's samples go. `write` hands `samples` to the OS (data, then
/// header) or reports how many of them made it; a retry after an error writes
/// the rest again from the last written position, never twice.
pub trait TrackStorage: Send {
    fn write(&mut self, samples: &[i16]) -> WriteOutcome;
    /// Names of the files written so far, in order.
    fn segment_names(&self) -> Vec<String>;
    /// Flush everything written to the disk itself (called at stop).
    fn sync(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl TrackStorage for Box<dyn TrackStorage> {
    fn write(&mut self, samples: &[i16]) -> WriteOutcome {
        (**self).write(samples)
    }
    fn segment_names(&self) -> Vec<String> {
        (**self).segment_names()
    }
    fn sync(&mut self) -> io::Result<()> {
        (**self).sync()
    }
}

/// WAV segments in a directory.
pub struct WavSegmentStorage {
    dir: PathBuf,
    kind: TrackKind,
    sample_rate: u32,
    segment_capacity: u64,
    current: Option<OpenSegment>,
    next_index: u32,
    names: Vec<String>,
    bytes: Vec<u8>,
}

struct OpenSegment {
    file: File,
    /// Samples written to this segment (data and header).
    committed: u64,
    last_sync: Instant,
}

impl WavSegmentStorage {
    /// Nothing touches the disk until the first write, so a missing folder is
    /// reported (and retried) like any other storage error.
    pub fn new(dir: impl Into<PathBuf>, kind: TrackKind, sample_rate: u32) -> Self {
        Self::with_segment_length(dir, kind, sample_rate, SEGMENT_LENGTH)
    }

    pub fn with_segment_length(
        dir: impl Into<PathBuf>,
        kind: TrackKind,
        sample_rate: u32,
        segment_length: Duration,
    ) -> Self {
        let capacity = (segment_length.as_secs_f64() * sample_rate as f64) as u64;
        Self {
            dir: dir.into(),
            kind,
            sample_rate,
            segment_capacity: capacity.max(1),
            current: None,
            next_index: 0,
            names: Vec::new(),
            bytes: Vec::new(),
        }
    }

    fn open_next(&mut self) -> io::Result<()> {
        // The finished segment goes to the disk before the next one starts.
        if let Some(previous) = self.current.as_mut() {
            previous.file.sync_data()?;
        }
        let name = segment_file_name(self.kind, self.next_index);
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(self.dir.join(&name))?;
        write_wav_header(&mut file, self.sample_rate, 0)?;
        self.next_index += 1;
        self.names.push(name);
        self.current = Some(OpenSegment {
            file,
            committed: 0,
            last_sync: Instant::now(),
        });
        Ok(())
    }

    /// Write `samples` into the open segment at its durable end, then the
    /// header, then sync.
    fn write_into_current(&mut self, samples: &[i16]) -> io::Result<()> {
        self.bytes.clear();
        self.bytes.reserve(samples.len() * 2);
        for s in samples {
            self.bytes.extend_from_slice(&s.to_le_bytes());
        }
        let Some(segment) = self.current.as_mut() else {
            return Err(io::Error::other("no open segment"));
        };
        let end = WAV_HEADER_LEN + segment.committed * 2;
        segment.file.seek(SeekFrom::Start(end))?;
        segment.file.write_all(&self.bytes)?;
        let committed = segment.committed + samples.len() as u64;
        update_wav_sizes(&mut segment.file, committed * 2)?;
        segment.committed = committed;
        if segment.last_sync.elapsed() >= SYNC_INTERVAL {
            segment.file.sync_data()?;
            segment.last_sync = Instant::now();
        }
        Ok(())
    }
}

impl TrackStorage for WavSegmentStorage {
    fn write(&mut self, samples: &[i16]) -> WriteOutcome {
        let mut done = 0;
        while done < samples.len() {
            let full = self
                .current
                .as_ref()
                .map_or(true, |s| s.committed >= self.segment_capacity);
            if full {
                if let Err(e) = self.open_next() {
                    return WriteOutcome {
                        committed: done,
                        error: Some(e),
                    };
                }
            }
            let room = self
                .current
                .as_ref()
                .map_or(0, |s| self.segment_capacity.saturating_sub(s.committed));
            let take = (samples.len() - done).min(room as usize);
            if let Err(e) = self.write_into_current(&samples[done..done + take]) {
                return WriteOutcome {
                    committed: done,
                    error: Some(e),
                };
            }
            done += take;
        }
        WriteOutcome::ok(done)
    }

    fn segment_names(&self) -> Vec<String> {
        self.names.clone()
    }

    fn sync(&mut self) -> io::Result<()> {
        match self.current.as_mut() {
            Some(segment) => {
                segment.file.sync_data()?;
                segment.last_sync = Instant::now();
                Ok(())
            }
            None => Ok(()),
        }
    }
}

/// A 44-byte PCM WAV header: mono, 16-bit, `data_bytes` of samples.
pub fn write_wav_header(
    out: &mut (impl Write + Seek),
    sample_rate: u32,
    data_bytes: u64,
) -> io::Result<()> {
    let data = data_bytes.min(u32::MAX as u64 - 36) as u32;
    let mut header = Vec::with_capacity(WAV_HEADER_LEN as usize);
    header.extend_from_slice(b"RIFF");
    header.extend_from_slice(&(36 + data).to_le_bytes());
    header.extend_from_slice(b"WAVE");
    header.extend_from_slice(b"fmt ");
    header.extend_from_slice(&16u32.to_le_bytes());
    header.extend_from_slice(&1u16.to_le_bytes()); // PCM
    header.extend_from_slice(&1u16.to_le_bytes()); // mono
    header.extend_from_slice(&sample_rate.to_le_bytes());
    header.extend_from_slice(&(sample_rate * 2).to_le_bytes()); // byte rate
    header.extend_from_slice(&2u16.to_le_bytes()); // block align
    header.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    header.extend_from_slice(b"data");
    header.extend_from_slice(&data.to_le_bytes());
    out.seek(SeekFrom::Start(0))?;
    out.write_all(&header)
}

/// Rewrite the RIFF and data sizes of a header written by [`write_wav_header`].
pub fn update_wav_sizes(out: &mut (impl Write + Seek), data_bytes: u64) -> io::Result<()> {
    let data = data_bytes.min(u32::MAX as u64 - 36) as u32;
    out.seek(SeekFrom::Start(4))?;
    out.write_all(&(36 + data).to_le_bytes())?;
    out.seek(SeekFrom::Start(40))?;
    out.write_all(&data.to_le_bytes())
}

/// What a segment file on disk holds.
#[derive(Debug, Clone, PartialEq)]
pub struct WavInspection {
    pub sample_rate: u32,
    /// Data size the header claims.
    pub header_data_bytes: u64,
    /// Whole samples actually present after the header, in bytes.
    pub actual_data_bytes: u64,
}

impl WavInspection {
    pub fn samples(&self) -> u64 {
        self.actual_data_bytes / 2
    }

    /// The header lags the data: a crash between the data write and the
    /// header update. Repairable without loss.
    pub fn header_lags(&self) -> bool {
        self.header_data_bytes < self.actual_data_bytes
    }

    /// The header claims more data than the file holds: data was lost after
    /// it was committed (truncated copy, power loss before the data reached
    /// the disk).
    pub fn truncated(&self) -> bool {
        self.header_data_bytes > self.actual_data_bytes
    }
}

/// Read the header of a segment written by [`WavSegmentStorage`]. Errors for
/// anything that is not a mono 16-bit PCM WAV with the 44-byte layout.
pub fn inspect_wav(path: &Path) -> io::Result<WavInspection> {
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    if len < WAV_HEADER_LEN {
        return Err(invalid("file is shorter than a WAV header"));
    }
    let mut header = [0u8; WAV_HEADER_LEN as usize];
    file.read_exact(&mut header)?;
    let u16_at = |i: usize| u16::from_le_bytes([header[i], header[i + 1]]);
    let u32_at =
        |i: usize| u32::from_le_bytes([header[i], header[i + 1], header[i + 2], header[i + 3]]);
    if &header[0..4] != b"RIFF"
        || &header[8..12] != b"WAVE"
        || &header[12..16] != b"fmt "
        || &header[36..40] != b"data"
    {
        return Err(invalid("not a WAV file"));
    }
    if u16_at(20) != 1 || u16_at(22) != 1 || u16_at(34) != 16 {
        return Err(invalid("not mono 16-bit PCM"));
    }
    let sample_rate = u32_at(24);
    if sample_rate == 0 {
        return Err(invalid("sample rate is 0"));
    }
    let present = len - WAV_HEADER_LEN;
    Ok(WavInspection {
        sample_rate,
        header_data_bytes: u32_at(40) as u64,
        actual_data_bytes: present - present % 2,
    })
}

fn invalid(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.to_string())
}

/// Read up to `max` samples starting at sample `start` of a segment.
pub fn read_wav_samples(
    file: &mut File,
    start: u64,
    max: usize,
    out: &mut Vec<i16>,
) -> io::Result<usize> {
    let mut bytes = vec![0u8; max * 2];
    file.seek(SeekFrom::Start(WAV_HEADER_LEN + start * 2))?;
    let mut filled = 0;
    while filled < bytes.len() {
        let n = file.read(&mut bytes[filled..])?;
        if n == 0 {
            break;
        }
        filled += n;
    }
    let samples = filled / 2;
    out.clear();
    out.extend(
        bytes[..samples * 2]
            .chunks_exact(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]])),
    );
    Ok(samples)
}

/// Danish user message for a storage error, e.g. "Lyden gemmes ikke: disken er fuld".
pub fn storage_error_message(error: &io::Error) -> String {
    let reason = match storage_error_kind(error) {
        StorageErrorKind::DiskFull => "disken er fuld".to_string(),
        StorageErrorKind::Locked => "filen er låst eller utilgængelig".to_string(),
        StorageErrorKind::Other => error.to_string(),
    };
    format!("Lyden gemmes ikke: {reason}")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageErrorKind {
    DiskFull,
    Locked,
    Other,
}

pub fn storage_error_kind(error: &io::Error) -> StorageErrorKind {
    // ERROR_HANDLE_DISK_FULL (39) and ERROR_DISK_FULL (112) on Windows,
    // ENOSPC (28) and EDQUOT (122 on Linux) elsewhere.
    #[cfg(windows)]
    const DISK_FULL: &[i32] = &[39, 112];
    #[cfg(not(windows))]
    const DISK_FULL: &[i32] = &[28, 122];
    // ERROR_SHARING_VIOLATION (32), ERROR_LOCK_VIOLATION (33).
    #[cfg(windows)]
    const LOCKED: &[i32] = &[32, 33];
    #[cfg(not(windows))]
    const LOCKED: &[i32] = &[];

    match error.raw_os_error() {
        Some(code) if DISK_FULL.contains(&code) => StorageErrorKind::DiskFull,
        Some(code) if LOCKED.contains(&code) => StorageErrorKind::Locked,
        _ if error.kind() == io::ErrorKind::PermissionDenied => StorageErrorKind::Locked,
        _ => StorageErrorKind::Other,
    }
}

/// A storage state change worth telling the user about.
#[derive(Debug, Clone, PartialEq)]
pub enum StorageEvent {
    Failed {
        message: String,
        kind: StorageErrorKind,
    },
    Recovered,
}

enum Block {
    Samples(Vec<i16>),
    Silence(u64),
}

impl Block {
    fn len(&self) -> u64 {
        match self {
            Block::Samples(s) => s.len() as u64,
            Block::Silence(n) => *n,
        }
    }
}

/// What a track ended up as.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TrackSummary {
    /// Position of the first sample on the shared recording clock.
    pub offset_samples: u64,
    /// Samples on disk, including filled gaps.
    pub written_samples: u64,
    /// Samples that never reached the disk (storage failed until the end).
    pub lost_samples: u64,
    /// Silence inserted for gaps and for audio dropped while storage failed.
    pub silence_samples: u64,
    pub segments: Vec<String>,
}

/// Timeline and failure handling for one track.
pub struct TrackWriter<S: TrackStorage> {
    storage: S,
    sample_rate: u32,
    /// Timeline position of the first sample, once known.
    offset: Option<u64>,
    /// Samples accepted so far (written + pending), including silence.
    accepted: u64,
    /// Clock position where the current lag beyond the threshold started,
    /// and the smallest lag seen since.
    lag_since: Option<(u64, u64)>,
    pending: VecDeque<Block>,
    pending_sample_count: usize,
    max_pending_samples: usize,
    written: u64,
    silence: u64,
    last_commit: Option<Instant>,
    /// Earliest time of the next write attempt while failing.
    retry_at: Option<Instant>,
    failing: bool,
    /// Buffered audio replaced by silence during the current failure.
    replaced: u64,
    attempts: u64,
    scratch: Vec<i16>,
}

impl<S: TrackStorage> TrackWriter<S> {
    pub fn new(storage: S, sample_rate: u32) -> Self {
        Self {
            storage,
            sample_rate,
            offset: None,
            accepted: 0,
            lag_since: None,
            pending: VecDeque::new(),
            pending_sample_count: 0,
            max_pending_samples: (MAX_PENDING.as_secs_f64() * sample_rate as f64) as usize,
            written: 0,
            silence: 0,
            last_commit: None,
            retry_at: None,
            failing: false,
            replaced: 0,
            attempts: 0,
            scratch: Vec::new(),
        }
    }

    /// Accept a chunk. `end_position` is the position of its last sample on
    /// the shared recording clock (in samples), if known; it places the first
    /// chunk and fills gaps. Returns the offset when this was the first chunk.
    pub fn push(&mut self, samples: &[f32], end_position: Option<u64>) -> Option<u64> {
        if samples.is_empty() {
            return None;
        }
        let len = samples.len() as u64;
        let mut started = None;
        match (self.offset, end_position) {
            (None, end) => {
                let offset = end.map_or(0, |e| e.saturating_sub(len));
                self.offset = Some(offset);
                started = Some(offset);
            }
            (Some(offset), Some(end)) => {
                let start = end.saturating_sub(len);
                let lag = start.saturating_sub(offset + self.accepted);
                let threshold = (GAP_FILL_THRESHOLD.as_secs_f64() * self.sample_rate as f64) as u64;
                let confirm = (GAP_CONFIRM.as_secs_f64() * self.sample_rate as f64) as u64;
                if lag <= threshold {
                    self.lag_since = None;
                } else {
                    match self.lag_since {
                        None => self.lag_since = Some((start, lag)),
                        Some((since, smallest)) => {
                            let smallest = smallest.min(lag);
                            if start.saturating_sub(since) >= confirm {
                                log::warn!(
                                    "Track lags the recording clock by {:.2}s; filled with silence",
                                    smallest as f64 / self.sample_rate as f64
                                );
                                self.push_silence(smallest);
                                self.lag_since = None;
                            } else {
                                self.lag_since = Some((since, smallest));
                            }
                        }
                    }
                }
            }
            (Some(_), None) => {}
        }
        let converted: Vec<i16> = samples.iter().map(|&x| f32_to_i16(x)).collect();
        self.pending_sample_count += converted.len();
        self.pending.push_back(Block::Samples(converted));
        self.accepted += len;
        if self.failing {
            self.enforce_pending_limit();
        }
        started
    }

    /// Add `n` samples of silence at the end of the track.
    pub fn push_silence(&mut self, n: u64) {
        if n == 0 {
            return;
        }
        if self.offset.is_none() {
            self.offset = Some(0);
        }
        match self.pending.back_mut() {
            Some(Block::Silence(m)) => *m += n,
            _ => self.pending.push_back(Block::Silence(n)),
        }
        self.accepted += n;
        self.silence += n;
    }

    /// Replace the oldest buffered audio by silence of the same length while
    /// more than `MAX_PENDING` is buffered.
    fn enforce_pending_limit(&mut self) {
        let mut replaced = 0usize;
        while self.pending_sample_count > self.max_pending_samples {
            let Some(index) = self
                .pending
                .iter()
                .position(|b| matches!(b, Block::Samples(_)))
            else {
                break;
            };
            let n = self.pending.get(index).map_or(0, Block::len);
            self.pending.remove(index);
            self.pending_sample_count -= n as usize;
            self.silence += n;
            replaced += n as usize;
            match index.checked_sub(1).and_then(|i| self.pending.get_mut(i)) {
                Some(Block::Silence(m)) => *m += n,
                _ => self.pending.insert(index, Block::Silence(n)),
            }
            // Merge the new silence with silence on either side.
            let mut i = index.saturating_sub(1);
            while i <= index && i + 1 < self.pending.len() {
                if let (Some(Block::Silence(a)), Some(Block::Silence(b))) =
                    (self.pending.get(i), self.pending.get(i + 1))
                {
                    let sum = a + b;
                    self.pending.remove(i + 1);
                    self.pending[i] = Block::Silence(sum);
                } else {
                    i += 1;
                }
            }
        }
        if replaced > 0 {
            if self.replaced == 0 {
                log::warn!("Track buffer full: the oldest buffered audio is replaced by silence");
            }
            self.replaced += replaced as u64;
        }
    }

    /// End of the track on the shared recording clock (offset + accepted).
    pub fn end_position(&self) -> Option<u64> {
        self.offset.map(|o| o + self.accepted)
    }

    pub fn offset(&self) -> Option<u64> {
        self.offset
    }

    /// Samples buffered in memory and not yet on disk.
    pub fn pending_samples(&self) -> usize {
        self.pending_sample_count
    }

    pub fn is_failing(&self) -> bool {
        self.failing
    }

    /// Write attempts made so far (for tests of the retry limit).
    pub fn attempts(&self) -> u64 {
        self.attempts
    }

    /// Commit if `COMMIT_INTERVAL` has passed since the last commit or that
    /// much audio is pending, or at most once per `RETRY_INTERVAL` while the
    /// storage fails.
    pub fn tick(&mut self, now: Instant) -> Option<StorageEvent> {
        if self.pending.is_empty() {
            return None;
        }
        if let Some(retry_at) = self.retry_at {
            if now < retry_at {
                return None;
            }
        } else if let Some(last) = self.last_commit {
            // Also due when a commit interval's worth of audio is waiting:
            // audio can arrive faster than real time (a burst after a stall).
            let due_by_size = self.pending_sample_count as f64
                >= COMMIT_INTERVAL.as_secs_f64() * self.sample_rate as f64;
            if now.duration_since(last) < COMMIT_INTERVAL && !due_by_size {
                return None;
            }
        }
        self.commit(now)
    }

    /// Write everything pending now, ignoring the commit and retry timers.
    pub fn commit(&mut self, now: Instant) -> Option<StorageEvent> {
        if self.pending.is_empty() {
            self.last_commit = Some(now);
            return None;
        }
        self.attempts += 1;
        let error = self.write_pending();
        self.last_commit = Some(now);
        match error {
            None => {
                self.retry_at = None;
                if self.failing {
                    self.failing = false;
                    if self.replaced > 0 {
                        log::warn!(
                            "Track buffer full: {:.2}s of audio was replaced by silence",
                            self.replaced as f64 / self.sample_rate as f64
                        );
                        self.replaced = 0;
                    }
                    log::info!("Storage recovered, the track is written again");
                    return Some(StorageEvent::Recovered);
                }
                None
            }
            Some(e) => {
                self.retry_at = Some(now + RETRY_INTERVAL);
                let message = storage_error_message(&e);
                log::error!("{message} ({e})");
                if !self.failing {
                    self.failing = true;
                    return Some(StorageEvent::Failed {
                        message,
                        kind: storage_error_kind(&e),
                    });
                }
                None
            }
        }
    }

    /// Write the pending blocks in as few storage writes as possible (each
    /// write syncs): consecutive blocks are joined into one buffer of at most
    /// `MAX_WRITE_SECONDS`.
    fn write_pending(&mut self) -> Option<io::Error> {
        let max = (MAX_WRITE_SECONDS * self.sample_rate as u64).max(1) as usize;
        while !self.pending.is_empty() {
            self.scratch.clear();
            for block in &self.pending {
                let room = max - self.scratch.len();
                if room == 0 {
                    break;
                }
                match block {
                    Block::Samples(s) => self.scratch.extend_from_slice(&s[..s.len().min(room)]),
                    Block::Silence(n) => self
                        .scratch
                        .resize(self.scratch.len() + (*n).min(room as u64) as usize, 0),
                }
            }
            let outcome = self.storage.write(&self.scratch);
            self.consume(outcome.committed);
            if let Some(e) = outcome.error {
                return Some(e);
            }
        }
        None
    }

    /// Drop `n` written samples from the front of the pending queue.
    fn consume(&mut self, mut n: usize) {
        self.written += n as u64;
        while n > 0 {
            let Some(block) = self.pending.front_mut() else {
                break;
            };
            match block {
                Block::Samples(s) => {
                    let take = n.min(s.len());
                    self.pending_sample_count -= take;
                    n -= take;
                    if take == s.len() {
                        self.pending.pop_front();
                    } else {
                        s.drain(..take);
                    }
                }
                Block::Silence(m) => {
                    let take = (n as u64).min(*m);
                    n -= take as usize;
                    *m -= take;
                    if *m == 0 {
                        self.pending.pop_front();
                    }
                }
            }
        }
    }

    /// Final commit at stop (one attempt regardless of the retry timer).
    pub fn finish(mut self, now: Instant) -> (TrackSummary, Option<StorageEvent>) {
        let event = self.commit(now);
        if let Err(e) = self.storage.sync() {
            log::warn!("Could not sync the track to the disk at stop: {e}");
        }
        let lost: u64 = self.pending.iter().map(Block::len).sum();
        let summary = TrackSummary {
            offset_samples: self.offset.unwrap_or(0),
            written_samples: self.written,
            lost_samples: lost,
            silence_samples: self.silence,
            segments: self.storage.segment_names(),
        };
        (summary, event)
    }
}

pub fn f32_to_i16(x: f32) -> i16 {
    let clamped = if x.is_finite() {
        x.clamp(-1.0, 1.0)
    } else {
        0.0
    };
    (clamped * i16::MAX as f32).round() as i16
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
pub(crate) mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// In-memory storage that can be told to fail.
    #[derive(Clone, Default)]
    pub(crate) struct MemoryStorage {
        pub samples: Arc<Mutex<Vec<i16>>>,
        pub fail: Arc<Mutex<Option<i32>>>,
        pub calls: Arc<Mutex<u64>>,
        pub delay: Option<Duration>,
    }

    impl TrackStorage for MemoryStorage {
        fn write(&mut self, samples: &[i16]) -> WriteOutcome {
            *self.calls.lock().unwrap() += 1;
            if let Some(delay) = self.delay {
                std::thread::sleep(delay);
            }
            if let Some(code) = *self.fail.lock().unwrap() {
                return WriteOutcome {
                    committed: 0,
                    error: Some(io::Error::from_raw_os_error(code)),
                };
            }
            self.samples.lock().unwrap().extend_from_slice(samples);
            WriteOutcome::ok(samples.len())
        }
        fn segment_names(&self) -> Vec<String> {
            vec!["memory".to_string()]
        }
    }

    #[cfg(windows)]
    pub(crate) const DISK_FULL: i32 = 112;
    #[cfg(not(windows))]
    pub(crate) const DISK_FULL: i32 = 28;

    #[test]
    fn commits_once_per_interval() {
        let storage = MemoryStorage::default();
        let mut w = TrackWriter::new(storage.clone(), 48000);
        let t0 = Instant::now();
        w.push(&[0.5; 480], None);
        assert_eq!(w.tick(t0), None);
        assert_eq!(storage.samples.lock().unwrap().len(), 480);
        w.push(&[0.5; 480], None);
        w.tick(t0 + Duration::from_millis(500));
        assert_eq!(
            storage.samples.lock().unwrap().len(),
            480,
            "no commit before the interval"
        );
        w.tick(t0 + Duration::from_millis(1000));
        assert_eq!(storage.samples.lock().unwrap().len(), 960);
    }

    #[test]
    fn first_chunk_sets_offset_and_gaps_are_filled() {
        let storage = MemoryStorage::default();
        let mut w = TrackWriter::new(storage.clone(), 1000);
        // First chunk ends at position 600: offset 500.
        assert_eq!(w.push(&[0.1; 100], Some(600)), Some(500));
        // Small jitter is ignored.
        w.push(&[0.1; 100], Some(800));
        assert_eq!(w.end_position(), Some(700));
        // Device outage of 1 s: the next chunk starts at 1700, the track
        // ends at 700. Audio continues in real time; once the lag has lasted
        // GAP_CONFIRM it is filled.
        let mut end = 1800;
        while end <= 3000 {
            w.push(&[0.1; 100], Some(end));
            end += 100;
        }
        assert_eq!(w.end_position(), Some(3000));
        let (summary, _) = w.finish(Instant::now());
        assert_eq!(summary.offset_samples, 500);
        assert_eq!(summary.silence_samples, 1000);
        let data = storage.samples.lock().unwrap();
        assert_eq!(data.len(), 2500);
        // The silence sits after the audio that confirmed the gap.
        assert_eq!(data.iter().filter(|&&s| s == 0).count(), 1000);
    }

    #[test]
    fn a_burst_after_a_stall_is_not_a_gap() {
        let storage = MemoryStorage::default();
        let mut w = TrackWriter::new(storage.clone(), 1000);
        for i in 1..=10u64 {
            w.push(&[0.1; 100], Some(i * 100));
        }
        // The device stalls for 0.5 s and then delivers the backlog at once:
        // five chunks all stamped at 1500, then real time again.
        for _ in 0..5 {
            w.push(&[0.1; 100], Some(1500));
        }
        for i in 16..=40u64 {
            w.push(&[0.1; 100], Some(i * 100));
        }
        let (summary, _) = w.finish(Instant::now());
        assert_eq!(summary.silence_samples, 0);
        assert_eq!(summary.written_samples, 4000);
    }

    #[test]
    fn retries_at_most_once_per_ten_seconds() {
        let storage = MemoryStorage::default();
        *storage.fail.lock().unwrap() = Some(DISK_FULL);
        let mut w = TrackWriter::new(storage.clone(), 1000);
        let t0 = Instant::now();
        let mut events = Vec::new();
        // 60 s of fake time, a chunk and a tick every 100 ms.
        for step in 0..600u64 {
            w.push(&[0.2; 100], None);
            if let Some(e) = w.tick(t0 + Duration::from_millis(step * 100)) {
                events.push(e);
            }
        }
        // Attempts at 0, 10, 20, 30, 40, 50 s.
        assert_eq!(w.attempts(), 6);
        assert_eq!(events.len(), 1, "the failure is reported once");
        assert!(
            matches!(&events[0], StorageEvent::Failed { kind: StorageErrorKind::DiskFull, message } if message == "Lyden gemmes ikke: disken er fuld")
        );

        // Space is freed: the next attempt succeeds and reports recovery.
        *storage.fail.lock().unwrap() = None;
        assert_eq!(
            w.tick(t0 + Duration::from_secs(59)),
            None,
            "still waiting for the retry time"
        );
        assert_eq!(
            w.tick(t0 + Duration::from_secs(60)),
            Some(StorageEvent::Recovered)
        );
        // The timeline is intact: 60 s of audio, of which the oldest beyond
        // MAX_PENDING became silence.
        let data = storage.samples.lock().unwrap();
        assert_eq!(data.len(), 60_000);
        assert!(data[..30_000].iter().all(|&s| s == 0));
        assert!(data[30_000..].iter().all(|&s| s != 0));
    }

    #[test]
    fn memory_is_bounded_while_storage_fails() {
        let storage = MemoryStorage::default();
        *storage.fail.lock().unwrap() = Some(DISK_FULL);
        let mut w = TrackWriter::new(storage, 48000);
        let t0 = Instant::now();
        // 10 minutes of 10 ms chunks with a failing disk.
        for step in 0..60_000u64 {
            w.push(&[0.3; 480], None);
            w.tick(t0 + Duration::from_millis(step * 10));
        }
        let max = (MAX_PENDING.as_secs_f64() * 48000.0) as usize;
        assert!(w.pending_samples() <= max);
        // 2 bytes per sample: under 3 MB per track.
        assert!(w.pending_samples() * 2 < 3 * 1024 * 1024);
        assert!(w.pending.len() < 4000, "silence blocks are merged");
    }

    #[test]
    fn memory_is_flat_over_an_hour() {
        let storage = MemoryStorage::default();
        let mut w = TrackWriter::new(NullStorage(storage), 48000);
        let t0 = Instant::now();
        let mut peak = 0;
        for step in 0..360_000u64 {
            w.push(&[0.3; 480], None);
            w.tick(t0 + Duration::from_millis(step * 10));
            peak = peak.max(w.pending_samples());
        }
        // Never more than the commit interval (plus one chunk) in memory.
        assert!(peak <= 48000 + 480, "peak {peak}");
    }

    /// Counts but keeps nothing.
    struct NullStorage(MemoryStorage);
    impl TrackStorage for NullStorage {
        fn write(&mut self, samples: &[i16]) -> WriteOutcome {
            *self.0.calls.lock().unwrap() += 1;
            WriteOutcome::ok(samples.len())
        }
        fn segment_names(&self) -> Vec<String> {
            Vec::new()
        }
    }

    #[test]
    fn wav_segments_roll_over_and_survive_a_crash() {
        let dir = tempfile::tempdir().unwrap();
        let storage = WavSegmentStorage::with_segment_length(
            dir.path(),
            TrackKind::Microphone,
            1000,
            Duration::from_secs(2),
        );
        let mut w = TrackWriter::new(storage, 1000);
        let t0 = Instant::now();
        // 5.5 s of audio in 100 ms chunks, ticking along.
        for step in 0..55u64 {
            w.push(&[0.25; 100], None);
            w.tick(t0 + Duration::from_millis(step * 100));
        }
        // Crash: no finish, no Drop.
        std::mem::forget(w);
        let names: Vec<_> = (0..3)
            .map(|i| dir.path().join(segment_file_name(TrackKind::Microphone, i)))
            .collect();
        let inspected: Vec<_> = names.iter().map(|p| inspect_wav(p).unwrap()).collect();
        assert_eq!(inspected[0].samples(), 2000);
        assert_eq!(inspected[1].samples(), 2000);
        let total: u64 = inspected.iter().map(|i| i.samples()).sum();
        // At most one commit interval lost.
        assert!(total >= 5500 - 1000, "recovered {total} of 5500 samples");
        assert!(inspected.iter().all(|i| !i.truncated()));
    }

    #[test]
    fn wav_write_retries_from_the_durable_end() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("missing");
        let mut storage = WavSegmentStorage::new(&sub, TrackKind::System, 1000);
        // Folder missing: nothing is written.
        let outcome = storage.write(&[1, 2, 3]);
        assert_eq!(outcome.committed, 0);
        assert!(outcome.error.is_some());
        std::fs::create_dir_all(&sub).unwrap();
        assert!(storage.write(&[1, 2, 3]).error.is_none());
        assert!(storage.write(&[4]).error.is_none());
        let wav = inspect_wav(&sub.join("system_000.wav")).unwrap();
        assert_eq!(wav.samples(), 4);
        assert_eq!(wav.header_data_bytes, 8);
        let mut file = File::open(sub.join("system_000.wav")).unwrap();
        let mut out = Vec::new();
        read_wav_samples(&mut file, 0, 10, &mut out).unwrap();
        assert_eq!(out, vec![1, 2, 3, 4]);
    }

    #[test]
    fn storage_error_messages_are_danish() {
        let full = io::Error::from_raw_os_error(DISK_FULL);
        assert_eq!(
            storage_error_message(&full),
            "Lyden gemmes ikke: disken er fuld"
        );
        let denied = io::Error::new(io::ErrorKind::PermissionDenied, "nope");
        assert_eq!(
            storage_error_message(&denied),
            "Lyden gemmes ikke: filen er låst eller utilgængelig"
        );
    }

    #[test]
    fn converts_and_clamps_samples() {
        assert_eq!(f32_to_i16(0.0), 0);
        assert_eq!(f32_to_i16(1.5), i16::MAX);
        assert_eq!(f32_to_i16(-1.5), -i16::MAX);
        assert_eq!(f32_to_i16(f32::NAN), 0);
    }
}
