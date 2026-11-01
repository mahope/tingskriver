use proptest::prelude::*;
use sqlx::SqlitePool;
use tingskriver_core::notes::{
    autosave::NotesSession, blocknote, markdown, migrate_legacy_notes, Block, BlockKind, Document,
    NotesStore, Span,
};

fn block(kind: BlockKind, text: &str, offset: Option<u64>) -> Block {
    Block {
        kind,
        spans: vec![Span {
            text: text.into(),
            bold: true,
            italic: true,
        }],
        created_offset_ms: offset,
    }
}
fn document(text: &str) -> Document {
    Document {
        blocks: vec![block(BlockKind::Paragraph, text, Some(123))],
    }
}
#[test]
fn round_trip_all_kinds_and_escaping() {
    for kind in [
        BlockKind::Paragraph,
        BlockKind::Heading(1),
        BlockKind::Heading(2),
        BlockKind::Heading(3),
        BlockKind::Bullet,
        BlockKind::Numbered,
        BlockKind::Checkbox(false),
        BlockKind::Checkbox(true),
    ] {
        for text in [
            "",
            "æøå 😀",
            "# titel",
            "- liste",
            "1. liste",
            "**x** *y* \\ \n\r",
            "<!--s--><!--empty--><!--offset:2--> ",
        ] {
            let mut doc = Document {
                blocks: vec![block(kind.clone(), text, Some(u64::MAX))],
            };
            doc.blocks[0].spans.extend([
                Span {
                    text: "next".into(),
                    ..Span::default()
                },
                Span {
                    text: "more".into(),
                    ..Span::default()
                },
                Span::default(),
            ]);
            assert_eq!(
                markdown::parse(&markdown::serialize(&doc)).unwrap(),
                doc.canonical()
            );
        }
    }
    for doc in [
        Document::default(),
        Document {
            blocks: vec![block(BlockKind::Paragraph, "", None)],
        },
    ] {
        assert_eq!(
            markdown::parse(&markdown::serialize(&doc)).unwrap(),
            doc.canonical()
        );
    }
}
fn arb_document() -> impl Strategy<Value = Document> {
    prop::collection::vec(
        (
            0u8..8,
            prop::option::of(any::<u64>()),
            prop::collection::vec(
                (
                    prop::collection::vec(any::<char>(), 0..40),
                    any::<bool>(),
                    any::<bool>(),
                ),
                0..8,
            ),
        ),
        0..12,
    )
    .prop_map(|blocks| Document {
        blocks: blocks
            .into_iter()
            .map(|(kind, created_offset_ms, spans)| Block {
                kind: match kind {
                    0 => BlockKind::Paragraph,
                    1..=3 => BlockKind::Heading(kind),
                    4 => BlockKind::Bullet,
                    5 => BlockKind::Numbered,
                    6 => BlockKind::Checkbox(false),
                    _ => BlockKind::Checkbox(true),
                },
                created_offset_ms,
                spans: spans
                    .into_iter()
                    .map(|(text, bold, italic)| Span {
                        text: text.into_iter().collect(),
                        bold,
                        italic,
                    })
                    .collect(),
            })
            .collect(),
    })
}
proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]
    #[test]
    fn arbitrary_unicode_round_trip(doc in arb_document()) {
        prop_assert_eq!(markdown::parse(&markdown::serialize(&doc)).unwrap(), doc.canonical());
    }
}
async fn database() -> SqlitePool {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    pool
}
async fn meeting(pool: &SqlitePool, id: &str) {
    sqlx::query("INSERT INTO meetings(id,title,created_at,updated_at) VALUES (?,'synthetic',datetime('now'),datetime('now'))").bind(id).execute(pool).await.unwrap();
}
#[tokio::test]
async fn store_offsets_lookup_projection_rollback_and_cascade() {
    let pool = database().await;
    meeting(&pool, "a").await;
    let store = NotesStore::new(pool.clone());
    let doc = Document {
        blocks: vec![
            block(BlockKind::Paragraph, "none", None),
            block(BlockKind::Bullet, "first", Some(100)),
            block(BlockKind::Checkbox(true), "last", Some(200)),
        ],
    };
    store.save("a", &doc).await.unwrap();
    assert_eq!(store.load("a").await.unwrap(), doc);
    assert!(store.notes_around("a", 0, 99).await.unwrap().is_empty());
    assert_eq!(
        store.notes_around("a", 0, 100).await.unwrap(),
        vec![doc.blocks[1].clone()]
    );
    assert_eq!(
        store.notes_around("a", 150, 50).await.unwrap(),
        doc.blocks[1..]
    );
    assert_eq!(
        store.notes_around("a", u64::MAX, u64::MAX).await.unwrap(),
        doc.blocks[1..]
    );
    let projection: String =
        sqlx::query_scalar("SELECT notes_markdown FROM meeting_notes WHERE meeting_id='a'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(projection, markdown::serialize(&doc));
    let invalid = Document {
        blocks: vec![block(BlockKind::Paragraph, "bad", Some(u64::MAX))],
    };
    assert!(store.save("a", &invalid).await.is_err());
    assert_eq!(store.load("a").await.unwrap(), doc);
    sqlx::query("DELETE FROM meetings WHERE id='a'")
        .execute(&pool)
        .await
        .unwrap();
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM meeting_note_blocks")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
}
#[test]
fn untimed_lookup_is_empty() {
    assert!(Document {
        blocks: vec![block(BlockKind::Paragraph, "x", None)]
    }
    .around(0, u64::MAX)
    .is_empty());
}
#[tokio::test]
async fn legacy_fixtures_preserve_original_and_are_idempotent() {
    let pool = database().await;
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/notes");
    let mut originals = Vec::new();
    for entry in std::fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if path.extension() != Some(std::ffi::OsStr::new("json")) {
            continue;
        }
        let id = path.file_stem().unwrap().to_str().unwrap().to_string();
        let json = std::fs::read_to_string(&path).unwrap();
        meeting(&pool, &id).await;
        sqlx::query("INSERT INTO meeting_notes VALUES (?, 'old', ?, 'old', 'old')")
            .bind(&id)
            .bind(&json)
            .execute(&pool)
            .await
            .unwrap();
        originals.push((id, json));
    }
    assert!(originals.len() >= 10);
    meeting(&pool, "broken").await;
    sqlx::query("INSERT INTO meeting_notes VALUES ('broken','preserve','{','old','old')")
        .execute(&pool)
        .await
        .unwrap();
    let report = migrate_legacy_notes(&pool).await.unwrap();
    assert_eq!(report.migrated, originals.len());
    assert_eq!(report.failed, 1);
    assert_eq!(report.unsupported_blocks, 4);
    let store = NotesStore::new(pool.clone());
    for (id, json) in originals {
        let original: String =
            sqlx::query_scalar("SELECT notes_json FROM meeting_notes WHERE meeting_id=?")
                .bind(&id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(original, json);
        let doc = store.load(&id).await.unwrap();
        assert_eq!(doc, blocknote::convert(&json).unwrap().0);
        let expected = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join(format!("tests/fixtures/notes/{id}.md")),
        )
        .unwrap();
        assert_eq!(
            markdown::serialize(&doc),
            expected.trim_end_matches('\n'),
            "{id}"
        );
    }
    let again = migrate_legacy_notes(&pool).await.unwrap();
    assert_eq!(again.migrated, 0);
    assert_eq!(again.skipped, report.migrated);
    assert_eq!(again.failed, 1);
}
#[tokio::test]
async fn lazy_migration_and_save_preserve_json_even_after_empty_edit() {
    let pool = database().await;
    meeting(&pool, "a").await;
    let json = r#"[{"type":"paragraph","content":[{"text":"original","styles":{}}]}]"#;
    sqlx::query("INSERT INTO meeting_notes VALUES ('a',NULL,?,'old','old')")
        .bind(json)
        .execute(&pool)
        .await
        .unwrap();
    let store = NotesStore::new(pool.clone());
    assert_eq!(
        store.load("a").await.unwrap().blocks[0].spans[0].text,
        "original"
    );
    store.save("a", &Document::default()).await.unwrap();
    assert_eq!(store.load("a").await.unwrap(), Document::default());
    let original: String =
        sqlx::query_scalar("SELECT notes_json FROM meeting_notes WHERE meeting_id='a'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(original, json);
}
#[tokio::test]
async fn autosave_deadline_and_crash_loss_are_bounded() {
    let pool = database().await;
    meeting(&pool, "a").await;
    let store = NotesStore::new(pool.clone());
    let session = NotesSession::load(store.clone(), "a".into()).await.unwrap();
    tokio::time::pause();
    let (sender, receiver) = tokio::sync::mpsc::channel(16);
    let task = tokio::spawn(session.run(receiver));
    for n in 0..20 {
        sender.send(document(&format!("edit {n}"))).await.unwrap();
        tokio::task::yield_now().await;
        tokio::time::advance(std::time::Duration::from_millis(100)).await;
    }
    // SQLite uses a real worker thread; resume time while waiting for the transaction.
    tokio::time::resume();
    for _ in 0..100 {
        if store.load("a").await.unwrap() == document("edit 19") {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert_eq!(store.load("a").await.unwrap(), document("edit 19"));
    tokio::time::pause();
    sender.send(document("lost on crash")).await.unwrap();
    tokio::task::yield_now().await;
    tokio::time::advance(std::time::Duration::from_millis(499)).await;
    task.abort();
    let _ = task.await;
    tokio::time::resume();
    assert_eq!(store.load("a").await.unwrap(), document("edit 19"));
    let mut session = NotesSession::load(store.clone(), "a".into()).await.unwrap();
    session.edit(|d| *d = document("flush"));
    assert!(session.is_dirty());
    session.flush().await.unwrap();
    assert!(!session.is_dirty());
    assert_eq!(store.load("a").await.unwrap(), document("flush"));
}

type SavedSnapshots = Vec<(tokio::time::Instant, Document)>;
#[derive(Clone, Default)]
struct MemoryStore(std::sync::Arc<std::sync::Mutex<SavedSnapshots>>);
impl tingskriver_core::notes::autosave::NotesPersistence for MemoryStore {
    async fn load(&self, _: &str) -> anyhow::Result<Document> {
        Ok(Document::default())
    }
    async fn save(&self, _: &str, doc: &Document) -> anyhow::Result<()> {
        self.0
            .lock()
            .unwrap()
            .push((tokio::time::Instant::now(), doc.clone()));
        Ok(())
    }
}
#[tokio::test(start_paused = true)]
async fn continuous_edits_cannot_postpone_two_second_deadline() {
    let store = MemoryStore::default();
    let session = NotesSession::load(store.clone(), "synthetic".into())
        .await
        .unwrap();
    let (sender, receiver) = tokio::sync::mpsc::channel(16);
    let task = tokio::spawn(session.run(receiver));
    let start = tokio::time::Instant::now();
    for n in 0..20 {
        sender.send(document(&format!("edit {n}"))).await.unwrap();
        tokio::task::yield_now().await;
        tokio::time::advance(std::time::Duration::from_millis(100)).await;
    }
    tokio::task::yield_now().await;
    let saved = store.0.lock().unwrap().clone();
    assert_eq!(saved.len(), 1);
    assert_eq!(
        saved[0].0.duration_since(start),
        std::time::Duration::from_secs(2)
    );
    assert_eq!(saved[0].1, document("edit 19"));
    for n in 0..20 {
        sender
            .send(document(&format!("unsaved {n}")))
            .await
            .unwrap();
        tokio::task::yield_now().await;
        tokio::time::advance(std::time::Duration::from_millis(if n == 19 {
            99
        } else {
            100
        }))
        .await;
    }
    task.abort();
    let _ = task.await;
    assert_eq!(store.0.lock().unwrap().len(), 1);
    assert!(
        tokio::time::Instant::now().duration_since(saved[0].0) < std::time::Duration::from_secs(2)
    );
}
#[tokio::test(start_paused = true)]
async fn closing_edit_channel_flushes_immediately() {
    let store = MemoryStore::default();
    let session = NotesSession::load(store.clone(), "synthetic".into())
        .await
        .unwrap();
    let (sender, receiver) = tokio::sync::mpsc::channel(1);
    sender.send(document("last")).await.unwrap();
    drop(sender);
    let start = tokio::time::Instant::now();
    session.run(receiver).await.unwrap();
    assert_eq!(
        store.0.lock().unwrap().as_slice(),
        &[(start, document("last"))]
    );
}

#[tokio::test(start_paused = true)]
async fn quiet_edit_is_debounced_for_half_a_second() {
    let store = MemoryStore::default();
    let session = NotesSession::load(store.clone(), "synthetic".into())
        .await
        .unwrap();
    let (sender, receiver) = tokio::sync::mpsc::channel(1);
    let task = tokio::spawn(session.run(receiver));
    sender.send(document("quiet")).await.unwrap();
    tokio::task::yield_now().await;
    tokio::time::advance(std::time::Duration::from_millis(499)).await;
    tokio::task::yield_now().await;
    assert!(store.0.lock().unwrap().is_empty());
    tokio::time::advance(std::time::Duration::from_millis(1)).await;
    tokio::task::yield_now().await;
    assert_eq!(store.0.lock().unwrap()[0].1, document("quiet"));
    drop(sender);
    task.await.unwrap().unwrap();
}
