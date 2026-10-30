//! Floating pill at the bottom of the screen: audio level while dictating,
//! "Skriver …" while processing. Also the base for the recording indicator.
//!
//! The window is a native Win32 layered window that never takes focus
//! (`WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_NOACTIVATE |
//! WS_EX_TOOLWINDOW`), drawn with tiny-skia and `UpdateLayeredWindow` at
//! about 30 fps, and only while it is visible.
//!
//! - [`model`]: states, timing and bar history (any OS, tested)
//! - [`layout`], [`render`], [`text`]: geometry and drawing (any OS, tested)
//! - `windows`: the window itself, [`windows::Overlay`] and
//!   [`windows::CoreOverlay`] for the dictation service in core (Windows)

pub mod layout;
pub mod model;
pub mod render;
pub mod text;
pub mod theme;

#[cfg(windows)]
pub mod windows;

pub use model::PillState;
use tingskriver_core::dictation::overlay::OverlayState;

/// The pill state for a state from core's dictation service.
pub fn from_core(state: OverlayState) -> PillState {
    match state {
        OverlayState::Recording { level, locked } => PillState::Listening {
            level,
            hands_free: locked,
        },
        OverlayState::Processing => PillState::Processing,
        // Core logs the reason and shows it in the app; the pill keeps it short.
        OverlayState::Error => PillState::Error(String::new()),
        OverlayState::Hidden => PillState::Idle,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_states_map_to_pill_states() {
        assert_eq!(
            from_core(OverlayState::Recording {
                level: 0.4,
                locked: true
            }),
            PillState::Listening {
                level: 0.4,
                hands_free: true
            }
        );
        assert_eq!(from_core(OverlayState::Processing), PillState::Processing);
        assert_eq!(from_core(OverlayState::Hidden), PillState::Idle);
        let error = from_core(OverlayState::Error);
        assert_eq!(error.look().unwrap().label, "Kunne ikke skrive");
    }
}
