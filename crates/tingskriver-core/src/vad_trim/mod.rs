//! Batch-only silence trimming. All sample positions refer to 16 kHz mono audio.
mod config;
mod energy;
mod file;
pub use file::trim_file;
pub mod meeting;
mod provider;
mod trim;
pub use config::*;
pub use provider::*;
pub use trim::*;
#[cfg(test)]
mod tests;
