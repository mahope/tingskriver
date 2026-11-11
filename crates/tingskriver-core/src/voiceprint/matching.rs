//! Matching speakers in a meeting against remembered voices. Pure: no disk,
//! no clock, no network.
//!
//! The thresholds are the starting values of the anarlog project (MIT),
//! measured on English audio. **They are not calibrated for Danish.** A match
//! is therefore only ever a [`Suggestion`] that the user confirms with one
//! click; nothing is named automatically.

use super::embedder::Embedding;

/// Tells the UI and the report that the thresholds have not been measured on
/// Danish audio. Set to `true` only with a fixture-based measurement (needs
/// the local diarisation of #196).
pub const THRESHOLDS_CALIBRATED_ON_DANISH: bool = false;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Thresholds {
    /// Lowest cosine similarity that counts as a match.
    pub min_score: f32,
    /// How far the best person must be ahead of the second best.
    pub min_margin: f32,
}

impl Default for Thresholds {
    fn default() -> Self {
        Thresholds {
            min_score: 0.62,
            min_margin: 0.08,
        }
    }
}

/// A remembered person with their exemplars.
pub struct Candidate<'a> {
    pub person_id: &'a str,
    pub exemplars: &'a [Embedding],
}

/// A proposal. It carries no biometric data and names nobody by itself.
#[derive(Debug, Clone, PartialEq)]
pub struct Suggestion {
    /// The speaker label in the meeting (what the user sees).
    pub speaker: String,
    pub person_id: String,
    pub score: f32,
}

pub fn cosine(a: &Embedding, b: &Embedding) -> f32 {
    if a.dimensions() != b.dimensions() {
        return 0.0;
    }
    let (mut dot, mut na, mut nb) = (0.0f64, 0.0f64, 0.0f64);
    for (x, y) in a.values().iter().zip(b.values()) {
        let (x, y) = (f64::from(*x), f64::from(*y));
        dot += x * y;
        na += x * x;
        nb += y * y;
    }
    if na == 0.0 || nb == 0.0 {
        return 0.0;
    }
    (dot / (na.sqrt() * nb.sqrt())) as f32
}

fn person_score(speaker: &Embedding, exemplars: &[Embedding]) -> Option<f32> {
    exemplars
        .iter()
        .map(|e| cosine(speaker, e))
        .reduce(f32::max)
}

/// Propose a person for each speaker where all three rules hold: the score
/// reaches `min_score`, the person leads the next person by `min_margin`, and
/// the match is mutual (the person's best speaker is this speaker, and
/// no other speaker is closer to the person).
pub fn suggest(
    speakers: &[(String, Embedding)],
    people: &[Candidate<'_>],
    thresholds: Thresholds,
) -> Vec<Suggestion> {
    let scores: Vec<Vec<Option<f32>>> = speakers
        .iter()
        .map(|(_, e)| {
            people
                .iter()
                .map(|p| person_score(e, p.exemplars))
                .collect()
        })
        .collect();
    let best = |values: &mut dyn Iterator<Item = (usize, f32)>| -> Option<(usize, f32, f32)> {
        let mut ranked: Vec<(usize, f32)> = values.collect();
        ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
        let first = *ranked.first()?;
        let second = ranked.get(1).map_or(f32::MIN, |s| s.1);
        Some((first.0, first.1, first.1 - second))
    };
    let mut out = Vec::new();
    for (s, (label, _)) in speakers.iter().enumerate() {
        let mut row = scores[s]
            .iter()
            .enumerate()
            .filter_map(|(p, v)| v.map(|v| (p, v)));
        let Some((p, score, person_margin)) = best(&mut row) else {
            continue;
        };
        if score < thresholds.min_score {
            continue;
        }
        // One candidate has no runner-up, so the margin is open.
        if people.len() > 1 && person_margin < thresholds.min_margin {
            continue;
        }
        let mut column = (0..speakers.len()).filter_map(|s2| scores[s2][p].map(|v| (s2, v)));
        let Some((best_speaker, _, _)) = best(&mut column) else {
            continue;
        };
        if best_speaker != s {
            continue;
        }
        out.push(Suggestion {
            speaker: label.clone(),
            person_id: people[p].person_id.to_string(),
            score,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(values: &[f32]) -> Embedding {
        Embedding::new(values.to_vec()).unwrap()
    }

    #[test]
    fn thresholds_start_at_the_anarlog_values_and_are_uncalibrated() {
        let t = Thresholds::default();
        assert_eq!((t.min_score, t.min_margin), (0.62, 0.08));
        assert!(!std::hint::black_box(THRESHOLDS_CALIBRATED_ON_DANISH));
    }

    #[test]
    fn cosine_basics() {
        assert!((cosine(&e(&[1.0, 0.0]), &e(&[2.0, 0.0])) - 1.0).abs() < 1e-6);
        assert!(cosine(&e(&[1.0, 0.0]), &e(&[0.0, 1.0])).abs() < 1e-6);
        assert_eq!(cosine(&e(&[1.0, 0.0]), &e(&[1.0, 0.0, 0.0])), 0.0);
    }

    #[test]
    fn clear_match_is_suggested() {
        let ann = [e(&[1.0, 0.0, 0.0])];
        let bob = [e(&[0.0, 1.0, 0.0])];
        let people = [
            Candidate {
                person_id: "ann",
                exemplars: &ann,
            },
            Candidate {
                person_id: "bob",
                exemplars: &bob,
            },
        ];
        let speakers = vec![("Taler 1".to_string(), e(&[0.95, 0.1, 0.0]))];
        let got = suggest(&speakers, &people, Thresholds::default());
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].person_id, "ann");
        assert_eq!(got[0].speaker, "Taler 1");
    }

    #[test]
    fn low_score_gives_no_suggestion() {
        let ann = [e(&[1.0, 0.0, 0.0])];
        let people = [Candidate {
            person_id: "ann",
            exemplars: &ann,
        }];
        let speakers = vec![("Taler 1".to_string(), e(&[0.5, 1.0, 0.0]))]; // cos ~0.45
        assert!(suggest(&speakers, &people, Thresholds::default()).is_empty());
    }

    #[test]
    fn small_margin_gives_no_suggestion() {
        let ann = [e(&[1.0, 0.1, 0.0])];
        let bob = [e(&[1.0, 0.2, 0.0])];
        let people = [
            Candidate {
                person_id: "ann",
                exemplars: &ann,
            },
            Candidate {
                person_id: "bob",
                exemplars: &bob,
            },
        ];
        let speakers = vec![("Taler 1".to_string(), e(&[1.0, 0.15, 0.0]))];
        assert!(suggest(&speakers, &people, Thresholds::default()).is_empty());
    }

    #[test]
    fn match_must_be_mutual() {
        let ann = [e(&[1.0, 0.0, 0.0])];
        let people = [Candidate {
            person_id: "ann",
            exemplars: &ann,
        }];
        // Both speakers are close to Ann, but speaker 2 is closer: speaker 1
        // gets nothing, speaker 2 gets the suggestion.
        let speakers = vec![
            ("Taler 1".to_string(), e(&[0.8, 0.6, 0.0])),
            ("Taler 2".to_string(), e(&[1.0, 0.05, 0.0])),
        ];
        let got = suggest(&speakers, &people, Thresholds::default());
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].speaker, "Taler 2");
    }

    #[test]
    fn no_people_no_suggestions() {
        let speakers = vec![("Taler 1".to_string(), e(&[1.0, 0.0]))];
        assert!(suggest(&speakers, &[], Thresholds::default()).is_empty());
    }

    #[test]
    fn embedding_debug_hides_the_numbers_and_rejects_bad_input() {
        assert_eq!(format!("{:?}", e(&[0.123, 0.456])), "Embedding(***)");
        assert!(Embedding::new(vec![]).is_none());
        assert!(Embedding::new(vec![0.0, 0.0]).is_none());
        assert!(Embedding::new(vec![f32::NAN, 1.0]).is_none());
    }
}
