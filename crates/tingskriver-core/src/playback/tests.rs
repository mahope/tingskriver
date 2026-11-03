#![allow(clippy::disallowed_methods)]
use super::*;
use std::{f64::consts::TAU, fs::File, io::Write, path::Path, time::Instant};

fn wav(path: &Path, rate: u32, channels: u16, seconds: u32) {
    let mut f = File::create(path).unwrap();
    let bytes = rate * channels as u32 * seconds * 2;
    f.write_all(b"RIFF").unwrap();
    f.write_all(&(bytes + 36).to_le_bytes()).unwrap();
    f.write_all(b"WAVEfmt \x10\0\0\0\x01\0").unwrap();
    f.write_all(&channels.to_le_bytes()).unwrap();
    f.write_all(&rate.to_le_bytes()).unwrap();
    f.write_all(&(rate * channels as u32 * 2).to_le_bytes())
        .unwrap();
    f.write_all(&(channels * 2).to_le_bytes()).unwrap();
    f.write_all(&16u16.to_le_bytes()).unwrap();
    f.write_all(b"data").unwrap();
    f.write_all(&bytes.to_le_bytes()).unwrap();
    let mut block = Vec::new();
    for second in 0..seconds {
        block.clear();
        for i in 0..rate {
            let marker = (second * 10 + i / (rate / 10)) % 100;
            let hz = 200.0 + marker as f64 * 60.0;
            let s = ((TAU * hz * i as f64 / rate as f64).sin() * 24000.0) as i16;
            for _ in 0..channels {
                block.extend_from_slice(&s.to_le_bytes());
            }
        }
        f.write_all(&block).unwrap();
    }
}
fn pull(player: &mut Player<FakeOutput>, n: usize) -> Vec<f32> {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut result = Vec::new();
    let mut block = vec![0.0; n];
    while result.len() < n {
        let remaining = n - result.len();
        let got = player.output_mut().pull(&mut block[..remaining]);
        result.extend_from_slice(&block[..got]);
        assert!(
            Instant::now() < deadline,
            "output timed out at {}",
            result.len()
        );
        if got == 0 {
            thread::yield_now();
        }
    }
    result
}
#[test]
fn failed_seek_preserves_playback_state() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tone.wav");
    wav(&path, 16000, 1, 1);
    for source in [
        Source::Mixed(path.clone()),
        Source::Tracks {
            mikrofon: path.clone(),
            system: path.clone(),
        },
    ] {
        for playing in [false, true] {
            let (mut p, _) = Player::open(source.clone(), FakeOutput::new(16000)).unwrap();
            p.play();
            pull(&mut p, 160);
            if !playing {
                p.pause();
            }
            let position = p.position();
            assert_eq!(p.seek(Duration::MAX), Err(PlaybackError::Seek));
            assert_eq!(p.position(), position);
            assert_eq!(p.shared.playing.load(Ordering::Acquire), playing);
            if !playing {
                assert_eq!(p.output_mut().pull(&mut [0.0; 160]), 0);
                p.play();
            }
            let samples = pull(&mut p, 160);
            assert!(samples.iter().any(|s| s.abs() > 0.01));
            assert_eq!(p.position(), position + Duration::from_millis(10));
            p.seek(Duration::from_millis(500)).unwrap();
            pull(&mut p, 160);
            assert_eq!(p.position(), Duration::from_millis(510));
        }
    }
}
#[test]
fn failed_second_track_reset_keeps_both_tracks_and_settings() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.wav");
    let b = dir.path().join("b.wav");
    wav(&a, 16000, 1, 1);
    wav(&b, 16000, 1, 1);
    let (mut p, _) = Player::open(
        Source::Tracks {
            mikrofon: a,
            system: b.clone(),
        },
        FakeOutput::new(16000),
    )
    .unwrap();
    p.play();
    pull(&mut p, 160);
    std::fs::rename(&b, dir.path().join("moved.wav")).unwrap();
    assert_eq!(p.seek(Duration::from_millis(500)), Err(PlaybackError::Open));
    assert_eq!(p.set_speed(2.0), Err(PlaybackError::Open));
    assert_eq!(p.speed, 1.0);
    assert_eq!(p.set_track_gains(0.0, 0.0), Err(PlaybackError::Open));
    assert_eq!(p.gains, [1.0, 1.0]);
    let samples = pull(&mut p, 16000 - 160);
    assert!(samples.iter().any(|s| s.abs() == 1.0));
    assert_eq!(p.position(), Duration::from_secs(1));
}
#[test]
fn fake_controls_speed_position_and_end() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tone.wav");
    wav(&path, 16000, 1, 1);
    for speed in [0.5, 1.0, 2.0] {
        let (mut p, events) =
            Player::open(Source::Mixed(path.clone()), FakeOutput::new(16000)).unwrap();
        assert_eq!(p.output_mut().pull(&mut [1.0; 128]), 0);
        for invalid in [0.49, 2.01, f32::NAN, f32::INFINITY] {
            assert_eq!(p.set_speed(invalid), Err(PlaybackError::Speed));
        }
        p.set_speed(speed).unwrap();
        p.play();
        let count = (16000.0 / speed) as usize;
        pull(&mut p, count / 2);
        assert!((p.position().as_secs_f64() - 0.5).abs() < 0.001);
        p.pause();
        let pos = p.position();
        assert_eq!(p.output_mut().pull(&mut [0.0; 64]), 0);
        assert_eq!(p.position(), pos);
        p.play();
        pull(&mut p, count - count / 2);
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            assert_eq!(
                p.output_mut().pull(&mut [0.0; 128]),
                0,
                "speed must deliver exactly the expected frame count"
            );
            if events.try_iter().any(|e| e == Event::Ended) {
                break;
            }
            assert!(Instant::now() < deadline);
            thread::yield_now();
        }
        assert!((p.position().as_secs_f64() - 1.0).abs() < 0.001);
        p.seek(Duration::from_millis(250)).unwrap();
        p.play();
        pull(&mut p, 160);
        assert!((p.position().as_secs_f64() - (0.25 + 0.01 * speed as f64)).abs() < 0.001);
    }
}
fn markers(path: &Path) {
    let (mut p, _) = Player::open(Source::Mixed(path.into()), FakeOutput::new(16000)).unwrap();
    let mut times = vec![3450, 1200, 8730, 2100, 5600, 7200];
    let mut seed = 17_u64;
    for _ in 0..32 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        times.push(((seed >> 32) % 98) * 100 + 10 + seed % 30);
    }
    for millis in times {
        let time = Duration::from_millis(millis);
        p.seek(time).unwrap();
        p.play();
        let data = pull(&mut p, 800);
        let expected = 200.0 + ((millis / 100) % 100) as f64 * 60.0;
        let crossings = data
            .windows(2)
            .filter(|s| s[0] <= 0.0 && s[1] > 0.0)
            .count();
        let hz = crossings as f64 / 0.05;
        assert!(
            (hz - expected).abs() < 25.0,
            "{}: expected {expected}, got {hz}",
            path.display()
        );
        assert!((p.position().as_secs_f64() - (time.as_secs_f64() + 0.05)).abs() < 0.001);
    }
}
#[test]
fn seek_tone_markers_wav() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tone.wav");
    wav(&path, 16000, 1, 10);
    markers(&path);
}
/// Requires the recording encoder, not audio hardware. Run explicitly on Linux.
#[test]
#[ignore = "requires ffmpeg; run for format acceptance"]
fn seek_markers_all_formats() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("tone.wav");
    wav(&input, 16000, 1, 10);
    for (ext, codec) in [
        ("m4a", "aac"),
        ("mp4", "aac"),
        ("mp3", "libmp3lame"),
        ("flac", "flac"),
        ("ogg", "libvorbis"),
    ] {
        let output = dir.path().join(format!("tone.{ext}"));
        assert!(std::process::Command::new("ffmpeg")
            .args(["-v", "error", "-i"])
            .arg(&input)
            .args(["-c:a", codec])
            .arg(&output)
            .status()
            .unwrap()
            .success());
        markers(&output);
    }
}
#[test]
fn different_rates_channels_mix_and_mute() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.wav");
    let b = dir.path().join("b.wav");
    wav(&a, 16000, 1, 1);
    wav(&b, 48000, 2, 2);
    let (mut p, _) = Player::open(
        Source::Tracks {
            mikrofon: a,
            system: b,
        },
        FakeOutput::new(16000),
    )
    .unwrap();
    assert_eq!(p.set_track_gains(f32::NAN, 1.0), Err(PlaybackError::Gain));
    assert_eq!(p.set_track_gains(1.0, -0.1), Err(PlaybackError::Gain));
    p.play();
    let mixed = pull(&mut p, 400);
    assert!(mixed.iter().any(|s| s.abs() == 1.0));
    p.set_track_gains(1.0, 0.0).unwrap();
    p.seek(Duration::ZERO).unwrap();
    let single = pull(&mut p, 400);
    for (a, b) in mixed.iter().zip(single) {
        assert!((a - (2.0 * b).clamp(-1.0, 1.0)).abs() < 0.001);
    }
    p.set_track_gains(0.0, 1.0).unwrap();
    p.seek(Duration::ZERO).unwrap();
    let system = pull(&mut p, 400);
    for (a, b) in mixed.iter().zip(system) {
        assert!((a - (2.0 * b).clamp(-1.0, 1.0)).abs() < 0.001);
    }
    p.seek(Duration::from_millis(1500)).unwrap();
    assert!(pull(&mut p, 400).iter().any(|s| s.abs() > 0.01));
    p.set_track_gains(0.0, 0.0).unwrap();
    assert!(pull(&mut p, 400).iter().all(|s| *s == 0.0));
}
#[test]
fn typed_errors_and_no_json_audio_command() {
    assert!(matches!(
        Player::open(Source::Mixed("not-present".into()), FakeOutput::new(16000)),
        Err(PlaybackError::Open)
    ));
    let tauri = include_str!("../../../../frontend/src-tauri/src/lib.rs");
    assert!(!tauri.contains("read_audio_file"));
    assert!(
        !include_str!("../../../../frontend/src/hooks/useAudioPlayer.ts")
            .contains("invoke<number[]>")
    );
}
#[cfg(target_os = "linux")]
fn rss() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .unwrap()
        .lines()
        .find_map(|l| l.strip_prefix("VmRSS:"))
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .parse()
        .unwrap()
}
#[test]
#[ignore = "isolated startup/RSS measurement; run with --test-threads=1"]
fn hour_startup_and_bounded_memory() {
    let dir = tempfile::tempdir().unwrap();
    let small = dir.path().join("small.wav");
    let large = dir.path().join("hour.wav");
    wav(&small, 16000, 1, 1);
    wav(&large, 16000, 1, 3600);
    for path in [small, large] {
        #[cfg(target_os = "linux")]
        let before = rss();
        let start = Instant::now();
        let (mut p, _) = Player::open(Source::Mixed(path.clone()), FakeOutput::new(16000)).unwrap();
        p.play();
        pull(&mut p, 1600);
        let elapsed = start.elapsed();
        assert!(elapsed < Duration::from_millis(500));
        #[cfg(target_os = "linux")]
        {
            let growth = rss().saturating_sub(before);
            println!(
                "{}: first 100ms {:?}, RSS growth {} KiB",
                path.file_name().unwrap().to_string_lossy(),
                elapsed,
                growth
            );
            assert!(growth < 16 * 1024);
        }
    }
}

#[test]
fn mp4_edits_are_bounded_and_validate_timeline() {
    fn atom(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut result = ((body.len() + 8) as u32).to_be_bytes().to_vec();
        result.extend_from_slice(kind);
        result.extend_from_slice(body);
        result
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("edit.mp4");
    for version in [0, 1] {
        let mut edit = vec![version, 0, 0, 0];
        edit.extend_from_slice(&1u32.to_be_bytes());
        if version == 0 {
            edit.extend_from_slice(&16000u32.to_be_bytes());
            edit.extend_from_slice(&1024i32.to_be_bytes());
        } else {
            edit.extend_from_slice(&16000u64.to_be_bytes());
            edit.extend_from_slice(&1024i64.to_be_bytes());
        }
        edit.extend_from_slice(&[0, 1, 0, 0]);
        let mut file = atom(b"ftyp", &[]);
        file.extend(atom(
            b"moov",
            &atom(b"trak", &atom(b"edts", &atom(b"elst", &edit))),
        ));
        std::fs::write(&path, &file).unwrap();
        assert_eq!(super::mp4::media_offset(&path, 0).unwrap(), 1024);
        assert_eq!(super::mp4::media_offset(&path, 1).unwrap(), 0);
        file.truncate(file.len() - 1);
        std::fs::write(&path, &file).unwrap();
        assert_eq!(
            super::mp4::media_offset(&path, 0),
            Err(PlaybackError::Decode)
        );
    }
}

#[test]
fn position_and_output_error_events() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tone.wav");
    wav(&path, 16000, 1, 1);
    let (mut p, events) = Player::open(Source::Mixed(path), FakeOutput::new(16000)).unwrap();
    p.play();
    pull(&mut p, 1600);
    p.pause();
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if events
            .recv_timeout(Duration::from_millis(100))
            .is_ok_and(|e| e == Event::Position(Duration::from_millis(100)))
        {
            break;
        }
        assert!(Instant::now() < deadline);
    }
    p.shared.fault.store(true, Ordering::Release);
    loop {
        if events
            .recv_timeout(Duration::from_millis(100))
            .is_ok_and(|e| e == Event::Error(PlaybackError::Output))
        {
            break;
        }
        assert!(Instant::now() < deadline);
    }
}
