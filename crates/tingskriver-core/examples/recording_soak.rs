//! Synthetic device at the production disk-writer boundary; no audio hardware.
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use tingskriver_core::audio::{
    finalize::{scan_raw_tracks, SourceReader},
    recording_writer::{spawn_writer, LogObserver},
    track_writer::{f32_to_i16, TrackKind},
};

const RATE: u32 = 48_000;
const BLOCK: u64 = 480;

// Nonzero square tone, with a distinct 10 ms marker each second.
fn signal(position: u64, kind: TrackKind) -> f32 {
    let amplitude = if position % (RATE as u64) < BLOCK {
        0.75
    } else {
        0.25
    };
    let period = if kind == TrackKind::System { 48 } else { 80 };
    if position % period < period / 2 {
        amplitude
    } else {
        -amplitude
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    let mode = args.get(1).ok_or("mode required")?;
    let dir = PathBuf::from(args.get(2).ok_or("directory required")?);
    let seconds: f64 = args.get(3).ok_or("seconds required")?.parse()?;
    if !seconds.is_finite() || seconds <= 0.0 {
        return Err("invalid duration".into());
    }
    if mode == "record" {
        let (sender, writer) = spawn_writer(dir.clone(), RATE, Arc::new(LogObserver))?;
        let start = Instant::now();
        std::fs::write(dir.join("ready"), b"ready")?;
        let blocks = (seconds * 100.0).round() as u64;
        for block in 0..blocks {
            let deadline = start + Duration::from_secs_f64((block + 1) as f64 / 100.0);
            std::thread::sleep(deadline.saturating_duration_since(Instant::now()));
            for kind in [TrackKind::Microphone, TrackKind::System] {
                let samples = (block * BLOCK..(block + 1) * BLOCK)
                    .map(|p| signal(p, kind))
                    .collect();
                sender
                    .send(kind, samples, Some((block + 1) as f64 / 100.0))
                    .map_err(|e| format!("capture dropped audio: {e:?}"))?;
            }
        }
        let elapsed = start.elapsed().as_secs_f64();
        assert!(
            (elapsed - seconds).abs() < 0.5,
            "device clock drift: {elapsed}"
        );
        drop(sender);
        let summary = writer.join()?;
        assert_eq!(summary.lost_samples(), 0);
        assert!(!summary.crashed && summary.storage_error.is_none());
    } else if mode == "verify" || mode == "recover" {
        let scanned = scan_raw_tracks(&dir, true)?;
        assert_eq!(scanned.tracks.len(), 2);
        assert!(scanned.problems.is_empty(), "{:?}", scanned.problems);
        assert_eq!(scanned.complete, mode == "verify");
        for track in scanned.tracks {
            let duration = track.end() as f64 / RATE as f64;
            let loss = seconds - duration;
            assert!(
                loss >= -0.1 && loss < if mode == "recover" { 2.0 } else { 0.5 },
                "duration {duration}, expected {seconds}"
            );
            assert_eq!(track.offset_samples, 0);
            let mut reader = SourceReader::new(track.sources);
            let mut block = Vec::new();
            let mut position = 0;
            while reader.read_block(&mut block, 8192)? {
                for sample in &block {
                    assert_eq!(
                        *sample,
                        f32_to_i16(signal(position, track.kind)),
                        "marker/gap at sample {position}"
                    );
                    position += 1;
                }
            }
            assert!(position > 0);
            println!(
                "{:?}: {position} samples, {duration:.3}s, loss {loss:.3}s",
                track.kind
            );
        }
    } else {
        return Err("unknown mode".into());
    }
    Ok(())
}
