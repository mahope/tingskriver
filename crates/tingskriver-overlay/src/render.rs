//! Draws a [`Frame`] into a premultiplied RGBA bitmap with tiny-skia.
//!
//! The Windows layer copies the bitmap into a DIB (as BGRA) and hands it to
//! `UpdateLayeredWindow`; the demo can save it as PNG on any OS.

use tiny_skia::{FillRule, LineCap, Paint, PathBuilder, Pixmap, Stroke, Transform};

use crate::layout::Layout;
use crate::model::{Frame, Indicator};
use crate::text::Font;
use crate::theme::{self, size};

/// Turns frames into bitmaps. Holds the font and its glyph scaler.
#[derive(Debug)]
pub struct Renderer {
    font: Option<Font>,
}

impl Renderer {
    /// A renderer with the system UI font. Without a font the pill is drawn
    /// without its label (logged), never fails.
    pub fn new() -> Self {
        let font = Font::system();
        if font.is_none() {
            log::warn!("Overlay: no UI font found, the pill is drawn without text");
        }
        Self { font }
    }

    pub fn with_font(font: Option<Font>) -> Self {
        Self { font }
    }

    /// Label as drawn (cut to fit) and its width in logical pixels.
    fn label(&self, text: &str) -> (String, f32) {
        match &self.font {
            Some(font) => {
                let fitted = font.fit(text, size::FONT, size::TEXT_MAX);
                let w = font.measure(&fitted, size::FONT).ceil();
                (fitted, w)
            }
            None => (String::new(), 0.0),
        }
    }

    /// The layout `frame` gets at `scale`.
    pub fn layout(&self, frame: &Frame, scale: f32) -> Layout {
        let (_, text_w) = self.label(&frame.look.label);
        Layout::new(scale, text_w, frame.look.bars)
    }

    /// Draw `frame` at `scale`. Opacity (fade-out) is not baked in; the
    /// window applies it, so the returned bitmap is always fully opaque.
    pub fn render(&mut self, frame: &Frame, scale: f32) -> (Pixmap, Layout) {
        let (label, text_w) = self.label(&frame.look.label);
        let layout = Layout::new(scale, text_w, frame.look.bars);
        let mut pixmap =
            Pixmap::new(layout.width.max(1), layout.height.max(1)).expect("pill bitmap has a size");
        let s = layout.scale;

        // Soft shadow: a few growing, fading rounded rectangles.
        for i in (1..=3).rev() {
            let grow = i as f32 * 2.0 * s;
            let alpha = theme::SHADOW_ALPHA / (i as f32 * 2.0);
            fill_capsule(
                &mut pixmap,
                layout.pill_x - grow / 2.0,
                layout.pill_y - grow / 2.0 + s,
                layout.pill_w + grow,
                layout.pill_h + grow,
                tiny_skia::Color::from_rgba(0.0, 0.0, 0.0, alpha)
                    .unwrap_or(tiny_skia::Color::BLACK),
            );
        }

        // Border, then the card inset by the border width.
        let border = (size::BORDER * s).max(1.0);
        fill_capsule(
            &mut pixmap,
            layout.pill_x,
            layout.pill_y,
            layout.pill_w,
            layout.pill_h,
            theme::BORDER.color(),
        );
        fill_capsule(
            &mut pixmap,
            layout.pill_x + border,
            layout.pill_y + border,
            layout.pill_w - 2.0 * border,
            layout.pill_h - 2.0 * border,
            theme::CARD.with_alpha(theme::CARD_ALPHA),
        );

        let cx = layout.indicator_cx;
        let cy = layout.center_y;
        match frame.look.indicator {
            Indicator::Dot(token) => {
                fill_circle(&mut pixmap, cx, cy, size::DOT * s / 2.0, token.color())
            }
            Indicator::Spinner => draw_spinner(&mut pixmap, cx, cy, s, frame.spinner),
            Indicator::Check => draw_check(&mut pixmap, cx, cy, s),
        }

        if let Some(bars_x) = layout.bars_x {
            let mut paint = Paint::default();
            paint.set_color(theme::CARD_FOREGROUND.color());
            paint.anti_alias = true;
            for (i, h) in frame.bars.iter().enumerate() {
                let x = bars_x + i as f32 * (size::BAR_WIDTH + size::BAR_GAP) * s;
                let h = h * s;
                let w = size::BAR_WIDTH * s;
                if let Some(path) = capsule_path(x, cy - h / 2.0, w, h) {
                    pixmap.fill_path(
                        &path,
                        &paint,
                        FillRule::Winding,
                        Transform::identity(),
                        None,
                    );
                }
            }
        }

        if let Some(font) = &mut self.font {
            let (ascent, descent) = font.vertical_metrics(layout.font_px);
            // Centre the ink box of a typical lowercase-and-capitals line.
            let baseline = cy + (ascent - descent) / 2.0 - ascent * 0.08;
            font.draw(
                &mut pixmap,
                &label,
                layout.text_x,
                baseline,
                layout.font_px,
                theme::CARD_FOREGROUND.color(),
            );
        }
        (pixmap, layout)
    }
}

impl Default for Renderer {
    fn default() -> Self {
        Self::new()
    }
}

/// Rounded rectangle with fully round ends.
fn capsule_path(x: f32, y: f32, w: f32, h: f32) -> Option<tiny_skia::Path> {
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    let r = (w.min(h)) / 2.0;
    // Cubic approximation of a quarter circle.
    let k = 0.552_284_8 * r;
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.cubic_to(x + w - r + k, y, x + w, y + r - k, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.cubic_to(x + w, y + h - r + k, x + w - r + k, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.cubic_to(x + r - k, y + h, x, y + h - r + k, x, y + h - r);
    pb.line_to(x, y + r);
    pb.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    pb.close();
    pb.finish()
}

fn fill_capsule(pixmap: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, color: tiny_skia::Color) {
    let Some(path) = capsule_path(x, y, w, h) else {
        return;
    };
    let mut paint = Paint::default();
    paint.set_color(color);
    paint.anti_alias = true;
    pixmap.fill_path(
        &path,
        &paint,
        FillRule::Winding,
        Transform::identity(),
        None,
    );
}

fn fill_circle(pixmap: &mut Pixmap, cx: f32, cy: f32, r: f32, color: tiny_skia::Color) {
    let Some(path) = PathBuilder::from_circle(cx, cy, r) else {
        return;
    };
    let mut paint = Paint::default();
    paint.set_color(color);
    paint.anti_alias = true;
    pixmap.fill_path(
        &path,
        &paint,
        FillRule::Winding,
        Transform::identity(),
        None,
    );
}

/// A faint ring with a bright quarter arc at `turn` (0.0..1.0).
fn draw_spinner(pixmap: &mut Pixmap, cx: f32, cy: f32, s: f32, turn: f32) {
    let stroke = Stroke {
        width: size::SPINNER_STROKE * s,
        line_cap: LineCap::Round,
        ..Stroke::default()
    };
    let r = (size::INDICATOR - size::SPINNER_STROKE) * s / 2.0;
    let mut paint = Paint::default();
    paint.anti_alias = true;

    if let Some(ring) = PathBuilder::from_circle(cx, cy, r) {
        paint.set_color(theme::MUTED_FOREGROUND.with_alpha(0.4));
        pixmap.stroke_path(&ring, &paint, &stroke, Transform::identity(), None);
    }

    let start = turn * std::f32::consts::TAU - std::f32::consts::FRAC_PI_2;
    let sweep = std::f32::consts::FRAC_PI_2;
    let steps = 12;
    let mut pb = PathBuilder::new();
    for i in 0..=steps {
        let a = start + sweep * i as f32 / steps as f32;
        let (px, py) = (cx + r * a.cos(), cy + r * a.sin());
        if i == 0 {
            pb.move_to(px, py);
        } else {
            pb.line_to(px, py);
        }
    }
    if let Some(arc) = pb.finish() {
        paint.set_color(theme::CARD_FOREGROUND.color());
        pixmap.stroke_path(&arc, &paint, &stroke, Transform::identity(), None);
    }
}

fn draw_check(pixmap: &mut Pixmap, cx: f32, cy: f32, s: f32) {
    let u = size::INDICATOR * s / 2.0;
    let mut pb = PathBuilder::new();
    pb.move_to(cx - u * 0.8, cy + u * 0.05);
    pb.line_to(cx - u * 0.25, cy + u * 0.6);
    pb.line_to(cx + u * 0.85, cy - u * 0.55);
    let Some(path) = pb.finish() else { return };
    let mut paint = Paint::default();
    paint.anti_alias = true;
    paint.set_color(theme::SUCCESS.color());
    let stroke = Stroke {
        width: size::CHECK_STROKE * s,
        line_cap: LineCap::Round,
        line_join: tiny_skia::LineJoin::Round,
        ..Stroke::default()
    };
    pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
}

/// Copy a premultiplied RGBA pixmap into premultiplied BGRA rows, the
/// layout a 32-bit top-down DIB for `UpdateLayeredWindow` expects.
pub fn to_bgra(pixmap: &Pixmap, out: &mut [u8]) {
    for (src, dst) in pixmap.data().chunks_exact(4).zip(out.chunks_exact_mut(4)) {
        dst[0] = src[2];
        dst[1] = src[1];
        dst[2] = src[0];
        dst[3] = src[3];
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Pill, PillState};
    use std::time::Instant;

    fn frame(state: PillState) -> Frame {
        let t = Instant::now();
        let mut pill = Pill::new(t);
        pill.set_state(state, t);
        pill.frame(t).unwrap()
    }

    fn pixel(p: &Pixmap, x: u32, y: u32) -> [u8; 4] {
        let c = p.pixel(x, y).unwrap();
        [c.red(), c.green(), c.blue(), c.alpha()]
    }

    #[test]
    fn corners_are_transparent_and_the_pill_is_opaque() {
        let mut r = Renderer::new();
        for scale in [1.0, 1.5, 2.0] {
            let (p, l) = r.render(
                &frame(PillState::Listening {
                    level: 0.5,
                    hands_free: false,
                }),
                scale,
            );
            assert_eq!((p.width(), p.height()), (l.width, l.height));
            assert_eq!(pixel(&p, 0, 0)[3], 0);
            assert_eq!(pixel(&p, l.width - 1, 0)[3], 0);
            // Inside the card, between the dot and the bars.
            let x = (l.indicator_cx + 8.0 * scale) as u32;
            let y = (l.pill_y + 3.0 * scale) as u32;
            assert_eq!(pixel(&p, x, y)[3], 255, "scale {scale}");
        }
    }

    #[test]
    fn output_stays_premultiplied() {
        let mut r = Renderer::new();
        for state in [
            PillState::Processing,
            PillState::Done,
            PillState::Error("Ingen mikrofon fundet".into()),
            PillState::Cancelled,
        ] {
            let (p, _) = r.render(&frame(state), 1.5);
            for px in p.data().chunks(4) {
                assert!(px[0] <= px[3] && px[1] <= px[3] && px[2] <= px[3]);
            }
        }
    }

    #[test]
    fn the_dot_uses_the_state_colour() {
        let mut r = Renderer::new();
        let (p, l) = r.render(&frame(PillState::Error("x".into())), 1.0);
        let c = pixel(&p, l.indicator_cx as u32, l.center_y as u32);
        let want = theme::DESTRUCTIVE.color().to_color_u8();
        assert!((c[0] as i32 - want.red() as i32).abs() <= 2, "{c:?}");
        assert!((c[1] as i32 - want.green() as i32).abs() <= 2, "{c:?}");
    }

    #[test]
    fn louder_level_inks_more_bar_pixels() {
        let mut r = Renderer::with_font(None);
        let mut quiet = frame(PillState::Listening {
            level: 0.0,
            hands_free: false,
        });
        let (q, l) = r.render(&quiet, 1.0);
        quiet.bars = [size::BAR_MAX; size::BAR_COUNT];
        let (loud, _) = r.render(&quiet, 1.0);
        let fg = theme::CARD_FOREGROUND.color().to_color_u8();
        let count = |p: &Pixmap| {
            let x0 = l.bars_x.unwrap() as u32;
            (x0..x0 + 40)
                .flat_map(|x| (0..l.height).map(move |y| (x, y)))
                .filter(|&(x, y)| pixel(p, x, y)[0] >= fg.red() - 10)
                .count()
        };
        assert!(count(&loud) > count(&q) * 3);
    }

    #[test]
    fn width_follows_the_label_without_font_too() {
        let mut r = Renderer::with_font(None);
        let (p, _) = r.render(&frame(PillState::Done), 1.0);
        assert!(p.width() > 0);
    }

    #[test]
    fn bgra_swaps_red_and_blue() {
        let mut p = Pixmap::new(1, 1).unwrap();
        p.fill(tiny_skia::Color::from_rgba8(10, 20, 30, 255));
        let mut out = [0u8; 4];
        to_bgra(&p, &mut out);
        assert_eq!(out, [30, 20, 10, 255]);
    }
}
