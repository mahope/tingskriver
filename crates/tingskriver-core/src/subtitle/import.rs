//! Turns a `.vtt`/`.srt` file into a meeting without audio and starts the
//! summary. The meeting and its transcript are committed before the summary
//! starts, so a summary that fails never takes them with it; "Lav referat
//! igen" is [`start_summary_for_meeting`] called again.

use super::parser::{cues_to_prompt_text, parse_subtitles, Cue};
use crate::database::repositories::setting::SettingsRepository;
use crate::host::{HostExt, HostHandle};
use crate::state::AppState;
use anyhow::{anyhow, Result};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use std::path::Path;
use uuid::Uuid;

/// Subtitle files are text; anything larger is not a transcript.
const MAX_SUBTITLE_BYTES: u64 = 20 * 1024 * 1024;

/// Template used for the summary of an imported transcript.
const IMPORT_SUMMARY_TEMPLATE: &str = "standard_meeting";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubtitleImportOutcome {
    pub meeting_id: String,
    pub title: String,
    pub segments_count: usize,
    pub duration_seconds: f64,
    /// The same content was imported before; no new meeting was created.
    pub already_imported: bool,
    /// Set when the summary could not be started. The meeting is kept.
    pub summary_error: Option<String>,
}

/// File contents and the time the file was made (creation time, else
/// modification time).
#[derive(Debug)]
pub struct SubtitleFile {
    pub content: String,
    pub file_time: Option<DateTime<Utc>>,
    pub title: String,
}

pub fn read_subtitle_file(path: &Path) -> Result<SubtitleFile> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    if !matches!(ext.as_deref(), Some("vtt") | Some("srt")) {
        return Err(anyhow!("Only .vtt and .srt files can be imported"));
    }
    let meta = std::fs::metadata(path).map_err(|e| anyhow!("Cannot read the file: {e}"))?;
    if !meta.is_file() {
        return Err(anyhow!("Not a file: {}", path.display()));
    }
    if meta.len() > MAX_SUBTITLE_BYTES {
        return Err(anyhow!("The file is too large to be a transcript"));
    }
    let file_time = meta
        .created()
        .or_else(|_| meta.modified())
        .ok()
        .map(DateTime::<Utc>::from);
    let bytes = std::fs::read(path).map_err(|e| anyhow!("Cannot read the file: {e}"))?;
    let content = decode_text(&bytes)?;
    let title = path
        .file_stem()
        .and_then(|s| s.to_str())
        .map(|s| s.replace(['_', '.'], " ").trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Importeret transskript".to_string());
    Ok(SubtitleFile {
        content,
        file_time,
        title,
    })
}

/// UTF-8 (with or without BOM) or UTF-16 with BOM.
pub fn decode_text(bytes: &[u8]) -> Result<String> {
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return decode_utf16(rest, u16::from_le_bytes);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return decode_utf16(rest, u16::from_be_bytes);
    }
    String::from_utf8(bytes.to_vec()).map_err(|_| anyhow!("The file is not valid UTF-8 text"))
}

fn decode_utf16(bytes: &[u8], f: fn([u8; 2]) -> u16) -> Result<String> {
    let units: Vec<u16> = bytes.chunks_exact(2).map(|c| f([c[0], c[1]])).collect();
    String::from_utf16(&units).map_err(|_| anyhow!("The file is not valid UTF-16 text"))
}

/// Hash of the parsed content, independent of line endings, BOM and
/// formatting differences between `.vtt` and `.srt`.
pub fn import_key(cues: &[Cue]) -> String {
    let mut h = Sha256::new();
    for c in cues {
        h.update(c.start_ms.to_le_bytes());
        h.update(c.end_ms.to_le_bytes());
        h.update(c.speaker.as_deref().unwrap_or("").as_bytes());
        h.update([0]);
        h.update(c.text.as_bytes());
        h.update([0]);
    }
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// Meeting start = file time minus the transcript's length, so a transcript
/// exported right after the meeting lands on the day the meeting happened.
pub fn meeting_start(file_time: Option<DateTime<Utc>>, now: DateTime<Utc>, duration_ms: u64) -> DateTime<Utc> {
    file_time.unwrap_or(now) - Duration::milliseconds(duration_ms.min(i64::MAX as u64) as i64)
}

/// Creates the meeting and transcript rows, or returns the existing meeting
/// for the same content. Returns `(meeting_id, already_imported)`.
pub async fn save_imported_meeting(
    pool: &SqlitePool,
    title: &str,
    cues: &[Cue],
    file_time: Option<DateTime<Utc>>,
) -> Result<(String, bool)> {
    let key = import_key(cues);
    if let Some(id) = find_by_key(pool, &key).await? {
        return Ok((id, true));
    }

    let duration_ms = cues.iter().map(|c| c.end_ms).max().unwrap_or(0);
    let started = meeting_start(file_time, Utc::now(), duration_ms);
    let meeting_id = format!("meeting-{}", Uuid::new_v4());

    let mut tx = pool.begin().await?;
    let inserted = sqlx::query(
        "INSERT INTO meetings (id, title, created_at, updated_at, folder_path, import_key)
         VALUES (?, ?, ?, ?, NULL, ?)",
    )
    .bind(&meeting_id)
    .bind(title)
    .bind(started)
    .bind(started)
    .bind(&key)
    .execute(&mut *tx)
    .await;
    if let Err(e) = inserted {
        drop(tx);
        // A concurrent import of the same file wins the unique index.
        if let Some(id) = find_by_key(pool, &key).await? {
            return Ok((id, true));
        }
        return Err(anyhow!("Failed to create meeting: {e}"));
    }

    for cue in cues {
        let start = cue.start_ms as f64 / 1000.0;
        let end = cue.end_ms as f64 / 1000.0;
        let at = started + Duration::milliseconds(cue.start_ms as i64);
        sqlx::query(
            "INSERT INTO transcripts
               (id, meeting_id, transcript, timestamp, audio_start_time, audio_end_time, duration, speaker)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(format!("transcript-{}", Uuid::new_v4()))
        .bind(&meeting_id)
        .bind(&cue.text)
        .bind(at.to_rfc3339())
        .bind(start)
        .bind(end)
        .bind(end - start)
        .bind(cue.speaker.as_deref())
        .execute(&mut *tx)
        .await
        .map_err(|e| anyhow!("Failed to insert transcript: {e}"))?;
    }
    tx.commit().await?;
    Ok((meeting_id, false))
}

async fn find_by_key(pool: &SqlitePool, key: &str) -> Result<Option<String>> {
    Ok(
        sqlx::query_scalar::<_, String>("SELECT id FROM meetings WHERE import_key = ?")
            .bind(key)
            .fetch_optional(pool)
            .await?,
    )
}

/// The saved transcript of a meeting as prompt text, speakers included.
pub async fn load_prompt_text(pool: &SqlitePool, meeting_id: &str) -> Result<String> {
    let rows: Vec<(String, Option<String>, Option<f64>, Option<f64>)> = sqlx::query_as(
        "SELECT transcript, speaker, audio_start_time, audio_end_time FROM transcripts
         WHERE meeting_id = ? ORDER BY audio_start_time ASC, rowid ASC",
    )
    .bind(meeting_id)
    .fetch_all(pool)
    .await?;
    let cues: Vec<Cue> = rows
        .into_iter()
        .map(|(text, speaker, start, end)| Cue {
            start_ms: (start.unwrap_or(0.0) * 1000.0) as u64,
            end_ms: (end.unwrap_or(0.0) * 1000.0) as u64,
            // `mic`/`system` mark audio sources, not people.
            speaker: speaker.filter(|s| s != "mic" && s != "system"),
            text,
        })
        .collect();
    if cues.is_empty() {
        return Err(anyhow!("The meeting has no transcript"));
    }
    Ok(cues_to_prompt_text(&cues))
}

/// Starts the summary with the provider chosen in settings (#44). Used for
/// the first summary after an import and for "Lav referat igen".
pub async fn start_summary_for_meeting(
    app: HostHandle,
    state: &AppState,
    meeting_id: &str,
) -> Result<(), String> {
    let pool = state.db_manager.pool();
    let config = SettingsRepository::get_model_config(pool)
        .await
        .map_err(|e| format!("Could not read the summary settings: {e}"))?
        .ok_or_else(|| "No summary provider is configured".to_string())?;
    let text = load_prompt_text(pool, meeting_id)
        .await
        .map_err(|e| e.to_string())?;
    crate::summary::commands::api_process_transcript(
        app,
        state,
        text,
        config.provider,
        config.model,
        Some(meeting_id.to_string()),
        None,
        None,
        None,
        Some(IMPORT_SUMMARY_TEMPLATE.to_string()),
        None,
        None,
    )
    .await
    .map(|_| ())
}

/// Reads, parses and saves a file, then starts the summary. Emits
/// `subtitle-import-complete` or `subtitle-import-error`.
pub async fn import_subtitle_file(
    app: HostHandle,
    state: &AppState,
    path: &str,
) -> Result<SubtitleImportOutcome, String> {
    let result = run_import(app.clone(), state, path).await;
    match &result {
        Ok(outcome) => {
            let _ = app.emit("subtitle-import-complete", outcome);
        }
        Err(error) => {
            let _ = app.emit("subtitle-import-error", serde_json::json!({ "error": error }));
        }
    }
    result
}

async fn run_import(
    app: HostHandle,
    state: &AppState,
    path: &str,
) -> Result<SubtitleImportOutcome, String> {
    let path = path.to_string();
    let file = tokio::task::spawn_blocking(move || read_subtitle_file(Path::new(&path)))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    let parsed = parse_subtitles(&file.content).map_err(|e| e.to_string())?;
    let duration_ms = parsed.cues.iter().map(|c| c.end_ms).max().unwrap_or(0);

    let (meeting_id, already_imported) = save_imported_meeting(
        state.db_manager.pool(),
        &file.title,
        &parsed.cues,
        file.file_time,
    )
    .await
    .map_err(|e| e.to_string())?;

    // A repeated file keeps its meeting and its summary as they are.
    let summary_error = if already_imported {
        None
    } else {
        start_summary_for_meeting(app, state, &meeting_id).await.err()
    };

    Ok(SubtitleImportOutcome {
        meeting_id,
        title: file.title,
        segments_count: parsed.cues.len(),
        duration_seconds: duration_ms as f64 / 1000.0,
        already_imported,
        summary_error,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use sqlx::sqlite::SqlitePoolOptions;

    const VTT: &str = "WEBVTT\n\n00:00:01.000 --> 00:00:04.000\n<v Anna Testesen>Goddag.</v>\n\n00:00:04.000 --> 00:00:09.000\n<v Bo Eksempel>Tak.</v>\n";

    async fn pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        pool
    }

    #[tokio::test]
    async fn imports_speakers_timecodes_and_date_without_audio() {
        let pool = pool().await;
        let cues = parse_subtitles(VTT).unwrap().cues;
        let file_time = Utc.with_ymd_and_hms(2026, 11, 3, 10, 0, 9).unwrap();
        let (id, again) = save_imported_meeting(&pool, "Test", &cues, Some(file_time))
            .await
            .unwrap();
        assert!(!again);

        let (created, folder): (DateTime<Utc>, Option<String>) =
            sqlx::query_as("SELECT created_at, folder_path FROM meetings WHERE id = ?")
                .bind(&id)
                .fetch_one(&pool)
                .await
                .unwrap();
        // 9 s of transcript: the meeting started at 10:00:00.
        assert_eq!(created, Utc.with_ymd_and_hms(2026, 11, 3, 10, 0, 0).unwrap());
        assert!(folder.is_none());

        let rows: Vec<(String, Option<String>, f64, f64)> = sqlx::query_as(
            "SELECT transcript, speaker, audio_start_time, audio_end_time FROM transcripts
             WHERE meeting_id = ? ORDER BY audio_start_time",
        )
        .bind(&id)
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0], ("Goddag.".into(), Some("Anna Testesen".into()), 1.0, 4.0));
        assert_eq!(rows[1].1.as_deref(), Some("Bo Eksempel"));

        assert_eq!(
            load_prompt_text(&pool, &id).await.unwrap(),
            "Anna Testesen: Goddag.\nBo Eksempel: Tak."
        );
    }

    #[tokio::test]
    async fn same_content_twice_gives_one_meeting() {
        let pool = pool().await;
        let a = parse_subtitles(VTT).unwrap().cues;
        // Same content with CRLF and a BOM: same key.
        let b = parse_subtitles(&format!("\u{feff}{}", VTT.replace('\n', "\r\n")))
            .unwrap()
            .cues;
        let (id1, first_again) = save_imported_meeting(&pool, "A", &a, None).await.unwrap();
        let (id2, second_again) = save_imported_meeting(&pool, "B", &b, None).await.unwrap();
        assert!(!first_again);
        assert!(second_again);
        assert_eq!(id1, id2);
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM meetings")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(n, 1);
        let t: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM transcripts")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(t, 2);
    }

    #[tokio::test]
    async fn different_content_gives_a_new_meeting() {
        let pool = pool().await;
        let a = parse_subtitles(VTT).unwrap().cues;
        let b = parse_subtitles("1\n00:00:00,000 --> 00:00:01,000\nAndet indhold\n")
            .unwrap()
            .cues;
        let (id1, _) = save_imported_meeting(&pool, "A", &a, None).await.unwrap();
        let (id2, again) = save_imported_meeting(&pool, "B", &b, None).await.unwrap();
        assert!(!again);
        assert_ne!(id1, id2);
    }

    #[tokio::test]
    async fn failed_summary_leaves_meeting_and_transcript() {
        let pool = pool().await;
        let cues = parse_subtitles(VTT).unwrap().cues;
        let (id, _) = save_imported_meeting(&pool, "A", &cues, None).await.unwrap();
        // The summary process fails (as `fail_and_cleanup` records it).
        sqlx::query(
            "INSERT INTO summary_processes (meeting_id, status, created_at, updated_at, error)
             VALUES (?, 'failed', ?, ?, 'provider down')",
        )
        .bind(&id)
        .bind(Utc::now())
        .bind(Utc::now())
        .execute(&pool)
        .await
        .unwrap();
        let t: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM transcripts WHERE meeting_id = ?")
            .bind(&id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(t, 2);
        // "Lav referat igen" reads the same transcript back.
        assert!(load_prompt_text(&pool, &id).await.unwrap().contains("Goddag."));
    }

    #[test]
    fn decodes_utf8_and_utf16_and_rejects_other_bytes() {
        assert_eq!(decode_text("æøå".as_bytes()).unwrap(), "æøå");
        let mut le = vec![0xFF, 0xFE];
        le.extend("æø".encode_utf16().flat_map(|u| u.to_le_bytes()));
        assert_eq!(decode_text(&le).unwrap(), "æø");
        assert!(decode_text(&[0x80, 0x81, 0x82]).is_err());
    }

    #[test]
    fn rejects_other_extensions() {
        let err = read_subtitle_file(Path::new("/nonexistent/møde.txt")).unwrap_err();
        assert!(err.to_string().contains(".vtt"));
    }
}
