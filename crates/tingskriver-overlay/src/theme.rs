//! Design tokens for the pill.
//!
//! Colours mirror the dark theme in `frontend/src/app/globals.css` (`.dark`)
//! under the same names, so the pill looks like the webview pill it replaces.
//! Sizes are logical pixels (100 % scaling); the renderer multiplies them by
//! the monitor's scale factor.

use tiny_skia::Color;

/// A colour token as HSL, the way `globals.css` writes it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hsl {
    pub h: f32,
    pub s: f32,
    pub l: f32,
}

impl Hsl {
    const fn new(h: f32, s: f32, l: f32) -> Self {
        Self { h, s, l }
    }

    /// The token as an opaque colour.
    pub fn color(self) -> Color {
        self.with_alpha(1.0)
    }

    /// The token with `alpha` opacity, like `hsl(var(--x) / alpha)`.
    pub fn with_alpha(self, alpha: f32) -> Color {
        let [r, g, b] = hsl_to_rgb(self.h, self.s / 100.0, self.l / 100.0);
        Color::from_rgba(r, g, b, alpha.clamp(0.0, 1.0)).unwrap_or(Color::BLACK)
    }
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> [f32; 3] {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = (h.rem_euclid(360.0)) / 60.0;
    let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
    let (r, g, b) = match hp as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    [
        (r + m).clamp(0.0, 1.0),
        (g + m).clamp(0.0, 1.0),
        (b + m).clamp(0.0, 1.0),
    ]
}

pub const CARD: Hsl = Hsl::new(222.0, 15.0, 12.0);
pub const CARD_FOREGROUND: Hsl = Hsl::new(40.0, 12.0, 91.0);
pub const MUTED_FOREGROUND: Hsl = Hsl::new(222.0, 8.0, 67.0);
pub const BORDER: Hsl = Hsl::new(222.0, 12.0, 22.0);
pub const PRIMARY: Hsl = Hsl::new(214.0, 75.0, 72.0);
pub const DESTRUCTIVE: Hsl = Hsl::new(4.0, 75.0, 62.0);
pub const SUCCESS: Hsl = Hsl::new(150.0, 45.0, 56.0);
pub const RECORDING: Hsl = Hsl::new(4.0, 75.0, 62.0);

/// Opacity of the pill background (`hsl(var(--card) / 0.96)`).
pub const CARD_ALPHA: f32 = 0.96;
/// Opacity of the soft shadow under the pill.
pub const SHADOW_ALPHA: f32 = 0.28;

/// Sizes in logical pixels.
pub mod size {
    pub const PILL_HEIGHT: f32 = 40.0;
    pub const PADDING_LEFT: f32 = 14.0;
    pub const PADDING_RIGHT: f32 = 16.0;
    pub const GAP: f32 = 10.0;
    pub const BORDER: f32 = 1.0;
    pub const INDICATOR: f32 = 12.0;
    pub const DOT: f32 = 10.0;
    pub const SPINNER_STROKE: f32 = 2.0;
    pub const CHECK_STROKE: f32 = 2.0;
    pub const BAR_COUNT: usize = 7;
    pub const BAR_WIDTH: f32 = 3.0;
    pub const BAR_GAP: f32 = 3.0;
    pub const BAR_MIN: f32 = 4.0;
    pub const BAR_MAX: f32 = 22.0;
    pub const FONT: f32 = 14.0;
    /// Longest label before it is cut with an ellipsis (error texts).
    pub const TEXT_MAX: f32 = 320.0;
    /// Room around the pill for the shadow.
    pub const SHADOW: f32 = 8.0;
    /// Distance from the bottom of the work area (above the taskbar).
    pub const MARGIN_BOTTOM: f32 = 64.0;
}

/// Timing in milliseconds.
pub mod timing {
    /// Frame interval while the pill is visible (about 30 fps).
    pub const FRAME_MS: u64 = 33;
    /// How often a new level enters the bar history.
    pub const BAR_STEP_MS: u64 = 50;
    pub const FADE_OUT_MS: u64 = 120;
    pub const SPINNER_TURN_MS: u64 = 800;
    pub const DONE_HIDE_MS: u64 = 900;
    pub const CANCELLED_HIDE_MS: u64 = 900;
    pub const ERROR_HIDE_MS: u64 = 3000;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgb8(c: Color) -> [u8; 3] {
        let c = c.to_color_u8();
        [c.red(), c.green(), c.blue()]
    }

    #[test]
    fn hsl_converts_like_css() {
        assert_eq!(rgb8(Hsl::new(0.0, 0.0, 100.0).color()), [255, 255, 255]);
        assert_eq!(rgb8(Hsl::new(0.0, 100.0, 50.0).color()), [255, 0, 0]);
        assert_eq!(rgb8(Hsl::new(120.0, 100.0, 25.0).color()), [0, 128, 0]);
        // --card: 222 15% 12% is #1a1d23 in the browser.
        assert_eq!(rgb8(CARD.color()), [0x1a, 0x1d, 0x23]);
    }

    #[test]
    fn alpha_is_clamped() {
        assert_eq!(CARD.with_alpha(2.0).alpha(), 1.0);
        assert_eq!(CARD.with_alpha(-1.0).alpha(), 0.0);
    }
}
