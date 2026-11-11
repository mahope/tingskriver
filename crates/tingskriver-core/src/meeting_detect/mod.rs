//! Pure meeting decisions. See `docs/moededetektor.md` for the input contract.
mod detect;
mod model;
mod rules;

pub use detect::detect;
pub use model::*;

#[cfg(test)]
mod fixtures_tests;
