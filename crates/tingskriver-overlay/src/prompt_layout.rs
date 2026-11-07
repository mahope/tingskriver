//! Geometry of the prompt window in physical pixels, hit-testing and where
//! the window goes on screen. Pure, so it is tested at 100, 150 and 200 %
//! and on low work areas on any OS.
//!
//! Two shapes, from the widths and heights used by notification popups:
//!
//! - **Compact** (no body text): one row, the title on the left and the
//!   buttons on the right, 64 logical pixels high.
//! - **Expanded** (with body text): the title, the body (up to
//!   [`MAX_BODY_LINES`] lines), then a row of buttons.
//!
//! Sizes are logical pixels (100 %); everything is multiplied by the scale
//! and rounded to whole pixels. The window never gets taller than the work
//! area: body lines are dropped first, then the body, then the height is
//! clamped.

use crate::layout::ScreenRect;

/// Logical sizes.
pub mod size {
    pub const WIDTH: f32 = 360.0;
    pub const COMPACT_HEIGHT: f32 = 64.0;
    pub const PADDING: f32 = 12.0;
    pub const TITLE_HEIGHT: f32 = 20.0;
    pub const BODY_LINE: f32 = 18.0;
    pub const BODY_GAP: f32 = 4.0;
    pub const BUTTON_HEIGHT: f32 = 32.0;
    pub const BUTTON_WIDTH: f32 = 112.0;
    pub const BUTTON_GAP: f32 = 8.0;
    /// Least width the title keeps in the compact row.
    pub const MIN_TITLE: f32 = 120.0;
    /// Gap between the text block and the button row (expanded).
    pub const ROW_GAP: f32 = 10.0;
    /// Distance from the work area's right and bottom edge.
    pub const MARGIN: f32 = 16.0;
}

/// Longest body shown before it is cut.
pub const MAX_BODY_LINES: u32 = 4;

/// A rectangle in the window's own pixels (origin top left).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PxRect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl PxRect {
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }
}

/// What is under a point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Button(usize),
    /// On the prompt, but not on a button.
    Surface,
    Outside,
}

/// Positions inside the prompt window, in physical pixels.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PromptLayout {
    pub scale_percent: u32,
    pub width: i32,
    pub height: i32,
    pub title: PxRect,
    /// `None` in the compact shape.
    pub body: Option<PxRect>,
    /// Left to right; the first (primary) button is the leftmost.
    pub buttons: Vec<PxRect>,
    pub compact: bool,
}

fn px(logical: f32, scale: f32) -> i32 {
    (logical * scale).round() as i32
}

impl PromptLayout {
    /// Lay out a prompt with `body_lines` lines of body text (0 = none) and
    /// `buttons` buttons on a monitor whose work area is `work_height` pixels
    /// high, at `scale` (1.0 = 100 %).
    pub fn new(scale: f32, body_lines: u32, buttons: usize, work_height: i32) -> Self {
        let scale = if scale.is_finite() && scale > 0.0 {
            scale
        } else {
            1.0
        };
        let available = (work_height - 2 * px(size::MARGIN, scale)).max(1);
        let mut lines = body_lines.min(MAX_BODY_LINES);
        let layout = loop {
            let layout = Self::build(scale, lines, buttons);
            if layout.height <= available || (lines == 0 && layout.compact) {
                break layout;
            }
            // Too tall: fewer body lines, then none (compact).
            lines = lines.saturating_sub(1);
        };
        layout.clamp_height(available)
    }

    fn build(scale: f32, body_lines: u32, buttons: usize) -> Self {
        let width = px(size::WIDTH, scale);
        let pad = px(size::PADDING, scale);
        let button_h = px(size::BUTTON_HEIGHT, scale);
        let gap = px(size::BUTTON_GAP, scale);
        let n = buttons as i32;
        let body_lines_zero = body_lines == 0;
        // Buttons shrink to fit: in the compact row the title keeps a minimum.
        let reserved = if body_lines_zero {
            px(size::MIN_TITLE, scale) + gap
        } else {
            0
        };
        let room = width - 2 * pad - reserved - (n - 1).max(0) * gap;
        let button_w = px(size::BUTTON_WIDTH, scale).min(room / n.max(1)).max(1);
        let buttons_w = n * button_w + (n - 1).max(0) * gap;
        let title_h = px(size::TITLE_HEIGHT, scale);
        let compact = body_lines == 0;

        let (height, title, body, row_y, row_x) = if compact {
            let height = px(size::COMPACT_HEIGHT, scale);
            let row_x = width - pad - buttons_w;
            let title = PxRect {
                x: pad,
                y: (height - title_h) / 2,
                w: (row_x - gap - pad).max(0),
                h: title_h,
            };
            (height, title, None, (height - button_h) / 2, row_x)
        } else {
            let line_h = px(size::BODY_LINE, scale);
            let body_gap = px(size::BODY_GAP, scale);
            let row_gap = px(size::ROW_GAP, scale);
            let title = PxRect {
                x: pad,
                y: pad,
                w: width - 2 * pad,
                h: title_h,
            };
            let body = PxRect {
                x: pad,
                y: pad + title_h + body_gap,
                w: width - 2 * pad,
                h: body_lines as i32 * line_h,
            };
            let row_y = body.y + body.h + row_gap;
            let height = row_y + button_h + pad;
            (height, title, Some(body), row_y, width - pad - buttons_w)
        };

        let buttons = (0..n)
            .map(|i| PxRect {
                x: row_x + i * (button_w + gap),
                y: row_y,
                w: button_w,
                h: button_h,
            })
            .collect();
        Self {
            scale_percent: (scale * 100.0).round() as u32,
            width,
            height,
            title,
            body,
            buttons,
            compact,
        }
    }

    /// Never taller than `available`. Only reached when even the compact
    /// shape does not fit (a work area a few rows high); the buttons are
    /// then clipped, which beats a window that covers the taskbar.
    fn clamp_height(mut self, available: i32) -> Self {
        if self.height > available {
            self.height = available;
        }
        self
    }

    /// What is at (`x`, `y`), in window pixels.
    pub fn hit_test(&self, x: i32, y: i32) -> Hit {
        if let Some(i) = self.buttons.iter().position(|b| b.contains(x, y)) {
            return Hit::Button(i);
        }
        if (PxRect {
            x: 0,
            y: 0,
            w: self.width,
            h: self.height,
        })
        .contains(x, y)
        {
            Hit::Surface
        } else {
            Hit::Outside
        }
    }
}

/// Width in physical pixels available to body text, for measuring it.
pub fn text_width(scale: f32) -> i32 {
    px(size::WIDTH - 2.0 * size::PADDING, scale)
}

/// Top-left corner for a `width` x `height` window at the bottom right of
/// `work` (the monitor's work area, so clear of the taskbar). Away from the
/// dictation pill, which sits at the bottom centre. Never outside `work`.
pub fn place(work: ScreenRect, width: i32, height: i32, scale: f32) -> (i32, i32) {
    let margin = px(size::MARGIN, scale);
    let x = (work.right - width - margin).max(work.left);
    let y = (work.bottom - height - margin).max(work.top);
    (x, y)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCALES: [f32; 3] = [1.0, 1.5, 2.0];
    const TALL: i32 = 4000;

    fn work(w: i32, h: i32) -> ScreenRect {
        ScreenRect {
            left: 0,
            top: 0,
            right: w,
            bottom: h,
        }
    }

    #[test]
    fn compact_is_360_by_64_at_100_percent() {
        let l = PromptLayout::new(1.0, 0, 2, TALL);
        assert!(l.compact);
        assert_eq!((l.width, l.height), (360, 64));
        assert!(l.body.is_none());
    }

    #[test]
    fn expanded_height_follows_the_body_lines() {
        let one = PromptLayout::new(1.0, 1, 2, TALL);
        let three = PromptLayout::new(1.0, 3, 2, TALL);
        assert!(!one.compact);
        assert_eq!(three.height - one.height, 2 * 18);
        // 12 + 20 + 4 + 18 + 10 + 32 + 12
        assert_eq!(one.height, 108);
    }

    #[test]
    fn everything_scales_with_dpi() {
        for lines in [0, 2] {
            let one = PromptLayout::new(1.0, lines, 2, TALL);
            for scale in SCALES {
                let l = PromptLayout::new(scale, lines, 2, TALL);
                assert_eq!(l.width, (360.0 * scale) as i32, "{scale}");
                assert_eq!(l.scale_percent, (scale * 100.0) as u32);
                for (a, b) in l.buttons.iter().zip(&one.buttons) {
                    assert_eq!(a.w, (b.w as f32 * scale).round() as i32, "{scale}");
                    assert_eq!(a.h, (b.h as f32 * scale).round() as i32, "{scale}");
                }
                assert_eq!(l.height, (one.height as f32 * scale).round() as i32);
            }
        }
    }

    #[test]
    fn contents_stay_inside_the_window_at_every_scale() {
        for scale in [1.0, 1.25, 1.5, 1.75, 2.0] {
            for lines in 0..=MAX_BODY_LINES {
                for buttons in 1..=3 {
                    let l = PromptLayout::new(scale, lines, buttons, TALL);
                    let rects = std::iter::once(l.title)
                        .chain(l.body)
                        .chain(l.buttons.iter().copied());
                    for r in rects {
                        assert!(r.x >= 0 && r.y >= 0, "{r:?} at {scale}");
                        assert!(r.x + r.w <= l.width, "{r:?} at {scale}");
                        assert!(r.y + r.h <= l.height, "{r:?} at {scale}");
                    }
                }
            }
        }
    }

    #[test]
    fn buttons_do_not_overlap_and_the_primary_is_leftmost() {
        for scale in SCALES {
            let l = PromptLayout::new(scale, 2, 3, TALL);
            for pair in l.buttons.windows(2) {
                assert!(pair[0].x + pair[0].w < pair[1].x, "gap at {scale}");
            }
            let last = l.buttons.last().unwrap();
            assert!(last.x + last.w < l.width);
        }
    }

    #[test]
    fn compact_title_does_not_run_into_the_buttons() {
        for scale in SCALES {
            let l = PromptLayout::new(scale, 0, 2, TALL);
            assert!(l.title.x + l.title.w < l.buttons[0].x, "{scale}");
        }
    }

    #[test]
    fn long_bodies_are_capped() {
        let l = PromptLayout::new(1.0, 50, 2, TALL);
        assert_eq!(l.body.unwrap().h, MAX_BODY_LINES as i32 * 18);
    }

    #[test]
    fn low_work_area_drops_body_lines_then_the_body() {
        // 1366x768-ish laptop at 200 %: 384 logical rows of work area... and
        // much lower ones from a huge taskbar or a small virtual screen.
        for scale in SCALES {
            for work_h in [100, 160, 220, 300, 400, 768] {
                let l = PromptLayout::new(scale, 4, 2, work_h);
                let limit = (work_h - 2 * (16.0 * scale).round() as i32).max(1);
                assert!(
                    l.height <= limit,
                    "height {} over {limit} (work {work_h}, scale {scale})",
                    l.height
                );
            }
        }
        // Enough room: nothing is dropped.
        assert_eq!(PromptLayout::new(1.0, 4, 2, 768).body.unwrap().h, 4 * 18);
        // Room for two lines only: 2 lines of the four are kept.
        let two_lines = PromptLayout::new(1.0, 2, 2, TALL).height + 32;
        assert_eq!(
            PromptLayout::new(1.0, 4, 2, two_lines).body.unwrap().h,
            2 * 18
        );
        // Very little room: compact.
        assert!(PromptLayout::new(1.0, 4, 2, 110).compact);
    }

    #[test]
    fn a_work_area_smaller_than_the_window_is_clamped() {
        let l = PromptLayout::new(2.0, 0, 2, 100);
        assert!(l.height <= 100 - 64);
    }

    #[test]
    fn bad_scale_falls_back_to_100_percent() {
        assert_eq!(
            PromptLayout::new(0.0, 1, 2, TALL),
            PromptLayout::new(1.0, 1, 2, TALL)
        );
        assert_eq!(
            PromptLayout::new(f32::NAN, 1, 2, TALL),
            PromptLayout::new(1.0, 1, 2, TALL)
        );
    }

    #[test]
    fn hit_test_finds_buttons_surface_and_outside() {
        for scale in SCALES {
            let l = PromptLayout::new(scale, 1, 2, TALL);
            for (i, b) in l.buttons.iter().enumerate() {
                assert_eq!(l.hit_test(b.x, b.y), Hit::Button(i));
                assert_eq!(l.hit_test(b.x + b.w - 1, b.y + b.h - 1), Hit::Button(i));
                // One pixel past the edge is not the button.
                assert_ne!(l.hit_test(b.x + b.w, b.y), Hit::Button(i));
                assert_ne!(l.hit_test(b.x - 1, b.y), Hit::Button(i));
            }
            assert_eq!(l.hit_test(l.title.x, l.title.y), Hit::Surface);
            assert_eq!(l.hit_test(0, 0), Hit::Surface);
            assert_eq!(l.hit_test(-1, 0), Hit::Outside);
            assert_eq!(l.hit_test(l.width, 0), Hit::Outside);
            assert_eq!(l.hit_test(0, l.height), Hit::Outside);
        }
    }

    #[test]
    fn placed_at_the_bottom_right_of_the_work_area() {
        // 1920x1080 with a 40 px taskbar at 100 %.
        let w = work(1920, 1040);
        assert_eq!(place(w, 360, 64, 1.0), (1920 - 360 - 16, 1040 - 64 - 16));
        // At 200 % the margin doubles.
        assert_eq!(place(w, 720, 128, 2.0), (1920 - 720 - 32, 1040 - 128 - 32));
    }

    #[test]
    fn placed_on_a_second_monitor_left_of_the_primary() {
        let w = ScreenRect {
            left: -2560,
            top: -200,
            right: 0,
            bottom: 1240,
        };
        assert_eq!(place(w, 540, 96, 1.5), (-540 - 24, 1240 - 96 - 24));
    }

    #[test]
    fn never_outside_a_small_work_area() {
        let w = ScreenRect {
            left: 50,
            top: 100,
            right: 300,
            bottom: 150,
        };
        assert_eq!(place(w, 360, 64, 1.0), (50, 100));
    }
}
