//! Carry confirmed speaker names and manual corrections over to a rerun, and
//! reject reruns that lost too much text. Pure functions over data, no model.
//!
//! The idea (cluster mapping by summed time overlap, assignment transfer by
//! overlap, a letters-and-digits truncation rule) is from anarlog's
//! `batch_refine` (MIT, Fastrepl, Inc.). This is a reimplementation.

use std::collections::{BTreeMap, BTreeSet, HashMap};

/// A new speaker id is only mapped to an old one from this share of its time.
pub const MIN_SPEAKER_OVERLAP_RATIO: f64 = 0.6;
/// A rerun must lose at least this many letters/digits to count as truncated...
pub const TRUNCATION_MIN_LOST_CHARS: usize = 200;
/// ...and keep less than this share of the previous letters/digits.
pub const TRUNCATION_MAX_KEPT_RATIO: f64 = 0.5;

/// One stretch of a numbered speaker ("Taler N") on the system channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpeakerSpan {
    pub start_ms: u64,
    pub slut_ms: u64,
    pub nummer: usize,
}

/// A user's manual assignment of a stretch of audio to a speaker number in
/// the previous version.
pub type Correction = SpeakerSpan;

fn overlap(a: (u64, u64), b: (u64, u64)) -> u64 {
    a.1.min(b.1).saturating_sub(a.0.max(b.0))
}

/// Map each speaker number in `new` to the number it should have. A new
/// speaker takes an old number when they overlap by at least
/// [`MIN_SPEAKER_OVERLAP_RATIO`] of the new speaker's time. The mapping is
/// one-to-one (best overlap wins a collision). Other speakers get the lowest
/// number that no old speaker used, so a number never silently changes owner.
pub fn reconcile_speakers(old: &[SpeakerSpan], new: &[SpeakerSpan]) -> BTreeMap<usize, usize> {
    let mut total: BTreeMap<usize, u64> = BTreeMap::new();
    let mut shared: HashMap<(usize, usize), u64> = HashMap::new();
    for n in new {
        *total.entry(n.nummer).or_default() += n.slut_ms.saturating_sub(n.start_ms);
        for o in old {
            let t = overlap((n.start_ms, n.slut_ms), (o.start_ms, o.slut_ms));
            if t > 0 {
                *shared.entry((n.nummer, o.nummer)).or_default() += t;
            }
        }
    }
    let mut candidates: Vec<_> = shared
        .into_iter()
        .filter(|&((n, _), t)| t as f64 >= MIN_SPEAKER_OVERLAP_RATIO * total[&n] as f64)
        .collect();
    candidates.sort_by_key(|&((n, o), t)| (std::cmp::Reverse(t), n, o));
    let mut mapping = BTreeMap::new();
    let mut taken = BTreeSet::new();
    for ((n, o), _) in candidates {
        if !mapping.contains_key(&n) && taken.insert(o) {
            mapping.insert(n, o);
        }
    }
    let reserved: BTreeSet<usize> = old.iter().map(|o| o.nummer).collect();
    let mut next = 1;
    for &n in total.keys() {
        if mapping.contains_key(&n) {
            continue;
        }
        while reserved.contains(&next) || taken.contains(&next) {
            next += 1;
        }
        mapping.insert(n, next);
        taken.insert(next);
    }
    mapping
}

/// For each new segment `(start, end)`, the old speaker number the user had
/// assigned to that stretch, if one correction covers at least
/// [`MIN_SPEAKER_OVERLAP_RATIO`] of the segment. Corrections win over the
/// provider's clustering. Numbers are in the old numbering.
pub fn transfer_corrections(
    corrections: &[Correction],
    segments: &[(u64, u64)],
) -> Vec<Option<usize>> {
    segments
        .iter()
        .map(|&(start, end)| {
            let duration = end.saturating_sub(start);
            if duration == 0 {
                return None;
            }
            corrections
                .iter()
                .map(|c| (overlap((start, end), (c.start_ms, c.slut_ms)), c.nummer))
                .filter(|&(t, _)| t as f64 >= MIN_SPEAKER_OVERLAP_RATIO * duration as f64)
                .max_by_key(|&(t, n)| (t, std::cmp::Reverse(n)))
                .map(|(_, n)| n)
        })
        .collect()
}

/// Keep only letters and digits, lowercased, in a Unicode-aware way so
/// "Blåbærgrød på Ærø" keeps its æ, ø and å (an `[a-z0-9]` filter would not).
pub fn normalize_letters(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// True if `new` lost at least [`TRUNCATION_MIN_LOST_CHARS`] letters/digits
/// compared with `old` and kept less than half of them.
pub fn is_truncated(old: &str, new: &str) -> bool {
    let old = normalize_letters(old).chars().count();
    let new = normalize_letters(new).chars().count();
    old.saturating_sub(new) >= TRUNCATION_MIN_LOST_CHARS
        && (new as f64) < TRUNCATION_MAX_KEPT_RATIO * old as f64
}

#[cfg(test)]
#[path = "reconcile_tests.rs"]
mod tests;
