//! Carry confirmed speaker names and corrections from the current version of
//! a meeting to a fresh rerun, and mark a truncated rerun as rejected.

use serde_json::{json, Value};

use crate::transcript_merge::reconcile::{
    is_truncated, reconcile_speakers, transfer_corrections, Correction, SpeakerSpan,
};
use crate::transcript_merge::NUMBERED_SPEAKER_PREFIX;

/// What the user has done to the version a rerun is compared with.
pub(super) struct Previous<'a> {
    pub job_id: &'a str,
    pub result: &'a Value,
    pub corrections: &'a [Correction],
}

fn number(label: &str) -> Option<usize> {
    label
        .strip_prefix(NUMBERED_SPEAKER_PREFIX)?
        .strip_prefix(' ')?
        .parse()
        .ok()
}

fn segments(result: &Value) -> &[Value] {
    result["segments"].as_array().map_or(&[], Vec::as_slice)
}

fn text(result: &Value) -> String {
    segments(result)
        .iter()
        .filter_map(|s| s["text"].as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

fn is_system(segment: &Value) -> bool {
    segment["source"] == "system"
}

fn range(segment: &Value) -> (u64, u64) {
    (
        segment["startMs"].as_u64().unwrap_or(0),
        segment["endMs"].as_u64().unwrap_or(0),
    )
}

fn spans(result: &Value) -> Vec<SpeakerSpan> {
    segments(result)
        .iter()
        .filter(|s| is_system(s))
        .filter_map(|s| {
            let (start_ms, slut_ms) = range(s);
            Some(SpeakerSpan {
                start_ms,
                slut_ms,
                nummer: number(s["speaker"].as_str()?)?,
            })
        })
        .collect()
}

/// Renumber the new system speakers to match `previous`, apply the user's
/// corrections, and add a `rejected` entry if the text was cut short. The
/// result stays a stored version in both cases.
pub(super) fn apply(previous: &Previous, new: &mut Value) {
    let old_spans = spans(previous.result);
    let mapping = reconcile_speakers(&old_spans, &spans(new));
    let ranges: Vec<_> = segments(new)
        .iter()
        .filter(|s| is_system(s))
        .map(range)
        .collect();
    let mut transferred = transfer_corrections(previous.corrections, &ranges).into_iter();
    if let Some(list) = new["segments"].as_array_mut() {
        for segment in list.iter_mut().filter(|s| is_system(s)) {
            let corrected = transferred.next().flatten();
            let current = segment["speaker"].as_str().and_then(number);
            if let Some(n) = corrected.or_else(|| current.map(|n| mapping[&n])) {
                segment["speaker"] = json!(format!("{NUMBERED_SPEAKER_PREFIX} {n}"));
                segment["speakersMissing"] = json!(false);
            }
        }
    }
    if is_truncated(&text(previous.result), &text(new)) {
        new["rejected"] = json!({
            "reason": "truncated",
            "message": crate::strings::TRANSCRIPT_TRUNCATED,
            "previousJobId": previous.job_id,
        });
    }
}
