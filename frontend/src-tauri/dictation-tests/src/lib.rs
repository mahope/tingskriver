//! The same module layout as `src/dictation`, so `super::` paths resolve.

#[path = "../../src/dictation/logic/mod.rs"]
pub mod logic;
#[path = "../../src/dictation/settings.rs"]
pub mod settings;
#[path = "../../src/dictation/store.rs"]
pub mod store;
#[path = "../../src/dictation/syv.rs"]
pub mod syv;
