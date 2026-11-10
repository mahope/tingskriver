use serde::{Deserialize, Serialize};

/// Word edit counts. WER is a fraction, not a percentage, and can exceed 1.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Score {
    pub reference_words: usize,
    pub substitutions: usize,
    pub insertions: usize,
    pub deletions: usize,
}

impl Score {
    pub fn errors(self) -> usize {
        self.substitutions + self.insertions + self.deletions
    }

    /// Both strings empty: zero. Empty reference with insertions: undefined.
    pub fn wer(self) -> Option<f64> {
        if self.reference_words == 0 {
            return if self.errors() == 0 { Some(0.0) } else { None };
        }
        Some(self.errors() as f64 / self.reference_words as f64)
    }

    /// Micro-average: add counts before dividing by reference words.
    pub fn add(&mut self, other: Self) {
        self.reference_words += other.reference_words;
        self.substitutions += other.substitutions;
        self.insertions += other.insertions;
        self.deletions += other.deletions;
    }
}

/// Word Levenshtein with deterministic ties: substitution, deletion, insertion.
/// Uses two rows of memory. Normalization is identical for both inputs.
pub fn score(reference: &str, hypothesis: &str) -> Score {
    let reference = super::normalize(reference);
    let hypothesis = super::normalize(hypothesis);
    let reference: Vec<_> = reference.split_whitespace().collect();
    let hypothesis: Vec<_> = hypothesis.split_whitespace().collect();
    let mut previous: Vec<_> = (0..=hypothesis.len())
        .map(|insertions| Score {
            insertions,
            ..Score::default()
        })
        .collect();
    for (r, word) in reference.iter().enumerate() {
        let mut current = vec![Score {
            deletions: r + 1,
            ..Score::default()
        }];
        for (h, candidate) in hypothesis.iter().enumerate() {
            let mut diagonal = previous[h];
            diagonal.substitutions += usize::from(word != candidate);
            let mut deletion = previous[h + 1];
            deletion.deletions += 1;
            let mut insertion = current[h];
            insertion.insertions += 1;
            let best = [diagonal, deletion, insertion]
                .into_iter()
                .min_by_key(|s| s.errors())
                .unwrap_or_default();
            current.push(best);
        }
        previous = current;
    }
    Score {
        reference_words: reference.len(),
        ..previous[hypothesis.len()]
    }
}
