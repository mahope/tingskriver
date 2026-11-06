//! A bounded local call journal, separate from the read-only meeting database.
use sqlx::SqlitePool;
use std::path::Path;

pub const TOOLS: &[&str] = &[
    "list_meetings",
    "get_meeting",
    "search",
    "list_tasks",
    "recent_dictations",
    "start_recording",
    "stop_recording",
    "regenerate_summary",
    "handoff",
    "add_note",
];
const ERROR: &str = "Kaldsloggen kunne ikke åbnes. Prøv igen.";

#[derive(Clone, Debug, sqlx::FromRow)]
pub struct Entry {
    pub time: String,
    pub tool: String,
    pub meeting_id: Option<String>,
    pub success: bool,
}

async fn open(dir: &Path) -> Result<SqlitePool, String> {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            sqlx::sqlite::SqliteConnectOptions::new()
                .filename(dir.join("mcp-audit.sqlite"))
                .create_if_missing(true),
        )
        .await
        .map_err(|_| ERROR)?;
    sqlx::query("CREATE TABLE IF NOT EXISTS calls (id INTEGER PRIMARY KEY, time TEXT NOT NULL, tool TEXT NOT NULL, meeting_id TEXT, success INTEGER NOT NULL)")
        .execute(&pool).await.map_err(|_| ERROR)?;
    Ok(pool)
}

/// Callers must pass an existing meeting ID, never a raw client argument.
pub async fn record(
    dir: &Path,
    tool: &str,
    meeting_id: Option<&str>,
    success: bool,
) -> Result<(), String> {
    if !TOOLS.contains(&tool) {
        return Ok(());
    }
    let pool = open(dir).await?;
    let mut tx = pool.begin().await.map_err(|_| ERROR)?;
    sqlx::query("INSERT INTO calls(time,tool,meeting_id,success) VALUES (?,?,?,?)")
        .bind(
            chrono::Utc::now()
                .format("%d.%m.%Y %H:%M:%S UTC")
                .to_string(),
        )
        .bind(tool)
        .bind(meeting_id)
        .bind(success)
        .execute(&mut *tx)
        .await
        .map_err(|_| ERROR)?;
    sqlx::query(
        "DELETE FROM calls WHERE id NOT IN (SELECT id FROM calls ORDER BY id DESC LIMIT 100)",
    )
    .execute(&mut *tx)
    .await
    .map_err(|_| ERROR)?;
    tx.commit().await.map_err(|_| ERROR)?;
    pool.close().await;
    Ok(())
}

pub async fn recent(dir: &Path) -> Result<Vec<Entry>, String> {
    let pool = open(dir).await?;
    let entries =
        sqlx::query_as("SELECT time,tool,meeting_id,success FROM calls ORDER BY id DESC LIMIT 100")
            .fetch_all(&pool)
            .await
            .map_err(|_| ERROR.to_string());
    pool.close().await;
    entries
}

pub async fn clear(dir: &Path) -> Result<(), String> {
    let pool = open(dir).await?;
    let result = sqlx::query("DELETE FROM calls")
        .execute(&pool)
        .await
        .map_err(|_| ERROR.to_string());
    pool.close().await;
    result.map(|_| ())
}
