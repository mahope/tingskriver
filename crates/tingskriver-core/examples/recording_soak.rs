//! Synthetic device at the production capture boundary; no audio hardware.
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use tingskriver_core::audio::{
    audio_processing::{HighPassFilter, LoudnessNormalizer},
    devices::{AudioDevice, DeviceType as DeviceKind},
    finalize::{
        decoded_duration_seconds, finalize_raw_tracks, pending_finalize_folders, scan_raw_tracks,
        AudioSaveStatus, Source, SourceReader, RAW_DIR,
    },
    incremental_saver::{has_audio_checkpoints, recover_audio_from_checkpoints},
    pipeline::AudioCapture,
    recording_state::{DeviceType, RecordingState},
    recording_writer::{spawn_writer, LogObserver},
    track_writer::{f32_to_i16, TrackKind},
};

const RATE: u32 = 48_000;
const BLOCK: u64 = 480;

fn recorded_duration(track: &tingskriver_core::audio::finalize::ScannedTrack) -> f64 {
    let samples: u64 = track
        .sources
        .iter()
        .rev()
        .skip_while(|source| matches!(source, Source::Silence(_)))
        .map(|source| match source {
            Source::File { samples, .. } | Source::Silence(samples) => *samples,
        })
        .sum();
    (track.offset_samples + samples) as f64 / RATE as f64
}

// Nonzero square tone, with a distinct 10 ms marker each second.
fn signal(position: u64, kind: TrackKind) -> f32 {
    let amplitude = if position % (RATE as u64) < BLOCK {
        0.5 + (((position / RATE as u64) >> ((position % RATE as u64) / 30)) & 1) as f32 * 0.25
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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    let mode = args.get(1).ok_or("mode required")?;
    let dir = PathBuf::from(args.get(2).ok_or("directory required")?);
    let seconds: f64 = args.get(3).ok_or("seconds required")?.parse()?;
    if !seconds.is_finite() || seconds <= 0.0 {
        return Err("invalid duration".into());
    }
    if mode == "record" {
        std::fs::create_dir_all(dir.join(RAW_DIR))?;
        let (sender, writer) = spawn_writer(dir.join(RAW_DIR), RATE, Arc::new(LogObserver))?;
        let state = RecordingState::new();
        state.set_audio_sender(sender);
        let captures: Vec<_> = [DeviceType::Microphone, DeviceType::System]
            .into_iter()
            .map(|kind| {
                AudioCapture::new(
                    Arc::new(AudioDevice::new("synthetic".into(), DeviceKind::Input)),
                    state.clone(),
                    RATE,
                    1,
                    kind,
                    None,
                )
            })
            .collect();
        state.start_recording()?;
        let start = Instant::now();
        std::fs::write(dir.join("ready"), b"ready")?;
        let blocks = (seconds * 100.0).round() as u64;
        for block in 0..blocks {
            let deadline = start + Duration::from_secs_f64((block + 1) as f64 / 100.0);
            std::thread::sleep(deadline.saturating_duration_since(Instant::now()));
            for (capture, kind) in captures
                .iter()
                .zip([TrackKind::Microphone, TrackKind::System])
            {
                let samples: Vec<_> = (block * BLOCK..(block + 1) * BLOCK)
                    .map(|p| signal(p, kind))
                    .collect();
                capture.process_audio_data(&samples);
            }
        }
        let elapsed = start.elapsed().as_secs_f64();
        assert!(
            (elapsed - seconds).abs() < 0.5,
            "device clock drift: {elapsed}"
        );
        assert_eq!(state.get_error_count(), 0);
        if args.get(4).is_some_and(|a| a == "--measure-final") {
            std::fs::write(dir.join("finished"), b"finished")?;
            let deadline = Instant::now() + Duration::from_secs(60);
            while !dir.join("measured").exists() {
                assert!(Instant::now() < deadline, "final measurement timeout");
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        for capture in &captures {
            capture.flush_tail();
        }
        state.stop_recording();
        let summary = writer.join()?;
        assert_eq!(summary.lost_samples(), 0);
        assert!(!summary.crashed && summary.storage_error.is_none());
    } else if mode == "verify" || mode == "recover" || mode == "finish" {
        if mode == "recover" {
            assert!(has_audio_checkpoints(dir.to_string_lossy().into_owned()).await?);
            let recovered =
                recover_audio_from_checkpoints(dir.to_string_lossy().into_owned(), RATE).await?;
            assert_eq!(recovered.status, "success", "{}", recovered.message);
        }
        let scanned = scan_raw_tracks(&dir.join(RAW_DIR), true)?;
        assert_eq!(scanned.tracks.len(), 2);
        assert!(scanned.problems.is_empty(), "{:?}", scanned.problems);
        assert_eq!(scanned.complete, mode != "recover");
        for track in scanned.tracks {
            let duration = recorded_duration(&track);
            let loss = seconds - duration;
            assert!(
                loss >= -0.1 && loss < if mode == "recover" { 2.0 } else { 0.5 },
                "duration {duration}, expected {seconds}"
            );
            assert!(track.offset_samples < RATE as u64 / 2);
            // Scanner padding can span whole callbacks after a crash. Only
            // file sources contain signal (plus the writer's sub-block tail).
            let signal_samples = track
                .sources
                .iter()
                .rev()
                .skip_while(|source| matches!(source, Source::Silence(_)))
                .map(|source| match source {
                    Source::File { samples, .. } | Source::Silence(samples) => *samples,
                })
                .sum::<u64>()
                / BLOCK
                * BLOCK;
            let mut reader = SourceReader::new(track.sources);
            let mut block = Vec::new();
            let mut position = 0;
            let mut filter = HighPassFilter::new(RATE, 80.0);
            let mut normalizer = LoudnessNormalizer::new(1, RATE)?;
            while reader.read_block(&mut block, BLOCK as usize)? {
                let expected: Vec<_> = (position..position + block.len() as u64)
                    .map(|p| signal(p, track.kind))
                    .collect();
                let expected = if track.kind == TrackKind::Microphone {
                    normalizer.normalize_loudness(&filter.process(&expected))
                } else {
                    expected
                };
                for (sample, expected) in block.iter().zip(expected) {
                    assert_eq!(
                        *sample,
                        if position < signal_samples {
                            f32_to_i16(expected)
                        } else {
                            0
                        },
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
        if mode == "finish" {
            let pending = pending_finalize_folders(dir.parent().ok_or("missing parent")?);
            assert!(pending.contains(&dir), "startup did not find recording");
            let result = finalize_raw_tracks(
                &dir,
                tingskriver_core::audio::ffmpeg::find_ffmpeg_path().as_deref(),
            );
            assert_eq!(result.status, AudioSaveStatus::Ok, "{:?}", result.problems);
        }
        if mode != "verify" {
            let duration = decoded_duration_seconds(&dir.join("audio.mp4"))?;
            assert!((duration - seconds).abs() < if mode == "recover" { 2.0 } else { 0.5 });
        }
    } else {
        return Err("unknown mode".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tingskriver_core::audio::finalize::ScannedTrack;

    #[test]
    fn loss_uses_each_tracks_unpadded_endpoint_with_offset() {
        let track = ScannedTrack {
            kind: TrackKind::Microphone,
            offset_samples: RATE as u64,
            sources: vec![
                Source::File {
                    path: "unused.wav".into(),
                    samples: RATE as u64 * 6,
                },
                Source::Silence(RATE as u64 * 3),
            ],
            segment_count: 1,
        };
        assert_eq!(track.end(), RATE as u64 * 10);
        assert_eq!(recorded_duration(&track), 7.0);
        assert!(10.0 - recorded_duration(&track) >= 2.0);
    }
}
