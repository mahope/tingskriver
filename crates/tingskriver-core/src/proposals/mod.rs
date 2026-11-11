//! AI proposals for meeting notes and summaries (#205).
//!
//! AI-writing paths (summary regeneration, assistant, chat, command mode,
//! MCP) can create a [`Proposal`] through [`ProposalStore::create`]. A proposal
//! changes nothing: the notes and the summary are only written by
//! [`ProposalStore::apply`] and [`ProposalStore::undo`], which both need a
//! [`UserConfirmed`] token that the UI creates from an explicit click.
//!
//! Versioning: a proposal remembers the exact text it was made against
//! (`base_content`) and the row's `updated_at` (`base_updated_at`, shown to
//! the user). If the current text differs, the proposal is rejected
//! automatically as stale. Content, not the timestamp, decides, because the
//! timestamp has one-second resolution.

pub mod diff;
mod store;

pub use diff::{DiffKind, DiffLine};
pub use store::ProposalStore;

use crate::strings;

/// Which part of the meeting a proposal changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProposalField {
    /// The user's notes, as canonical note markdown (one block per line).
    Notes,
    /// The `markdown` of the AI summary.
    Summary,
}

impl ProposalField {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Notes => "notes",
            Self::Summary => "summary",
        }
    }
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "notes" => Some(Self::Notes),
            "summary" => Some(Self::Summary),
            _ => None,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Notes => strings::PROPOSAL_FIELD_NOTES,
            Self::Summary => strings::PROPOSAL_FIELD_SUMMARY,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceKind {
    Regenerate,
    Assistant,
    Chat,
    CommandMode,
    Mcp,
}

impl SourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Regenerate => "regenerate",
            Self::Assistant => "assistant",
            Self::Chat => "chat",
            Self::CommandMode => "command_mode",
            Self::Mcp => "mcp",
        }
    }
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "regenerate" => Self::Regenerate,
            "assistant" => Self::Assistant,
            "chat" => Self::Chat,
            "command_mode" => Self::CommandMode,
            "mcp" => Self::Mcp,
            _ => return None,
        })
    }
    /// Proposals from a chat or command session expire when it closes;
    /// the others (regeneration, assistant, MCP) are kept.
    pub fn expires_on_close(self) -> bool {
        matches!(self, Self::Chat | Self::CommandMode)
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Regenerate => strings::PROPOSAL_SOURCE_REGENERATE,
            Self::Assistant => strings::PROPOSAL_SOURCE_ASSISTANT,
            Self::Chat => strings::PROPOSAL_SOURCE_CHAT,
            Self::CommandMode => strings::PROPOSAL_SOURCE_COMMAND,
            Self::Mcp => strings::PROPOSAL_SOURCE_MCP,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProposalSource {
    pub kind: SourceKind,
    /// A short technical label, such as the MCP tool name. Not shown as text.
    pub label: String,
    /// The chat or command session. Needed to expire the proposal on close.
    pub session_id: Option<String>,
}

impl ProposalSource {
    pub fn new(kind: SourceKind) -> Self {
        Self {
            kind,
            label: String::new(),
            session_id: None,
        }
    }
    pub fn labelled(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }
    pub fn in_session(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = Some(session_id.into());
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProposalStatus {
    Pending,
    Applied,
    Rejected,
    /// Rejected automatically: the base text changed after the proposal.
    Stale,
    /// A chat or command proposal whose session closed.
    Expired,
    /// Applied and then undone.
    Undone,
}

impl ProposalStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Applied => "applied",
            Self::Rejected => "rejected",
            Self::Stale => "stale",
            Self::Expired => "expired",
            Self::Undone => "undone",
        }
    }
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "pending" => Self::Pending,
            "applied" => Self::Applied,
            "rejected" => Self::Rejected,
            "stale" => Self::Stale,
            "expired" => Self::Expired,
            "undone" => Self::Undone,
            _ => return None,
        })
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Pending => strings::PROPOSAL_STATUS_PENDING,
            Self::Applied => strings::PROPOSAL_STATUS_APPLIED,
            Self::Rejected => strings::PROPOSAL_STATUS_REJECTED,
            Self::Stale => strings::PROPOSAL_STATUS_STALE,
            Self::Expired => strings::PROPOSAL_STATUS_EXPIRED,
            Self::Undone => strings::PROPOSAL_STATUS_UNDONE,
        }
    }
}

/// What a caller hands to [`ProposalStore::create`].
#[derive(Clone, Debug)]
pub struct NewProposal {
    pub meeting_id: String,
    pub field: ProposalField,
    /// The complete new text. For notes: note markdown, one block per line.
    pub proposed: String,
    pub source: ProposalSource,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Proposal {
    pub id: String,
    pub meeting_id: String,
    pub field: ProposalField,
    /// The `updated_at` of the notes/summary row when the proposal was made.
    pub base_updated_at: Option<String>,
    pub base_content: String,
    pub proposed_content: String,
    /// Unified diff of the visible text, base to proposed.
    pub diff: String,
    pub source: ProposalSource,
    pub status: ProposalStatus,
    /// Why the proposal is stale or expired, in Danish, ready to show.
    pub status_reason: Option<String>,
    /// The text that was written when the proposal was applied.
    pub applied_content: Option<String>,
    pub created_at: String,
    pub resolved_at: Option<String>,
}

impl Proposal {
    /// The diff as structured lines, for rendering with colour and labels.
    pub fn diff_lines(&self) -> Vec<DiffLine> {
        diff::diff_lines(
            &visible_lines(self.field, &self.base_content),
            &visible_lines(self.field, &self.proposed_content),
        )
    }
}

/// Proof that the user asked for this change. Create it only in a UI event
/// handler for the apply or undo button; never in an AI-driven path.
#[derive(Clone, Copy, Debug)]
pub struct UserConfirmed(());

impl UserConfirmed {
    pub fn from_ui_click() -> Self {
        Self(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ProposalError {
    #[error("meeting or proposal not found")]
    NotFound,
    #[error("proposal is {0:?}, not pending")]
    NotPending(ProposalStatus),
    #[error("proposal is {0:?}, not applied")]
    NotApplied(ProposalStatus),
    /// The base text changed; the proposal is now `Stale`.
    #[error("proposal base is outdated")]
    Stale { reason: String },
    #[error("proposal changes nothing")]
    NoChange,
    #[error("undo would overwrite later edits")]
    UndoConflict,
    #[error("meeting has no summary to change")]
    NoSummary,
    #[error("summary format does not support proposals")]
    UnsupportedSummary,
    #[error("empty summary cannot be applied")]
    EmptySummary,
    #[error("proposal content is invalid: {0}")]
    Invalid(String),
    #[error("storage error: {0}")]
    Storage(String),
}

impl ProposalError {
    /// Danish text for the user.
    pub fn user_message(&self) -> String {
        match self {
            Self::NotFound => strings::PROPOSAL_NOT_FOUND.into(),
            Self::NotPending(_) => strings::PROPOSAL_NOT_PENDING.into(),
            Self::NotApplied(_) => strings::PROPOSAL_NOT_APPLIED.into(),
            Self::Stale { reason } => reason.clone(),
            Self::NoChange => strings::PROPOSAL_NO_CHANGE.into(),
            Self::UndoConflict => strings::PROPOSAL_UNDO_CONFLICT.into(),
            Self::NoSummary => strings::PROPOSAL_NO_SUMMARY.into(),
            Self::UnsupportedSummary => strings::PROPOSAL_UNSUPPORTED_SUMMARY.into(),
            Self::EmptySummary => strings::PROPOSAL_EMPTY_SUMMARY.into(),
            Self::Invalid(_) => strings::PROPOSAL_INVALID.into(),
            Self::Storage(_) => strings::PROPOSAL_FAILED.into(),
        }
    }
}

impl From<sqlx::Error> for ProposalError {
    fn from(error: sqlx::Error) -> Self {
        Self::Storage(error.to_string())
    }
}

impl From<anyhow::Error> for ProposalError {
    fn from(error: anyhow::Error) -> Self {
        Self::Storage(format!("{error:#}"))
    }
}

/// Removes the internal `<!--offset:N--> ` marker, so the diff shows only what
/// the user sees. The marker keeps the per-block timestamp (#15).
fn strip_offset(line: &str) -> &str {
    line.strip_prefix("<!--offset:")
        .and_then(|rest| rest.split_once("--> ").map(|(_, text)| text))
        .unwrap_or(line)
}

fn has_offset(line: &str) -> bool {
    strip_offset(line).len() != line.len()
}

pub(crate) fn visible_lines(field: ProposalField, content: &str) -> Vec<String> {
    let lines = diff::lines(content);
    match field {
        ProposalField::Notes => lines.iter().map(|l| strip_offset(l).to_string()).collect(),
        ProposalField::Summary => lines,
    }
}

/// Gives unchanged proposed note lines the timestamp of the matching base
/// line. An assistant returns plain text without the markers, and applying it
/// must not erase the time of every note the assistant left alone.
pub(crate) fn carry_offsets(base: &str, proposed: &str) -> String {
    let base_lines = diff::lines(base);
    let mut proposed_lines = diff::lines(proposed);
    let base_visible: Vec<String> = base_lines
        .iter()
        .map(|l| strip_offset(l).to_string())
        .collect();
    let proposed_visible: Vec<String> = proposed_lines
        .iter()
        .map(|l| strip_offset(l).to_string())
        .collect();
    for (b, p) in diff::matches(&base_visible, &proposed_visible) {
        if !has_offset(&proposed_lines[p]) && has_offset(&base_lines[b]) {
            proposed_lines[p] = base_lines[b].clone();
        }
    }
    diff::join(&proposed_lines)
}
