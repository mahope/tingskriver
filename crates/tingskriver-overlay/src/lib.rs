//! Floating pill and recording indicator. Windows only; empty until the
//! overlay is built on top of `tingskriver-core`.

#[cfg(windows)]
pub mod windows {
    /// Placeholder for the Windows overlay window.
    pub struct Overlay;
}
