//! The small pill at the bottom of the screen while dictating.
//!
//! The UI layer draws it; the dictation service only says which state to
//! show through [`Overlay`].

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OverlayState {
    Recording { level: f32, locked: bool },
    Processing,
    Error,
    Hidden,
}

/// The dictation pill, implemented by the UI layer.
pub trait Overlay: Send + Sync + 'static {
    /// Create the (hidden) overlay window if it does not exist yet.
    fn ensure(&self) -> Result<(), String>;

    /// Show `state`, or hide the pill for [`OverlayState::Hidden`].
    fn set(&self, state: OverlayState);
}
