//! Turns the raw tracks of a meeting into the final audio files, after a
//! normal stop or after a crash.
//!
//! 1. Scan the WAV segments in `.checkpoints/` ([`scan_raw_tracks`]): repair
//!    headers that lag their data (crash between data and header write),
//!    keep the readable part of truncated files and skip unreadable ones.
//! 2. Encode `microphone.m4a`, `system.m4a` and the mixed `audio.mp4`, all on
//!    the shared recording clock (a track's offset becomes leading silence).
//! 3. Decode each file and require at most [`MAX_DURATION_DEVIATION`] from the
//!    recorded length.
//! 4. Delete the raw tracks only when the status is [`AudioSaveStatus::Ok`].
//!    Anything else keeps them: "Lyden kunne ikke samles. Originalen er gemt."

#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::collections::HashSet;
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use super::encode::encode_pcm_s16;
use super::recording_writer::{read_manifest, TrackManifest};
use super::track_writer::{
    inspect_wav, read_wav_samples, update_wav_sizes, TrackKind, SEGMENT_LENGTH,
};

/// Raw tracks live here inside the meeting folder (the name predates the
/// WAV writer; the frontend's recovery looks for it).
pub const RAW_DIR: &str = ".checkpoints";
pub const MIXED_FILE: &str = "audio.mp4";
/// Written into the raw folder after verification.
pub const VERIFIED_MARKER: &str = "verified.json";
/// Largest allowed difference between a final file and its recorded length.
pub const MAX_DURATION_DEVIATION: f64 = 1.0;

pub const KEPT_MESSAGE: &str = "Lyden kunne ikke samles. Originalen er gemt.";

/// File name of an encoded track: `microphone.m4a`.
pub fn track_file_name(kind: TrackKind) -> String {
    format!("{}.m4a", kind.file_stem())
}

/// Outcome of saving a recording, shown to the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AudioSaveStatus {
    /// Everything recorded is in the final files; the raw tracks are deleted.
    #[serde(rename = "ok")]
    Ok,
    /// The final files hold what could be read; some audio is missing. The
    /// raw tracks are kept.
    #[serde(rename = "delvis")]
    Partial,
    /// No usable final file; the raw tracks are kept.
    #[serde(rename = "fejlet")]
    Failed,
}

impl AudioSaveStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            AudioSaveStatus::Ok => "ok",
            AudioSaveStatus::Partial => "delvis",
            AudioSaveStatus::Failed => "fejlet",
        }
    }
}

/// Where a track's samples come from, in order.
#[derive(Debug, Clone, PartialEq)]
pub enum Source {
    File { path: PathBuf, samples: u64 },
    Silence(u64),
}

impl Source {
    fn len(&self) -> u64 {
        match self {
            Source::File { samples, .. } => *samples,
            Source::Silence(n) => *n,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ScannedTrack {
    pub kind: TrackKind,
    pub offset_samples: u64,
    pub sources: Vec<Source>,
    pub segment_count: usize,
}

impl ScannedTrack {
    /// Samples of the track itself (without the offset).
    pub fn samples(&self) -> u64 {
        self.sources.iter().map(Source::len).sum()
    }

    pub fn end(&self) -> u64 {
        self.offset_samples + self.samples()
    }

    fn audio_samples(&self) -> u64 {
        self.sources
            .iter()
            .map(|s| match s {
                Source::File { samples, .. } => *samples,
                Source::Silence(_) => 0,
            })
            .sum()
    }
}

#[derive(Debug, Clone)]
pub struct ScannedTracks {
    pub sample_rate: u32,
    pub tracks: Vec<ScannedTrack>,
    /// Missing, unreadable or truncated pieces (Danish, for the message).
    pub problems: Vec<String>,
    /// The writer finished normally (stop, not a crash).
    pub complete: bool,
}

impl ScannedTracks {
    pub fn end(&self) -> u64 {
        self.tracks.iter().map(ScannedTrack::end).max().unwrap_or(0)
    }

    pub fn duration_seconds(&self) -> f64 {
        if self.sample_rate == 0 {
            return 0.0;
        }
        self.end() as f64 / self.sample_rate as f64
    }

    pub fn segment_count(&self) -> usize {
        self.tracks.iter().map(|t| t.segment_count).sum()
    }

    pub fn has_audio(&self) -> bool {
        self.tracks.iter().any(|t| t.audio_samples() > 0)
    }
}

/// `true` if the raw folder holds WAV segments from the track writer.
pub fn has_raw_tracks(raw_dir: &Path) -> bool {
    list_segments(raw_dir)
        .map(|s| !s.is_empty())
        .unwrap_or(false)
}

/// `(kind, index, path)` of every `microphone_NNN.wav` / `system_NNN.wav`.
fn list_segments(raw_dir: &Path) -> io::Result<Vec<(TrackKind, u32, PathBuf)>> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(raw_dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        for kind in TrackKind::ALL {
            let prefix = format!("{}_", kind.file_stem());
            if let Some(index) = name
                .strip_prefix(&prefix)
                .and_then(|rest| rest.strip_suffix(".wav"))
                .and_then(|n| n.parse::<u32>().ok())
            {
                found.push((kind, index, entry.path()));
            }
        }
    }
    found.sort_by_key(|a| (a.0, a.1));
    Ok(found)
}

/// Read what is on disk. With `repair`, headers that lag their data are
/// rewritten so the files play in any player.
pub fn scan_raw_tracks(raw_dir: &Path, repair: bool) -> io::Result<ScannedTracks> {
    let mut problems = Vec::new();
    let manifest = match read_manifest(raw_dir) {
        Ok(m) => Some(m),
        Err(e) => {
            log::warn!("No usable tracks.json in {}: {e}", raw_dir.display());
            None
        }
    };
    let segments = list_segments(raw_dir)?;
    let mut sample_rate = manifest.as_ref().map(|m| m.sample_rate).filter(|&r| r > 0);
    let mut tracks = Vec::new();

    for kind in TrackKind::ALL {
        let files: Vec<_> = segments.iter().filter(|s| s.0 == kind).collect();
        if files.is_empty() {
            continue;
        }
        let mut sources = Vec::new();
        let mut expected_index = 0u32;
        for (position, (_, index, path)) in files.iter().enumerate() {
            let is_last = position + 1 == files.len();
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            while expected_index < *index {
                problems.push(format!(
                    "{} mangler",
                    super::track_writer::segment_file_name(kind, expected_index)
                ));
                sources.push(Source::Silence(segment_samples(sample_rate)));
                expected_index += 1;
            }
            expected_index = index + 1;
            match inspect_wav(path) {
                Err(e) => {
                    log::warn!("Skipping unreadable segment {}: {e}", path.display());
                    problems.push(format!("{name} er ødelagt"));
                    if !is_last {
                        // Keep the later segments in place on the time axis.
                        sources.push(Source::Silence(segment_samples(sample_rate)));
                    }
                }
                Ok(wav) => {
                    if sample_rate.is_none() {
                        sample_rate = Some(wav.sample_rate);
                    }
                    if wav.truncated() {
                        log::warn!(
                            "Segment {} is truncated: header says {} bytes, file holds {}",
                            path.display(),
                            wav.header_data_bytes,
                            wav.actual_data_bytes
                        );
                        problems.push(format!("{name} er afkortet"));
                    }
                    // A truncated header is left as it is: shrinking it would
                    // hide the loss from the next scan (and turn `delvis`
                    // into `ok`). The reader stops at the real data.
                    if repair && wav.header_lags() {
                        if let Err(e) = repair_header(path, wav.actual_data_bytes) {
                            log::warn!("Could not repair the header of {}: {e}", path.display());
                        } else {
                            log::info!(
                                "Repaired header of {} ({} samples)",
                                path.display(),
                                wav.samples()
                            );
                        }
                    }
                    sources.push(Source::File {
                        path: (*path).clone(),
                        samples: wav.samples(),
                    });
                }
            }
        }
        let entry = manifest
            .as_ref()
            .and_then(|m| m.tracks.iter().find(|t| t.kind == kind));
        if manifest.is_some() && entry.is_none() {
            log::warn!(
                "{} track is not in tracks.json; assuming offset 0",
                kind.file_stem()
            );
        }
        let track = ScannedTrack {
            kind,
            offset_samples: entry.map_or(0, |t| t.offset_samples),
            sources,
            segment_count: files.len(),
        };
        if let Some(recorded) = entry.and_then(|t| t.recorded_samples) {
            if track.samples() < recorded {
                problems.push(format!(
                    "{}-sporet mangler {:.1} s",
                    danish_track_name(kind),
                    (recorded - track.samples()) as f64 / sample_rate.unwrap_or(48000) as f64
                ));
            }
        }
        if let Some(lost) = entry.map(|t| t.lost_samples).filter(|&l| l > 0) {
            problems.push(format!(
                "{:.1} s af {}-sporet nåede aldrig disken",
                lost as f64 / sample_rate.unwrap_or(48000) as f64,
                danish_track_name(kind)
            ));
        }
        tracks.push(track);
    }

    // After a crash the tracks end where each was last committed; pad them
    // to a common end like the writer does at stop.
    let end = tracks.iter().map(ScannedTrack::end).max().unwrap_or(0);
    for track in &mut tracks {
        let pad = end - track.end();
        if pad > 0 {
            track.sources.push(Source::Silence(pad));
        }
    }

    Ok(ScannedTracks {
        sample_rate: sample_rate.unwrap_or(48000),
        tracks,
        problems,
        complete: manifest.is_some_and(|m: TrackManifest| m.complete),
    })
}

fn danish_track_name(kind: TrackKind) -> &'static str {
    match kind {
        TrackKind::Microphone => "mikrofon",
        TrackKind::System => "system",
    }
}

fn segment_samples(sample_rate: Option<u32>) -> u64 {
    (SEGMENT_LENGTH.as_secs_f64() * sample_rate.unwrap_or(48000) as f64) as u64
}

fn repair_header(path: &Path, data_bytes: u64) -> io::Result<()> {
    let mut file = std::fs::OpenOptions::new().write(true).open(path)?;
    update_wav_sizes(&mut file, data_bytes)?;
    file.sync_data()
}

/// Reads a list of sources as one stream of samples.
pub struct SourceReader {
    sources: Vec<Source>,
    index: usize,
    position: u64,
    file: Option<File>,
    scratch: Vec<i16>,
}

impl SourceReader {
    pub fn new(sources: Vec<Source>) -> Self {
        Self {
            sources,
            index: 0,
            position: 0,
            file: None,
            scratch: Vec::new(),
        }
    }

    /// Fill `out` with up to `max` samples; `Ok(false)` at the end.
    pub fn read_block(&mut self, out: &mut Vec<i16>, max: usize) -> io::Result<bool> {
        out.clear();
        while out.len() < max {
            let Some(source) = self.sources.get(self.index) else {
                break;
            };
            let remaining = source.len() - self.position;
            if remaining == 0 {
                self.index += 1;
                self.position = 0;
                self.file = None;
                continue;
            }
            let want = ((max - out.len()) as u64).min(remaining) as usize;
            match source {
                Source::Silence(_) => {
                    out.resize(out.len() + want, 0);
                    self.position += want as u64;
                }
                Source::File { path, .. } => {
                    if self.file.is_none() {
                        self.file = Some(File::open(path)?);
                    }
                    let Some(file) = self.file.as_mut() else {
                        break;
                    };
                    let got = read_wav_samples(file, self.position, want, &mut self.scratch)?;
                    if got == 0 {
                        // The file shrank since the scan: treat the rest as silence.
                        out.resize(out.len() + want, 0);
                        self.position += want as u64;
                    } else {
                        out.extend_from_slice(&self.scratch[..got]);
                        self.position += got as u64;
                    }
                }
            }
        }
        Ok(!out.is_empty())
    }
}

/// Track sources on the shared clock: offset as leading silence.
fn timeline_sources(track: &ScannedTrack) -> Vec<Source> {
    let mut sources = Vec::with_capacity(track.sources.len() + 1);
    if track.offset_samples > 0 {
        sources.push(Source::Silence(track.offset_samples));
    }
    sources.extend(track.sources.iter().cloned());
    sources
}

/// Sum two tracks without clipping: a sum beyond full scale is scaled back
/// to full scale (the live mixer's rule).
pub fn mix_samples(a: i16, b: i16) -> i16 {
    let sum = a as f32 / i16::MAX as f32 + b as f32 / i16::MAX as f32;
    let scaled = if sum.abs() > 1.0 {
        sum / sum.abs()
    } else {
        sum
    };
    (scaled * i16::MAX as f32).round() as i16
}

const BLOCK: usize = 48_000;

fn encode_track(
    ffmpeg: &Path,
    sample_rate: u32,
    track: &ScannedTrack,
    output: &Path,
) -> anyhow::Result<()> {
    let mut reader = SourceReader::new(timeline_sources(track));
    encode_pcm_s16(ffmpeg, sample_rate, "96k", output, |block| {
        reader.read_block(block, BLOCK)
    })
}

fn encode_mix(ffmpeg: &Path, scanned: &ScannedTracks, output: &Path) -> anyhow::Result<()> {
    let mut readers: Vec<SourceReader> = scanned
        .tracks
        .iter()
        .map(|t| SourceReader::new(timeline_sources(t)))
        .collect();
    let mut part = Vec::new();
    encode_pcm_s16(ffmpeg, scanned.sample_rate, "128k", output, |block| {
        block.clear();
        for reader in readers.iter_mut() {
            reader.read_block(&mut part, BLOCK)?;
            if block.len() < part.len() {
                block.resize(part.len(), 0);
            }
            for (out, &s) in block.iter_mut().zip(part.iter()) {
                *out = mix_samples(*out, s);
            }
        }
        Ok(!block.is_empty())
    })
}

/// Length of an audio file found by decoding all of it (the container's own
/// duration is not trusted: with `+faststart` it is written before the data).
pub fn decoded_duration_seconds(path: &Path) -> anyhow::Result<f64> {
    use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
    use symphonia::core::errors::Error as SymphoniaError;
    use symphonia::core::formats::FormatOptions;
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;
    use symphonia::core::probe::Hint;

    let file = File::open(path)?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| anyhow::anyhow!("cannot read {}: {e}", path.display()))?;
    let mut format = probed.format;
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or_else(|| anyhow::anyhow!("no audio track in {}", path.display()))?;
    let track_id = track.id;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| anyhow::anyhow!("no decoder for {}: {e}", path.display()))?;
    let mut frames = 0u64;
    let mut rate = track.codec_params.sample_rate.unwrap_or(0);
    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(SymphoniaError::IoError(e)) if e.kind() == io::ErrorKind::UnexpectedEof => break,
            Err(SymphoniaError::ResetRequired) => break,
            Err(e) => return Err(anyhow::anyhow!("reading {} failed: {e}", path.display())),
        };
        if packet.track_id() != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(decoded) => {
                frames += decoded.frames() as u64;
                rate = decoded.spec().rate;
            }
            Err(SymphoniaError::DecodeError(e)) => {
                log::warn!("Decode error in {}: {e}", path.display())
            }
            Err(e) => return Err(anyhow::anyhow!("decoding {} failed: {e}", path.display())),
        }
    }
    if rate == 0 {
        return Err(anyhow::anyhow!("unknown sample rate in {}", path.display()));
    }
    Ok(frames as f64 / rate as f64)
}

/// Result of [`finalize_raw_tracks`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinalizeOutcome {
    pub status: AudioSaveStatus,
    /// Danish message for the user.
    pub message: String,
    pub audio_file: Option<PathBuf>,
    pub track_files: Vec<PathBuf>,
    pub duration_seconds: f64,
    pub segment_count: usize,
    pub raw_kept: bool,
    pub problems: Vec<String>,
}

impl FinalizeOutcome {
    fn failed(message: String, scanned: Option<&ScannedTracks>) -> Self {
        Self {
            status: AudioSaveStatus::Failed,
            message,
            audio_file: None,
            track_files: Vec::new(),
            duration_seconds: scanned.map_or(0.0, |s| s.duration_seconds()),
            segment_count: scanned.map_or(0, |s| s.segment_count()),
            raw_kept: true,
            problems: scanned.map(|s| s.problems.clone()).unwrap_or_default(),
        }
    }
}

/// Folders being finalized right now (stop in the background and recovery
/// must not encode the same folder twice).
static IN_PROGRESS: Mutex<Option<HashSet<PathBuf>>> = Mutex::new(None);

struct FolderGuard(PathBuf);

impl FolderGuard {
    fn acquire(folder: &Path) -> Option<Self> {
        let mut guard = IN_PROGRESS.lock().unwrap_or_else(|e| e.into_inner());
        let set = guard.get_or_insert_with(HashSet::new);
        if set.insert(folder.to_path_buf()) {
            Some(Self(folder.to_path_buf()))
        } else {
            None
        }
    }
}

impl Drop for FolderGuard {
    fn drop(&mut self) {
        let mut guard = IN_PROGRESS.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(set) = guard.as_mut() {
            set.remove(&self.0);
        }
    }
}

/// Encode, verify and (only on `Ok`) delete the raw tracks of a meeting
/// folder. Blocking; run it on a blocking thread. `ffmpeg` is the encoder to
/// use (`None`: not found).
pub fn finalize_raw_tracks(meeting_folder: &Path, ffmpeg: Option<&Path>) -> FinalizeOutcome {
    let Some(_guard) = FolderGuard::acquire(meeting_folder) else {
        return FinalizeOutcome::failed("Lyden samles allerede.".to_string(), None);
    };
    let raw_dir = meeting_folder.join(RAW_DIR);
    let started = std::time::Instant::now();
    let scanned = match scan_raw_tracks(&raw_dir, true) {
        Ok(s) => s,
        Err(e) => {
            log::error!("Cannot read the raw tracks in {}: {e}", raw_dir.display());
            return FinalizeOutcome::failed(format!("{KEPT_MESSAGE} ({e})"), None);
        }
    };
    if !scanned.has_audio() {
        return FinalizeOutcome::failed("Der er ingen lyd at samle.".to_string(), Some(&scanned));
    }
    let Some(ffmpeg) = ffmpeg else {
        log::error!("FFmpeg not found; raw tracks kept in {}", raw_dir.display());
        return finish(
            &raw_dir,
            FinalizeOutcome::failed(
                format!("{KEPT_MESSAGE} FFmpeg blev ikke fundet."),
                Some(&scanned),
            ),
        );
    };

    let expected = scanned.duration_seconds();
    let mut outputs: Vec<(PathBuf, PathBuf)> = Vec::new();
    for track in &scanned.tracks {
        let file = meeting_folder.join(track_file_name(track.kind));
        outputs.push((file.clone(), temp_path(&file)));
    }
    let mixed = meeting_folder.join(MIXED_FILE);
    outputs.push((mixed.clone(), temp_path(&mixed)));

    let mut encode_error = None;
    for (i, (_, tmp)) in outputs.iter().enumerate() {
        let result = match scanned.tracks.get(i) {
            Some(track) => encode_track(ffmpeg, scanned.sample_rate, track, tmp),
            None => encode_mix(ffmpeg, &scanned, tmp),
        };
        if let Err(e) = result {
            log::error!("Encoding {} failed: {e:#}", tmp.display());
            encode_error = Some(format!("{e:#}"));
            break;
        }
    }
    if let Some(error) = encode_error {
        remove_temps(&outputs);
        return finish(
            &raw_dir,
            FinalizeOutcome::failed(format!("{KEPT_MESSAGE} ({error})"), Some(&scanned)),
        );
    }

    for (_, tmp) in &outputs {
        match decoded_duration_seconds(tmp) {
            Ok(actual) if (actual - expected).abs() <= MAX_DURATION_DEVIATION => {
                log::info!(
                    "Verified {}: {actual:.2}s (recorded {expected:.2}s)",
                    tmp.display()
                );
            }
            Ok(actual) => {
                log::error!(
                    "{} is {actual:.2}s but {expected:.2}s were recorded; raw tracks kept",
                    tmp.display()
                );
                remove_temps(&outputs);
                return finish(
                    &raw_dir,
                    FinalizeOutcome::failed(
                        format!("{KEPT_MESSAGE} Den samlede fil er {actual:.1} s, men der blev optaget {expected:.1} s."),
                        Some(&scanned),
                    ),
                );
            }
            Err(e) => {
                log::error!("Cannot verify {}: {e:#}", tmp.display());
                remove_temps(&outputs);
                return finish(
                    &raw_dir,
                    FinalizeOutcome::failed(format!("{KEPT_MESSAGE} ({e})"), Some(&scanned)),
                );
            }
        }
    }
    for (file, tmp) in &outputs {
        if let Err(e) = std::fs::rename(tmp, file) {
            log::error!("Cannot move {} into place: {e}", file.display());
            remove_temps(&outputs);
            return finish(
                &raw_dir,
                FinalizeOutcome::failed(format!("{KEPT_MESSAGE} ({e})"), Some(&scanned)),
            );
        }
    }

    let status = if scanned.problems.is_empty() {
        AudioSaveStatus::Ok
    } else {
        AudioSaveStatus::Partial
    };
    let message = match status {
        AudioSaveStatus::Ok => "Lyden er gemt.".to_string(),
        _ => format!(
            "Lyden er delvist gemt: {}. Originalen er gemt.",
            scanned.problems.join(", ")
        ),
    };
    log::info!(
        "Finalized {} in {:.1}s: status {}, {:.1}s of audio",
        meeting_folder.display(),
        started.elapsed().as_secs_f64(),
        status.as_str(),
        expected
    );
    let outcome = FinalizeOutcome {
        status,
        message,
        audio_file: Some(mixed),
        track_files: outputs
            .iter()
            .take(scanned.tracks.len())
            .map(|(f, _)| f.clone())
            .collect(),
        duration_seconds: expected,
        segment_count: scanned.segment_count(),
        raw_kept: true,
        problems: scanned.problems.clone(),
    };
    finish(&raw_dir, outcome)
}

/// Record the outcome next to the raw tracks, and delete them on `Ok`.
fn finish(raw_dir: &Path, mut outcome: FinalizeOutcome) -> FinalizeOutcome {
    match serde_json::to_string_pretty(&outcome) {
        Ok(json) => {
            if let Err(e) = std::fs::write(raw_dir.join(VERIFIED_MARKER), json) {
                log::warn!("Could not write {VERIFIED_MARKER}: {e}");
            }
        }
        Err(e) => log::warn!("Could not serialize the finalize outcome: {e}"),
    }
    if outcome.status == AudioSaveStatus::Ok {
        match std::fs::remove_dir_all(raw_dir) {
            Ok(()) => {
                outcome.raw_kept = false;
                log::info!(
                    "Raw tracks deleted after verification: {}",
                    raw_dir.display()
                );
            }
            Err(e) => log::warn!(
                "Could not delete the raw tracks in {}: {e}",
                raw_dir.display()
            ),
        }
    } else {
        log::warn!(
            "Raw tracks kept in {} (status {})",
            raw_dir.display(),
            outcome.status.as_str()
        );
    }
    if let Some(folder) = raw_dir.parent() {
        if let Err(e) = update_metadata(folder, &outcome) {
            log::warn!(
                "Could not update metadata.json in {}: {e}",
                folder.display()
            );
        }
    }
    outcome
}

/// Finalize attempts recorded in a stopped meeting's raw folder.
const ATTEMPTS_FILE: &str = "finalize_attempts";

/// Startup retries before a failing meeting is left for the user.
pub const MAX_STARTUP_ATTEMPTS: u32 = 3;

/// Meeting folders under `recordings_dir` whose recording was stopped
/// normally but whose final files were never made: the app closed during
/// the background encode, or FFmpeg failed. Crashed recordings (manifest not
/// complete) are left to the recovery dialog, and `partial` results are
/// final.
pub fn pending_finalize_folders(recordings_dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(recordings_dir) else {
        return Vec::new();
    };
    let mut folders: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|folder| {
            let raw = folder.join(RAW_DIR);
            has_raw_tracks(&raw)
                && read_manifest(&raw).is_ok_and(|m| m.complete)
                && matches!(read_verified_status(&raw), None | Some(AudioSaveStatus::Failed))
                && read_attempts(&raw) < MAX_STARTUP_ATTEMPTS
        })
        .collect();
    folders.sort();
    folders
}

fn read_attempts(raw_dir: &Path) -> u32 {
    std::fs::read_to_string(raw_dir.join(ATTEMPTS_FILE))
        .ok()
        .and_then(|t| t.trim().parse().ok())
        .unwrap_or(0)
}

/// Finalize every folder from [`pending_finalize_folders`]. Blocking; run it
/// on a blocking thread at startup, before a recording can start.
pub fn finalize_pending_recordings(
    recordings_dir: &Path,
    ffmpeg: Option<&Path>,
) -> Vec<(PathBuf, FinalizeOutcome)> {
    pending_finalize_folders(recordings_dir)
        .into_iter()
        .map(|folder| {
            let raw = folder.join(RAW_DIR);
            let attempts = read_attempts(&raw) + 1;
            if let Err(e) = std::fs::write(raw.join(ATTEMPTS_FILE), attempts.to_string()) {
                log::warn!("Could not record the finalize attempt in {}: {e}", raw.display());
            }
            log::info!(
                "Finishing the audio of {} (startup attempt {attempts})",
                folder.display()
            );
            let outcome = finalize_raw_tracks(&folder, ffmpeg);
            (folder, outcome)
        })
        .collect()
}

/// Run [`finalize_pending_recordings`] on the recordings folder. Call once at
/// app startup.
pub async fn finalize_pending_at_startup() {
    let recordings = super::recording_preferences::get_default_recordings_folder();
    let result = tokio::task::spawn_blocking(move || {
        let ffmpeg = super::ffmpeg::find_ffmpeg_path();
        finalize_pending_recordings(&recordings, ffmpeg.as_deref())
    })
    .await;
    match result {
        Ok(done) => {
            for (folder, outcome) in done {
                log::info!(
                    "Startup finalize of {}: {} ({})",
                    folder.display(),
                    outcome.status.as_str(),
                    outcome.message
                );
            }
        }
        Err(e) => log::error!("Startup finalize panicked: {e}"),
    }
}

/// Status of the last finalize, read from the marker in the raw folder.
pub fn read_verified_status(raw_dir: &Path) -> Option<AudioSaveStatus> {
    let text = std::fs::read_to_string(raw_dir.join(VERIFIED_MARKER)).ok()?;
    serde_json::from_str::<FinalizeOutcome>(&text)
        .ok()
        .map(|o| o.status)
}

/// Add the audio outcome to the meeting's metadata.json (if it exists).
fn update_metadata(folder: &Path, outcome: &FinalizeOutcome) -> io::Result<()> {
    let path = folder.join("metadata.json");
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    let mut value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let Some(map) = value.as_object_mut() else {
        return Ok(());
    };
    let file_name = |p: &PathBuf| p.file_name().map(|n| n.to_string_lossy().to_string());
    map.insert("audio_status".into(), outcome.status.as_str().into());
    map.insert("audio_message".into(), outcome.message.clone().into());
    if let Some(name) = outcome.audio_file.as_ref().and_then(file_name) {
        map.insert("audio_file".into(), name.into());
    }
    let tracks: Vec<String> = outcome.track_files.iter().filter_map(file_name).collect();
    map.insert("track_files".into(), tracks.into());
    let json = serde_json::to_string_pretty(&value).map_err(io::Error::other)?;
    let tmp = folder.join(".metadata.json.tmp");
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, &path)
}

fn temp_path(file: &Path) -> PathBuf {
    let name = file
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    file.with_file_name(format!(".{name}.tmp"))
}

fn remove_temps(outputs: &[(PathBuf, PathBuf)]) {
    for (_, tmp) in outputs {
        let _ = std::fs::remove_file(tmp);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::super::recording_writer::{
        spawn_writer, write_manifest, LogObserver, ManifestTrack,
    };
    use super::super::track_writer::{segment_file_name, write_wav_header};
    use super::*;
    use std::io::Write;
    use std::sync::Arc;

    fn ffmpeg() -> Option<PathBuf> {
        super::super::ffmpeg::find_ffmpeg_path()
    }

    /// Record `seconds` of a tone on both tracks through the real writer.
    fn record(folder: &Path, seconds: u32) {
        let raw = folder.join(RAW_DIR);
        std::fs::create_dir_all(&raw).unwrap();
        let (tx, handle) = spawn_writer(raw, 48000, Arc::new(LogObserver)).unwrap();
        for i in 0..seconds * 100 {
            let end = (i + 1) as f64 * 0.01;
            let tone: Vec<f32> = (0..480)
                .map(|n| ((i * 480 + n) as f32 * 0.05).sin() * 0.3)
                .collect();
            tx.send(TrackKind::Microphone, tone.clone(), Some(end))
                .unwrap();
            tx.send(TrackKind::System, tone, Some(end)).unwrap();
        }
        drop(tx);
        handle.join().unwrap();
    }

    #[test]
    fn mixes_without_clipping() {
        assert_eq!(mix_samples(1000, 2000), 3000);
        assert_eq!(mix_samples(i16::MAX, i16::MAX), i16::MAX);
        assert_eq!(mix_samples(-30000, -30000), -i16::MAX);
    }

    #[test]
    fn finalize_encodes_verifies_and_deletes_raw_tracks() {
        let Some(ffmpeg) = ffmpeg() else {
            eprintln!("ffmpeg not available, skipping");
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        record(dir.path(), 3);
        let outcome = finalize_raw_tracks(dir.path(), Some(&ffmpeg));
        assert_eq!(outcome.status, AudioSaveStatus::Ok, "{}", outcome.message);
        assert!(!outcome.raw_kept);
        assert!(!dir.path().join(RAW_DIR).exists());
        for name in ["audio.mp4", "microphone.m4a", "system.m4a"] {
            let d = decoded_duration_seconds(&dir.path().join(name)).unwrap();
            assert!((d - 3.0).abs() < 0.2, "{name} is {d}s");
        }
    }

    /// An ffmpeg that cannot run: the final encode fails visibly and the raw
    /// tracks stay.
    #[test]
    fn unusable_ffmpeg_keeps_raw_tracks() {
        let dir = tempfile::tempdir().unwrap();
        record(dir.path(), 1);
        let missing = dir.path().join("no-such-ffmpeg");
        let outcome = finalize_raw_tracks(dir.path(), Some(&missing));
        assert_eq!(outcome.status, AudioSaveStatus::Failed);
        assert!(
            outcome.message.starts_with(KEPT_MESSAGE),
            "{}",
            outcome.message
        );
        assert!(outcome.raw_kept);
        assert!(has_raw_tracks(&dir.path().join(RAW_DIR)));
        assert!(!dir.path().join(MIXED_FILE).exists());
        assert_eq!(
            read_verified_status(&dir.path().join(RAW_DIR)),
            Some(AudioSaveStatus::Failed)
        );

        let not_found = finalize_raw_tracks(dir.path(), None);
        assert_eq!(not_found.status, AudioSaveStatus::Failed);
        assert!(has_raw_tracks(&dir.path().join(RAW_DIR)));
    }

    /// A merge whose result has the wrong length must not delete the raw
    /// tracks: a fake ffmpeg that writes a valid but short file.
    #[cfg(unix)]
    #[test]
    fn wrong_duration_keeps_raw_tracks() {
        let Some(real) = ffmpeg() else {
            eprintln!("ffmpeg not available, skipping");
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        record(dir.path(), 3);
        // Reads all input but plays it three times too fast: a valid file of
        // a third of the length.
        let fake = dir.path().join("short-ffmpeg.sh");
        let script = format!(
            "#!/bin/bash\nargs=(\"$@\")\nout=\"${{args[-1]}}\"\nunset 'args[-1]'\nexec \"{}\" \"${{args[@]}}\" -af asetrate=144000 \"$out\"\n",
            real.display()
        );
        std::fs::write(&fake, script).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();

        let outcome = finalize_raw_tracks(dir.path(), Some(&fake));
        assert_eq!(
            outcome.status,
            AudioSaveStatus::Failed,
            "{}",
            outcome.message
        );
        assert!(
            outcome.message.contains("der blev optaget"),
            "{}",
            outcome.message
        );
        assert!(has_raw_tracks(&dir.path().join(RAW_DIR)));
        assert!(!dir.path().join(MIXED_FILE).exists());
    }

    /// A stopped meeting whose background encode never ran (app closed) is
    /// finished at the next startup; a crashed one is left to recovery, and a
    /// meeting that keeps failing is retried a limited number of times.
    #[test]
    fn startup_finishes_stopped_meetings_only() {
        let recordings = tempfile::tempdir().unwrap();
        let stopped = recordings.path().join("stopped");
        let crashed = recordings.path().join("crashed");
        record(&stopped, 1);
        record(&crashed, 1);
        let raw = crashed.join(RAW_DIR);
        let mut manifest = read_manifest(&raw).unwrap();
        manifest.complete = false;
        write_manifest(&raw, &manifest).unwrap();
        std::fs::create_dir_all(recordings.path().join("empty")).unwrap();

        assert_eq!(pending_finalize_folders(recordings.path()), vec![stopped.clone()]);

        // FFmpeg missing: failed, kept, and retried only MAX_STARTUP_ATTEMPTS times.
        let missing = recordings.path().join("no-such-ffmpeg");
        for _ in 0..MAX_STARTUP_ATTEMPTS {
            let done = finalize_pending_recordings(recordings.path(), Some(&missing));
            assert_eq!(done.len(), 1);
            assert_eq!(done[0].1.status, AudioSaveStatus::Failed);
        }
        assert!(pending_finalize_folders(recordings.path()).is_empty());
        assert!(has_raw_tracks(&stopped.join(RAW_DIR)));
        assert!(has_raw_tracks(&crashed.join(RAW_DIR)));

        let Some(ffmpeg) = ffmpeg() else {
            eprintln!("ffmpeg not available, skipping the encode");
            return;
        };
        std::fs::remove_file(stopped.join(RAW_DIR).join(ATTEMPTS_FILE)).unwrap();
        let done = finalize_pending_recordings(recordings.path(), Some(&ffmpeg));
        assert_eq!(done.len(), 1);
        assert_eq!(done[0].1.status, AudioSaveStatus::Ok, "{}", done[0].1.message);
        assert!(stopped.join(MIXED_FILE).exists());
        assert!(!stopped.join(RAW_DIR).exists());
        assert!(pending_finalize_folders(recordings.path()).is_empty());
    }

    fn write_segment(path: &Path, samples: &[i16], header_samples: u64) {
        let mut file = File::create(path).unwrap();
        write_wav_header(&mut file, 48000, header_samples * 2).unwrap();
        file.seek(io::SeekFrom::Start(44)).unwrap();
        for s in samples {
            file.write_all(&s.to_le_bytes()).unwrap();
        }
    }
    use std::io::Seek;

    #[test]
    fn scan_repairs_lagging_headers_without_loss() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(segment_file_name(TrackKind::Microphone, 0));
        // Crash after the data write, before the header update.
        write_segment(&path, &[7; 1000], 600);
        let scanned = scan_raw_tracks(dir.path(), true).unwrap();
        assert!(scanned.problems.is_empty(), "{:?}", scanned.problems);
        assert_eq!(scanned.tracks[0].samples(), 1000);
        assert_eq!(inspect_wav(&path).unwrap().header_data_bytes, 2000);
    }

    #[test]
    fn truncated_last_segment_gives_partial_with_the_other_pieces() {
        let dir = tempfile::tempdir().unwrap();
        write_manifest(
            dir.path(),
            &TrackManifest {
                version: 1,
                sample_rate: 48000,
                tracks: vec![ManifestTrack {
                    kind: TrackKind::Microphone,
                    offset_samples: 0,
                    recorded_samples: None,
                    lost_samples: 0,
                    segments: Vec::new(),
                }],
                complete: false,
            },
        )
        .unwrap();
        write_segment(
            &dir.path().join(segment_file_name(TrackKind::Microphone, 0)),
            &[1; 4800],
            4800,
        );
        write_segment(
            &dir.path().join(segment_file_name(TrackKind::Microphone, 1)),
            &[2; 4800],
            4800,
        );
        // Last segment: header claims 4800 samples, only 1000 made it.
        write_segment(
            &dir.path().join(segment_file_name(TrackKind::Microphone, 2)),
            &[3; 1000],
            4800,
        );
        let scanned = scan_raw_tracks(dir.path(), false).unwrap();
        assert_eq!(
            scanned.problems,
            vec!["microphone_002.wav er afkortet".to_string()]
        );
        assert_eq!(scanned.tracks[0].samples(), 4800 + 4800 + 1000);
        // A repairing scan does not hide the loss from the next run.
        for _ in 0..2 {
            let scanned = scan_raw_tracks(dir.path(), true).unwrap();
            assert_eq!(
                scanned.problems,
                vec!["microphone_002.wav er afkortet".to_string()]
            );
        }

        // Unreadable last segment: skipped, the others remain.
        std::fs::write(
            dir.path().join(segment_file_name(TrackKind::Microphone, 2)),
            b"garbage",
        )
        .unwrap();
        let scanned = scan_raw_tracks(dir.path(), false).unwrap();
        assert_eq!(
            scanned.problems,
            vec!["microphone_002.wav er ødelagt".to_string()]
        );
        assert_eq!(scanned.tracks[0].samples(), 9600);

        if let Some(ffmpeg) = ffmpeg() {
            let folder = dir
                .path()
                .parent()
                .unwrap()
                .join(format!("meeting-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&folder);
            std::fs::create_dir_all(&folder).unwrap();
            let raw = folder.join(RAW_DIR);
            std::fs::rename(dir.path(), &raw).unwrap();
            let outcome = finalize_raw_tracks(&folder, Some(&ffmpeg));
            assert_eq!(
                outcome.status,
                AudioSaveStatus::Partial,
                "{}",
                outcome.message
            );
            assert!(outcome.raw_kept, "raw tracks are only deleted on ok");
            assert!(raw.exists());
            assert!(folder.join(MIXED_FILE).exists());
            let d = decoded_duration_seconds(&folder.join(MIXED_FILE)).unwrap();
            assert!((d - 0.2).abs() < 0.1, "{d}");
            std::fs::remove_dir_all(&folder).unwrap();
        }
    }

    #[test]
    fn missing_middle_segment_keeps_the_time_axis() {
        let dir = tempfile::tempdir().unwrap();
        write_segment(
            &dir.path().join(segment_file_name(TrackKind::System, 0)),
            &[1; 100],
            100,
        );
        write_segment(
            &dir.path().join(segment_file_name(TrackKind::System, 2)),
            &[1; 100],
            100,
        );
        let scanned = scan_raw_tracks(dir.path(), false).unwrap();
        assert_eq!(scanned.problems, vec!["system_001.wav mangler".to_string()]);
        assert_eq!(
            scanned.tracks[0].samples(),
            200 + SEGMENT_LENGTH.as_secs() * 48000
        );
    }

    #[test]
    fn crash_recovery_loses_at_most_two_seconds() {
        // Synthetic 10 s recording through the real writer thread, killed
        // without stop: the thread is leaked with its channel still open.
        let dir = tempfile::tempdir().unwrap();
        let raw = dir.path().join(RAW_DIR);
        std::fs::create_dir_all(&raw).unwrap();
        let (tx, handle) = spawn_writer(raw.clone(), 48000, Arc::new(LogObserver)).unwrap();
        let started = std::time::Instant::now();
        let mut fed = 0u32;
        // Real time, so commits happen as in the app.
        while started.elapsed() < std::time::Duration::from_secs(10) {
            let end = (fed + 1) as f64 * 0.01;
            tx.send(TrackKind::Microphone, vec![0.2; 480], Some(end))
                .unwrap();
            tx.send(TrackKind::System, vec![0.1; 480], Some(end))
                .unwrap();
            fed += 1;
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let fed_seconds = fed as f64 * 0.01;
        // "Kill": scan what is on disk while the writer still holds data.
        let scanned = scan_raw_tracks(&raw, true).unwrap();
        let recovered = scanned.duration_seconds();
        assert!(
            fed_seconds - recovered <= 2.0,
            "fed {fed_seconds:.2}s, recovered {recovered:.2}s"
        );
        for track in &scanned.tracks {
            assert!(fed_seconds - track.end() as f64 / 48000.0 <= 2.0);
        }
        drop(tx);
        handle.join().unwrap();
    }
}
