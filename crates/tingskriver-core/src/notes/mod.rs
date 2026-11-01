//! UI-independent meeting notes and durable editing sessions.
pub mod autosave;
pub mod blocknote;
pub mod markdown;
pub mod model;
pub mod store;
pub use model::{Block, BlockKind, Document, Span};
pub use store::{migrate_legacy_notes, MigrationReport, NotesStore};
