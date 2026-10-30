//! Label text: font loading, measuring and drawing into a pixmap.
//!
//! Glyphs are rasterised with swash (hinted, grayscale) and blended into the
//! premultiplied tiny-skia pixmap. Labels are a word or two, so there is no
//! shaping beyond the font's advances.

use std::path::{Path, PathBuf};

use swash::scale::{Render, ScaleContext, Source};
use swash::zeno::{Format, Vector};
use swash::FontRef;
use tiny_skia::{Color, Pixmap};

const ELLIPSIS: char = '…';

/// A font file read into memory.
pub struct Font {
    data: Vec<u8>,
    scale: ScaleContext,
}

impl std::fmt::Debug for Font {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Font")
            .field("bytes", &self.data.len())
            .finish()
    }
}

impl Font {
    /// Use `data` as the font. `None` when it is not a font swash can read.
    pub fn from_bytes(data: Vec<u8>) -> Option<Self> {
        FontRef::from_index(&data, 0)?;
        Some(Self {
            data,
            scale: ScaleContext::new(),
        })
    }

    pub fn from_file(path: &Path) -> Option<Self> {
        Self::from_bytes(std::fs::read(path).ok()?)
    }

    /// The UI font: Segoe UI Semibold on Windows (the webview pill used
    /// weight 500), a common sans on other systems for the demo and tests.
    pub fn system() -> Option<Self> {
        candidates().iter().find_map(|path| Self::from_file(path))
    }

    fn font(&self) -> FontRef<'_> {
        // Checked in from_bytes.
        FontRef::from_index(&self.data, 0).expect("font was validated on load")
    }

    /// Advance width of `text` at `px` pixels per em.
    pub fn measure(&self, text: &str, px: f32) -> f32 {
        let font = self.font();
        let charmap = font.charmap();
        let metrics = font.glyph_metrics(&[]).scale(px);
        text.chars()
            .map(|c| metrics.advance_width(charmap.map(c)))
            .sum()
    }

    /// Ascent and descent at `px`, both positive.
    pub fn vertical_metrics(&self, px: f32) -> (f32, f32) {
        let m = self.font().metrics(&[]).scale(px);
        (m.ascent, m.descent.abs())
    }

    /// `text` shortened with an ellipsis so it fits in `max` pixels.
    pub fn fit(&self, text: &str, px: f32, max: f32) -> String {
        if self.measure(text, px) <= max {
            return text.to_string();
        }
        let room = max - self.measure(&ELLIPSIS.to_string(), px);
        let mut out = String::new();
        let mut width = 0.0;
        for c in text.chars() {
            let w = self.measure(&c.to_string(), px);
            if width + w > room {
                break;
            }
            width += w;
            out.push(c);
        }
        let mut out = out.trim_end().to_string();
        out.push(ELLIPSIS);
        out
    }

    /// Draw `text` with its baseline starting at (`x`, `baseline`).
    pub fn draw(
        &mut self,
        pixmap: &mut Pixmap,
        text: &str,
        x: f32,
        baseline: f32,
        px: f32,
        color: Color,
    ) {
        let font = FontRef::from_index(&self.data, 0).expect("font was validated on load");
        let charmap = font.charmap();
        let metrics = font.glyph_metrics(&[]).scale(px);
        let mut scaler = self.scale.builder(font).size(px).hint(true).build();
        let mut pen = x;
        for c in text.chars() {
            let glyph = charmap.map(c);
            let origin_x = pen.floor();
            let fraction = pen - origin_x;
            let image = Render::new(&[Source::Outline])
                .format(Format::Alpha)
                .offset(Vector::new(fraction, 0.0))
                .render(&mut scaler, glyph);
            if let Some(image) = image {
                let left = origin_x as i32 + image.placement.left;
                let top = baseline.round() as i32 - image.placement.top;
                blend_mask(
                    pixmap,
                    &image.data,
                    image.placement.width,
                    image.placement.height,
                    left,
                    top,
                    color,
                );
            }
            pen += metrics.advance_width(glyph);
        }
    }
}

fn candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if cfg!(windows) {
        let windir = std::env::var_os("WINDIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
        let fonts = windir.join("Fonts");
        out.push(fonts.join("seguisb.ttf"));
        out.push(fonts.join("segoeui.ttf"));
    } else {
        for path in [
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            "/usr/share/fonts/TTF/DejaVuSans.ttf",
            "/usr/share/fonts/truetype/freefont/FreeSans.ttf",
            "/System/Library/Fonts/SFNS.ttf",
            "/System/Library/Fonts/Helvetica.ttc",
        ] {
            out.push(PathBuf::from(path));
        }
    }
    out
}

/// Blend an 8-bit coverage mask of `color` onto a premultiplied pixmap.
pub(crate) fn blend_mask(
    pixmap: &mut Pixmap,
    mask: &[u8],
    w: u32,
    h: u32,
    left: i32,
    top: i32,
    color: Color,
) {
    let pw = pixmap.width() as i32;
    let ph = pixmap.height() as i32;
    let c = color.premultiply().to_color_u8();
    let src = [
        c.red() as u32,
        c.green() as u32,
        c.blue() as u32,
        c.alpha() as u32,
    ];
    let data = pixmap.data_mut();
    for row in 0..h as i32 {
        let y = top + row;
        if y < 0 || y >= ph {
            continue;
        }
        for col in 0..w as i32 {
            let x = left + col;
            if x < 0 || x >= pw {
                continue;
            }
            let coverage = mask[(row * w as i32 + col) as usize] as u32;
            if coverage == 0 {
                continue;
            }
            let i = ((y * pw + x) * 4) as usize;
            let sa = div255(src[3] * coverage);
            for k in 0..4 {
                let s = div255(src[k] * coverage);
                let d = data[i + k] as u32;
                data[i + k] = (s + div255(d * (255 - sa))).min(255) as u8;
            }
        }
    }
}

fn div255(v: u32) -> u32 {
    (v + 128 + ((v + 128) >> 8)) >> 8
}

#[cfg(test)]
mod tests {
    use super::*;

    fn font() -> Option<Font> {
        let font = Font::system();
        if font.is_none() {
            eprintln!("no system font found, skipping");
        }
        font
    }

    #[test]
    fn rejects_non_fonts() {
        assert!(Font::from_bytes(b"not a font".to_vec()).is_none());
    }

    #[test]
    fn measures_wider_text_wider() {
        let Some(font) = font() else { return };
        let short = font.measure("Lytter", 14.0);
        let long = font.measure("Kunne ikke skrive", 14.0);
        assert!(short > 20.0 && long > short);
        // Twice the size is twice as wide (advances scale linearly).
        let double = font.measure("Lytter", 28.0);
        assert!((double - 2.0 * short).abs() < 0.5);
    }

    #[test]
    fn danish_letters_have_glyphs() {
        let Some(font) = font() else { return };
        let charmap = font.font().charmap();
        for c in ['æ', 'ø', 'å', 'Æ', 'Ø', 'Å', '…'] {
            assert_ne!(charmap.map(c), 0, "{c} has no glyph");
        }
    }

    #[test]
    fn fit_cuts_with_an_ellipsis() {
        let Some(font) = font() else { return };
        let text = "Modellen kunne ikke indlæses, fordi filen er beskadiget";
        assert_eq!(font.fit("Lytter", 14.0, 300.0), "Lytter");
        let cut = font.fit(text, 14.0, 120.0);
        assert!(cut.ends_with('…'));
        assert!(font.measure(&cut, 14.0) <= 120.0);
        assert!(cut.chars().count() < text.chars().count());
    }

    #[test]
    fn draw_puts_ink_inside_the_pixmap_only() {
        let Some(mut font) = font() else { return };
        let mut pixmap = Pixmap::new(80, 30).unwrap();
        font.draw(&mut pixmap, "Håndfri", 4.0, 20.0, 14.0, Color::WHITE);
        let inked = pixmap.data().chunks(4).filter(|p| p[3] > 0).count();
        assert!(inked > 30, "text drew {inked} pixels");
        // Drawing far outside must not panic or draw anything.
        let mut empty = Pixmap::new(10, 10).unwrap();
        font.draw(&mut empty, "Lytter", 200.0, -50.0, 14.0, Color::WHITE);
        assert!(empty.data().iter().all(|&b| b == 0));
    }

    #[test]
    fn blend_keeps_pixels_premultiplied() {
        let mut pixmap = Pixmap::new(2, 1).unwrap();
        pixmap.fill(Color::from_rgba8(10, 20, 30, 128));
        let color = Color::from_rgba8(255, 200, 100, 255);
        blend_mask(&mut pixmap, &[255, 100], 2, 1, 0, 0, color);
        for p in pixmap.data().chunks(4) {
            assert!(p[0] <= p[3] && p[1] <= p[3] && p[2] <= p[3], "{p:?}");
        }
        assert_eq!(&pixmap.data()[0..4], &[255, 200, 100, 255]);
    }
}
