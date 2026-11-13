//! The move from the Tauri app to the GPUI app (#282): a data folder as a
//! Tauri installation leaves it (database of an older schema, settings
//! stores, models, a recording) goes through the NSIS uninstall and the first
//! GPUI start without losing anything. Synthetic data only.

use std::path::Path;

use sqlx::migrate::Migrator;
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{Connection, SqliteConnection};
use tingskriver_core::database::manager::DatabaseManager;
use tingskriver_core::install::tauri_data;
use tingskriver_core::onboarding;
use tingskriver_core::paths::DATABASE_FILE;

/// The newest migration the last Tauri release has (it is built from main
/// after 0.5.0-dev.2, which ended at 20251229000000; the transcripts index
/// came after that). Later migrations are GPUI-era.
const LAST_TAURI_MIGRATION: i64 = 20261031000000;

/// The last schema the Tauri release had: everything up to that migration.
fn tauri_migrator() -> Migrator {
    let mut migrator = sqlx::migrate!("./migrations");
    let all: Vec<_> = migrator.migrations.iter().cloned().collect();
    migrator.migrations = std::borrow::Cow::Owned(
        all.into_iter()
            .filter(|m| m.version <= LAST_TAURI_MIGRATION)
            .collect(),
    );
    migrator
}

fn put(dir: &Path, relative: &str, bytes: &[u8]) {
    let path = dir.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

/// Database, settings and models as a Tauri 0.5.0-dev.x installation has them.
async fn seed_tauri_installation(data: &Path) {
    let options = SqliteConnectOptions::new()
        .filename(data.join(DATABASE_FILE))
        .create_if_missing(true)
        .foreign_keys(true);
    let mut db = SqliteConnection::connect_with(&options).await.unwrap();
    tauri_migrator().run(&mut db).await.unwrap();
    for (id, title) in [("a", "Projektstart"), ("b", "Kvartalsstatus")] {
        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES (?, ?, datetime('now'), datetime('now'))")
            .bind(id).bind(title).execute(&mut db).await.unwrap();
        sqlx::query("INSERT INTO transcripts (id, meeting_id, transcript, timestamp) VALUES (?, ?, ?, datetime('now'))")
            .bind(format!("{id}0")).bind(id).bind("Vi aftalte nye åbningstider")
            .execute(&mut db).await.unwrap();
    }
    // Legacy provider/model choice, without any key.
    sqlx::query("INSERT INTO settings (id, provider, model, whisperModel) VALUES ('1', 'ollama', 'qwen3.5:2b', 'large-v3')")
        .execute(&mut db).await.unwrap();
    db.close().await.unwrap();

    // Settings stores written by tauri-plugin-store and the notification and
    // recording preference files.
    let status = onboarding::completed_status(None, "qwen3.5:2b", "2026-11-01T10:00:00Z");
    put(
        data,
        "onboarding-status.json",
        status.to_string().as_bytes(),
    );
    put(
        data,
        "recording_preferences.json",
        br#"{"preferences":{"auto_save":true}}"#,
    );
    put(data, "notifications.json", br#"{"enabled":true}"#);
    // Models and a recording.
    put(data, "models/parakeet/parakeet-v3.onnx", &[3; 256]);
    put(data, "models/whisper/ggml-large-v3.bin", &[5; 512]);
    put(data, "meetings/a/audio.mp4", &[9; 128]);
}

#[tokio::test]
async fn a_tauri_installation_moves_to_gpui_without_losing_data() {
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("dk.mahoje.tingskriver");
    std::fs::create_dir_all(&data).unwrap();
    seed_tauri_installation(&data).await;

    // The helper: inventory and backup, then the NSIS uninstaller removes the
    // install folder (not the data folder).
    let install = root.path().join("Tingskriver");
    put(&install, "tingskriver.exe", b"MZ");
    let prepared = tauri_data::prepare(&data, &root.path().join("move-backups")).unwrap();
    assert_eq!(prepared.inventory.len(), 7);
    let backup = prepared.backup.clone().expect("a database to back up");
    std::fs::remove_dir_all(&install).unwrap();
    assert!(tauri_data::check(&data, &prepared.inventory)
        .unwrap()
        .is_intact());

    // The first GPUI start: the schema is upgraded, the rows are still there.
    let db = DatabaseManager::open(&data).await.expect("GPUI opens it");
    let meetings: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM meetings")
        .fetch_one(db.pool())
        .await
        .unwrap();
    let text: String = sqlx::query_scalar("SELECT transcript FROM transcripts WHERE id = 'b0'")
        .fetch_one(db.pool())
        .await
        .unwrap();
    let model: String = sqlx::query_scalar("SELECT whisperModel FROM settings WHERE id = '1'")
        .fetch_one(db.pool())
        .await
        .unwrap();
    assert_eq!(
        (meetings, text.as_str(), model.as_str()),
        (2, "Vi aftalte nye åbningstider", "large-v3")
    );
    // The upgrade added what GPUI needs.
    sqlx::query("SELECT COUNT(*) FROM meeting_note_blocks")
        .fetch_one(db.pool())
        .await
        .unwrap();
    db.pool().close().await;

    // Settings stores and models are byte for byte what Tauri left.
    let status: serde_json::Value =
        serde_json::from_slice(&std::fs::read(data.join("onboarding-status.json")).unwrap())
            .unwrap();
    assert!(onboarding::status_completed(&status));
    assert_eq!(
        std::fs::read(data.join("models/whisper/ggml-large-v3.bin"))
            .unwrap()
            .len(),
        512
    );
    // Nothing the Tauri app had is missing (the database file only grew).
    assert!(tauri_data::check(&data, &prepared.inventory)
        .unwrap()
        .is_intact());

    // The backup still holds the Tauri-era database and can be opened by the
    // old schema's reader: it has no GPUI tables.
    let options = SqliteConnectOptions::new()
        .filename(backup.join(DATABASE_FILE))
        .read_only(true);
    let mut old = SqliteConnection::connect_with(&options).await.unwrap();
    let old_meetings: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM meetings")
        .fetch_one(&mut old)
        .await
        .unwrap();
    let has_blocks: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM sqlite_master WHERE name = 'meeting_note_blocks'")
            .fetch_one(&mut old)
            .await
            .unwrap();
    assert_eq!((old_meetings, has_blocks), (2, 0));
}

#[tokio::test]
async fn a_database_deleted_by_the_uninstaller_is_restored_before_gpui_starts() {
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("dk.mahoje.tingskriver");
    std::fs::create_dir_all(&data).unwrap();
    seed_tauri_installation(&data).await;
    let prepared = tauri_data::prepare(&data, &root.path().join("move-backups")).unwrap();

    // Simulate an uninstaller that wipes the data folder's database and models.
    std::fs::remove_file(data.join(DATABASE_FILE)).unwrap();
    std::fs::remove_dir_all(data.join("models")).unwrap();
    let report = tauri_data::check(&data, &prepared.inventory).unwrap();
    assert!(report.database_missing());
    assert_eq!(
        report.missing.len(),
        3,
        "database and two models: {report:?}"
    );

    assert!(tauri_data::restore_database(&data, prepared.backup.as_ref().unwrap()).unwrap());
    let db = DatabaseManager::open(&data)
        .await
        .expect("restored database opens");
    let meetings: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM meetings")
        .fetch_one(db.pool())
        .await
        .unwrap();
    assert_eq!(meetings, 2);
    db.pool().close().await;
    // The models cannot be restored (they are re-downloadable) and stay reported.
    let after = tauri_data::check(&data, &prepared.inventory).unwrap();
    assert_eq!(after.missing.len(), 2);
}

#[tokio::test]
async fn a_wiped_data_folder_gets_its_database_back_from_outside_it() {
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("dk.mahoje.tingskriver");
    std::fs::create_dir_all(&data).unwrap();
    seed_tauri_installation(&data).await;
    let prepared = tauri_data::prepare(&data, &root.path().join("move-backups")).unwrap();

    std::fs::remove_dir_all(&data).unwrap();
    assert!(tauri_data::restore_database(&data, prepared.backup.as_ref().unwrap()).unwrap());
    let db = DatabaseManager::open(&data)
        .await
        .expect("restored database opens");
    let meetings: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM meetings")
        .fetch_one(db.pool())
        .await
        .unwrap();
    assert_eq!(meetings, 2);
    db.pool().close().await;
}
