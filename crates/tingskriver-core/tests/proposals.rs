//! #205: AI proposals never change notes or summaries until the user applies
//! them, go stale when the base changes, and can be undone without losing
//! later edits. Runs the real migrations on SQLite.

use sqlx::{migrate::Migrator, sqlite::SqlitePoolOptions, SqlitePool};
use tingskriver_core::notes::{markdown, NotesStore};
use tingskriver_core::proposals::{
    NewProposal, ProposalError, ProposalField, ProposalSource, ProposalStatus, ProposalStore,
    SourceKind, UserConfirmed,
};

const M: &str = "meeting-1";

async fn pool() -> SqlitePool {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    Migrator::new(std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/migrations"
    )))
    .await
    .unwrap()
    .run(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO meetings (id,title,created_at,updated_at) VALUES (?, 'Syntetisk møde', datetime('now'), datetime('now'))")
        .bind(M)
        .execute(&pool)
        .await
        .unwrap();
    pool
}

async fn set_notes(pool: &SqlitePool, text: &str) {
    NotesStore::new(pool.clone())
        .save(M, &markdown::parse(text).unwrap())
        .await
        .unwrap();
}

async fn notes_text(pool: &SqlitePool) -> String {
    markdown::serialize(&NotesStore::new(pool.clone()).load(M).await.unwrap())
}

async fn set_summary(pool: &SqlitePool, json: &str) {
    sqlx::query("INSERT OR REPLACE INTO summary_processes (meeting_id,status,created_at,updated_at,result) VALUES (?, 'completed', datetime('now'), datetime('now'), ?)")
        .bind(M)
        .bind(json)
        .execute(pool)
        .await
        .unwrap();
}

async fn summary_value(pool: &SqlitePool) -> serde_json::Value {
    let r: String = sqlx::query_scalar("SELECT result FROM summary_processes WHERE meeting_id=?")
        .bind(M)
        .fetch_one(pool)
        .await
        .unwrap();
    serde_json::from_str(&r).unwrap()
}

fn new(field: ProposalField, proposed: &str, kind: SourceKind) -> NewProposal {
    NewProposal {
        meeting_id: M.into(),
        field,
        proposed: proposed.into(),
        source: ProposalSource::new(kind),
    }
}

fn click() -> UserConfirmed {
    UserConfirmed::from_ui_click()
}

const NOTES: &str = "<!--offset:1000--> Første punkt\n<!--offset:2000--> Andet punkt\n<!--offset:3000--> Tredje punkt";

#[tokio::test]
async fn a_proposal_changes_nothing_until_it_is_applied() {
    let pool = pool().await;
    set_notes(&pool, NOTES).await;
    set_summary(
        &pool,
        r#"{"markdown":"Gammelt referat","summary_json":[1]}"#,
    )
    .await;
    let before = (notes_text(&pool).await, summary_value(&pool).await);
    let store = ProposalStore::new(pool.clone());

    let n = store
        .create(new(
            ProposalField::Notes,
            "Første punkt\nRettet punkt\nTredje punkt",
            SourceKind::Mcp,
        ))
        .await
        .unwrap();
    let s = store
        .create(new(
            ProposalField::Summary,
            "Nyt referat",
            SourceKind::Regenerate,
        ))
        .await
        .unwrap();

    assert_eq!(
        (notes_text(&pool).await, summary_value(&pool).await),
        before
    );
    assert_eq!(n.status, ProposalStatus::Pending);
    assert!(n.diff.contains("-Andet punkt") && n.diff.contains("+Rettet punkt"));
    assert!(!n.diff.contains("offset"), "diff shows only visible text");
    assert_eq!(store.pending_count(M).await.unwrap(), 2);

    store.apply(&n.id, click()).await.unwrap();
    store.apply(&s.id, click()).await.unwrap();
    assert_ne!(notes_text(&pool).await, before.0);
    assert_eq!(summary_value(&pool).await["markdown"], "Nyt referat");
    assert_eq!(
        summary_value(&pool).await["summary_json"],
        serde_json::json!([1])
    );
}

#[tokio::test]
async fn applying_keeps_timestamps_of_untouched_notes() {
    let pool = pool().await;
    set_notes(&pool, NOTES).await;
    let store = ProposalStore::new(pool.clone());
    let p = store
        .create(new(
            ProposalField::Notes,
            "Første punkt\nRettet punkt\nTredje punkt",
            SourceKind::Assistant,
        ))
        .await
        .unwrap();
    store.apply(&p.id, click()).await.unwrap();
    let doc = NotesStore::new(pool.clone()).load(M).await.unwrap();
    let offsets: Vec<_> = doc.blocks.iter().map(|b| b.created_offset_ms).collect();
    assert_eq!(offsets, [Some(1000), None, Some(3000)]);
}

#[tokio::test]
async fn a_stale_proposal_is_rejected_with_an_explanation() {
    let pool = pool().await;
    set_notes(&pool, NOTES).await;
    let store = ProposalStore::new(pool.clone());
    let p = store
        .create(new(ProposalField::Notes, "Helt nye noter", SourceKind::Mcp))
        .await
        .unwrap();

    // The user keeps typing after the proposal was made.
    let user_text = format!("{NOTES}\n<!--offset:4000--> Fjerde punkt");
    set_notes(&pool, &user_text).await;

    let err = store.apply(&p.id, click()).await.unwrap_err();
    let ProposalError::Stale { reason } = &err else {
        panic!("expected Stale, got {err:?}");
    };
    assert!(reason.contains("noterne er ændret"), "{reason}");
    assert_eq!(
        notes_text(&pool).await,
        markdown::serialize(&markdown::parse(&user_text).unwrap())
    );
    let after = store.get(&p.id).await.unwrap();
    assert_eq!(after.status, ProposalStatus::Stale);
    assert_eq!(after.status_reason.as_deref(), Some(reason.as_str()));
}

#[tokio::test]
async fn listing_rejects_stale_proposals_automatically() {
    let pool = pool().await;
    set_summary(&pool, r#"{"markdown":"A"}"#).await;
    let store = ProposalStore::new(pool.clone());
    let p = store
        .create(new(ProposalField::Summary, "B", SourceKind::Mcp))
        .await
        .unwrap();
    set_summary(&pool, r#"{"markdown":"C"}"#).await;
    assert!(store.pending(M).await.unwrap().is_empty());
    let after = store.get(&p.id).await.unwrap();
    assert_eq!(after.status, ProposalStatus::Stale);
    assert!(after.status_reason.unwrap().contains("referatet er ændret"));
    assert_eq!(summary_value(&pool).await["markdown"], "C");
}

#[tokio::test]
async fn undo_restores_the_previous_text() {
    let pool = pool().await;
    set_notes(&pool, NOTES).await;
    set_summary(&pool, r#"{"markdown":"Gammelt"}"#).await;
    let store = ProposalStore::new(pool.clone());
    let before = notes_text(&pool).await;
    let n = store
        .create(new(
            ProposalField::Notes,
            "Ingenting tilbage",
            SourceKind::Assistant,
        ))
        .await
        .unwrap();
    let s = store
        .create(new(ProposalField::Summary, "Nyt", SourceKind::Regenerate))
        .await
        .unwrap();
    store.apply(&n.id, click()).await.unwrap();
    store.apply(&s.id, click()).await.unwrap();
    store.undo(&n.id, click()).await.unwrap();
    store.undo(&s.id, click()).await.unwrap();
    assert_eq!(notes_text(&pool).await, before);
    assert_eq!(summary_value(&pool).await["markdown"], "Gammelt");
    assert_eq!(
        store.get(&n.id).await.unwrap().status,
        ProposalStatus::Undone
    );
}

#[tokio::test]
async fn undo_keeps_what_the_user_wrote_after_applying() {
    let pool = pool().await;
    set_notes(&pool, NOTES).await;
    let store = ProposalStore::new(pool.clone());
    let p = store
        .create(new(
            ProposalField::Notes,
            "Første punkt\nRettet punkt\nTredje punkt",
            SourceKind::Assistant,
        ))
        .await
        .unwrap();
    store.apply(&p.id, click()).await.unwrap();

    let applied = notes_text(&pool).await;
    set_notes(
        &pool,
        &format!("{applied}\n<!--offset:9000--> Min egen senere note"),
    )
    .await;

    store.undo(&p.id, click()).await.unwrap();
    let text = notes_text(&pool).await;
    assert!(text.contains("Andet punkt"), "the old line is back: {text}");
    assert!(!text.contains("Rettet punkt"));
    assert!(
        text.contains("Min egen senere note"),
        "later writing is kept: {text}"
    );
    assert!(text.contains("<!--offset:9000-->"));
}

#[tokio::test]
async fn undo_refuses_when_the_user_changed_the_same_lines() {
    let pool = pool().await;
    set_notes(&pool, NOTES).await;
    let store = ProposalStore::new(pool.clone());
    let p = store
        .create(new(
            ProposalField::Notes,
            "Første punkt\nRettet punkt\nTredje punkt",
            SourceKind::Assistant,
        ))
        .await
        .unwrap();
    store.apply(&p.id, click()).await.unwrap();
    set_notes(&pool, "Første punkt\nMin egen rettelse\nTredje punkt").await;
    let before = notes_text(&pool).await;

    let err = store.undo(&p.id, click()).await.unwrap_err();
    assert!(matches!(err, ProposalError::UndoConflict));
    assert_eq!(notes_text(&pool).await, before, "nothing was changed");
    assert_eq!(
        store.get(&p.id).await.unwrap().status,
        ProposalStatus::Applied
    );
}

#[tokio::test]
async fn reject_and_double_apply_are_handled() {
    let pool = pool().await;
    set_notes(&pool, NOTES).await;
    let store = ProposalStore::new(pool.clone());
    let a = store
        .create(new(ProposalField::Notes, "A", SourceKind::Mcp))
        .await
        .unwrap();
    store.reject(&a.id).await.unwrap();
    assert!(matches!(
        store.apply(&a.id, click()).await,
        Err(ProposalError::NotPending(ProposalStatus::Rejected))
    ));
    assert_eq!(
        notes_text(&pool).await,
        markdown::serialize(&markdown::parse(NOTES).unwrap())
    );
    assert!(matches!(
        store.undo(&a.id, click()).await,
        Err(ProposalError::NotApplied(_))
    ));
    assert!(matches!(
        store
            .create(new(ProposalField::Notes, NOTES, SourceKind::Mcp))
            .await,
        Err(ProposalError::NoChange)
    ));
    assert!(matches!(
        store.get("nope").await,
        Err(ProposalError::NotFound)
    ));
}

#[tokio::test]
async fn summary_proposals_need_a_summary_with_text() {
    let pool = pool().await;
    let store = ProposalStore::new(pool.clone());
    assert!(matches!(
        store
            .create(new(ProposalField::Summary, "X", SourceKind::Mcp))
            .await,
        Err(ProposalError::NoSummary)
    ));
    set_summary(&pool, r#"{"markdown":"A"}"#).await;
    assert!(matches!(
        store
            .create(new(ProposalField::Summary, "  ", SourceKind::Mcp))
            .await,
        Err(ProposalError::EmptySummary)
    ));
}

#[tokio::test]
async fn chat_proposals_expire_on_close_others_are_kept() {
    let pool = pool().await;
    set_notes(&pool, NOTES).await;
    let store = ProposalStore::new(pool.clone());
    let mk = |text: &str, source: ProposalSource| NewProposal {
        meeting_id: M.into(),
        field: ProposalField::Notes,
        proposed: text.into(),
        source,
    };
    let chat = store
        .create(mk(
            "C",
            ProposalSource::new(SourceKind::Chat).in_session("s1"),
        ))
        .await
        .unwrap();
    let cmd = store
        .create(mk(
            "D",
            ProposalSource::new(SourceKind::CommandMode).in_session("s2"),
        ))
        .await
        .unwrap();
    let mcp = store
        .create(mk(
            "E",
            ProposalSource::new(SourceKind::Mcp).labelled("add_note"),
        ))
        .await
        .unwrap();

    assert_eq!(store.close_session("s1").await.unwrap(), 1);
    assert_eq!(
        store.get(&chat.id).await.unwrap().status,
        ProposalStatus::Expired
    );
    assert_eq!(
        store.get(&cmd.id).await.unwrap().status,
        ProposalStatus::Pending
    );

    // A restart ends every session; MCP proposals survive it.
    let restarted = ProposalStore::new(pool.clone());
    assert_eq!(restarted.expire_orphaned_sessions().await.unwrap(), 1);
    let pending = restarted.pending(M).await.unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].id, mcp.id);
    assert_eq!(pending[0].source.label, "add_note");
}

#[tokio::test]
async fn proposals_survive_reopening_the_database_file() {
    let dir = std::env::temp_dir().join(format!("tingskriver-proposals-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("test.db");
    let _ = std::fs::remove_file(&path);
    let options = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(&path)
        .create_if_missing(true)
        .foreign_keys(true);
    let open = || async {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options.clone())
            .await
            .unwrap();
        Migrator::new(std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/migrations"
        )))
        .await
        .unwrap()
        .run(&pool)
        .await
        .unwrap();
        pool
    };
    let first = open().await;
    sqlx::query("INSERT INTO meetings (id,title,created_at,updated_at) VALUES (?, 'x', datetime('now'), datetime('now'))")
        .bind(M)
        .execute(&first)
        .await
        .unwrap();
    set_notes(&first, NOTES).await;
    let id = ProposalStore::new(first.clone())
        .create(new(ProposalField::Notes, "Ny", SourceKind::Mcp))
        .await
        .unwrap()
        .id;
    first.close().await;

    let second = open().await;
    let store = ProposalStore::new(second.clone());
    let again = store.get(&id).await.unwrap();
    assert_eq!(again.status, ProposalStatus::Pending);
    assert_eq!(store.pending_count(M).await.unwrap(), 1);
    second.close().await;
    let _ = std::fs::remove_dir_all(&dir);
}

/// `UserConfirmed::from_ui_click` is `pub`, so convention is the only guard.
/// This test makes the convention checkable: only the proposal module itself
/// and the GPUI review view may create the token.
#[test]
fn only_ui_code_creates_user_confirmed() {
    fn walk(dir: &std::path::Path, hits: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, hits);
            } else if path.extension().is_some_and(|e| e == "rs")
                && std::fs::read_to_string(&path)
                    .unwrap_or_default()
                    .contains("from_ui_click")
            {
                hits.push(path.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    let crates = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut hits = Vec::new();
    for entry in std::fs::read_dir(&crates).unwrap().flatten() {
        let src = entry.path().join("src");
        if src.is_dir() {
            walk(&src, &mut hits);
        }
    }
    let unexpected: Vec<_> = hits
        .iter()
        .filter(|p| {
            !p.ends_with("tingskriver-core/src/proposals/mod.rs")
                && !p.ends_with("tingskriver-app/src/proposals.rs")
        })
        .collect();
    assert!(
        unexpected.is_empty(),
        "AI paths must not confirm: {unexpected:?}"
    );
}
