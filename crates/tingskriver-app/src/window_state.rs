//! Remembering where the main window was.
//!
//! The position is saved in logical pixels in the app data folder and only
//! restored when enough of the window lands on a connected display, so a
//! window saved on an unplugged monitor comes back centered instead of
//! off-screen.

use std::path::{Path, PathBuf};

use gpui::{point, px, size, Bounds, Pixels, WindowBounds};
use serde::{Deserialize, Serialize};

use crate::theme::layout;

/// File in the app data folder.
pub const FILE_NAME: &str = "gpui-window.json";

/// At least this much of the window must be on a display to restore it.
const MIN_VISIBLE_WIDTH: f32 = 120.0;
const MIN_VISIBLE_HEIGHT: f32 = 40.0;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SavedWindow {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    #[serde(default)]
    pub maximized: bool,
}

impl SavedWindow {
    /// The state to save for the window's current bounds. Fullscreen is
    /// saved as its restore size, not as fullscreen.
    pub fn from_window_bounds(bounds: WindowBounds) -> Self {
        let (b, maximized) = match bounds {
            WindowBounds::Windowed(b) => (b, false),
            WindowBounds::Maximized(b) => (b, true),
            WindowBounds::Fullscreen(b) => (b, false),
        };
        SavedWindow {
            x: b.origin.x.as_f32(),
            y: b.origin.y.as_f32(),
            width: b.size.width.as_f32(),
            height: b.size.height.as_f32(),
            maximized,
        }
    }

    fn bounds(&self) -> Bounds<Pixels> {
        Bounds {
            origin: point(px(self.x), px(self.y)),
            size: size(px(self.width), px(self.height)),
        }
    }

    fn is_sane(&self) -> bool {
        [self.x, self.y, self.width, self.height]
            .iter()
            .all(|v| v.is_finite())
            && self.width > 0.0
            && self.height > 0.0
    }
}

/// Where to open the window, given the saved state and the visible bounds
/// of the connected displays. `None` means "no usable saved position":
/// open centered on the primary display.
pub fn restore(saved: Option<SavedWindow>, displays: &[Bounds<Pixels>]) -> Option<WindowBounds> {
    let mut saved = saved.filter(SavedWindow::is_sane)?;
    saved.width = saved.width.max(layout::MIN_WINDOW_WIDTH.as_f32());
    saved.height = saved.height.max(layout::MIN_WINDOW_HEIGHT.as_f32());
    let wanted = saved.bounds();
    let display = displays
        .iter()
        .filter(|d| {
            let overlap = d.intersect(&wanted);
            overlap.size.width.as_f32() >= MIN_VISIBLE_WIDTH
                && overlap.size.height.as_f32() >= MIN_VISIBLE_HEIGHT
        })
        .max_by(|a, b| {
            let area = |d: &Bounds<Pixels>| {
                let o = d.intersect(&wanted);
                o.size.width.as_f32() * o.size.height.as_f32()
            };
            area(a).total_cmp(&area(b))
        })?;

    // Never larger than the display it lands on, never smaller than the
    // minimum size, and with the title bar inside the display.
    let width = saved.width.min(display.size.width.as_f32());
    let height = saved.height.min(display.size.height.as_f32());
    let min_y = display.origin.y.as_f32();
    let max_y = min_y + display.size.height.as_f32() - MIN_VISIBLE_HEIGHT;
    let y = saved.y.clamp(min_y, max_y.max(min_y));
    let bounds = Bounds {
        origin: point(px(saved.x), px(y)),
        size: size(px(width), px(height)),
    };
    Some(if saved.maximized {
        WindowBounds::Maximized(bounds)
    } else {
        WindowBounds::Windowed(bounds)
    })
}

pub fn path_in(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join(FILE_NAME)
}

/// The saved state, or `None` when the file is missing or unreadable (a
/// broken file only costs the position, never the start).
pub fn load(path: &Path) -> Option<SavedWindow> {
    let text = std::fs::read_to_string(path).ok()?;
    match serde_json::from_str(&text) {
        Ok(saved) => Some(saved),
        Err(error) => {
            log::warn!("Ignoring unreadable window state {}: {error}", path.display());
            None
        }
    }
}

/// Write the state atomically (temporary file, then rename).
pub fn save(path: &Path, state: &SavedWindow) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(state)?)?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f32, y: f32, w: f32, h: f32) -> Bounds<Pixels> {
        Bounds {
            origin: point(px(x), px(y)),
            size: size(px(w), px(h)),
        }
    }

    fn saved(x: f32, y: f32, w: f32, h: f32) -> SavedWindow {
        SavedWindow { x, y, width: w, height: h, maximized: false }
    }

    const PRIMARY: (f32, f32, f32, f32) = (0.0, 0.0, 1920.0, 1040.0);
    // A second monitor to the left with another scale: logical bounds.
    const LEFT: (f32, f32, f32, f32) = (-1280.0, 0.0, 1280.0, 680.0);

    fn displays() -> Vec<Bounds<Pixels>> {
        [PRIMARY, LEFT].iter().map(|&(x, y, w, h)| rect(x, y, w, h)).collect()
    }

    #[test]
    fn nothing_saved_opens_centered() {
        assert_eq!(restore(None, &displays()), None);
    }

    #[test]
    fn restores_a_window_on_a_connected_display() {
        let s = saved(100.0, 80.0, 1100.0, 720.0);
        assert_eq!(
            restore(Some(s), &displays()),
            Some(WindowBounds::Windowed(rect(100.0, 80.0, 1100.0, 720.0)))
        );
    }

    #[test]
    fn restores_on_the_second_monitor_and_fits_its_size() {
        let s = saved(-1200.0, 20.0, 1600.0, 900.0);
        assert_eq!(
            restore(Some(s), &displays()),
            Some(WindowBounds::Windowed(rect(-1200.0, 20.0, 1280.0, 680.0)))
        );
    }

    #[test]
    fn window_on_an_unplugged_monitor_is_centered_instead() {
        let s = saved(2400.0, 100.0, 1100.0, 720.0);
        assert_eq!(restore(Some(s), &displays()), None);
        let barely = saved(1920.0 - 60.0, 100.0, 1100.0, 720.0);
        assert_eq!(restore(Some(barely), &displays()), None);
    }

    #[test]
    fn title_bar_is_kept_on_screen() {
        let above = saved(100.0, -300.0, 1100.0, 720.0);
        let Some(WindowBounds::Windowed(b)) = restore(Some(above), &displays()) else {
            panic!("expected a windowed restore");
        };
        assert_eq!(b.origin.y, px(0.0));
    }

    #[test]
    fn tiny_or_broken_sizes_are_fixed() {
        let tiny = saved(10.0, 10.0, 50.0, 20.0);
        let Some(WindowBounds::Windowed(b)) = restore(Some(tiny), &displays()) else {
            panic!("expected a windowed restore");
        };
        assert_eq!(b.size, size(layout::MIN_WINDOW_WIDTH, layout::MIN_WINDOW_HEIGHT));
        assert_eq!(restore(Some(saved(f32::NAN, 0.0, 800.0, 600.0)), &displays()), None);
        assert_eq!(restore(Some(saved(0.0, 0.0, -5.0, 600.0)), &displays()), None);
    }

    #[test]
    fn maximized_is_restored_with_its_restore_size() {
        let s = SavedWindow { maximized: true, ..saved(100.0, 80.0, 1100.0, 720.0) };
        assert_eq!(
            restore(Some(s), &displays()),
            Some(WindowBounds::Maximized(rect(100.0, 80.0, 1100.0, 720.0)))
        );
        let from = SavedWindow::from_window_bounds(WindowBounds::Maximized(rect(1.0, 2.0, 3.0, 4.0)));
        assert!(from.maximized);
        let full = SavedWindow::from_window_bounds(WindowBounds::Fullscreen(rect(1.0, 2.0, 3.0, 4.0)));
        assert!(!full.maximized);
    }

    #[test]
    fn saves_and_loads_through_a_file() {
        let dir = std::env::temp_dir().join(format!("tingskriver-window-state-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = path_in(&dir);
        assert_eq!(load(&path), None);
        let s = saved(-1200.0, 20.0, 900.0, 600.0);
        save(&path, &s).unwrap();
        assert_eq!(load(&path), Some(s));
        std::fs::write(&path, "{ ikke json").unwrap();
        assert_eq!(load(&path), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
