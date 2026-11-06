//! Synthetic data only, for the native MCP screenshot regression test.
use std::{path::PathBuf, time::Duration};
use tingskriver_core::{database::manager::DatabaseManager, mcp, mcp_audit, paths::DATABASE_FILE};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let dir = PathBuf::from(args.next().ok_or("fixture directory required")?);
    let mode = args.next().ok_or("fixture mode required")?;
    std::fs::create_dir(&dir)?; // Never touch an existing data directory.
    let db = DatabaseManager::open_at(
        &dir.join(DATABASE_FILE),
        &dir.join("absent.db"),
        Duration::from_secs(5),
    )
    .await?;
    if mode != "empty" {
        sqlx::query("INSERT INTO meetings(id,title,created_at,updated_at,ai_shared) VALUES ('mock-001','Prøvemøde','2026-11-01','2026-11-01',1)").execute(db.pool()).await?;
    }
    if mode == "enabled" {
        mcp::save_settings(
            db.pool(),
            mcp::Settings {
                enabled: true,
                notes: true,
                ..Default::default()
            },
        )
        .await?;
        mcp_audit::record(&dir, "list_meetings", None, true).await?;
        mcp_audit::record(&dir, "get_meeting", Some("mock-001"), false).await?;
        let pool = sqlx::SqlitePool::connect_with(
            sqlx::sqlite::SqliteConnectOptions::new().filename(dir.join("mcp-audit.sqlite")),
        )
        .await?;
        sqlx::query("UPDATE calls SET time='05.11.2026 10:00:00 UTC'")
            .execute(&pool)
            .await?;
        pool.close().await;
    }
    if mode == "error" {
        sqlx::query("DROP TABLE mcp_settings")
            .execute(db.pool())
            .await?;
    }
    Ok(())
}
