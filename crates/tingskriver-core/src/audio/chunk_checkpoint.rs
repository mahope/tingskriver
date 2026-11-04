use crate::stt::TimedWord;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// Conservative local-inference budgets, including boundary context. These are
/// deliberately below hardware-dependent model maxima, not advertised limits.
pub(crate) fn chunk_limit(use_parakeet: bool) -> usize {
    if use_parakeet {
        60 * 16000
    } else {
        25 * 16000
    }
}

#[derive(Clone)]
pub(crate) struct ChunkCheckpoint {
    path: PathBuf,
}

#[derive(Serialize, Deserialize)]
struct CompletedChunk {
    start_ms: f64,
    end_ms: f64,
    text: String,
    confidence: f32,
    words: Vec<TimedWord>,
}

impl ChunkCheckpoint {
    pub(crate) async fn load_async(
        &self,
        index: usize,
        start_ms: f64,
        end_ms: f64,
    ) -> Result<Option<(String, f32, Vec<TimedWord>)>> {
        let checkpoint = self.clone();
        tokio::task::spawn_blocking(move || checkpoint.load(index, start_ms, end_ms)).await?
    }

    pub(crate) async fn save_async(
        &self,
        index: usize,
        start_ms: f64,
        end_ms: f64,
        text: &str,
        confidence: f32,
        words: &[TimedWord],
    ) -> Result<()> {
        let checkpoint = self.clone();
        let text = text.to_owned();
        let words = words.to_vec();
        tokio::task::spawn_blocking(move || {
            checkpoint.save(index, start_ms, end_ms, &text, confidence, &words)
        })
        .await?
    }

    #[allow(clippy::disallowed_methods)] // Synchronous checkpoint disk worker.
    pub(crate) fn finish(self) -> Result<()> {
        std::fs::remove_dir_all(self.path)
            .context("Cannot remove completed transcription checkpoints")
    }

    #[allow(clippy::disallowed_methods)] // Synchronous fingerprint and disk worker.
    pub(crate) fn open(
        root: &Path,
        source: &Path,
        provider: &str,
        model: &str,
        language: Option<&str>,
    ) -> Result<Self> {
        let mut hash = DefaultHasher::new();
        // Bump the schema when splitting, VAD or stitching semantics change.
        (
            "chunks-v2-timed",
            env!("CARGO_PKG_VERSION"),
            provider,
            model,
            language,
        )
            .hash(&mut hash);
        let mut file =
            std::fs::File::open(source).context("Cannot fingerprint transcription source")?;
        let mut buffer = [0; 65536];
        loop {
            let size = file.read(&mut buffer)?;
            if size == 0 {
                break;
            }
            hash.write(&buffer[..size]);
        }
        let path = root
            .join(".transcription-chunks")
            .join(format!("{:016x}", hash.finish()));
        std::fs::create_dir_all(&path)?;
        Ok(Self { path })
    }

    #[allow(clippy::disallowed_methods)] // Synchronous checkpoint disk worker.
    pub(crate) fn load(
        &self,
        index: usize,
        start_ms: f64,
        end_ms: f64,
    ) -> Result<Option<(String, f32, Vec<TimedWord>)>> {
        let bytes = match std::fs::read(self.path.join(format!("{index}.json"))) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e).context("Cannot read transcription checkpoint"),
        };
        let chunk: CompletedChunk = serde_json::from_slice(&bytes)
            .context("Invalid transcription checkpoint; remove it to retry this chunk")?;
        anyhow::ensure!(
            chunk.confidence.is_finite(),
            "Invalid checkpoint confidence"
        );
        if chunk.start_ms != start_ms || chunk.end_ms != end_ms {
            return Ok(None);
        }
        validate_words(&chunk.words, end_ms - start_ms)?;
        Ok(Some((chunk.text, chunk.confidence, chunk.words)))
    }

    #[allow(clippy::disallowed_methods)] // Synchronous checkpoint disk worker.
    pub(crate) fn save(
        &self,
        index: usize,
        start_ms: f64,
        end_ms: f64,
        text: &str,
        confidence: f32,
        words: &[TimedWord],
    ) -> Result<()> {
        anyhow::ensure!(confidence.is_finite(), "Invalid transcription confidence");
        validate_words(words, end_ms - start_ms)?;
        let chunk = CompletedChunk {
            start_ms,
            end_ms,
            text: text.into(),
            confidence,
            words: words.to_vec(),
        };
        let mut file = tempfile::NamedTempFile::new_in(&self.path)?;
        file.write_all(&serde_json::to_vec(&chunk)?)?;
        file.as_file().sync_all()?;
        file.persist(self.path.join(format!("{index}.json")))
            .map_err(|e| e.error)
            .context("Cannot commit transcription checkpoint")?;
        #[cfg(unix)]
        std::fs::File::open(&self.path)?.sync_all()?;
        Ok(())
    }
}

fn validate_words(words: &[TimedWord], duration_ms: f64) -> Result<()> {
    let mut previous = 0.0;
    for word in words {
        anyhow::ensure!(
            !word.text.trim().is_empty()
                && word.start_ms.is_finite()
                && word.end_ms.is_finite()
                && word.start_ms >= previous
                && word.end_ms >= word.start_ms
                && word.end_ms <= duration_ms,
            "Invalid provider word timestamps"
        );
        previous = word.start_ms;
    }
    Ok(())
}

/// Word times are relative to the chunk; output is on the original audio clock.
pub(crate) fn append_chunk(
    output: &mut Vec<(String, f64, f64)>,
    words: Vec<TimedWord>,
    start: f64,
    _end: f64,
) -> bool {
    let normalize = |s: &str| {
        s.trim_matches(|c: char| !c.is_alphanumeric())
            .to_lowercase()
    };
    let first_candidate = output.partition_point(|(_, _, end)| *end < start);
    let before = &output[first_candidate..];
    let shared = (1..=before.len().min(words.len()))
        .rev()
        .find(|&n| {
            before[before.len() - n..]
                .iter()
                .zip(&words[..n])
                .all(|((text, a, b), word)| {
                    let key = normalize(text);
                    !key.is_empty()
                        && key == normalize(&word.text)
                        && ((*a < start + word.end_ms && start + word.start_ms < *b)
                            || (*a == start + word.start_ms
                                && (*a == *b || word.start_ms == word.end_ms)))
                })
        })
        .unwrap_or(0);
    let initial_len = output.len();
    for word in words.into_iter().skip(shared) {
        let start_ms = (start + word.start_ms).max(output.last().map_or(0.0, |s| s.2));
        let end_ms = (start + word.end_ms).max(start_ms);
        output.push((word.text, start_ms, end_ms));
    }
    output.len() > initial_len
}

#[cfg(test)]
#[allow(clippy::disallowed_methods)]
mod tests {
    use super::*;

    fn word(text: &str, start_ms: f64, end_ms: f64) -> TimedWord {
        TimedWord {
            text: text.into(),
            start_ms,
            end_ms,
        }
    }

    #[test]
    fn repetition_outside_shared_audio_is_not_removed() {
        let mut output = Vec::new();
        append_chunk(
            &mut output,
            vec![word("ja", 23000.0, 23500.0)],
            0.0,
            25000.0,
        );
        append_chunk(
            &mut output,
            vec![word("ja", 1500.0, 1800.0), word("næste", 2000.0, 2300.0)],
            24000.0,
            49000.0,
        );
        assert_eq!(
            output.iter().map(|s| s.0.as_str()).collect::<Vec<_>>(),
            ["ja", "ja", "næste"]
        );
        assert_eq!(output[1].1, 25500.0);
    }

    #[test]
    fn distinct_repetitions_within_overlap_keep_their_times() {
        let mut output = vec![("ja".into(), 24200.0, 24400.0)];
        append_chunk(
            &mut output,
            vec![word("ja", 600.0, 800.0)],
            24000.0,
            49000.0,
        );
        assert_eq!(
            output,
            vec![
                ("ja".into(), 24200.0, 24400.0),
                ("ja".into(), 24600.0, 24800.0)
            ]
        );
        let mut points = vec![("ja".into(), 24200.0, 24200.0)];
        append_chunk(
            &mut points,
            vec![word("ja", 200.0, 200.0)],
            24000.0,
            49000.0,
        );
        assert_eq!(points.len(), 1);
    }

    #[test]
    fn timed_checkpoint_round_trip_preserves_player_positions() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.wav");
        std::fs::write(&source, b"synthetic").unwrap();
        let cache = ChunkCheckpoint::open(dir.path(), &source, "whisper", "tiny", None).unwrap();
        let words = vec![word("ja", 200.0, 500.0), word("næste", 800.0, 950.0)];
        cache
            .save(0, 24000.0, 49000.0, "ja næste", 0.9, &words)
            .unwrap();
        drop(cache);
        let cache = ChunkCheckpoint::open(dir.path(), &source, "whisper", "tiny", None).unwrap();
        let restored = cache.load(0, 24000.0, 49000.0).unwrap().unwrap().2;
        assert_eq!(restored, words);
        let mut output = vec![("ja".into(), 24200.0, 24500.0)];
        append_chunk(&mut output, restored, 24000.0, 49000.0);
        assert_eq!(
            output,
            vec![
                ("ja".into(), 24200.0, 24500.0),
                ("næste".into(), 24800.0, 24950.0)
            ]
        );
        let player = crate::audio::common::create_transcript_segments(&output);
        assert_eq!(player[0].audio_start_time, Some(24.2));
        assert_eq!(player[1].audio_start_time, Some(24.8));
        assert_eq!(player[1].audio_end_time, Some(24.95));
        assert!(cache
            .save(1, 0.0, 1000.0, "bad", 0.9, &[word("bad", -1.0, 50.0)])
            .is_err());
        assert!(cache
            .save(1, 0.0, 1000.0, "bad", 0.9, &[word("bad", 10.0, 1001.0)])
            .is_err());
    }

    #[test]
    fn provider_budgets_preserve_audio_coverage_and_original_clock() {
        use crate::audio::{common::split_segment_at_silence, vad::SpeechSegment};
        let source = SpeechSegment {
            samples: (0..70 * 16000)
                .map(|i| 0.5 + i as f32 / 2_000_000.0)
                .collect(),
            start_timestamp_ms: 120000.0,
            end_timestamp_ms: 190000.0,
            confidence: 0.9,
        };
        for parakeet in [false, true] {
            let limit = chunk_limit(parakeet);
            let parts = split_segment_at_silence(&source, limit);
            let mut covered = 0;
            for part in &parts {
                let start =
                    ((part.start_timestamp_ms - source.start_timestamp_ms) * 16.0).round() as usize;
                let end = start + part.samples.len();
                assert!(start <= covered);
                assert!(part.samples.len() <= limit);
                assert_eq!(part.samples, source.samples[start..end]);
                covered = end;
            }
            assert_eq!(covered, source.samples.len());
            assert!(parts
                .windows(2)
                .all(|s| s[0].end_timestamp_ms - s[1].start_timestamp_ms == 1000.0));
            assert_eq!(
                parts.last().unwrap().end_timestamp_ms,
                source.end_timestamp_ms
            );
        }
    }

    #[test]
    fn successful_job_removes_only_its_own_checkpoint_directory() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.wav");
        std::fs::write(&source, b"synthetic audio").unwrap();
        let first = ChunkCheckpoint::open(dir.path(), &source, "whisper", "tiny", None).unwrap();
        let other = ChunkCheckpoint::open(dir.path(), &source, "parakeet", "v3", None).unwrap();
        first
            .save(0, 0.0, 1000.0, "finished words", 0.9, &[])
            .unwrap();
        other.save(0, 0.0, 1000.0, "other words", 0.9, &[]).unwrap();
        let path = first.path.clone();
        first.finish().unwrap();
        assert!(!path.exists());
        assert_eq!(
            other.load(0, 0.0, 1000.0).unwrap().unwrap().0,
            "other words"
        );
        assert_eq!(std::fs::read(source).unwrap(), b"synthetic audio");
    }

    #[test]
    fn resumes_after_process_exits_without_running_destructors() {
        const CHILD_ROOT: &str = "TINGSKRIVER_CHUNK_TEST_ROOT";
        if let Some(root) = std::env::var_os(CHILD_ROOT) {
            let root = PathBuf::from(root);
            let cache =
                ChunkCheckpoint::open(&root, &root.join("source.wav"), "whisper", "tiny", None)
                    .unwrap();
            cache
                .save(
                    0,
                    0.0,
                    25000.0,
                    "completed before interruption",
                    0.9,
                    &[
                        word("completed", 200.0, 600.0),
                        word("before", 700.0, 1000.0),
                        word("interruption", 1100.0, 1800.0),
                    ],
                )
                .unwrap();
            std::process::exit(47);
        }
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.wav");
        std::fs::write(&source, b"synthetic audio").unwrap();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "audio::chunk_checkpoint::tests::resumes_after_process_exits_without_running_destructors"])
            .env(CHILD_ROOT, dir.path()).output().unwrap().status;
        assert_eq!(status.code(), Some(47));
        let resumed = ChunkCheckpoint::open(dir.path(), &source, "whisper", "tiny", None).unwrap();
        let mut calls = Vec::new();
        for index in 0..3 {
            let start = index as f64 * 24000.0;
            if resumed
                .load(index, start, start + 25000.0)
                .unwrap()
                .is_none()
            {
                calls.push(index);
                resumed
                    .save(index, start, start + 25000.0, "remaining words", 0.9, &[])
                    .unwrap();
            }
        }
        assert_eq!(calls, [1, 2]);
        assert_eq!(
            resumed.load(0, 0.0, 25000.0).unwrap().unwrap().2[0],
            word("completed", 200.0, 600.0)
        );
        assert_eq!(
            resumed.load(0, 0.0, 25000.0).unwrap().unwrap().0,
            "completed before interruption"
        );
    }

    #[test]
    fn restart_reuses_only_completed_chunks_and_rejects_changed_settings() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.wav");
        std::fs::write(&source, b"synthetic audio").unwrap();
        let cache = ChunkCheckpoint::open(dir.path(), &source, "whisper", "tiny", None).unwrap();
        assert!(cache.load(0, 0.0, 25000.0).unwrap().is_none());
        cache
            .save(0, 0.0, 25000.0, "synthetic words", 0.9, &[])
            .unwrap();
        drop(cache);
        let resumed = ChunkCheckpoint::open(dir.path(), &source, "whisper", "tiny", None).unwrap();
        assert_eq!(
            resumed.load(0, 0.0, 25000.0).unwrap().unwrap().0,
            "synthetic words"
        );
        assert!(resumed.load(1, 24000.0, 49000.0).unwrap().is_none());
        assert!(resumed.load(0, 0.0, 24000.0).unwrap().is_none());
        let changed =
            ChunkCheckpoint::open(dir.path(), &source, "whisper", "tiny", Some("da")).unwrap();
        assert!(changed.load(0, 0.0, 25000.0).unwrap().is_none());
        std::fs::write(&source, b"different audio").unwrap();
        let changed = ChunkCheckpoint::open(dir.path(), &source, "whisper", "tiny", None).unwrap();
        assert!(changed.load(0, 0.0, 25000.0).unwrap().is_none());
    }

    #[test]
    fn corrupt_checkpoint_fails_closed_and_empty_text_is_complete() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.wav");
        std::fs::write(&source, b"synthetic").unwrap();
        let cache = ChunkCheckpoint::open(dir.path(), &source, "parakeet", "v3", None).unwrap();
        cache.save(0, 0.0, 1000.0, "", 0.9, &[]).unwrap();
        assert_eq!(cache.load(0, 0.0, 1000.0).unwrap().unwrap().0, "");
        std::fs::write(cache.path.join("0.json"), b"{").unwrap();
        assert!(cache.load(0, 0.0, 1000.0).is_err());
    }

    #[test]
    fn overlap_matching_preserves_punctuation_and_nonoverlapping_repetition() {
        let mut output = vec![("Pærer.".into(), 24200.0, 24700.0)];
        append_chunk(
            &mut output,
            vec![word("PÆRER", 200.0, 700.0), word("Næste", 1500.0, 2000.0)],
            24000.0,
            49000.0,
        );
        assert_eq!(output[0].0, "Pærer.");
        assert_eq!(output[1], ("Næste".into(), 25500.0, 26000.0));
        append_chunk(
            &mut output,
            vec![word("Næste", 0.0, 500.0)],
            50000.0,
            60000.0,
        );
        assert_eq!(output[2].0, "Næste");
    }

    #[test]
    fn two_hours_of_known_words_have_no_gaps_or_duplicates_at_joins() {
        let mut output = Vec::new();
        let mut start = 0;
        while start < 7200 {
            let end = (start + 25).min(7200);
            let words = (start..end)
                .map(|i| {
                    word(
                        &format!("ord{i}"),
                        (i - start) as f64 * 1000.0,
                        (i - start) as f64 * 1000.0 + 800.0,
                    )
                })
                .collect();
            append_chunk(
                &mut output,
                words,
                start as f64 * 1000.0,
                end as f64 * 1000.0,
            );
            if end == 7200 {
                break;
            }
            start = end - 1;
        }
        let expected = (0..7200).map(|i| format!("ord{i}")).collect::<Vec<_>>();
        assert_eq!(
            output.iter().map(|s| &s.0).collect::<Vec<_>>(),
            expected.iter().collect::<Vec<_>>()
        );
        assert!(output.windows(2).all(|s| s[1].1 >= s[0].2));
        for (i, (_, start, end)) in output.iter().enumerate() {
            assert_eq!(*start, i as f64 * 1000.0);
            assert_eq!(*end, i as f64 * 1000.0 + 800.0);
        }
    }
}
