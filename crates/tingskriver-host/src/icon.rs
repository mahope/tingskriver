//! The tray icon: the app icon with a coloured dot for the status.
//!
//! Colours are the pill's design tokens (`tingskriver_overlay::theme`), so
//! the tray and the pill show recording in the same red.

use tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, Transform};
use tingskriver_overlay::theme;

use crate::status::IconState;

/// Side of the tray icon in pixels. Windows scales it to the tray size.
pub const SIZE: u32 = 32;

const APP_ICON: &[u8] = include_bytes!("../assets/tray-32.png");

/// Dot radius and centre, in pixels of a [`SIZE`] icon.
const DOT_RADIUS: f32 = 7.0;
const DOT_RING: f32 = 2.0;
const PROBLEM_RING: f32 = 2.5;
const DOT_CENTRE: (f32, f32) = (SIZE as f32 - DOT_RADIUS - 1.0, SIZE as f32 - DOT_RADIUS - 1.0);

/// The dot colour for a state; `None` draws no dot.
pub fn dot_color(state: IconState) -> Option<Color> {
    match state {
        IconState::Idle => None,
        IconState::Recording => Some(theme::RECORDING.color()),
        IconState::Paused => Some(theme::MUTED_FOREGROUND.color()),
        IconState::Problem => Some(theme::DESTRUCTIVE.color()),
    }
}

/// The icon for `state` as straight (not premultiplied) RGBA, row by row.
pub fn rgba(state: IconState) -> Vec<u8> {
    let mut pixmap = Pixmap::decode_png(APP_ICON)
        .ok()
        .filter(|p| p.width() == SIZE && p.height() == SIZE)
        .unwrap_or_else(|| {
            let mut p = Pixmap::new(SIZE, SIZE).expect("non-zero icon size");
            p.fill(theme::CARD.color());
            p
        });
    if let Some(color) = dot_color(state) {
        let (cx, cy) = DOT_CENTRE;
        // A ring in the card colour keeps the dot readable on the icon.
        fill_circle(&mut pixmap, cx, cy, DOT_RADIUS + DOT_RING, theme::CARD.color());
        fill_circle(&mut pixmap, cx, cy, DOT_RADIUS, color);
        // A problem is a hollow ring, so it never reads as "recording".
        if state == IconState::Problem {
            fill_circle(&mut pixmap, cx, cy, DOT_RADIUS - PROBLEM_RING, theme::CARD.color());
        }
    }
    pixmap
        .pixels()
        .iter()
        .flat_map(|p| {
            let c = p.demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
        .collect()
}

fn fill_circle(pixmap: &mut Pixmap, cx: f32, cy: f32, r: f32, color: Color) {
    let Some(path) = PathBuilder::from_circle(cx, cy, r) else {
        return;
    };
    let mut paint = Paint::default();
    paint.set_color(color);
    paint.anti_alias = true;
    pixmap.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(rgba: &[u8], x: u32, y: u32) -> [u8; 4] {
        let i = ((y * SIZE + x) * 4) as usize;
        [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]
    }

    fn color_u8(c: Color) -> [u8; 4] {
        let c = c.to_color_u8();
        [c.red(), c.green(), c.blue(), c.alpha()]
    }

    #[test]
    fn app_icon_decodes_at_tray_size() {
        let p = Pixmap::decode_png(APP_ICON).unwrap();
        assert_eq!((p.width(), p.height()), (SIZE, SIZE));
    }

    #[test]
    fn every_state_has_a_full_icon() {
        for state in [IconState::Idle, IconState::Recording, IconState::Paused, IconState::Problem] {
            assert_eq!(rgba(state).len(), (SIZE * SIZE * 4) as usize);
        }
    }

    #[test]
    fn recording_draws_the_recording_token_in_the_dot() {
        let (cx, cy) = (DOT_CENTRE.0 as u32, DOT_CENTRE.1 as u32);
        let idle = rgba(IconState::Idle);
        let recording = rgba(IconState::Recording);
        let paused = rgba(IconState::Paused);
        assert_eq!(pixel(&recording, cx, cy), color_u8(theme::RECORDING.color()));
        assert_eq!(pixel(&paused, cx, cy), color_u8(theme::MUTED_FOREGROUND.color()));
        assert_eq!(pixel(&rgba(IconState::Problem), cx, cy), color_u8(theme::CARD.color()));
        assert_ne!(pixel(&idle, cx, cy), pixel(&recording, cx, cy));
        // Away from the dot the icon is unchanged.
        assert_eq!(pixel(&idle, 6, 6), pixel(&recording, 6, 6));
        assert_eq!(dot_color(IconState::Idle), None);
    }
}
