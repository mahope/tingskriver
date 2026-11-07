use super::{
    carry_offsets, diff, visible_lines, NewProposal, Proposal, ProposalError, ProposalField,
    ProposalSource, ProposalStatus, SourceKind, UserConfirmed,
};
use crate::notes::{markdown, store as notes_store, NotesStore};
use crate::strings;
use chrono::Utc;
use sqlx::{Row, SqliteConnection, SqlitePool};

type Result<T> = std::result::Result<T, ProposalError>;

const COLUMNS: &str = "id, meeting_id, field, base_updated_at, base_content, proposed_content, diff, source_kind, source_label, session_id, status, status_reason, applied_content, created_at, resolved_at";

#[derive(Clone)]
pub struct ProposalStore {
    pool: SqlitePool,
}

impl ProposalStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Stores a proposal. Never touches the notes or the summary.
    pub async fn create(&self, new: NewProposal) -> Result<Proposal> {
        let exists = sqlx::query("SELECT 1 FROM meetings WHERE id=?")
            .bind(&new.meeting_id)
            .fetch_optional(&self.pool)
            .await?
            .is_some();
        if !exists {
            return Err(ProposalError::NotFound);
        }
        if new.field == ProposalField::Notes {
            // Migrates legacy notes first, so the base is what the user sees.
            NotesStore::new(self.pool.clone())
                .load(&new.meeting_id)
                .await?;
        }
        let mut tx = self.pool.begin().await?;
        let (base, base_updated_at) = read_current(&mut tx, new.field, &new.meeting_id).await?;
        let proposed = match new.field {
            ProposalField::Notes => {
                let parsed = markdown::parse(&new.proposed)
                    .map_err(|e| ProposalError::Invalid(e.to_string()))?;
                carry_offsets(&base, &markdown::serialize(&parsed))
            }
            ProposalField::Summary => {
                if new.proposed.trim().is_empty() {
                    return Err(ProposalError::EmptySummary);
                }
                if summary_row(&mut tx, &new.meeting_id).await?.is_none() {
                    return Err(ProposalError::NoSummary);
                }
                new.proposed.clone()
            }
        };
        if proposed == base {
            return Err(ProposalError::NoChange);
        }
        let unified = diff::unified(
            &visible_lines(new.field, &base),
            &visible_lines(new.field, &proposed),
            3,
        );
        let id = uuid::Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO meeting_proposals(id, meeting_id, field, base_updated_at, base_content, proposed_content, diff, source_kind, source_label, session_id, status, created_at) VALUES (?,?,?,?,?,?,?,?,?,?,'pending',?)")
            .bind(&id)
            .bind(&new.meeting_id)
            .bind(new.field.as_str())
            .bind(&base_updated_at)
            .bind(&base)
            .bind(&proposed)
            .bind(&unified)
            .bind(new.source.kind.as_str())
            .bind(&new.source.label)
            .bind(&new.source.session_id)
            .bind(now())
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        self.get(&id).await
    }

    pub async fn get(&self, id: &str) -> Result<Proposal> {
        let mut conn = self.pool.acquire().await?;
        fetch(&mut conn, id).await
    }

    /// All proposals of a meeting, newest first (the history).
    pub async fn list(&self, meeting_id: &str) -> Result<Vec<Proposal>> {
        self.refresh_stale(meeting_id).await?;
        let rows = sqlx::query(&format!(
            "SELECT {COLUMNS} FROM meeting_proposals WHERE meeting_id=? ORDER BY created_at DESC, rowid DESC"
        ))
        .bind(meeting_id)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(from_row).collect()
    }

    /// Pending proposals, oldest first. Proposals whose base has changed are
    /// rejected automatically first and are not returned.
    pub async fn pending(&self, meeting_id: &str) -> Result<Vec<Proposal>> {
        self.refresh_stale(meeting_id).await?;
        let rows = sqlx::query(&format!(
            "SELECT {COLUMNS} FROM meeting_proposals WHERE meeting_id=? AND status='pending' ORDER BY created_at, rowid"
        ))
        .bind(meeting_id)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(from_row).collect()
    }

    /// The number for the "N ventende ændringer" band.
    pub async fn pending_count(&self, meeting_id: &str) -> Result<usize> {
        self.refresh_stale(meeting_id).await?;
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM meeting_proposals WHERE meeting_id=? AND status='pending'",
        )
        .bind(meeting_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(count as usize)
    }

    /// Applies a pending proposal on the user's click. A proposal whose base
    /// has changed is marked stale and not applied.
    pub async fn apply(&self, id: &str, _confirmed: UserConfirmed) -> Result<Proposal> {
        let mut tx = self.pool.begin().await?;
        // Claiming first takes the write lock, so the check and the write
        // below cannot interleave with another writer.
        let claimed = sqlx::query(
            "UPDATE meeting_proposals SET status='applied', resolved_at=? WHERE id=? AND status='pending'",
        )
        .bind(now())
        .bind(id)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        let proposal = fetch(&mut tx, id).await?;
        if claimed == 0 {
            return Err(ProposalError::NotPending(proposal.status));
        }
        let (current, _) = read_current(&mut tx, proposal.field, &proposal.meeting_id).await?;
        if current != proposal.base_content {
            let reason = stale_reason(proposal.field);
            mark(&mut tx, id, ProposalStatus::Stale, Some(&reason)).await?;
            tx.commit().await?;
            return Err(ProposalError::Stale { reason });
        }
        write_current(
            &mut tx,
            proposal.field,
            &proposal.meeting_id,
            &proposal.proposed_content,
            false,
        )
        .await?;
        let (written, _) = read_current(&mut tx, proposal.field, &proposal.meeting_id).await?;
        sqlx::query("UPDATE meeting_proposals SET applied_content=? WHERE id=?")
            .bind(&written)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        self.get(id).await
    }

    pub async fn reject(&self, id: &str) -> Result<Proposal> {
        let changed = sqlx::query(
            "UPDATE meeting_proposals SET status='rejected', resolved_at=? WHERE id=? AND status='pending'",
        )
        .bind(now())
        .bind(id)
        .execute(&self.pool)
        .await?
        .rows_affected();
        let proposal = self.get(id).await?;
        if changed == 0 {
            return Err(ProposalError::NotPending(proposal.status));
        }
        Ok(proposal)
    }

    /// Undoes an applied proposal. Edits the user made after applying it are
    /// kept: the change is reversed by a three-way merge, and if the user
    /// changed the same lines, nothing is changed and `UndoConflict` returned.
    pub async fn undo(&self, id: &str, _confirmed: UserConfirmed) -> Result<Proposal> {
        let mut tx = self.pool.begin().await?;
        let claimed = sqlx::query(
            "UPDATE meeting_proposals SET status='undone', resolved_at=? WHERE id=? AND status='applied'",
        )
        .bind(now())
        .bind(id)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        let proposal = fetch(&mut tx, id).await?;
        if claimed == 0 {
            return Err(ProposalError::NotApplied(proposal.status));
        }
        let applied = proposal.applied_content.clone().unwrap_or_default();
        let (current, _) = read_current(&mut tx, proposal.field, &proposal.meeting_id).await?;
        let restored = if current == applied {
            proposal.base_content.clone()
        } else {
            let merged = diff::merge3(
                &diff::lines(&applied),
                &diff::lines(&current),
                &diff::lines(&proposal.base_content),
            )
            .ok_or(ProposalError::UndoConflict)?;
            diff::join(&merged)
        };
        write_current(
            &mut tx,
            proposal.field,
            &proposal.meeting_id,
            &restored,
            true,
        )
        .await?;
        tx.commit().await?;
        self.get(id).await
    }

    /// Expires the pending proposals of a closed chat or command session.
    pub async fn close_session(&self, session_id: &str) -> Result<u64> {
        let reason = strings::PROPOSAL_EXPIRED_REASON;
        Ok(sqlx::query("UPDATE meeting_proposals SET status='expired', status_reason=?, resolved_at=? WHERE session_id=? AND status='pending' AND source_kind IN ('chat','command_mode')")
            .bind(reason)
            .bind(now())
            .bind(session_id)
            .execute(&self.pool)
            .await?
            .rows_affected())
    }

    /// Call at startup: no chat or command session survives a restart, so
    /// their proposals expire. Proposals from other sources are kept.
    pub async fn expire_orphaned_sessions(&self) -> Result<u64> {
        let reason = strings::PROPOSAL_EXPIRED_REASON;
        Ok(sqlx::query("UPDATE meeting_proposals SET status='expired', status_reason=?, resolved_at=? WHERE status='pending' AND source_kind IN ('chat','command_mode')")
            .bind(reason)
            .bind(now())
            .execute(&self.pool)
            .await?
            .rows_affected())
    }

    /// Rejects pending proposals whose base text is no longer the current text.
    async fn refresh_stale(&self, meeting_id: &str) -> Result<()> {
        let rows = sqlx::query(
            "SELECT id, field, base_content FROM meeting_proposals WHERE meeting_id=? AND status='pending'",
        )
        .bind(meeting_id)
        .fetch_all(&self.pool)
        .await?;
        if rows.is_empty() {
            return Ok(());
        }
        let mut tx = self.pool.begin().await?;
        for row in rows {
            let field = parse_field(row.try_get("field")?)?;
            let (current, _) = read_current(&mut tx, field, meeting_id).await?;
            if current != row.try_get::<String, _>("base_content")? {
                let id: String = row.try_get("id")?;
                mark(
                    &mut tx,
                    &id,
                    ProposalStatus::Stale,
                    Some(&stale_reason(field)),
                )
                .await?;
            }
        }
        tx.commit().await?;
        Ok(())
    }
}

fn now() -> String {
    Utc::now().to_rfc3339()
}

fn stale_reason(field: ProposalField) -> String {
    strings::proposal_stale_reason(match field {
        ProposalField::Notes => strings::PROPOSAL_STALE_NOTES,
        ProposalField::Summary => strings::PROPOSAL_STALE_SUMMARY,
    })
}

async fn mark(
    conn: &mut SqliteConnection,
    id: &str,
    status: ProposalStatus,
    reason: Option<&str>,
) -> Result<()> {
    // Only a pending or just-claimed proposal can become stale.
    sqlx::query("UPDATE meeting_proposals SET status=?, status_reason=?, resolved_at=? WHERE id=? AND status IN ('pending','applied')")
        .bind(status.as_str())
        .bind(reason)
        .bind(now())
        .bind(id)
        .execute(conn)
        .await?;
    Ok(())
}

fn parse_field(value: String) -> Result<ProposalField> {
    ProposalField::parse(&value).ok_or_else(|| ProposalError::Invalid(format!("field {value}")))
}

async fn fetch(conn: &mut SqliteConnection, id: &str) -> Result<Proposal> {
    let row = sqlx::query(&format!(
        "SELECT {COLUMNS} FROM meeting_proposals WHERE id=?"
    ))
    .bind(id)
    .fetch_optional(conn)
    .await?
    .ok_or(ProposalError::NotFound)?;
    from_row(&row)
}

fn from_row(row: &sqlx::sqlite::SqliteRow) -> Result<Proposal> {
    let kind: String = row.try_get("source_kind")?;
    let status: String = row.try_get("status")?;
    Ok(Proposal {
        id: row.try_get("id")?,
        meeting_id: row.try_get("meeting_id")?,
        field: parse_field(row.try_get("field")?)?,
        base_updated_at: row.try_get("base_updated_at")?,
        base_content: row.try_get("base_content")?,
        proposed_content: row.try_get("proposed_content")?,
        diff: row.try_get("diff")?,
        source: ProposalSource {
            kind: SourceKind::parse(&kind)
                .ok_or_else(|| ProposalError::Invalid(format!("source {kind}")))?,
            label: row.try_get("source_label")?,
            session_id: row.try_get("session_id")?,
        },
        status: ProposalStatus::parse(&status)
            .ok_or_else(|| ProposalError::Invalid(format!("status {status}")))?,
        status_reason: row.try_get("status_reason")?,
        applied_content: row.try_get("applied_content")?,
        created_at: row.try_get("created_at")?,
        resolved_at: row.try_get("resolved_at")?,
    })
}

/// The current text of a field and its row's `updated_at`.
async fn read_current(
    conn: &mut SqliteConnection,
    field: ProposalField,
    meeting_id: &str,
) -> Result<(String, Option<String>)> {
    match field {
        ProposalField::Notes => {
            let doc = notes_store::read_blocks(conn, meeting_id).await?;
            let updated: Option<String> =
                sqlx::query_scalar("SELECT updated_at FROM meeting_notes WHERE meeting_id=?")
                    .bind(meeting_id)
                    .fetch_optional(&mut *conn)
                    .await?;
            Ok((markdown::serialize(&doc), updated))
        }
        ProposalField::Summary => Ok(match summary_row(conn, meeting_id).await? {
            Some((result, updated)) => (summary_markdown(result.as_deref()), Some(updated)),
            None => (String::new(), None),
        }),
    }
}

async fn summary_row(
    conn: &mut SqliteConnection,
    meeting_id: &str,
) -> Result<Option<(Option<String>, String)>> {
    let row = sqlx::query("SELECT result, updated_at FROM summary_processes WHERE meeting_id=?")
        .bind(meeting_id)
        .fetch_optional(conn)
        .await?;
    row.map(|r| Ok((r.try_get("result")?, r.try_get("updated_at")?)))
        .transpose()
}

fn summary_markdown(result: Option<&str>) -> String {
    result
        .and_then(|r| serde_json::from_str::<serde_json::Value>(r).ok())
        .and_then(|v| v.get("markdown")?.as_str().map(str::to_string))
        .unwrap_or_default()
}

/// Writes the text. The only code in this module that changes notes or
/// summaries; reached only from `apply` and `undo`.
async fn write_current(
    conn: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    field: ProposalField,
    meeting_id: &str,
    content: &str,
    allow_empty: bool,
) -> Result<()> {
    match field {
        ProposalField::Notes => {
            let doc =
                markdown::parse(content).map_err(|e| ProposalError::Invalid(e.to_string()))?;
            notes_store::write(conn, meeting_id, &doc).await?;
        }
        ProposalField::Summary => {
            if !allow_empty && content.trim().is_empty() {
                return Err(ProposalError::EmptySummary);
            }
            let (result, _) = summary_row(conn, meeting_id)
                .await?
                .ok_or(ProposalError::NoSummary)?;
            let mut value = result
                .as_deref()
                .and_then(|r| serde_json::from_str::<serde_json::Value>(r).ok())
                .filter(serde_json::Value::is_object)
                .unwrap_or_else(|| serde_json::json!({}));
            value["markdown"] = serde_json::Value::String(content.to_string());
            let stamp = Utc::now();
            sqlx::query("UPDATE summary_processes SET result=?, updated_at=? WHERE meeting_id=?")
                .bind(value.to_string())
                .bind(stamp)
                .bind(meeting_id)
                .execute(&mut **conn)
                .await?;
            sqlx::query("UPDATE meetings SET updated_at=? WHERE id=?")
                .bind(stamp)
                .bind(meeting_id)
                .execute(&mut **conn)
                .await?;
        }
    }
    Ok(())
}
