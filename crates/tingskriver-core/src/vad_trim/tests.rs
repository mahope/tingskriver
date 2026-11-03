#![allow(clippy::disallowed_methods)]
use super::*;
use crate::audio::transcription::provider::{
    TranscriptResult, TranscriptionError, TranscriptionProvider,
};
use std::sync::atomic::{AtomicUsize, Ordering};

fn config() -> TrimConfig {
    TrimConfig {
        backend: BackendPreference::Energy,
        ..Default::default()
    }
}
fn clip(level: f32) -> (Vec<f32>, Vec<(usize, usize)>) {
    let mut audio = vec![0.0; 16_000 * 18];
    let ranges = vec![(16_000, 32_000), (96_000, 120_000), (224_000, 256_000)];
    let mut seed = 37u32;
    for &(start, end) in &ranges {
        for (i, sample) in audio[start..end].iter_mut().enumerate() {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let noise = (seed >> 8) as f32 / 16_777_216.0 * 2.0 - 1.0;
            *sample = noise * level * (0.6 + 0.4 * (i as f32 * 0.002).sin().abs());
        }
    }
    (audio, ranges)
}
struct Fake {
    calls: AtomicUsize,
    expected_samples: usize,
}
#[async_trait::async_trait]
impl TranscriptionProvider for Fake {
    async fn transcribe(
        &self,
        audio: Vec<f32>,
        _: Option<String>,
    ) -> Result<TranscriptResult, TranscriptionError> {
        assert_eq!(audio.len(), self.expected_samples);
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(TranscriptResult {
            text: "test".into(),
            confidence: None,
            is_partial: false,
        })
    }
    async fn is_model_loaded(&self) -> bool {
        true
    }
    async fn get_current_model(&self) -> Option<String> {
        None
    }
    fn provider_name(&self) -> &'static str {
        "fake"
    }
}
#[tokio::test]
async fn thirty_seconds_silence_and_low_noise_never_call_provider() {
    for backend in [BackendPreference::Energy, BackendPreference::Silero] {
        for amplitude in [0.0, 0.0003] {
            let audio: Vec<_> = (0..480_000)
                .map(|i| amplitude * (i as f32 * 1.79).sin())
                .collect();
            let fake = Fake {
                calls: AtomicUsize::new(0),
                expected_samples: 0,
            };
            let result = transcribe_trimmed(
                &fake,
                &audio,
                &TrimConfig {
                    backend,
                    ..config()
                },
                None,
            )
            .await
            .unwrap();
            assert!(result.transcript.text.is_empty());
            assert_eq!(fake.calls.load(Ordering::SeqCst), 0);
        }
    }
}
#[test]
fn five_clips_preserve_every_known_speech_sample() {
    for level in [0.012, 0.025, 0.06, 0.15, 0.4] {
        let (audio, truth) = clip(level);
        let result = trim(&audio, &config()).unwrap();
        let forced = trim_with_runtime_probe(
            &audio,
            &TrimConfig {
                backend: BackendPreference::Auto,
                ..config()
            },
            || false,
        )
        .unwrap();
        assert_eq!(result.samples, forced.samples);
        assert_eq!(result.segments, forced.segments);
        assert_eq!(forced.backend, Backend::Energy);
        assert!(result.samples.len() < audio.len() / 2);
        for (start, end) in truth {
            let overlap: usize = result
                .segments
                .iter()
                .map(|s| end.min(s.orig_end).saturating_sub(start.max(s.orig_start)))
                .sum();
            assert_eq!(overlap, end - start, "speech coverage at level {level}");
        }
    }
}
#[test]
fn missing_runtime_selects_energy_without_environment_changes() {
    let mut called = false;
    assert_eq!(
        select_backend(BackendPreference::Auto, || {
            called = true;
            false
        }),
        Backend::Energy
    );
    assert!(called);
    assert_eq!(
        select_backend(BackendPreference::Silero, || false),
        Backend::Energy
    );
}
#[test]
fn hum_and_stationary_noise_are_not_speech() {
    let audio: Vec<_> = (0..480_000)
        .map(|i| 0.03 * (std::f32::consts::TAU * 50.0 * i as f32 / 16000.0).sin())
        .collect();
    assert!(trim(&audio, &config()).unwrap().samples.is_empty());
}

struct TimedFake {
    calls: AtomicUsize,
    spans: Vec<TimedSpan>,
}
impl TimedTranscriptionProvider for TimedFake {
    fn transcribe_timed<'a>(
        &'a self,
        _: Vec<f32>,
        _: Option<String>,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<TimedTranscript, TranscriptionError>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(TimedTranscript {
                text: "test".into(),
                words: self.spans.clone(),
                segments: self.spans.clone(),
            })
        })
    }
}
#[tokio::test]
async fn word_and_segment_times_return_to_original_axis_within_100ms() {
    let (audio, truth) = clip(0.06);
    let trimmed = trim(&audio, &config()).unwrap();
    let originals: Vec<_> = truth
        .iter()
        .map(|(s, _)| (*s as f64 / 16.0 + 100.0, *s as f64 / 16.0 + 300.0))
        .collect();
    // Compute the fake's clock independently from the time-map implementation.
    let mut spans = Vec::new();
    for &(start_ms, end_ms) in &originals {
        let segment = trimmed
            .segments
            .iter()
            .find(|s| start_ms * 16.0 >= s.orig_start as f64 && end_ms * 16.0 <= s.orig_end as f64)
            .unwrap();
        let removed_ms = (segment.orig_start - segment.trimmed_start) as f64 / 16.0;
        spans.push(TimedSpan {
            text: "word".into(),
            start_ms: start_ms - removed_ms,
            end_ms: end_ms - removed_ms,
        });
    }
    let fake = TimedFake {
        calls: AtomicUsize::new(0),
        spans,
    };
    let result = transcribe_trimmed_timed(&fake, &audio, &config(), None)
        .await
        .unwrap();
    assert_eq!(fake.calls.load(Ordering::SeqCst), 1);
    for spans in [&result.words, &result.segments] {
        for (span, &(start, end)) in spans.iter().zip(&originals) {
            assert!((span.start_ms - start).abs() < 100.0);
            assert!((span.end_ms - end).abs() < 100.0);
        }
    }
}
#[test]
fn mapping_is_monotonic_and_join_endpoints_are_explicit() {
    let audio = TrimmedAudio {
        samples: vec![0.0; 32000],
        backend: Backend::Energy,
        segments: vec![
            TrimSegment {
                orig_start: 16000,
                orig_end: 32000,
                trimmed_start: 0,
            },
            TrimSegment {
                orig_start: 96000,
                orig_end: 112000,
                trimmed_start: 16000,
            },
        ],
    };
    assert_eq!(audio.to_original_ms(1000.0), 6000.0);
    assert_eq!(audio.end_to_original_ms(1000.0), 2000.0);
    assert_eq!(audio.to_original_ms(-1.0), 1000.0);
    assert_eq!(audio.to_original_ms(99999.0), 7000.0);
    let mut previous = 0.0;
    for i in 0..=20000 {
        let mapped = audio.to_original_ms(i as f64 / 10.0);
        assert!(mapped >= previous);
        previous = mapped;
    }
}
#[test]
fn streaming_file_matches_buffer_across_frames_and_tail() {
    use std::io::Write;
    let (mut audio, _) = clip(0.06);
    audio.extend_from_slice(&[0.0; 137]);
    let mut file = tempfile::NamedTempFile::new().unwrap();
    for sample in &audio {
        file.write_all(&sample.to_le_bytes()).unwrap();
    }
    let buffer = trim(&audio, &config()).unwrap();
    let streamed = trim_pcm_file(file.path(), &config()).unwrap();
    assert_eq!(streamed.samples, buffer.samples);
    assert_eq!(streamed.segments, buffer.segments);
}
#[test]
fn threshold_minimums_padding_and_forced_fallback_are_effective() {
    let (audio, _) = clip(0.012);
    let cfg = config();
    let normal = trim(&audio, &cfg).unwrap();
    let forced = trim_with_runtime_probe(
        &audio,
        &TrimConfig {
            backend: BackendPreference::Auto,
            ..cfg.clone()
        },
        || false,
    )
    .unwrap();
    assert_eq!(normal.samples, forced.samples);
    assert_eq!(forced.backend, Backend::Energy);
    let strict = trim(
        &audio,
        &TrimConfig {
            threshold: 1.0,
            ..cfg.clone()
        },
    )
    .unwrap();
    assert!(strict.samples.len() < normal.samples.len());
    let long_min = trim(
        &audio,
        &TrimConfig {
            min_speech_ms: 3000,
            ..cfg.clone()
        },
    )
    .unwrap();
    assert!(long_min.samples.is_empty());
    let no_pad = trim(
        &audio,
        &TrimConfig {
            speech_pad_ms: 0,
            ..cfg.clone()
        },
    )
    .unwrap();
    assert!(no_pad.samples.len() < normal.samples.len());
    assert!(trim(
        &audio,
        &TrimConfig {
            threshold: f32::NAN,
            ..cfg
        }
    )
    .is_err());
}
#[test]
fn padding_merges_overlapping_intervals_and_eof_speech_is_kept() {
    let (audio, _) = clip(0.15);
    let speech = &audio[16000..32000];
    let mut short = speech.to_vec();
    short.extend_from_slice(&[0.0; 3200]);
    short.extend_from_slice(speech);
    let result = trim(&short, &config()).unwrap();
    assert_eq!(result.segments.len(), 1);
    assert_eq!(result.samples, short);
}

fn write_wav(path: &std::path::Path, samples: &[f32], rate: u32) {
    use std::io::Write;
    let mut file = std::fs::File::create(path).unwrap();
    let bytes = samples.len() as u32 * 4;
    file.write_all(b"RIFF").unwrap();
    file.write_all(&(36 + bytes).to_le_bytes()).unwrap();
    file.write_all(b"WAVEfmt ").unwrap();
    file.write_all(&16u32.to_le_bytes()).unwrap();
    file.write_all(&3u16.to_le_bytes()).unwrap(); // IEEE float PCM
    file.write_all(&1u16.to_le_bytes()).unwrap();
    file.write_all(&rate.to_le_bytes()).unwrap();
    file.write_all(&(rate * 4).to_le_bytes()).unwrap();
    file.write_all(&4u16.to_le_bytes()).unwrap();
    file.write_all(&32u16.to_le_bytes()).unwrap();
    file.write_all(b"data").unwrap();
    file.write_all(&bytes.to_le_bytes()).unwrap();
    for sample in samples {
        file.write_all(&sample.to_le_bytes()).unwrap();
    }
}
#[test]
fn wav_streaming_matches_buffer_and_rejects_wrong_clock() {
    let (audio, _) = clip(0.15);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.wav");
    write_wav(&path, &audio, 16000);
    let expected = trim(&audio, &config()).unwrap();
    let actual = trim_file(&path, &config()).unwrap();
    assert_eq!(actual.samples, expected.samples);
    assert_eq!(actual.segments, expected.segments);
    write_wav(&path, &audio, 48000);
    assert!(trim_file(&path, &config()).is_err());
}
#[test]
fn invalid_samples_and_partial_pcm_fail_closed() {
    assert!(trim(&[f32::INFINITY; 480], &config()).is_err());
    let file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(file.path(), [1, 2, 3]).unwrap();
    assert!(trim_pcm_file(file.path(), &config()).is_err());
}
#[test]
fn saved_meeting_adapter_reports_progress_and_cancels() {
    let mut progress = Vec::new();
    let silent = vec![0.0; 480000];
    let segments = meeting::get_speech_chunks_with_progress(&silent, 500, |p, _| {
        progress.push(p);
        true
    })
    .unwrap();
    assert!(segments.is_empty());
    assert_eq!(progress.last(), Some(&100));
    assert!(progress.windows(2).all(|p| p[0] < p[1]));
    let error = meeting::get_speech_chunks_with_progress(&silent, 500, |p, _| p < 20).unwrap_err();
    assert!(error.to_string().contains("cancelled"));
}
#[test]
fn silero_startup_and_empty_buffer_smoke() {
    let result = trim(
        &vec![0.0; 16000],
        &TrimConfig {
            backend: BackendPreference::Silero,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(result.samples.is_empty());
    // Startup may fall back on machines without a working runtime; this is not a
    // speech-quality fixture. That requires the licensed corpus from issue #30.
}

#[test]
fn quiet_speech_survives_mains_hum_and_silence_setting_bridges_pauses() {
    let (mut audio, truth) = clip(0.025);
    for (i, sample) in audio.iter_mut().enumerate() {
        *sample += 0.03 * (std::f32::consts::TAU * 50.0 * i as f32 / 16000.0).sin();
    }
    let result = trim(&audio, &config()).unwrap();
    for (start, end) in truth {
        let overlap: usize = result
            .segments
            .iter()
            .map(|s| end.min(s.orig_end).saturating_sub(start.max(s.orig_start)))
            .sum();
        assert_eq!(overlap, end - start);
    }
    let (clean, _) = clip(0.15);
    let mut joined = clean[16000..32000].to_vec();
    joined.extend_from_slice(&[0.0; 3200]);
    joined.extend_from_slice(&clean[16000..32000]);
    let cfg = TrimConfig {
        speech_pad_ms: 0,
        ..config()
    };
    assert_eq!(trim(&joined, &cfg).unwrap().segments.len(), 1);
    assert_eq!(
        trim(
            &joined,
            &TrimConfig {
                min_silence_ms: 100,
                ..cfg
            }
        )
        .unwrap()
        .segments
        .len(),
        2
    );
}
#[tokio::test]
async fn timed_provider_is_not_called_on_silence() {
    let fake = TimedFake {
        calls: AtomicUsize::new(0),
        spans: Vec::new(),
    };
    let result = transcribe_trimmed_timed(&fake, &vec![0.0; 480000], &config(), None)
        .await
        .unwrap();
    assert!(result.text.is_empty());
    assert_eq!(fake.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn text_provider_receives_only_trimmed_audio_in_one_call() {
    let (audio, _) = clip(0.06);
    let trimmed = trim(&audio, &config()).unwrap();
    let fake = Fake {
        calls: AtomicUsize::new(0),
        expected_samples: trimmed.samples.len(),
    };
    let result = transcribe_trimmed(&fake, &audio, &config(), Some("da".into()))
        .await
        .unwrap();
    assert_eq!(fake.calls.load(Ordering::SeqCst), 1);
    assert_eq!(result.transcript.text, "test");
    assert_eq!(result.audio.samples, trimmed.samples);
    assert!(result.audio.samples.len() < audio.len());
}

#[tokio::test]
async fn stationary_broadband_noise_above_initial_gate_skips_provider() {
    let mut seed = 37u32;
    let noise: Vec<f32> = (0..480_000)
        .map(|_| {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            ((seed >> 8) as f32 / 16_777_216.0 * 2.0 - 1.0) * 0.01
        })
        .collect();
    let fake = Fake {
        calls: AtomicUsize::new(0),
        expected_samples: 0,
    };
    let result = transcribe_trimmed(&fake, &noise, &config(), None)
        .await
        .unwrap();
    assert!(result.transcript.text.is_empty());
    assert_eq!(fake.calls.load(Ordering::SeqCst), 0);
    let (mut audio, truth) = clip(0.06);
    for (i, sample) in audio.iter_mut().enumerate() {
        *sample += noise[i % noise.len()];
    }
    let result = trim(&audio, &config()).unwrap();
    for (start, end) in truth {
        assert!(result
            .segments
            .iter()
            .any(|s| s.orig_start <= start && s.orig_end >= end));
    }
}

#[test]
fn silero_streaming_silence_has_bounded_session_storage() {
    let mut processor = super::trim::silero(&config()).unwrap();
    for _ in 0..10_000 {
        assert!(processor.process_audio(&[0.0; 480]).unwrap().is_empty());
        assert!(processor.buffered_session_samples() <= 4000);
    }
    assert!(processor.flush().unwrap().is_empty());
}
