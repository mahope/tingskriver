use super::{Bundle, Error, Sidecar};
use crate::{
    database::repositories::meeting::MeetingsRepository,
    notes::{markdown, NotesStore},
};

/// The caller supplies the exact visible summary approved by the user. Until
/// #44 supplies structured speakers/tasks, these fields remain empty.
pub async fn snapshot(
    pool: &sqlx::SqlitePool,
    meeting_id: &str,
    summary: String,
) -> Result<Bundle, Error> {
    let meeting = MeetingsRepository::get_meeting(pool, meeting_id)
        .await
        .map_err(|_| Error::Storage)?
        .ok_or(Error::Missing)?;
    let notes = NotesStore::new(pool.clone())
        .load(meeting_id)
        .await
        .map_err(|_| Error::Storage)?;
    let bundle = Bundle {
        sidecar: Sidecar {
            kilde: "tingskriver".into(),
            skemaversion: 1,
            titel: meeting.title,
            tid: meeting.created_at.parse().map_err(|_| Error::Invalid)?,
            talere: vec![],
            opgaver: vec![],
        },
        transcript: meeting
            .transcripts
            .into_iter()
            .map(|t| format!("[{}] {}", t.timestamp, t.text))
            .collect::<Vec<_>>()
            .join("\n\n"),
        summary,
        notes: markdown::serialize(&notes),
    };
    bundle.validate()?;
    Ok(bundle)
}
