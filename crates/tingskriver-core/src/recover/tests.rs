use super::*;
use crate::audio::track_writer::write_wav_header;
use std::io::{Seek, SeekFrom, Write};
use std::path::PathBuf;

fn fixture(root: &Path, id: &str, truncated: bool) -> PathBuf {
    let folder = root.join(id);
    let raw = folder.join(RAW_DIR);
    std::fs::create_dir_all(&raw).unwrap();
    let mut f = std::fs::File::create(raw.join("microphone_000.wav")).unwrap();
    write_wav_header(&mut f, 48000, 96000 * 3).unwrap();
    f.seek(SeekFrom::Start(44)).unwrap();
    f.write_all(&vec![0; if truncated { 96000 * 2 + 1 } else { 96000 * 3 }])
        .unwrap();
    folder
}

#[test]
fn scan_finds_audio_without_ui_state_and_ignores_empty_and_finished() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path(), "complete", false);
    fixture(root.path(), "truncated", true);
    std::fs::create_dir_all(root.path().join("empty").join(RAW_DIR)).unwrap();
    let header_only = fixture(root.path(), "header-only", false);
    std::fs::OpenOptions::new()
        .write(true)
        .open(header_only.join(RAW_DIR).join("microphone_000.wav"))
        .unwrap()
        .set_len(44)
        .unwrap();
    let finished = fixture(root.path(), "finished", false);
    std::fs::write(finished.join(MIXED_FILE), b"finished").unwrap();
    let found = scan(root.path()).unwrap();
    assert_eq!(found.len(), 2);
    assert_eq!(found[0].id, "complete");
    assert_eq!(found[0].duration_seconds, 3.0);
    assert_eq!(found[1].duration_seconds, 2.0);
    assert_eq!(found[0].tracks, vec![TrackKind::Microphone]);
    assert!(found[0].last_written > 0);
    scan_at_startup(root.path());
    assert_eq!(
        startup_recordings()
            .iter()
            .map(|r| &r.id)
            .collect::<Vec<_>>(),
        found.iter().map(|r| &r.id).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn restore_verifies_audio_and_registers_meeting_idempotently() {
    let root = tempfile::tempdir().unwrap();
    let folder = fixture(root.path(), "crashed", true);
    let db_dir = tempfile::tempdir().unwrap();
    let db = crate::database::manager::DatabaseManager::open(db_dir.path())
        .await
        .unwrap();
    let ffmpeg = crate::audio::ffmpeg::find_ffmpeg_path().expect("ffmpeg required");
    sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at, folder_path) VALUES ('existing', 'Fixture', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, ?)")
        .bind(folder.canonicalize().unwrap().to_string_lossy().as_ref())
        .execute(db.pool()).await.unwrap();
    let result = restore(root.path(), "crashed", db.pool(), Some(&ffmpeg))
        .await
        .unwrap();
    assert_eq!(result.meeting_id, "existing");
    assert!(result.audio.audio_file.is_some());
    assert!(
        (crate::audio::finalize::decoded_duration_seconds(&folder.join(MIXED_FILE)).unwrap() - 2.0)
            .abs()
            < 0.2
    );
    assert!(
        folder.join(RAW_DIR).exists(),
        "truncated originals must stay"
    );
    let again = restore(root.path(), "crashed", db.pool(), Some(&ffmpeg))
        .await
        .unwrap();
    assert_eq!(result.meeting_id, again.meeting_id);
    let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM meetings")
        .fetch_one(db.pool())
        .await
        .unwrap();
    assert_eq!(count, 1);
    assert!(restore(root.path(), "../escape", db.pool(), Some(&ffmpeg))
        .await
        .is_err());
}

#[tokio::test]
async fn failed_database_save_remains_discoverable_after_encoding() {
    let root = tempfile::tempdir().unwrap();
    let folder = fixture(root.path(), "db-failure", false);
    let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
    // SELECT succeeds, but INSERT fails, simulating unavailable storage.
    sqlx::query("CREATE TABLE meetings (id TEXT, folder_path TEXT)")
        .execute(&pool)
        .await
        .unwrap();
    let ffmpeg = crate::audio::ffmpeg::find_ffmpeg_path().expect("ffmpeg required");
    assert!(restore(root.path(), "db-failure", &pool, Some(&ffmpeg))
        .await
        .is_err());
    assert!(folder.join(MIXED_FILE).exists());
    assert_eq!(scan(root.path()).unwrap().len(), 1);
    assert!(folder.join(RAW_DIR).exists());
}

#[tokio::test]
async fn complete_restore_and_empty_folder() {
    let root = tempfile::tempdir().unwrap();
    let folder = fixture(root.path(), "complete", false);
    let db_dir = tempfile::tempdir().unwrap();
    let db = crate::database::manager::DatabaseManager::open(db_dir.path())
        .await
        .unwrap();
    let ffmpeg = crate::audio::ffmpeg::find_ffmpeg_path().expect("ffmpeg required");
    let restored = restore(root.path(), "complete", db.pool(), Some(&ffmpeg))
        .await
        .unwrap();
    assert_eq!(restored.audio.status, AudioSaveStatus::Ok);
    assert!(scan(root.path()).unwrap().is_empty());
    assert!(
        (crate::audio::finalize::decoded_duration_seconds(&folder.join(MIXED_FILE)).unwrap() - 3.0)
            .abs()
            < 0.2
    );
    std::fs::create_dir_all(root.path().join("empty").join(RAW_DIR)).unwrap();
    assert!(restore(root.path(), "empty", db.pool(), Some(&ffmpeg))
        .await
        .is_err());
    let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM meetings")
        .fetch_one(db.pool())
        .await
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn crash_writer_child() {
    let Some(root) = std::env::var_os("TINGSKRIVER_RECOVERY_CRASH_FIXTURE") else {
        return;
    };
    use crate::audio::recording_writer::{spawn_writer, LogObserver};
    let root = PathBuf::from(root);
    std::fs::create_dir_all(root.join("killed").join(RAW_DIR)).unwrap();
    let (sender, _handle) = spawn_writer(
        root.join("killed").join(RAW_DIR),
        48000,
        std::sync::Arc::new(LogObserver),
    )
    .unwrap();
    for n in 1..=100 {
        for kind in TrackKind::ALL {
            sender
                .send(kind, vec![0.1; 4800], Some(n as f64 / 10.0))
                .unwrap();
        }
        if n == 30 {
            std::fs::write(root.join("ready"), b"3.0").unwrap();
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    panic!("parent did not terminate crash fixture");
}

#[tokio::test]
async fn killed_writer_recovers_both_tracks_without_ui_storage() {
    use std::process::{Command, Stdio};
    let root = tempfile::tempdir().unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "recover::tests::crash_writer_child",
            "--nocapture",
        ])
        .env("TINGSKRIVER_RECOVERY_CRASH_FIXTURE", root.path())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while !root.path().join("ready").exists() && std::time::Instant::now() < deadline {
        if child.try_wait().unwrap().is_some() {
            panic!("crash fixture exited before ready");
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    child.kill().unwrap();
    child.wait().unwrap();
    assert!(
        root.path().join("ready").exists(),
        "writer startup timed out"
    );
    let found = scan(root.path()).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].tracks, TrackKind::ALL);
    assert!(
        found[0].duration_seconds >= 1.0,
        "more than 2 seconds lost: {:?}",
        found[0]
    );
    // A fresh database substitutes for cleared/absent UI storage.
    let db_dir = tempfile::tempdir().unwrap();
    let db = crate::database::manager::DatabaseManager::open(db_dir.path())
        .await
        .unwrap();
    let ffmpeg = crate::audio::ffmpeg::find_ffmpeg_path().expect("ffmpeg required");
    let restored = restore(root.path(), &found[0].id, db.pool(), Some(&ffmpeg))
        .await
        .unwrap();
    let duration = crate::audio::finalize::decoded_duration_seconds(
        restored.audio.audio_file.as_ref().unwrap(),
    )
    .unwrap();
    assert!((duration - found[0].duration_seconds).abs() < 0.2);
    assert!(duration >= 1.0, "more than 2 seconds lost after restore");
}
