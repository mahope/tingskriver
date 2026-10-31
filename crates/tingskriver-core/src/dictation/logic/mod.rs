//! Platform independent dictation logic: plain `std` plus `serde` for the
//! formats users can edit, unit tested on any OS (`cargo test -p tingskriver-core`).

pub mod address;
pub mod app_style;
pub mod audio;
pub mod cleanup;
pub mod engine;
pub mod gesture;
pub mod hotkey;
pub mod modes;
pub mod numbers;
pub mod spoken;
mod token;
