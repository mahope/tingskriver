//! UI-free meeting list. Build once on refresh; scrolling borrows a slice.
use chrono::{Local, NaiveDate};
use sqlx::SqlitePool;

use crate::database::{models::MeetingModel, repositories::meeting::MeetingsRepository};
use crate::{format_da, recover};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Saved,
    Recording,
    Pending,
    Transcribing,
    Transcribed,
    Summarizing,
    SummaryReady,
    Failed,
    Cancelled,
    Unknown,
}
impl Status {
    pub fn from_jobs(recording: bool, transcription: Option<&str>, summary: Option<&str>) -> Self {
        if recording {
            return Self::Recording;
        }
        match transcription {
            Some("failed") => return Self::Failed,
            Some("running") => return Self::Transcribing,
            Some("pending") => return Self::Pending,
            Some("completed") | None => {}
            Some(_) => return Self::Unknown,
        }
        let summary = summary.map(str::to_ascii_lowercase);
        match summary.as_deref() {
            Some("completed") => Self::SummaryReady,
            Some("failed" | "error") => Self::Failed,
            Some("cancelled") => Self::Cancelled,
            Some("pending") => Self::Pending,
            Some("processing" | "running") => Self::Summarizing,
            None if transcription == Some("completed") => Self::Transcribed,
            None => Self::Saved,
            Some(_) => Self::Unknown,
        }
    }
    /// The first sentence of [`Status::text`] for states the user should
    /// notice in the sidebar (work in progress, failure); `None` when the
    /// meeting is simply saved or done.
    pub fn short(self) -> Option<&'static str> {
        match self {
            Self::Recording
            | Self::Pending
            | Self::Transcribing
            | Self::Summarizing
            | Self::Failed
            | Self::Cancelled => self.text().split('.').next(),
            _ => None,
        }
    }
    pub fn text(self) -> &'static str {
        match self {
            Self::Saved => "Gemt. Lyden er klar til transskription.",
            Self::Recording => "Optager. Lyden gemmes løbende.",
            Self::Pending => "I kø. Behandlingen starter, når der er plads.",
            Self::Transcribing => "Transskriberer. Den gemte lyd bliver til tekst.",
            Self::Transcribed => "Transskription klar. Der er endnu ikke et referat.",
            Self::Summarizing => "Skriver referat. Transskriptionen er gemt.",
            Self::SummaryReady => "Referat klar. Åbn mødet for at læse det.",
            Self::Failed => {
                "Behandlingen fejlede. Den gemte lyd er bevaret. Åbn mødet for at prøve igen."
            }
            Self::Cancelled => "Referatgenereringen blev afbrudt. Transskriptionen er gemt.",
            Self::Unknown => "Ukendt status. Åbn mødet for at se detaljerne.",
        }
    }
}

#[derive(Debug)]
pub struct Meeting {
    pub data: MeetingModel,
    pub status: Status,
    pub duration: String,
    /// Local clock time of the start, "09.30"; set by [`List::new`].
    pub time: String,
}
#[derive(Debug)]
pub enum Row {
    Date { date: NaiveDate, label: String },
    Meeting(Meeting),
}
#[derive(Default, Debug)]
pub struct List {
    rows: Vec<Row>,
}
impl List {
    pub fn new<T: chrono::TimeZone>(mut meetings: Vec<Meeting>, timezone: &T) -> Self {
        meetings.sort_by(|a, b| {
            b.data
                .created_at
                .0
                .cmp(&a.data.created_at.0)
                .then_with(|| a.data.id.cmp(&b.data.id))
        });
        let mut rows = Vec::with_capacity(meetings.len() * 2);
        let mut previous = None;
        for mut meeting in meetings {
            let local = meeting.data.created_at.0.with_timezone(timezone);
            let date = local.date_naive();
            meeting.time = format_da::clock(local.time());
            if previous != Some(date) {
                rows.push(Row::Date {
                    date,
                    label: format_da::date(date),
                });
                previous = Some(date);
            }
            rows.push(Row::Meeting(meeting));
        }
        Self { rows }
    }
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }
    pub fn same_rows(&self, other: &Self) -> bool {
        self.rows.len() == other.rows.len()
            && self
                .rows
                .iter()
                .zip(&other.rows)
                .all(|(a, b)| match (a, b) {
                    (Row::Date { date: a, .. }, Row::Date { date: b, .. }) => a == b,
                    (Row::Meeting(a), Row::Meeting(b)) => {
                        a.data.id == b.data.id
                            && a.data.title == b.data.title
                            && a.status == b.status
                            && a.duration == b.duration
                    }
                    _ => false,
                })
    }
    pub fn visible(&self, range: std::ops::Range<usize>) -> &[Row] {
        &self.rows[range.start.min(self.rows.len())
            ..range
                .end
                .min(self.rows.len())
                .max(range.start.min(self.rows.len()))]
    }
}

/// The heading of a day in the sidebar, as the Tauri app: "I dag", "I går",
/// otherwise weekday and date ("mandag 26. oktober").
pub fn day_label(date: NaiveDate, today: NaiveDate) -> String {
    if date == today {
        crate::strings::DAY_TODAY.to_string()
    } else if today.pred_opt() == Some(date) {
        crate::strings::DAY_YESTERDAY.to_string()
    } else {
        format_da::day_long(date)
    }
}

/// The rows a sidebar search shows: meetings whose title contains `query`
/// (ignoring case) or that a full-text hit names (`hits`, meeting ids), each
/// with its date heading. An empty query shows every row. Indices point into
/// [`List::rows`], so the list itself is never copied.
pub fn matching_rows(
    list: &List,
    query: &str,
    hits: &std::collections::HashSet<String>,
) -> Vec<usize> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return (0..list.rows.len()).collect();
    }
    let mut shown = Vec::new();
    let mut heading = None;
    for (index, row) in list.rows.iter().enumerate() {
        match row {
            Row::Date { .. } => heading = Some(index),
            Row::Meeting(meeting) => {
                if hits.contains(&meeting.data.id)
                    || meeting.data.title.to_lowercase().contains(&query)
                {
                    if let Some(heading) = heading.take() {
                        shown.push(heading);
                    }
                    shown.push(index);
                }
            }
        }
    }
    shown
}

/// Where `row` (from an earlier list) sits among the `shown` rows of `list`:
/// the same meeting (by id) or the same day heading. Keeps the sidebar's
/// scroll position on the row the user was looking at when the list reloads.
pub fn position_of_row(row: &Row, list: &List, shown: &[usize]) -> Option<usize> {
    shown
        .iter()
        .position(|&index| match (row, list.rows.get(index)) {
            (Row::Meeting(a), Some(Row::Meeting(b))) => a.data.id == b.data.id,
            (Row::Date { date: a, .. }, Some(Row::Date { date: b, .. })) => a == b,
            _ => false,
        })
}

/// A search snippet as plain text: the match markers of
/// `database::repositories::search` removed.
pub fn snippet_text(snippet: &str) -> String {
    snippet
        .replace(crate::database::repositories::search::MATCH_START, "")
        .replace(crate::database::repositories::search::MATCH_END, "")
}

pub fn title(value: &str) -> Result<&str, &'static str> {
    let value = value.trim();
    if value.is_empty() {
        Err("Skriv en titel til mødet.")
    } else {
        Ok(value)
    }
}
pub fn delete_allowed(confirmed: bool, status: Status) -> bool {
    confirmed
        && matches!(
            status,
            Status::Saved
                | Status::Transcribed
                | Status::SummaryReady
                | Status::Failed
                | Status::Cancelled
        )
}
pub fn recovery_candidates(mut recordings: Vec<recover::Recording>) -> Vec<recover::Recording> {
    recordings.retain(|r| {
        r.duration_seconds.is_finite() && r.duration_seconds > 0.0 && !r.tracks.is_empty()
    });
    recordings.sort_by(|a, b| {
        b.last_written
            .cmp(&a.last_written)
            .then_with(|| a.id.cmp(&b.id))
    });
    let mut seen = std::collections::HashSet::new();
    recordings.retain(|r| seen.insert(r.id.clone()));
    recordings
}

/// Failed/running/pending chunks take precedence over completed chunks.
pub fn job_status<'a>(
    id: &str,
    jobs: &'a [crate::transcription_queue::QueueEvent],
) -> Option<&'a str> {
    jobs.iter()
        .filter(|job| job.meeting_id == id)
        .min_by_key(|job| match job.status.as_str() {
            "failed" => 0,
            "running" => 1,
            "pending" => 2,
            "completed" => 4,
            _ => 3,
        })
        .map(|job| job.status.as_str())
}

async fn current_jobs(
    pool: &SqlitePool,
    jobs: &[crate::transcription_queue::QueueEvent],
) -> anyhow::Result<Vec<crate::transcription_queue::QueueEvent>> {
    let has_queue: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='transcription_jobs')",
    ).fetch_one(pool).await?;
    let mut stored_jobs = if has_queue {
        crate::transcription_queue::Queue::statuses_from_pool(pool).await?
    } else {
        Vec::new()
    };
    for event in jobs {
        if !stored_jobs.iter().any(|job| job.job_id == event.job_id) {
            stored_jobs.push(event.clone());
        }
    }
    Ok(stored_jobs)
}

pub async fn load(pool: &SqlitePool) -> anyhow::Result<List> {
    load_with_jobs(pool, &[]).await
}

pub async fn load_with_jobs(
    pool: &SqlitePool,
    jobs: &[crate::transcription_queue::QueueEvent],
) -> anyhow::Result<List> {
    let jobs = current_jobs(pool, jobs).await?;
    let meetings = MeetingsRepository::get_meetings(pool).await?;
    let summaries: Vec<(String, String)> =
        sqlx::query_as("SELECT meeting_id, status FROM summary_processes")
            .fetch_all(pool)
            .await?;
    let durations: Vec<(String, Option<f64>)> = sqlx::query_as(
        "SELECT meeting_id, MAX(audio_end_time) FROM transcripts GROUP BY meeting_id",
    )
    .fetch_all(pool)
    .await?;
    let summaries: std::collections::HashMap<_, _> = summaries.into_iter().collect();
    let durations: std::collections::HashMap<_, _> = durations.into_iter().collect();
    let mut rows = Vec::with_capacity(meetings.len());
    for data in meetings {
        let metadata = if let Some(folder) = &data.folder_path {
            match tokio::fs::read(std::path::Path::new(folder).join("metadata.json")).await {
                Ok(bytes) => {
                    serde_json::from_slice::<crate::audio::recording_saver::MeetingMetadata>(&bytes)
                        .ok()
                }
                Err(_) => None,
            }
        } else {
            None
        };
        let seconds = metadata
            .as_ref()
            .and_then(|m| m.duration_seconds)
            .or_else(|| durations.get(&data.id).copied().flatten());
        let duration = seconds
            .filter(|s| s.is_finite() && *s >= 0.0)
            .map(|s| format_da::duration(s as u64))
            .unwrap_or_else(|| "Varighed ukendt".into());
        let recording = metadata.as_ref().is_some_and(|m| m.status == "recording");
        let folder = data.folder_path.clone();
        let live = tokio::task::spawn_blocking(move || {
            folder.as_ref().is_some_and(|folder| {
                crate::audio::recording_writer::has_live_writer(
                    &std::path::Path::new(folder).join(crate::audio::finalize::RAW_DIR),
                )
            })
        })
        .await?;
        let status = Status::from_jobs(
            live,
            job_status(&data.id, &jobs).or_else(|| {
                metadata
                    .as_ref()
                    .and_then(|m| (m.status == "error" || (recording && !live)).then_some("failed"))
                    .or_else(|| durations.contains_key(&data.id).then_some("completed"))
            }),
            summaries.get(&data.id).map(String::as_str),
        );
        rows.push(Meeting {
            data,
            status,
            duration,
            time: String::new(),
        });
    }
    Ok(List::new(rows, &Local))
}

pub async fn rename(pool: &SqlitePool, id: &str, value: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        MeetingsRepository::update_meeting_name(
            pool,
            id,
            title(value).map_err(anyhow::Error::msg)?
        )
        .await?,
        "Mødet findes ikke længere."
    );
    Ok(())
}
pub async fn delete(pool: &SqlitePool, id: &str, confirmed: bool) -> anyhow::Result<()> {
    delete_with_jobs(pool, id, confirmed, &[]).await
}

pub async fn delete_with_jobs(
    pool: &SqlitePool,
    id: &str,
    confirmed: bool,
    jobs: &[crate::transcription_queue::QueueEvent],
) -> anyhow::Result<()> {
    let jobs = current_jobs(pool, jobs).await?;
    anyhow::ensure!(
        !jobs.iter().any(|job| job.meeting_id == id && matches!(job.status.as_str(), "pending" | "running")),
        "Vent til transskriptionen er afsluttet."
    );
    let list = load_with_jobs(pool, &jobs).await?;
    let meeting = list
        .rows()
        .iter()
        .find_map(|row| match row {
            Row::Meeting(m) if m.data.id == id => Some(m),
            _ => None,
        })
        .ok_or_else(|| anyhow::anyhow!("Mødet findes ikke længere."))?;
    anyhow::ensure!(
        delete_allowed(confirmed, meeting.status),
        "Bekræft sletning, og vent til behandlingen er afsluttet."
    );
    let raw = meeting
        .data
        .folder_path
        .as_ref()
        .map(|folder| std::path::Path::new(folder).join(crate::audio::finalize::RAW_DIR));
    let _raw_lock = tokio::task::spawn_blocking(move || {
        raw.filter(|dir| !matches!(dir.try_exists(), Ok(false)))
            .map(|dir| crate::audio::recording_writer::lock_raw_folder(&dir))
            .transpose()
    })
    .await?
    .map_err(|error| match error {
        crate::audio::recording_writer::LockError::Held => anyhow::anyhow!(
            "Mødet kan ikke slettes, mens lyden optages eller samles. Prøv igen om lidt."
        ),
        crate::audio::recording_writer::LockError::Io(source) => {
            log::error!("Cannot lock the audio folder of meeting {id}: {source}");
            anyhow::anyhow!("Mødets lydmappe kunne ikke låses, så mødet er ikke slettet.")
        }
    })?;
    anyhow::ensure!(
        MeetingsRepository::delete_meeting(pool, id).await?,
        "Mødet findes ikke længere."
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::models::DateTimeUtc;
    fn make_list(meetings: Vec<Meeting>) -> List {
        List::new(meetings, &chrono::Utc)
    }
    fn meeting(id: usize, day: u32) -> Meeting {
        let date = chrono::DateTime::parse_from_rfc3339(&format!("2026-11-{day:02}T12:00:00Z"))
            .unwrap()
            .to_utc();
        Meeting {
            data: MeetingModel {
                id: id.to_string(),
                title: "Møde".into(),
                created_at: DateTimeUtc(date),
                updated_at: DateTimeUtc(date),
                folder_path: None,
                import_key: None,
            },
            status: Status::Saved,
            duration: "Varighed ukendt".into(),
            time: String::new(),
        }
    }
    #[test]
    fn day_labels_follow_the_tauri_sidebar() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 29).unwrap();
        assert_eq!(day_label(today, today), "I dag");
        assert_eq!(day_label(today.pred_opt().unwrap(), today), "I går");
        assert_eq!(
            day_label(NaiveDate::from_ymd_opt(2026, 10, 26).unwrap(), today),
            "mandag 26. oktober"
        );
        // A day after today (clock moved back) is not "I går".
        assert_eq!(
            day_label(today.succ_opt().unwrap(), today),
            "fredag 30. oktober"
        );
    }
    #[test]
    fn rows_carry_the_local_clock_time() {
        let list = make_list(vec![meeting(1, 3)]);
        let Row::Meeting(m) = &list.rows()[1] else {
            panic!()
        };
        assert_eq!(m.time, "12.00");
    }
    #[test]
    fn search_keeps_matching_meetings_with_their_day_headings() {
        let mut a = meeting(1, 3);
        a.data.title = "Kickoff med Havnens Bageri".into();
        let mut b = meeting(2, 3);
        b.data.title = "Ugentligt statusmøde".into();
        let mut c = meeting(3, 1);
        c.data.title = "Retrospektiv".into();
        let list = make_list(vec![a, b, c]);
        let none = std::collections::HashSet::new();
        assert_eq!(matching_rows(&list, "  ", &none).len(), list.rows().len());
        let rows = matching_rows(&list, "BAGERI", &none);
        let ids: Vec<_> = rows
            .iter()
            .map(|&i| match &list.rows()[i] {
                Row::Date { .. } => "date".to_string(),
                Row::Meeting(m) => m.data.id.clone(),
            })
            .collect();
        assert_eq!(ids, ["date", "1"]);
        // A transcript hit shows a meeting whose title does not match.
        let hits = std::collections::HashSet::from(["3".to_string()]);
        let rows = matching_rows(&list, "budget", &hits);
        assert_eq!(rows.len(), 2);
        assert!(matches!(&list.rows()[rows[1]], Row::Meeting(m) if m.data.id == "3"));
        assert!(matching_rows(&list, "budget", &none).is_empty());
    }
    #[test]
    fn a_row_is_found_again_after_the_list_changes() {
        let old = make_list(vec![meeting(1, 3), meeting(2, 1), meeting(3, 1)]);
        let new = make_list(vec![meeting(4, 5), meeting(1, 3), meeting(3, 1)]);
        let none = std::collections::HashSet::new();
        let shown = matching_rows(&new, "", &none);
        // Old rows: [date 3, 1, date 1, 2, 3]; new: [date 5, 4, date 3, 1, date 1, 3].
        assert_eq!(position_of_row(&old.rows()[1], &new, &shown), Some(3));
        assert_eq!(position_of_row(&old.rows()[2], &new, &shown), Some(4));
        // A deleted meeting is not found.
        assert_eq!(position_of_row(&old.rows()[3], &new, &shown), None);
    }
    #[test]
    fn snippets_drop_match_markers_and_short_status_is_one_sentence() {
        use crate::database::repositories::search::{MATCH_END, MATCH_START};
        assert_eq!(
            snippet_text(&format!("om {MATCH_START}budget{MATCH_END} i år")),
            "om budget i år"
        );
        assert_eq!(Status::Transcribing.short(), Some("Transskriberer"));
        assert_eq!(Status::SummaryReady.short(), None);
        assert_eq!(Status::Saved.short(), None);
    }
    #[test]
    fn unchanged_rows_preserve_measurements() {
        let a = make_list(vec![meeting(1, 1)]);
        assert!(a.same_rows(&make_list(vec![meeting(1, 1)])));
        for change in 0..4 {
            let mut m = meeting(1, 1);
            match change {
                0 => m.data.id = "other".into(),
                1 => m.data.title = "Renamed".into(),
                2 => m.status = Status::Recording,
                _ => m.duration = "1:00".into(),
            }
            assert!(!a.same_rows(&make_list(vec![m])));
        }
    }

    #[tokio::test]
    #[allow(clippy::disallowed_methods)]
    async fn independent_writer_lock_blocks_recovery_and_deletion() {
        use crate::audio::{finalize, recording_writer::WRITER_LOCK};
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("live");
        let raw = folder.join(finalize::RAW_DIR);
        std::fs::create_dir_all(&raw).unwrap();
        let lock = std::fs::File::create(raw.join(WRITER_LOCK)).unwrap();
        lock.try_lock().unwrap();
        let db = crate::database::manager::DatabaseManager::new_from_app_data_dir(dir.path())
            .await
            .unwrap();
        let now = chrono::Utc::now();
        sqlx::query("INSERT INTO meetings (id,title,created_at,updated_at,folder_path) VALUES ('live','Meeting',?,?,?)")
            .bind(now).bind(now).bind(folder.to_str().unwrap()).execute(db.pool()).await.unwrap();
        let mut wav = std::fs::File::create(raw.join("microphone_000.wav")).unwrap();
        crate::audio::track_writer::write_wav_header(&mut wav, 48000, 0).unwrap();
        use std::io::Write;
        wav.write_all(&[0; 960]).unwrap();
        let before = std::fs::read(raw.join("microphone_000.wav")).unwrap();
        assert!(crate::audio::recording_writer::spawn_writer(
            raw.clone(),
            48000,
            std::sync::Arc::new(crate::audio::recording_writer::LogObserver),
        )
        .is_err());
        assert!(recover::scan(dir.path()).unwrap().is_empty());
        assert!(
            matches!(&load(db.pool()).await.unwrap().rows()[1], Row::Meeting(m) if m.status == Status::Recording)
        );
        assert!(delete(db.pool(), "live", true).await.is_err());
        assert!(
            finalize::recover_raw_tracks(&folder, None).status == finalize::AudioSaveStatus::Failed
        );
        assert!(
            finalize::finalize_raw_tracks(&folder, None).status
                == finalize::AudioSaveStatus::Failed
        );
        assert_eq!(
            std::fs::read(raw.join("microphone_000.wav")).unwrap(),
            before
        );
        // A concurrently spawned child can inherit the locked descriptor until exec.
        let inherited_lock = lock.try_clone().unwrap();
        lock.unlock().unwrap();
        drop(lock);
        assert_eq!(recover::scan(dir.path()).unwrap().len(), 1);
        drop(inherited_lock);
    }

    #[tokio::test]
    async fn stored_queue_jobs_block_deletion_without_events() {
        use crate::transcription_queue::{JobSpec, Queue};
        let dir = tempfile::tempdir().unwrap();
        let db = crate::database::manager::DatabaseManager::new_from_app_data_dir(dir.path())
            .await
            .unwrap();
        let queue = Queue::open(&dir.path().join(crate::paths::DATABASE_FILE))
            .await
            .unwrap();
        let now = chrono::Utc::now();
        sqlx::query(
            "INSERT INTO meetings (id,title,created_at,updated_at) VALUES ('test','Meeting',?,?)",
        )
        .bind(now)
        .bind(now)
        .execute(db.pool())
        .await
        .unwrap();
        queue
            .enqueue(&JobSpec::new("test", dir.path().join("audio.wav"), None), 0)
            .await
            .unwrap();
        for (state, expected) in [
            ("pending", Status::Pending),
            ("running", Status::Transcribing),
        ] {
            sqlx::query("UPDATE transcription_jobs SET status=?")
                .bind(state)
                .execute(db.pool())
                .await
                .unwrap();
            assert!(
                matches!(&load(db.pool()).await.unwrap().rows()[1], Row::Meeting(m) if m.status == expected)
            );
            assert!(delete(db.pool(), "test", true).await.is_err());
        }
        queue
            .enqueue(&JobSpec::new("test", dir.path().join("other.wav"), None), 1)
            .await
            .unwrap();
        sqlx::query("UPDATE transcription_jobs SET status='failed' WHERE created_at=0")
            .execute(db.pool())
            .await
            .unwrap();
        assert!(delete(db.pool(), "test", true).await.is_err());
    }

    #[test]
    fn grouping_sorts_dates_and_keeps_every_meeting() {
        let list = make_list(vec![meeting(1, 1), meeting(2, 3), meeting(3, 1)]);
        assert_eq!(list.rows().len(), 5);
        assert!(matches!(&list.rows()[0], Row::Date { date, .. } if date.day() == 3));
        assert!(matches!(&list.rows()[3], Row::Meeting(m) if m.data.id == "1"));
        assert!(make_list(vec![]).rows().is_empty());
    }
    use chrono::Datelike;
    #[test]
    fn every_job_status_has_a_danish_explanation() {
        for (input, expected) in [
            ("pending", Status::Pending),
            ("running", Status::Transcribing),
            ("completed", Status::Transcribed),
            ("failed", Status::Failed),
            ("unexpected", Status::Unknown),
        ] {
            assert_eq!(Status::from_jobs(false, Some(input), None), expected);
            assert!(expected.text().contains('.'));
        }
        for (input, expected) in [
            ("PENDING", Status::Pending),
            ("Processing", Status::Summarizing),
            ("COMPLETED", Status::SummaryReady),
            ("cancelled", Status::Cancelled),
        ] {
            assert_eq!(Status::from_jobs(false, None, Some(input)), expected);
        }
        assert_eq!(Status::from_jobs(true, None, None), Status::Recording);
        assert_eq!(
            Status::from_jobs(false, None, Some("completed")),
            Status::SummaryReady
        );
        assert_eq!(
            Status::from_jobs(false, Some("failed"), Some("completed")),
            Status::Failed
        );
    }
    #[test]
    fn grouping_uses_the_supplied_timezone_at_midnight() {
        let mut m = meeting(1, 1);
        m.data.created_at.0 = chrono::DateTime::parse_from_rfc3339("2026-11-01T23:30:00Z")
            .unwrap()
            .to_utc();
        let list = List::new(vec![m], &chrono_tz::Europe::Copenhagen);
        assert!(matches!(&list.rows()[0], Row::Date { date, .. } if date.day() == 2));
    }
    #[test]
    fn active_or_failed_chunks_override_completed_chunks() {
        let job = |status: &str| crate::transcription_queue::QueueEvent {
            job_id: status.into(),
            meeting_id: "meeting".into(),
            status: status.into(),
            attempts: 0,
            next_attempt: None,
            track: None,
            rejected: None,
        };
        assert_eq!(
            job_status("meeting", &[job("completed"), job("running")]),
            Some("running")
        );
        assert_eq!(
            job_status("meeting", &[job("pending"), job("failed")]),
            Some("failed")
        );
        assert_eq!(job_status("missing", &[job("failed")]), None);
        assert_eq!(
            Status::from_jobs(false, None, Some("processing")),
            Status::Summarizing
        );
        assert!(!delete_allowed(true, Status::Summarizing));
        assert!(!delete_allowed(true, Status::Pending));
    }
    #[test]
    fn scrolling_a_thousand_meetings_borrows_original_storage() {
        let list = make_list(
            (0..1000)
                .map(|id| meeting(id, 1 + (id % 28) as u32))
                .collect(),
        );
        let base = list.rows().as_ptr();
        for start in 0..list.rows().len() {
            let visible = list.visible(start..start + 20);
            assert_eq!(visible.as_ptr(), base.wrapping_add(start));
        }
        assert_eq!(
            list.rows()
                .iter()
                .filter(|r| matches!(r, Row::Meeting(_)))
                .count(),
            1000
        );
    }
    #[test]
    fn mutations_require_a_title_and_explicit_safe_deletion() {
        assert!(title("  ").is_err());
        assert_eq!(title("  Møde  ").unwrap(), "Møde");
        assert!(!delete_allowed(false, Status::Saved));
        assert!(!delete_allowed(true, Status::Recording));
        assert!(!delete_allowed(true, Status::Transcribing));
        assert!(delete_allowed(true, Status::Failed));
    }
    #[test]
    #[allow(clippy::disallowed_methods)]
    fn recovery_reads_checkpoint_audio_without_ui_storage() {
        use std::io::Write;
        let root = tempfile::tempdir().unwrap();
        let raw = root
            .path()
            .join("interrupted")
            .join(crate::audio::finalize::RAW_DIR);
        std::fs::create_dir_all(&raw).unwrap();
        let mut file = std::fs::File::create(raw.join("microphone_000.wav")).unwrap();
        crate::audio::track_writer::write_wav_header(&mut file, 48000, 96000).unwrap();
        file.write_all(&vec![0; 96000]).unwrap();
        let candidates = recovery_candidates(recover::scan(root.path()).unwrap());
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].id, "interrupted");
        assert_eq!(candidates[0].duration_seconds, 1.0);
    }
    #[tokio::test]
    async fn rename_and_delete_persist_and_missing_meetings_fail() {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::database::manager::DatabaseManager::new_from_app_data_dir(dir.path())
            .await
            .unwrap();
        let now = chrono::Utc::now();
        sqlx::query(
            "INSERT INTO meetings (id,title,created_at,updated_at) VALUES ('test','Old',?,?)",
        )
        .bind(now)
        .bind(now)
        .execute(db.pool())
        .await
        .unwrap();
        assert!(rename(db.pool(), "test", " ").await.is_err());
        rename(db.pool(), "test", "  New title  ").await.unwrap();
        let list = load(db.pool()).await.unwrap();
        assert!(matches!(&list.rows()[1], Row::Meeting(m) if m.data.title == "New title"));
        assert!(delete(db.pool(), "test", false).await.is_err());
        assert!(!load(db.pool()).await.unwrap().rows().is_empty());
        delete(db.pool(), "test", true).await.unwrap();
        assert!(load(db.pool()).await.unwrap().rows().is_empty());
        assert!(rename(db.pool(), "missing", "title").await.is_err());
        assert!(delete(db.pool(), "missing", true).await.is_err());
    }
    #[tokio::test]
    async fn stored_transcripts_and_repository_summary_states_survive_reload() {
        use crate::database::repositories::summary::SummaryProcessesRepository;
        let dir = tempfile::tempdir().unwrap();
        let db = crate::database::manager::DatabaseManager::new_from_app_data_dir(dir.path())
            .await
            .unwrap();
        let now = chrono::Utc::now();
        sqlx::query("INSERT INTO meetings (id,title,created_at,updated_at,folder_path) VALUES ('test','Meeting',?,?,?)")
            .bind(now).bind(now).bind(dir.path().to_str().unwrap())
            .execute(db.pool()).await.unwrap();
        tokio::fs::write(dir.path().join("metadata.json"),
            br#"{"meeting_id":"test","meeting_name":"Meeting","created_at":"2026-11-10T12:00:00Z","status":"completed","version":"1","devices":{"microphone":null,"system_audio":null},"audio_file":"audio.mp4","transcript_file":"transcript.json","sample_rate":48000}"#)
            .await.unwrap();
        sqlx::query("INSERT INTO transcripts (id,meeting_id,transcript,timestamp) VALUES ('text','test','Saved transcript',?)")
            .bind(now).execute(db.pool()).await.unwrap();
        let status = |list: &List| match &list.rows()[1] {
            Row::Meeting(m) => m.status,
            _ => panic!("expected meeting"),
        };
        assert_eq!(status(&load(db.pool()).await.unwrap()), Status::Transcribed);
        let jobs = [crate::transcription_queue::QueueEvent {
            job_id: "retry".into(),
            meeting_id: "test".into(),
            status: "running".into(),
            attempts: 0,
            next_attempt: None,
            track: None,
            rejected: None,
        }];
        assert_eq!(
            status(&load_with_jobs(db.pool(), &jobs).await.unwrap()),
            Status::Transcribing
        );
        SummaryProcessesRepository::create_or_reset_process(db.pool(), "test", now)
            .await
            .unwrap();
        assert_eq!(status(&load(db.pool()).await.unwrap()), Status::Pending);
        assert!(
            SummaryProcessesRepository::update_process_cancelled(db.pool(), "test", now)
                .await
                .unwrap()
        );
        let cancelled = status(&load(db.pool()).await.unwrap());
        assert_ne!(cancelled, Status::Unknown);
        assert!(cancelled.text().contains("afbrudt"));
        assert!(delete_allowed(true, cancelled));
        delete(db.pool(), "test", true).await.unwrap();
    }
    #[test]
    fn recovery_filters_empty_audio_and_orders_newest_first() {
        use crate::audio::track_writer::TrackKind;
        let make = |id: &str, seconds, last_written| recover::Recording {
            id: id.into(),
            duration_seconds: seconds,
            last_written,
            tracks: vec![TrackKind::Microphone],
        };
        let result = recovery_candidates(vec![
            make("old", 1.0, 1),
            make("empty", 0.0, 3),
            make("new", 2.0, 2),
        ]);
        assert_eq!(
            result.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            ["new", "old"]
        );
    }
}
