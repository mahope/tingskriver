//! Platform independent dictation logic: plain `std` plus `serde` for the
//! formats users can edit, unit tested on any OS (`cargo test -p tingskriver-core`).

pub mod address;
pub mod app_style;
pub mod audio;
pub mod cancellation;
pub mod cleanup;
pub mod engine;
pub mod gesture;
pub mod hook_stats;
pub mod hotkey;
mod identifiers;
pub mod modes;
pub mod numbers;
pub mod order;
pub mod paste;
pub mod polish_guard;
pub mod residency;
pub mod spoken;
pub mod startup;
pub mod suggest;
pub mod target_check;
pub mod timing;
mod token;
