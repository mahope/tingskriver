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

pub(crate) struct ChunkCheckpoint {
    path: PathBuf,
}

#[derive(Serialize, Deserialize)]
struct CompletedChunk {
    start_ms: f64,
    end_ms: f64,
    text: String,
    confidence: f32,
}

impl ChunkCheckpoint {
    pub(crate) fn finish(self) -> Result<()> {
        std::fs::remove_dir_all(self.path)
            .context("Cannot remove completed transcription checkpoints")
    }

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
            "chunks-v1",
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

    pub(crate) fn load(
        &self,
        index: usize,
        start_ms: f64,
        end_ms: f64,
    ) -> Result<Option<(String, f32)>> {
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
        Ok(Some((chunk.text, chunk.confidence)))
    }

    pub(crate) fn save(
        &self,
        index: usize,
        start_ms: f64,
        end_ms: f64,
        text: &str,
        confidence: f32,
    ) -> Result<()> {
        anyhow::ensure!(confidence.is_finite(), "Invalid transcription confidence");
        let chunk = CompletedChunk {
            start_ms,
            end_ms,
            text: text.into(),
            confidence,
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

/// Text-only engines expose no word times. Match only suffix/prefix words from
/// overlapping audio; never deduplicate repetitions across a real pause.
pub(crate) fn append_chunk(
    output: &mut Vec<(String, f64, f64)>,
    text: String,
    mut start: f64,
    end: f64,
) -> bool {
    let mut text = text.trim().to_string();
    if let Some((previous, _, previous_end)) = output.last() {
        if start < *previous_end {
            let before: Vec<_> = previous.split_whitespace().rev().take(64).collect();
            let after: Vec<_> = text.split_whitespace().take(64).collect();
            let normalize = |s: &str| {
                s.trim_matches(|c: char| !c.is_alphanumeric())
                    .to_lowercase()
            };
            let shared = (1..=before.len().min(after.len()))
                .rev()
                .find(|&n| {
                    before[..n]
                        .iter()
                        .rev()
                        .zip(&after[..n])
                        .all(|(a, b)| !normalize(a).is_empty() && normalize(a) == normalize(b))
                })
                .unwrap_or(0);
            if shared > 0 {
                let offset = text
                    .split_whitespace()
                    .take(shared)
                    .last()
                    .map(|word| word.as_ptr() as usize - text.as_ptr() as usize + word.len())
                    .unwrap();
                text = text[offset..].trim_start().to_string();
            }
            start = *previous_end;
        }
    }
    if !text.is_empty() {
        output.push((text, start, end.max(start)));
        true
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        first.save(0, 0.0, 1000.0, "finished words", 0.9).unwrap();
        other.save(0, 0.0, 1000.0, "other words", 0.9).unwrap();
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
                .save(0, 0.0, 25000.0, "completed before interruption", 0.9)
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
                    .save(index, start, start + 25000.0, "remaining words", 0.9)
                    .unwrap();
            }
        }
        assert_eq!(calls, [1, 2]);
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
        cache.save(0, 0.0, 25000.0, "synthetic words", 0.9).unwrap();
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
        cache.save(0, 0.0, 1000.0, "", 0.9).unwrap();
        assert_eq!(cache.load(0, 0.0, 1000.0).unwrap().unwrap().0, "");
        std::fs::write(cache.path.join("0.json"), b"{").unwrap();
        assert!(cache.load(0, 0.0, 1000.0).is_err());
    }

    #[test]
    fn overlap_matching_preserves_punctuation_and_nonoverlapping_repetition() {
        let mut output = vec![("Vi taler om æbler, og pærer.".into(), 0.0, 25000.0)];
        append_chunk(
            &mut output,
            "ÆBLER og pærer. Næste punkt.".into(),
            24000.0,
            49000.0,
        );
        assert_eq!(output[1].0, "Næste punkt.");
        assert_eq!(output[1].1, 25000.0);
        append_chunk(&mut output, "Næste punkt.".into(), 50000.0, 60000.0);
        assert_eq!(output[2].0, "Næste punkt.");
    }

    #[test]
    fn two_hours_of_known_words_have_no_gaps_or_duplicates_at_joins() {
        let mut output = Vec::new();
        let mut start = 0;
        while start < 7200 {
            let end = (start + 25).min(7200);
            let text = (start..end)
                .map(|i| format!("ord{i}"))
                .collect::<Vec<_>>()
                .join(" ");
            append_chunk(
                &mut output,
                text,
                start as f64 * 1000.0,
                end as f64 * 1000.0,
            );
            if end == 7200 {
                break;
            }
            start = end - 1;
        }
        let actual = output
            .iter()
            .flat_map(|s| s.0.split_whitespace())
            .collect::<Vec<_>>();
        let expected = (0..7200).map(|i| format!("ord{i}")).collect::<Vec<_>>();
        assert_eq!(actual, expected);
        assert!(output.windows(2).all(|s| s[1].1 >= s[0].2));
        assert_eq!(output.last().unwrap().2, 7200000.0);
    }
}
