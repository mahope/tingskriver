use super::{Document, NotesStore};
use std::time::Duration;
use tokio::time::Instant;

/// Persistence boundary for deterministic clock tests and host integrations.
#[async_trait::async_trait]
pub trait NotesPersistence: Send + Sync {
    async fn load(&self, meeting_id: &str) -> anyhow::Result<Document>;
    async fn save(&self, meeting_id: &str, document: &Document) -> anyhow::Result<()>;
}
#[async_trait::async_trait]
impl NotesPersistence for NotesStore {
    async fn load(&self, meeting_id: &str) -> anyhow::Result<Document> {
        NotesStore::load(self, meeting_id).await
    }
    async fn save(&self, meeting_id: &str, document: &Document) -> anyhow::Result<()> {
        NotesStore::save(self, meeting_id, document).await
    }
}

/// The caller owns and drives the session; no detached task survives its owner.
pub struct NotesSession<S = NotesStore> {
    store: S,
    meeting_id: String,
    document: Document,
    dirty_since: Option<Instant>,
}
impl<S: NotesPersistence> NotesSession<S> {
    pub async fn load(store: S, meeting_id: String) -> anyhow::Result<Self> {
        let document = store.load(&meeting_id).await?;
        Ok(Self {
            store,
            meeting_id,
            document,
            dirty_since: None,
        })
    }
    pub fn document(&self) -> &Document {
        &self.document
    }
    pub fn is_dirty(&self) -> bool {
        self.dirty_since.is_some()
    }
    pub fn edit(&mut self, edit: impl FnOnce(&mut Document)) {
        edit(&mut self.document);
        self.dirty_since.get_or_insert_with(Instant::now);
    }
    pub async fn flush(&mut self) -> anyhow::Result<()> {
        if self.is_dirty() {
            self.store.save(&self.meeting_id, &self.document).await?;
            self.dirty_since = None;
        }
        Ok(())
    }
    /// Drive on the host runtime. Channel closure flushes; dropping the future simulates a crash.
    /// The deadline is anchored to the FIRST unsaved edit, never postponed by later edits.
    pub async fn run(
        mut self,
        mut edits: tokio::sync::mpsc::Receiver<Document>,
    ) -> anyhow::Result<()> {
        loop {
            let deadline = self.dirty_since.map(|t| t + Duration::from_secs(2));
            tokio::select! {
                biased;
                _ = async { match deadline { Some(t) => tokio::time::sleep_until(t).await, None => std::future::pending().await } } => self.flush().await?,
                doc = edits.recv() => match doc { Some(doc) => self.edit(|current| *current = doc), None => { self.flush().await?; return Ok(()); } }
            }
        }
    }
}
