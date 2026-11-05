//! What turns a voice into numbers. The matching, storage and consent code
//! only sees this trait, so a real model can be added later (issue #196)
//! without touching them.

use std::fmt;

/// Longest embedding we store. A Credential Manager blob holds 2,560 bytes;
/// 256 floats are 1,368 characters in base64.
pub const MAX_DIMENSIONS: usize = 256;

/// A voiceprint. Biometric data: `Debug` never shows the numbers, and there
/// is deliberately no `Serialize`, so it cannot reach a log, a config file,
/// an export or a report by accident.
#[derive(Clone, PartialEq)]
pub struct Embedding(Vec<f32>);

impl Embedding {
    /// `None` for an empty, too long, non-finite or all-zero vector.
    pub fn new(values: Vec<f32>) -> Option<Embedding> {
        let usable = !values.is_empty()
            && values.len() <= MAX_DIMENSIONS
            && values.iter().all(|v| v.is_finite())
            && values.iter().any(|v| *v != 0.0);
        usable.then_some(Embedding(values))
    }

    pub(crate) fn values(&self) -> &[f32] {
        &self.0
    }

    pub fn dimensions(&self) -> usize {
        self.0.len()
    }
}

impl fmt::Debug for Embedding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Embedding(***)")
    }
}

/// Mono audio at `sample_rate` Hz from one speaker (a clean single-speaker
/// stretch, 1.5 to 10 seconds).
pub trait SpeakerEmbedder: Send + Sync {
    fn embed(&self, samples: &[f32], sample_rate: u32) -> Result<Embedding, String>;
}

/// **TEST STUB. NOT A VOICE MODEL.** It folds the samples into a few
/// energy bins so tests get stable, different vectors for different input.
/// It recognises nobody and must never be offered to users. The repository
/// has no speaker embedding model; see `docs/stemmeaftryk.md` for what a
/// real embedder needs.
pub struct StubEmbedder;

impl SpeakerEmbedder for StubEmbedder {
    fn embed(&self, samples: &[f32], _sample_rate: u32) -> Result<Embedding, String> {
        const BINS: usize = 8;
        let mut bins = vec![0.0f32; BINS];
        for (i, s) in samples.iter().enumerate() {
            bins[i % BINS] += s.abs();
        }
        Embedding::new(bins).ok_or_else(|| "no usable audio".to_string())
    }
}
