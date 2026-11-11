//! Audio retention planning and permanent file removal. Call only for completed
//! audio: the host owns the inventory and must exclude active/in-use recordings.
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DictationRetention {
    Immediately,
    Hours24,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeetingRetention {
    Immediately,
    Days7,
    Days30,
    Never,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioRetention {
    Dictation(DictationRetention),
    Meeting(MeetingRetention),
}

impl AudioRetention {
    fn duration(self) -> Option<Duration> {
        let days = match self {
            Self::Dictation(DictationRetention::Immediately)
            | Self::Meeting(MeetingRetention::Immediately) => 0,
            Self::Dictation(DictationRetention::Hours24) => 1,
            Self::Meeting(MeetingRetention::Days7) => 7,
            Self::Meeting(MeetingRetention::Days30) => 30,
            Self::Meeting(MeetingRetention::Never) => return None,
        };
        Some(Duration::from_secs(days * 24 * 60 * 60))
    }
}

/// One audio file; transcripts and other meeting files are never inferred.
#[derive(Clone)]
pub struct AudioFile {
    pub path: PathBuf,
    pub completed_at: SystemTime,
    pub retention: AudioRetention,
}

/// The paths stay private and are never formatted into logs or errors.
pub struct DeletionPlan {
    paths: Vec<PathBuf>,
}

pub fn plan(files: &[AudioFile], now: SystemTime) -> DeletionPlan {
    let mut paths = Vec::new();
    let mut seen = HashSet::new();
    for file in files {
        let due = file.retention.duration().is_some_and(|duration| {
            now.duration_since(file.completed_at)
                .is_ok_and(|age| age >= duration)
        });
        if due && seen.insert(&file.path) {
            paths.push(file.path.clone());
        }
    }
    DeletionPlan { paths }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct DeletionReport {
    pub deleted: usize,
    pub already_absent: usize,
    pub failed: usize,
}

/// Unlinks files directly, without a recycle bin. This does not promise secure
/// erasure of storage blocks, backups or copies outside the supplied inventory.
/// Failures are counted so the caller can rebuild and retry the plan.
pub async fn execute(plan: DeletionPlan) -> DeletionReport {
    let mut report = DeletionReport::default();
    for path in plan.paths {
        match tokio::fs::remove_file(path).await {
            Ok(()) => report.deleted += 1,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                report.already_absent += 1
            }
            Err(_) => report.failed += 1,
        }
    }
    log::info!(
        "Audio retention: deleted={}, absent={}, failed={}",
        report.deleted,
        report.already_absent,
        report.failed
    );
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_policies_expire_at_the_exact_boundary() {
        use DictationRetention as D;
        use MeetingRetention as M;
        for (policy, seconds) in [
            (AudioRetention::Dictation(D::Immediately), 0),
            (AudioRetention::Dictation(D::Hours24), 86400),
            (AudioRetention::Meeting(M::Immediately), 0),
            (AudioRetention::Meeting(M::Days7), 7 * 86400),
            (AudioRetention::Meeting(M::Days30), 30 * 86400),
        ] {
            let file = AudioFile {
                path: "audio.wav".into(),
                completed_at: SystemTime::UNIX_EPOCH + Duration::from_secs(1),
                retention: policy,
            };
            let deadline = file.completed_at + Duration::from_secs(seconds);
            assert_eq!(plan(std::slice::from_ref(&file), deadline).paths.len(), 1);
            // 1 ms, not 1 ns: Windows SystemTime counts in 100 ns ticks, so a
            // nanosecond step rounds back onto the deadline.
            assert!(plan(&[file], deadline - Duration::from_millis(1))
                .paths
                .is_empty());
        }
        let file = AudioFile {
            path: "audio.wav".into(),
            completed_at: SystemTime::UNIX_EPOCH,
            retention: AudioRetention::Meeting(M::Never),
        };
        assert!(plan(&[file], SystemTime::now()).paths.is_empty());
    }

    #[tokio::test]
    async fn deletes_only_planned_audio_and_retries_safely() {
        let dir = tempfile::tempdir().unwrap();
        let audio = dir.path().join("audio.wav");
        let transcript = dir.path().join("transcript.txt");
        tokio::fs::write(&audio, b"audio content").await.unwrap();
        tokio::fs::write(&transcript, b"text content")
            .await
            .unwrap();
        let file = AudioFile {
            path: audio.clone(),
            completed_at: SystemTime::UNIX_EPOCH,
            retention: AudioRetention::Dictation(DictationRetention::Immediately),
        };
        let files = [file.clone(), file];
        assert_eq!(execute(plan(&files, SystemTime::now())).await.deleted, 1);
        assert!(!tokio::fs::try_exists(&audio).await.unwrap());
        assert_eq!(tokio::fs::read(&transcript).await.unwrap(), b"text content");
        assert_eq!(
            execute(plan(&files, SystemTime::now()))
                .await
                .already_absent,
            1
        );
        let report = execute(DeletionPlan {
            paths: vec![dir.path().to_path_buf()],
        })
        .await;
        assert_eq!(report.failed, 1);
        assert!(tokio::fs::try_exists(&transcript).await.unwrap());
    }
}
