use super::{score, Score};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct ClipReport {
    pub id: String,
    pub reference: String,
    pub hypothesis: String,
    pub score: Score,
    pub wer: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Report {
    pub schema_version: u32,
    pub normalization: String,
    pub dataset_kind: String,
    /// Includes model name; compare only like-for-like engines and settings.
    pub engine: String,
    pub date: String,
    pub git_sha: String,
    pub clips: Vec<ClipReport>,
    pub total: Score,
    pub wer: Option<f64>,
}

impl Report {
    pub fn new(engine: String, dataset_kind: String, git_sha: String) -> Self {
        Self {
            schema_version: 1,
            normalization: "unicode-alphanumeric-lower-v1".into(),
            dataset_kind,
            engine,
            date: chrono::Utc::now().to_rfc3339(),
            git_sha,
            clips: Vec::new(),
            total: Score::default(),
            wer: Some(0.0),
        }
    }

    pub fn push(&mut self, id: String, reference: String, hypothesis: String) {
        let score = score(&reference, &hypothesis);
        self.total.add(score);
        self.wer = self.total.wer();
        self.clips.push(ClipReport {
            id,
            reference,
            hypothesis,
            score,
            wer: score.wer(),
        });
    }
}
