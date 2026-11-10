//! Durable post-meeting jobs. See `docs/transcription-queue.md` for host wiring.

mod store;
mod worker;

pub use store::{Job, JobSpec, Queue, Version, MAX_ATTEMPTS};
pub use worker::transcribe_local;

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueEvent {
    pub job_id: String,
    pub meeting_id: String,
    /// pending, running, completed, or failed ("Fejlede, prøv igen").
    pub status: String,
    pub attempts: i64,
    /// Unix seconds. Only meaningful for pending jobs.
    pub next_attempt: Option<i64>,
    pub track: Option<String>,
}

#[cfg(test)]
#[allow(clippy::disallowed_methods)]
mod tests;
