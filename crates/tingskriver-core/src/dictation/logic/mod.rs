//! Platform independent dictation logic.
//!
//! Everything in here is plain `std` so it can be unit tested on any OS
//! without building the full Tauri crate:
//!
//! ```text
//! rustc --edition 2021 --test src/dictation/logic/mod.rs -o /tmp/dictation_logic
//! /tmp/dictation_logic
//! ```

pub mod app_style;
pub mod audio;
pub mod cleanup;
pub mod engine;
pub mod gesture;
pub mod hotkey;
pub mod residency;
pub mod timing;
