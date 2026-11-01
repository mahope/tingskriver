use super::{blocknote, markdown, Block, Document};
use anyhow::Context;
use sqlx::{Row, SqlitePool};

#[derive(Clone)]
pub struct NotesStore {
    pool: SqlitePool,
}
#[derive(Debug, Default, PartialEq, Eq)]
pub struct MigrationReport {
    pub migrated: usize,
    pub skipped: usize,
    pub failed: usize,
    pub unsupported_blocks: usize,
}
impl NotesStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
    pub async fn save(&self, meeting_id: &str, doc: &Document) -> anyhow::Result<()> {
        let mut tx = self.pool.begin().await?;
        write(&mut tx, meeting_id, doc).await?;
        tx.commit().await.context("Commit meeting notes")
    }
    pub async fn load(&self, meeting_id: &str) -> anyhow::Result<Document> {
        self.migrate_one(meeting_id).await?;
        let rows = sqlx::query(
            "SELECT block_type,level,checked,text_markdown,created_offset_ms FROM meeting_note_blocks WHERE meeting_id=? ORDER BY position",
        )
        .bind(meeting_id)
        .fetch_all(&self.pool)
        .await?;
        let mut doc = Document::default();
        for row in rows {
            let kind = match row.try_get::<&str, _>("block_type")? {
                "paragraph" => super::BlockKind::Paragraph,
                "heading" => super::BlockKind::Heading(row.try_get("level")?),
                "bullet" => super::BlockKind::Bullet,
                "numbered" => super::BlockKind::Numbered,
                "checkbox" => super::BlockKind::Checkbox(row.try_get("checked")?),
                _ => anyhow::bail!("Invalid stored note block type"),
            };
            let inline = markdown::parse(row.try_get("text_markdown")?)?;
            let spans = inline
                .blocks
                .into_iter()
                .next()
                .map(|b| b.spans)
                .unwrap_or_default();
            let offset: Option<i64> = row.try_get("created_offset_ms")?;
            doc.blocks.push(Block {
                kind,
                spans,
                created_offset_ms: offset
                    .map(u64::try_from)
                    .transpose()
                    .context("Invalid stored note offset")?,
            });
        }
        Ok(doc)
    }
    pub async fn notes_around(
        &self,
        meeting_id: &str,
        t_ms: u64,
        window_ms: u64,
    ) -> anyhow::Result<Vec<Block>> {
        Ok(self.load(meeting_id).await?.around(t_ms, window_ms))
    }
    async fn migrate_one(&self, id: &str) -> anyhow::Result<Option<usize>> {
        let mut tx = self.pool.begin().await?;
        // Claim the document before reading legacy content, serializing concurrent writers.
        let claimed = sqlx::query("INSERT OR IGNORE INTO meeting_note_documents(meeting_id) SELECT id FROM meetings WHERE id=?").bind(id).execute(&mut *tx).await?.rows_affected();
        if claimed == 0 {
            return Ok(None);
        }
        let row =
            sqlx::query("SELECT notes_json, notes_markdown FROM meeting_notes WHERE meeting_id=?")
                .bind(id)
                .fetch_optional(&mut *tx)
                .await?;
        let (doc, unsupported) = if let Some(row) = row {
            if let Some(json) = row.try_get::<Option<String>, _>("notes_json")? {
                blocknote::convert(&json)?
            } else {
                (
                    markdown::parse(
                        &row.try_get::<Option<String>, _>("notes_markdown")?
                            .unwrap_or_default(),
                    )?,
                    0,
                )
            }
        } else {
            (Document::default(), 0)
        };
        write(&mut tx, id, &doc).await?;
        tx.commit().await?;
        Ok(Some(unsupported))
    }
}
async fn write(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    id: &str,
    doc: &Document,
) -> anyhow::Result<()> {
    let doc = doc.canonical();
    sqlx::query("INSERT OR IGNORE INTO meeting_note_documents(meeting_id) VALUES (?)")
        .bind(id)
        .execute(&mut **tx)
        .await?;
    sqlx::query("DELETE FROM meeting_note_blocks WHERE meeting_id=?")
        .bind(id)
        .execute(&mut **tx)
        .await?;
    for (position, block) in doc.blocks.iter().enumerate() {
        let offset = block
            .created_offset_ms
            .map(i64::try_from)
            .transpose()
            .context("Note offset exceeds SQLite integer range")?;
        let (kind, level, checked) = match block.kind {
            super::BlockKind::Paragraph => ("paragraph", None, None),
            super::BlockKind::Heading(n) => ("heading", Some(n), None),
            super::BlockKind::Bullet => ("bullet", None, None),
            super::BlockKind::Numbered => ("numbered", None, None),
            super::BlockKind::Checkbox(v) => ("checkbox", None, Some(v)),
        };
        sqlx::query("INSERT INTO meeting_note_blocks(meeting_id,position,block_type,level,checked,text_markdown,created_offset_ms) VALUES (?,?,?,?,?,?,?)").bind(id).bind(position as i64).bind(kind).bind(level).bind(checked).bind(markdown::serialize(&Document { blocks: vec![Block { kind: super::BlockKind::Paragraph, spans: block.spans.clone(), created_offset_ms: None }] })).bind(offset).execute(&mut **tx).await?;
    }
    sqlx::query("INSERT INTO meeting_notes(meeting_id,notes_markdown,created_at,updated_at) VALUES (?,?,datetime('now'),datetime('now')) ON CONFLICT(meeting_id) DO UPDATE SET notes_markdown=excluded.notes_markdown,updated_at=excluded.updated_at").bind(id).bind(markdown::serialize(&doc)).execute(&mut **tx).await?;
    Ok(())
}
pub async fn migrate_legacy_notes(pool: &SqlitePool) -> anyhow::Result<MigrationReport> {
    let ids: Vec<String> =
        sqlx::query_scalar("SELECT meeting_id FROM meeting_notes ORDER BY meeting_id")
            .fetch_all(pool)
            .await?;
    let store = NotesStore::new(pool.clone());
    let mut report = MigrationReport::default();
    for id in ids {
        match store.migrate_one(&id).await {
            Ok(Some(n)) => {
                report.migrated += 1;
                report.unsupported_blocks += n;
            }
            Ok(None) => report.skipped += 1,
            Err(_) => report.failed += 1,
        }
    }
    Ok(report)
}
