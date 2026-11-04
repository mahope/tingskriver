//! MCP's read surface. All reads use a snapshot containing both policy and data.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{Row, Sqlite, SqlitePool, Transaction};
use std::path::Path;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Access {
    All,
    #[default]
    Marked,
    Metadata,
    Off,
}

const STORAGE: &str = "MCP-data kunne ikke læses. Åbn Tingskriver og prøv igen.";
const DENIED: &str = "Adgang er slået fra, eller mødet er ikke delt med AI-værktøjer.";

pub async fn set_access(pool: &SqlitePool, access: Access) -> Result<(), String> {
    let value = serde_json::to_value(access).map_err(|_| STORAGE)?;
    sqlx::query("UPDATE mcp_settings SET access=? WHERE id=1")
        .bind(value.as_str())
        .execute(pool)
        .await
        .map_err(|_| STORAGE)?;
    Ok(())
}

pub async fn set_shared(pool: &SqlitePool, id: &str, shared: bool) -> Result<(), String> {
    let result = sqlx::query("UPDATE meetings SET ai_shared=? WHERE id=?")
        .bind(shared)
        .bind(id)
        .execute(pool)
        .await
        .map_err(|_| STORAGE)?;
    if result.rows_affected() != 1 {
        return Err(DENIED.into());
    }
    Ok(())
}

#[derive(Clone)]
pub struct Reader {
    pool: SqlitePool,
}

impl Reader {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn open(path: &Path) -> Result<Self, String> {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(
                sqlx::sqlite::SqliteConnectOptions::new()
                    .filename(path)
                    .read_only(true),
            )
            .await
            .map_err(|_| STORAGE)?;
        Ok(Self::new(pool))
    }

    pub async fn access(&self) -> Result<Access, String> {
        let mut tx = self.pool.begin().await.map_err(|_| STORAGE)?;
        policy(&mut tx).await
    }

    async fn snapshot(&self) -> Result<(Transaction<'_, Sqlite>, Access), String> {
        let mut tx = self.pool.begin().await.map_err(|_| STORAGE)?;
        let access = policy(&mut tx).await?;
        if access == Access::Off {
            return Err(DENIED.into());
        }
        Ok((tx, access))
    }

    pub async fn authorize(&self, id: &str) -> Result<(), String> {
        let (mut tx, access) = self.snapshot().await?;
        visible(&mut tx, access, id).await
    }

    pub async fn list_meetings(
        &self,
        from: Option<&str>,
        to: Option<&str>,
        query: Option<&str>,
    ) -> Result<Vec<Value>, String> {
        let (mut tx, access) = self.snapshot().await?;
        let rows = sqlx::query(
            "SELECT m.id,m.title,m.created_at,m.ai_shared,
            (SELECT MAX(audio_end_time) FROM transcripts WHERE meeting_id=m.id) AS duration
            FROM meetings m WHERE (? != 'marked' OR m.ai_shared=1)
            AND (? IS NULL OR julianday(m.created_at)>=julianday(?))
            AND (? IS NULL OR julianday(m.created_at)<julianday(?))
            AND (? IS NULL OR instr(lower(m.title),lower(?))>0)
            ORDER BY m.created_at DESC,m.id LIMIT 500",
        )
        .bind(access_name(access))
        .bind(from)
        .bind(from)
        .bind(to)
        .bind(to)
        .bind(query)
        .bind(query)
        .fetch_all(&mut *tx)
        .await
        .map_err(|_| STORAGE)?;
        rows.into_iter()
            .map(|r| {
                Ok(
                    json!({"id":r.try_get::<String,_>("id").map_err(|_| STORAGE)?,
            "title":r.try_get::<String,_>("title").map_err(|_| STORAGE)?,
            "time":r.try_get::<String,_>("created_at").map_err(|_| STORAGE)?,
            "duration":r.try_get::<Option<f64>,_>("duration").map_err(|_| STORAGE)?,
            "shared":r.try_get::<bool,_>("ai_shared").map_err(|_| STORAGE)?}),
                )
            })
            .collect()
    }

    pub async fn get_meeting(&self, id: &str, parts: &[String]) -> Result<Value, String> {
        if parts
            .iter()
            .any(|p| !["referat", "noter", "transskript"].contains(&p.as_str()))
        {
            return Err("Ukendt mødedel".into());
        }
        let (mut tx, access) = self.snapshot().await?;
        visible(&mut tx, access, id).await?;
        let row = sqlx::query("SELECT title,created_at,ai_shared FROM meetings WHERE id=?")
            .bind(id)
            .fetch_one(&mut *tx)
            .await
            .map_err(|_| STORAGE)?;
        let mut result = json!({"id":id,"title":row.try_get::<String,_>("title").map_err(|_| STORAGE)?,
            "time":row.try_get::<String,_>("created_at").map_err(|_| STORAGE)?,
            "shared":row.try_get::<bool,_>("ai_shared").map_err(|_| STORAGE)?});
        if access == Access::Metadata {
            return Ok(result);
        }
        let wants = |p: &str| parts.is_empty() || parts.iter().any(|s| s == p);
        if wants("referat") {
            result["referat"] = sqlx::query_scalar::<_, Option<String>>(
                "SELECT result FROM summary_processes WHERE meeting_id=?",
            )
            .bind(id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|_| STORAGE)?
            .flatten()
            .map(Value::String)
            .unwrap_or(Value::Null);
        }
        if wants("noter") {
            // NotesStore::load migrates legacy documents, so it cannot be used on a read-only connection.
            let blocks: Vec<String> = sqlx::query_scalar("SELECT text_markdown FROM meeting_note_blocks WHERE meeting_id=? ORDER BY position")
                .bind(id).fetch_all(&mut *tx).await.map_err(|_| STORAGE)?;
            let notes = if blocks.is_empty() {
                sqlx::query_scalar::<_,Option<String>>("SELECT COALESCE(notes_markdown,notes_json) FROM meeting_notes WHERE meeting_id=?")
                    .bind(id).fetch_optional(&mut *tx).await.map_err(|_| STORAGE)?.flatten().unwrap_or_default()
            } else {
                blocks.join("\n\n")
            };
            result["noter"] = json!(notes);
        }
        if wants("transskript") {
            let rows = sqlx::query("SELECT transcript,timestamp,speaker,audio_start_time,audio_end_time FROM transcripts WHERE meeting_id=? ORDER BY audio_start_time,timestamp,id")
                .bind(id).fetch_all(&mut *tx).await.map_err(|_| STORAGE)?;
            let mut items = vec![];
            for r in rows {
                items.push(
                    json!({"text":r.try_get::<String,_>("transcript").map_err(|_| STORAGE)?,
                    "timestamp":r.try_get::<String,_>("timestamp").map_err(|_| STORAGE)?,
                    "speaker":r.try_get::<Option<String>,_>("speaker").map_err(|_| STORAGE)?,
                    "start":r.try_get::<Option<f64>,_>("audio_start_time").map_err(|_| STORAGE)?,
                    "end":r.try_get::<Option<f64>,_>("audio_end_time").map_err(|_| STORAGE)?}),
                );
            }
            result["transskript"] = json!(items);
        }
        Ok(result)
    }

    pub async fn search(&self, text: &str) -> Result<Vec<Value>, String> {
        let (mut tx, access) = self.snapshot().await?;
        let query = text
            .split_whitespace()
            .filter(|s| s.chars().any(char::is_alphanumeric))
            .map(|s| format!("\"{}\"", s.replace('"', "\"\"")))
            .collect::<Vec<_>>()
            .join(" AND ");
        if query.is_empty() {
            return Ok(vec![]);
        }
        let rows = sqlx::query("SELECT d.meeting_id,d.source,d.time_seconds,snippet(search_fts,0,'','','…',32) AS excerpt
            FROM search_fts JOIN search_documents d ON d.id=search_fts.rowid JOIN meetings m ON m.id=d.meeting_id
            WHERE search_fts MATCH ? AND (? != 'marked' OR m.ai_shared=1)
            AND (? != 'metadata' OR d.source='title') ORDER BY bm25(search_fts),d.id LIMIT 100")
            .bind(query).bind(access_name(access)).bind(access_name(access)).fetch_all(&mut *tx).await.map_err(|_| STORAGE)?;
        rows.into_iter()
            .map(|r| {
                Ok(
                    json!({"id":r.try_get::<String,_>("meeting_id").map_err(|_| STORAGE)?,
            "source":r.try_get::<String,_>("source").map_err(|_| STORAGE)?,
            "excerpt":r.try_get::<String,_>("excerpt").map_err(|_| STORAGE)?,
            "time":r.try_get::<Option<f64>,_>("time_seconds").map_err(|_| STORAGE)?}),
                )
            })
            .collect()
    }
}

fn access_name(access: Access) -> &'static str {
    match access {
        Access::All => "all",
        Access::Marked => "marked",
        Access::Metadata => "metadata",
        Access::Off => "off",
    }
}

async fn policy(tx: &mut Transaction<'_, Sqlite>) -> Result<Access, String> {
    let value: String = sqlx::query_scalar("SELECT access FROM mcp_settings WHERE id=1")
        .fetch_one(&mut **tx)
        .await
        .map_err(|_| STORAGE)?;
    serde_json::from_value(json!(value)).map_err(|_| STORAGE.into())
}

async fn visible(tx: &mut Transaction<'_, Sqlite>, access: Access, id: &str) -> Result<(), String> {
    let shared: Option<bool> = sqlx::query_scalar("SELECT ai_shared FROM meetings WHERE id=?")
        .bind(id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|_| STORAGE)?;
    match shared {
        Some(shared) if access != Access::Off && (access != Access::Marked || shared) => Ok(()),
        _ => Err(DENIED.into()),
    }
}
