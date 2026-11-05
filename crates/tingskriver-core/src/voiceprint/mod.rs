//! Voiceprints across meetings (issue #198): remember who is who from one
//! meeting to the next. Biometric data under GDPR article 9, so:
//!
//! - **off by default**, and nothing is stored or matched while off,
//! - **local only**: no network code, and nothing here is reachable from
//!   export, delivery, MCP, logs or diagnostics (guarded by
//!   `tests/voiceprint_guards.rs`),
//! - **consent per person**, stored and withdrawable,
//! - the voiceprints (embeddings) live only in Windows Credential Manager,
//!   behind [`SecretBackend`],
//! - **deletion**: per person, all, on withdrawal, and automatically after
//!   30/90/180 days without a match,
//! - a match is a **suggestion** the user confirms.
//!
//! The model that turns audio into an embedding sits behind
//! [`SpeakerEmbedder`]. The repository has none yet; see `docs/stemmeaftryk.md`.

pub mod embedder;
pub mod matching;
pub mod service;
pub mod store;
pub mod texts;

pub use embedder::{Embedding, SpeakerEmbedder};
pub use matching::{Suggestion, Thresholds, THRESHOLDS_CALIBRATED_ON_DANISH};
pub use service::{
    Clock, ConsentInput, ConsentRecord, NamedSuggestion, Settings, SystemClock, VoiceprintError,
    VoiceprintService,
};
pub use store::{CredentialManagerBackend, MemoryBackend, SecretBackend};

#[cfg(test)]
mod tests;
