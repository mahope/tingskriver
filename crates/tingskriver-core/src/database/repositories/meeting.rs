use crate::api::{MeetingDetails, MeetingTranscript};
use crate::database::models::{MeetingModel, Transcript};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::{Connection, Error as SqlxError, FromRow, Row, SqliteConnection, SqlitePool};
use tracing::{error, info};

pub struct MeetingsRepository;

/// Where the next page of a meeting's transcripts starts: just after this
/// segment in recording order (`audio_start_time`, then insertion order).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TranscriptCursor {
    pub audio_start_time: Option<f64>,
    pub row: i64,
}

/// The transcripts of a meeting in recording order, one page at a time.
/// Both queries read `idx_transcripts_meeting_start`, whose entries end in
/// the rowid, so the order needs no sort and a page seeks straight to the
/// cursor instead of skipping rows like OFFSET. Segments without
/// `audio_start_time` (old recordings) sort first, as NULLs do in SQLite.
pub const TRANSCRIPT_PAGE_SQL: &str = "SELECT *, rowid AS cursor_row FROM transcripts
     WHERE meeting_id = ?1 AND (audio_start_time, rowid) > (?2, ?3)
     ORDER BY audio_start_time ASC, rowid ASC
     LIMIT ?4";
const TRANSCRIPT_PAGE_AFTER_NULL_SQL: &str = "SELECT *, rowid AS cursor_row FROM transcripts
     WHERE meeting_id = ?1 AND (audio_start_time IS NOT NULL OR rowid > ?2)
     ORDER BY audio_start_time ASC, rowid ASC
     LIMIT ?3";
const TRANSCRIPT_FIRST_PAGE_SQL: &str = "SELECT *, rowid AS cursor_row FROM transcripts
     WHERE meeting_id = ?1
     ORDER BY audio_start_time ASC, rowid ASC
     LIMIT ?2";

impl MeetingsRepository {
    pub async fn get_meetings(pool: &SqlitePool) -> Result<Vec<MeetingModel>, sqlx::Error> {
        let meetings =
            sqlx::query_as::<_, MeetingModel>("SELECT * FROM meetings ORDER BY created_at DESC")
                .fetch_all(pool)
                .await?;
        Ok(meetings)
    }

    pub async fn delete_meeting(pool: &SqlitePool, meeting_id: &str) -> Result<bool, SqlxError> {
        if meeting_id.trim().is_empty() {
            return Err(SqlxError::Protocol(
                "meeting_id cannot be empty".to_string(),
            ));
        }

        let mut conn = pool.acquire().await?;
        let mut transaction = conn.begin().await?;

        match delete_meeting_with_transaction(&mut transaction, meeting_id).await {
            Ok(success) => {
                if success {
                    transaction.commit().await?;
                    info!(
                        "Successfully deleted meeting {} and all associated data",
                        meeting_id
                    );
                    Ok(true)
                } else {
                    transaction.rollback().await?;
                    Ok(false)
                }
            }
            Err(e) => {
                let _ = transaction.rollback().await;
                error!("Failed to delete meeting {}: {}", meeting_id, e);
                Err(e)
            }
        }
    }

    pub async fn get_meeting(
        pool: &SqlitePool,
        meeting_id: &str,
    ) -> Result<Option<MeetingDetails>, SqlxError> {
        if meeting_id.trim().is_empty() {
            return Err(SqlxError::Protocol(
                "meeting_id cannot be empty".to_string(),
            ));
        }

        let mut conn = pool.acquire().await?;
        let mut transaction = conn.begin().await?;

        // Get meeting details
        let meeting: Option<MeetingModel> =
            sqlx::query_as("SELECT id, title, created_at, updated_at, folder_path FROM meetings WHERE id = ?")
                .bind(meeting_id)
                .fetch_optional(&mut *transaction)
                .await?;

        if meeting.is_none() {
            transaction.rollback().await?;
            return Err(SqlxError::RowNotFound);
        }

        if let Some(meeting) = meeting {
            // Get all transcripts for this meeting
            let transcripts =
                sqlx::query_as::<_, Transcript>("SELECT * FROM transcripts WHERE meeting_id = ?")
                    .bind(meeting_id)
                    .fetch_all(&mut *transaction)
                    .await?;

            transaction.commit().await?;

            // Convert Transcript to MeetingTranscript
            let meeting_transcripts = transcripts
                .into_iter()
                .map(|t| MeetingTranscript {
                    id: t.id,
                    text: t.transcript,
                    timestamp: t.timestamp,
                    audio_start_time: t.audio_start_time,
                    audio_end_time: t.audio_end_time,
                    duration: t.duration,
                })
                .collect::<Vec<_>>();

            Ok(Some(MeetingDetails {
                id: meeting.id,
                title: meeting.title,
                created_at: meeting.created_at.0.to_rfc3339(),
                updated_at: meeting.updated_at.0.to_rfc3339(),
                transcripts: meeting_transcripts,
            }))
        } else {
            transaction.rollback().await?;
            Ok(None)
        }
    }

    /// Get meeting metadata without transcripts (for pagination)
    pub async fn get_meeting_metadata(
        pool: &SqlitePool,
        meeting_id: &str,
    ) -> Result<Option<MeetingModel>, SqlxError> {
        if meeting_id.trim().is_empty() {
            return Err(SqlxError::Protocol(
                "meeting_id cannot be empty".to_string(),
            ));
        }

        let meeting: Option<MeetingModel> =
            sqlx::query_as("SELECT id, title, created_at, updated_at, folder_path FROM meetings WHERE id = ?")
                .bind(meeting_id)
                .fetch_optional(pool)
                .await?;

        Ok(meeting)
    }

    /// Get meeting transcripts with pagination support
    pub async fn get_meeting_transcripts_paginated(
        pool: &SqlitePool,
        meeting_id: &str,
        limit: i64,
        offset: i64,
    ) -> Result<(Vec<Transcript>, i64), SqlxError> {
        if meeting_id.trim().is_empty() {
            return Err(SqlxError::Protocol(
                "meeting_id cannot be empty".to_string(),
            ));
        }

        // Get total count of transcripts for this meeting
        let total: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM transcripts WHERE meeting_id = ?"
        )
        .bind(meeting_id)
        .fetch_one(pool)
        .await?;

        // Get paginated transcripts ordered by audio_start_time
        let transcripts = sqlx::query_as::<_, Transcript>(
            "SELECT * FROM transcripts
             WHERE meeting_id = ?
             ORDER BY audio_start_time ASC, rowid ASC
             LIMIT ? OFFSET ?"
        )
        .bind(meeting_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await?;

        Ok((transcripts, total.0))
    }

    /// One page of a meeting's transcripts in recording order, starting
    /// after `after` (the first page when `None`), and the cursor for the
    /// next page (`None` after the last page).
    pub async fn get_meeting_transcripts_page(
        pool: &SqlitePool,
        meeting_id: &str,
        after: Option<&TranscriptCursor>,
        limit: i64,
    ) -> Result<(Vec<Transcript>, Option<TranscriptCursor>), SqlxError> {
        if meeting_id.trim().is_empty() || limit <= 0 {
            return Err(SqlxError::Protocol(
                "meeting_id must not be empty and limit must be positive".to_string(),
            ));
        }

        let fetch_limit = limit.saturating_add(1);
        let mut rows = match after {
            None => {
                sqlx::query(TRANSCRIPT_FIRST_PAGE_SQL)
                    .bind(meeting_id)
                    .bind(fetch_limit)
                    .fetch_all(pool)
                    .await?
            }
            Some(TranscriptCursor {
                audio_start_time: None,
                row,
            }) => {
                sqlx::query(TRANSCRIPT_PAGE_AFTER_NULL_SQL)
                    .bind(meeting_id)
                    .bind(row)
                    .bind(fetch_limit)
                    .fetch_all(pool)
                    .await?
            }
            Some(TranscriptCursor {
                audio_start_time: Some(start),
                row,
            }) => {
                sqlx::query(TRANSCRIPT_PAGE_SQL)
                    .bind(meeting_id)
                    .bind(start)
                    .bind(row)
                    .bind(fetch_limit)
                    .fetch_all(pool)
                    .await?
            }
        };
        let has_more = rows.len() as i64 > limit;
        if has_more {
            rows.pop();
        }

        let mut transcripts = Vec::with_capacity(rows.len());
        let mut last_row = None;
        for row in &rows {
            transcripts.push(Transcript::from_row(row)?);
            last_row = Some(row.try_get::<i64, _>("cursor_row")?);
        }
        let next = match (last_row, transcripts.last()) {
            (Some(row), Some(last)) if has_more => Some(TranscriptCursor {
                audio_start_time: last.audio_start_time,
                row,
            }),
            _ => None,
        };
        Ok((transcripts, next))
    }

    pub async fn update_meeting_title(
        pool: &SqlitePool,
        meeting_id: &str,
        new_title: &str,
    ) -> Result<bool, SqlxError> {
        if meeting_id.trim().is_empty() {
            return Err(SqlxError::Protocol(
                "meeting_id cannot be empty".to_string(),
            ));
        }

        let mut conn = pool.acquire().await?;
        let mut transaction = conn.begin().await?;

        let now = Utc::now().naive_utc();

        // A title typed by the user is never replaced by a generated one.
        let rows_affected =
            sqlx::query("UPDATE meetings SET title = ?, title_source = 'user', updated_at = ? WHERE id = ?")
                .bind(new_title)
                .bind(now)
                .bind(meeting_id)
                .execute(&mut *transaction)
                .await?;
        if rows_affected.rows_affected() == 0 {
            transaction.rollback().await?;
            return Ok(false);
        }
        transaction.commit().await?;
        Ok(true)
    }

    /// SQL condition: the title may be replaced by a generated one. It is
    /// atomic with the update, so a title the user saves while the summary is
    /// being made is not overwritten.
    const TITLE_REPLACEABLE: &'static str = "(title_source = 'auto' OR (title_source IS NULL AND (trim(title) = '' OR title GLOB 'Meeting [0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]_[0-9][0-9]-[0-9][0-9]-[0-9][0-9]')))";

    /// True if the meeting exists and its title is still empty, the default
    /// name or an earlier generated title (not one the user chose).
    pub async fn title_is_replaceable(pool: &SqlitePool, meeting_id: &str) -> Result<bool, SqlxError> {
        let sql = format!("SELECT COUNT(*) FROM meetings WHERE id = ? AND {}", Self::TITLE_REPLACEABLE);
        let count: i64 = sqlx::query_scalar(&sql).bind(meeting_id).fetch_one(pool).await?;
        Ok(count > 0)
    }

    /// Stores a title made from the finished report, only if the title is
    /// still replaceable. Returns whether it was written.
    pub async fn set_generated_title(
        pool: &SqlitePool,
        meeting_id: &str,
        title: &str,
    ) -> Result<bool, SqlxError> {
        let title = title.trim();
        if title.is_empty() {
            return Ok(false);
        }
        let mut transaction = pool.begin().await?;
        let sql = format!(
            "UPDATE meetings SET title = ?, title_source = 'auto', updated_at = ? WHERE id = ? AND {}",
            Self::TITLE_REPLACEABLE
        );
        let updated = sqlx::query(&sql)
            .bind(title)
            .bind(Utc::now())
            .bind(meeting_id)
            .execute(&mut *transaction)
            .await?;
        if updated.rows_affected() == 0 {
            transaction.rollback().await?;
            return Ok(false);
        }
        sqlx::query("UPDATE transcript_chunks SET meeting_name = ? WHERE meeting_id = ?")
            .bind(title)
            .bind(meeting_id)
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        Ok(true)
    }

    pub async fn update_meeting_name(
        pool: &SqlitePool,
        meeting_id: &str,
        new_title: &str,
    ) -> Result<bool, SqlxError> {
        let mut transaction = pool.begin().await?;
        let now = Utc::now();

        // Update meetings table
        let meeting_update =
            sqlx::query("UPDATE meetings SET title = ?, updated_at = ? WHERE id = ?")
                .bind(new_title)
                .bind(now)
                .bind(meeting_id)
                .execute(&mut *transaction)
                .await?;

        if meeting_update.rows_affected() == 0 {
            transaction.rollback().await?;
            return Ok(false); // Meeting not found
        }

        // Update transcript_chunks table
        sqlx::query("UPDATE transcript_chunks SET meeting_name = ? WHERE meeting_id = ?")
            .bind(new_title)
            .bind(meeting_id)
            .execute(&mut *transaction)
            .await?;

        transaction.commit().await?;
        Ok(true)
    }
}

async fn delete_meeting_with_transaction(
    transaction: &mut SqliteConnection,
    meeting_id: &str,
) -> Result<bool, SqlxError> {
    // Check if meeting exists
    let meeting_exists: Option<(i64,)> = sqlx::query_as("SELECT 1 FROM meetings WHERE id = ?")
        .bind(meeting_id)
        .fetch_optional(&mut *transaction)
        .await?;

    if meeting_exists.is_none() {
        error!("Meeting {} not found for deletion", meeting_id);
        return Ok(false);
    }

    // Delete from related tables in proper order
    // 1. Delete from transcript_chunks
    sqlx::query("DELETE FROM transcript_chunks WHERE meeting_id = ?")
        .bind(meeting_id)
        .execute(&mut *transaction)
        .await?;

    // 2. Delete from summary_processes
    sqlx::query("DELETE FROM summary_processes WHERE meeting_id = ?")
        .bind(meeting_id)
        .execute(&mut *transaction)
        .await?;

    // 3. Delete from transcripts
    sqlx::query("DELETE FROM transcripts WHERE meeting_id = ?")
        .bind(meeting_id)
        .execute(&mut *transaction)
        .await?;

    // 4. Finally, delete the meeting
    let result = sqlx::query("DELETE FROM meetings WHERE id = ?")
        .bind(meeting_id)
        .execute(&mut *transaction)
        .await?;

    Ok(result.rows_affected() > 0)
}

#[cfg(test)]
mod title_tests {
    use super::*;

    async fn pool_with(title: &str, source: Option<&str>) -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::query(
            "CREATE TABLE meetings (id TEXT PRIMARY KEY, title TEXT NOT NULL, created_at TEXT, updated_at TEXT, folder_path TEXT, title_source TEXT)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("CREATE TABLE transcript_chunks (meeting_id TEXT, meeting_name TEXT)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO meetings (id, title, title_source) VALUES ('m', ?, ?)")
            .bind(title)
            .bind(source)
            .execute(&pool)
            .await
            .unwrap();
        pool
    }

    async fn title(pool: &SqlitePool) -> String {
        sqlx::query_scalar("SELECT title FROM meetings WHERE id = 'm'")
            .fetch_one(pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn default_and_generated_titles_are_replaced() {
        for (existing, source) in [
            ("Meeting 2026-11-05_08-25-23", None),
            ("  ", None),
            ("Tidligere titel", Some("auto")),
        ] {
            let pool = pool_with(existing, source).await;
            assert!(MeetingsRepository::title_is_replaceable(&pool, "m").await.unwrap());
            assert!(MeetingsRepository::set_generated_title(&pool, "m", "Budgetmøde").await.unwrap());
            assert_eq!(title(&pool).await, "Budgetmøde");
        }
    }

    #[tokio::test]
    async fn a_user_title_is_never_overwritten() {
        for (existing, source) in [
            ("Min egen titel", Some("user")),
            ("Meeting 2026-11-05_08-25-23", Some("user")),
            ("Gammel omdøbning", None),
        ] {
            let pool = pool_with(existing, source).await;
            assert!(!MeetingsRepository::title_is_replaceable(&pool, "m").await.unwrap());
            assert!(!MeetingsRepository::set_generated_title(&pool, "m", "Genereret").await.unwrap());
            assert_eq!(title(&pool).await, existing);
        }
    }

    #[tokio::test]
    async fn saving_a_title_by_hand_blocks_a_later_generated_title() {
        let pool = pool_with("Meeting 2026-11-05_08-25-23", None).await;
        assert!(MeetingsRepository::update_meeting_title(&pool, "m", "Mit navn").await.unwrap());
        assert!(!MeetingsRepository::set_generated_title(&pool, "m", "Genereret").await.unwrap());
        assert_eq!(title(&pool).await, "Mit navn");
    }

    #[tokio::test]
    async fn empty_generated_title_changes_nothing() {
        let pool = pool_with("Meeting 2026-11-05_08-25-23", None).await;
        assert!(!MeetingsRepository::set_generated_title(&pool, "m", "  ").await.unwrap());
        assert_eq!(title(&pool).await, "Meeting 2026-11-05_08-25-23");
    }
}
