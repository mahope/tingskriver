use super::Event;
use chrono::{DateTime, Duration, Utc};

/// Match only events active now and overlapping the observed meeting, with
/// a ten-minute early-start allowance. Stable IDs break equal scores.
pub fn match_meeting(
    now: DateTime<Utc>,
    started_at: DateTime<Utc>,
    candidates: &[Event],
) -> Option<Event> {
    if started_at > now {
        return None;
    }
    candidates
        .iter()
        .filter(|e| e.end > now && e.start <= now + Duration::minutes(10) && e.end > started_at)
        .min_by_key(|e| {
            (
                i64::from(e.all_day) * 86_400 + (e.start - started_at).num_seconds().abs(),
                e.start,
                e.uid.clone(),
                e.title.clone(),
            )
        })
        .cloned()
}
