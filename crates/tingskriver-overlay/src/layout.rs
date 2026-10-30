//! Pill geometry in physical pixels and where the window goes on screen.

use crate::theme::size;

/// A rectangle in physical screen pixels (like Win32's `RECT`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScreenRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl ScreenRect {
    pub fn width(&self) -> i32 {
        self.right - self.left
    }
}

/// Positions inside the window bitmap, in physical pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    pub scale: f32,
    /// Window (bitmap) size.
    pub width: u32,
    pub height: u32,
    /// The pill itself, inset by the shadow margin.
    pub pill_x: f32,
    pub pill_y: f32,
    pub pill_w: f32,
    pub pill_h: f32,
    pub indicator_cx: f32,
    pub center_y: f32,
    /// Left edge of the bars, when shown.
    pub bars_x: Option<f32>,
    pub text_x: f32,
    pub font_px: f32,
}

impl Layout {
    /// Lay out a pill whose label is `text_w` logical pixels wide, at
    /// `scale` (1.0 = 100 %, 1.5 = 150 %, ...).
    pub fn new(scale: f32, text_w: f32, bars: bool) -> Self {
        let scale = if scale.is_finite() && scale > 0.0 {
            scale
        } else {
            1.0
        };
        let bars_w =
            size::BAR_COUNT as f32 * size::BAR_WIDTH + (size::BAR_COUNT - 1) as f32 * size::BAR_GAP;
        let mut inner = size::INDICATOR + size::GAP;
        let bars_offset = inner;
        if bars {
            inner += bars_w + size::GAP;
        }
        let text_offset = inner;
        inner += text_w.clamp(0.0, size::TEXT_MAX);
        let pill_w_logical = size::PADDING_LEFT + inner + size::PADDING_RIGHT;

        // Whole pixels for the bitmap and the pill, so edges stay crisp.
        let shadow = (size::SHADOW * scale).round();
        let pill_w = (pill_w_logical * scale).round();
        let pill_h = (size::PILL_HEIGHT * scale).round();
        let pill_x = shadow;
        let pill_y = shadow;
        let content_x = pill_x + (size::PADDING_LEFT * scale).round();
        Self {
            scale,
            width: (pill_w + 2.0 * shadow) as u32,
            height: (pill_h + 2.0 * shadow) as u32,
            pill_x,
            pill_y,
            pill_w,
            pill_h,
            indicator_cx: content_x + size::INDICATOR * scale / 2.0,
            center_y: pill_y + pill_h / 2.0,
            bars_x: bars.then(|| content_x + (bars_offset * scale).round()),
            text_x: content_x + (text_offset * scale).round(),
            font_px: size::FONT * scale,
        }
    }
}

/// Top-left corner for a `width` x `height` window: centred at the bottom
/// of `work` (the monitor's work area, so above the taskbar), with the
/// bottom margin scaled for that monitor. Never above the work area.
pub fn place(work: ScreenRect, width: u32, height: u32, scale: f32) -> (i32, i32) {
    let margin = (size::MARGIN_BOTTOM * scale).round() as i32;
    let x = work.left + (work.width() - width as i32) / 2;
    let y = (work.bottom - height as i32 - margin).max(work.top);
    (x, y)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scales_with_dpi() {
        for scale in [1.0, 1.25, 1.5, 2.0] {
            let l = Layout::new(scale, 50.0, true);
            assert_eq!(l.pill_h, (size::PILL_HEIGHT * scale).round());
            assert_eq!(
                l.height as f32,
                l.pill_h + 2.0 * (size::SHADOW * scale).round()
            );
            assert_eq!(l.font_px, size::FONT * scale);
        }
        let one = Layout::new(1.0, 50.0, true);
        let two = Layout::new(2.0, 50.0, true);
        assert_eq!(two.width, one.width * 2);
        assert_eq!(two.height, one.height * 2);
    }

    #[test]
    fn content_is_ordered_left_to_right_inside_the_pill() {
        let l = Layout::new(1.5, 60.0, true);
        let bars = l.bars_x.unwrap();
        assert!(l.pill_x < l.indicator_cx && l.indicator_cx < bars && bars < l.text_x);
        assert!(l.text_x + 60.0 * 1.5 <= l.pill_x + l.pill_w);
        assert!(Layout::new(1.0, 60.0, false).bars_x.is_none());
    }

    #[test]
    fn long_text_is_capped() {
        let a = Layout::new(1.0, size::TEXT_MAX, false);
        let b = Layout::new(1.0, 5000.0, false);
        assert_eq!(a.width, b.width);
    }

    #[test]
    fn bad_scale_falls_back_to_100_percent() {
        assert_eq!(Layout::new(0.0, 10.0, false), Layout::new(1.0, 10.0, false));
        assert_eq!(
            Layout::new(f32::NAN, 10.0, false),
            Layout::new(1.0, 10.0, false)
        );
    }

    #[test]
    fn placed_bottom_centre_of_the_work_area() {
        // 1920x1080 at 100 % with a 40 px taskbar.
        let work = ScreenRect {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1040,
        };
        assert_eq!(place(work, 200, 56, 1.0), (860, 1040 - 56 - 64));
        // At 200 % the margin doubles.
        assert_eq!(place(work, 400, 112, 2.0), (760, 1040 - 112 - 128));
    }

    #[test]
    fn placed_on_a_second_monitor_left_of_the_primary() {
        let work = ScreenRect {
            left: -2560,
            top: -200,
            right: 0,
            bottom: 1240,
        };
        let (x, y) = place(work, 300, 84, 1.5);
        assert_eq!(x, -2560 + (2560 - 300) / 2);
        assert_eq!(y, 1240 - 84 - 96);
    }

    #[test]
    fn never_above_a_tiny_work_area() {
        let work = ScreenRect {
            left: 0,
            top: 100,
            right: 300,
            bottom: 150,
        };
        assert_eq!(place(work, 200, 56, 1.0).1, 100);
    }
}
