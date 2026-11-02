//! Provider-independent word error rate and reproducible evaluation reports.
//! See `docs/wer.md` for normalization, manifest and manual model evaluation.
mod distance;
pub mod manifest;
mod normalize;
pub mod report;
pub mod runner;
pub use distance::{score, Score};
pub use normalize::normalize;

#[cfg(test)]
mod tests;
