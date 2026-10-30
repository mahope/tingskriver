//! Design tokens from the 0.5.0-dev.2 design (`frontend/src/app/globals.css`)
//! and how they reach gpui-component's theme.
//!
//! Every color, spacing and type size in the app comes from here. The light
//! surface is warm paper with an ink-blue accent, the dark one a cool slate.
//! The theme follows the system appearance and changes live.

use gpui::{px, App, Hsla, Pixels, Window, WindowAppearance};
use gpui_component::{Theme, ThemeMode};

/// `hsl(h s% l%)` as in the CSS tokens.
const fn hsl(h: f32, s: f32, l: f32) -> Hsla {
    gpui::hsla(h / 360.0, s / 100.0, l / 100.0, 1.0)
}

/// The semantic colors of one appearance, named as in the CSS tokens.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    pub background: Hsla,
    pub foreground: Hsla,
    pub card: Hsla,
    pub card_foreground: Hsla,
    pub popover: Hsla,
    pub popover_foreground: Hsla,
    pub primary: Hsla,
    pub primary_foreground: Hsla,
    pub secondary: Hsla,
    pub secondary_foreground: Hsla,
    pub muted: Hsla,
    pub muted_foreground: Hsla,
    pub accent: Hsla,
    pub accent_foreground: Hsla,
    pub destructive: Hsla,
    pub destructive_foreground: Hsla,
    pub success: Hsla,
    pub success_foreground: Hsla,
    pub warning: Hsla,
    pub warning_foreground: Hsla,
    pub recording: Hsla,
    pub border: Hsla,
    pub input: Hsla,
    pub ring: Hsla,
    pub scrollbar: Hsla,
}

impl Palette {
    pub const LIGHT: Palette = Palette {
        background: hsl(40.0, 14.0, 96.0),
        foreground: hsl(222.0, 24.0, 13.0),
        card: hsl(40.0, 25.0, 99.0),
        card_foreground: hsl(222.0, 24.0, 13.0),
        popover: hsl(40.0, 25.0, 99.0),
        popover_foreground: hsl(222.0, 24.0, 13.0),
        primary: hsl(216.0, 58.0, 30.0),
        primary_foreground: hsl(40.0, 25.0, 99.0),
        secondary: hsl(40.0, 10.0, 92.0),
        secondary_foreground: hsl(222.0, 24.0, 13.0),
        muted: hsl(40.0, 10.0, 92.0),
        muted_foreground: hsl(222.0, 9.0, 36.0),
        accent: hsl(40.0, 12.0, 89.0),
        accent_foreground: hsl(222.0, 24.0, 13.0),
        destructive: hsl(0.0, 62.0, 40.0),
        destructive_foreground: hsl(40.0, 25.0, 99.0),
        success: hsl(150.0, 50.0, 24.0),
        success_foreground: hsl(40.0, 25.0, 99.0),
        warning: hsl(32.0, 85.0, 26.0),
        warning_foreground: hsl(40.0, 25.0, 99.0),
        recording: hsl(4.0, 68.0, 42.0),
        border: hsl(40.0, 9.0, 85.0),
        input: hsl(40.0, 6.0, 62.0),
        ring: hsl(216.0, 70.0, 45.0),
        scrollbar: hsl(40.0, 6.0, 72.0),
    };

    pub const DARK: Palette = Palette {
        background: hsl(222.0, 16.0, 9.0),
        foreground: hsl(40.0, 12.0, 91.0),
        card: hsl(222.0, 15.0, 12.0),
        card_foreground: hsl(40.0, 12.0, 91.0),
        popover: hsl(222.0, 15.0, 13.0),
        popover_foreground: hsl(40.0, 12.0, 91.0),
        primary: hsl(214.0, 75.0, 72.0),
        primary_foreground: hsl(222.0, 30.0, 9.0),
        secondary: hsl(222.0, 13.0, 16.0),
        secondary_foreground: hsl(40.0, 12.0, 91.0),
        muted: hsl(222.0, 13.0, 16.0),
        muted_foreground: hsl(222.0, 8.0, 67.0),
        accent: hsl(222.0, 13.0, 20.0),
        accent_foreground: hsl(40.0, 12.0, 91.0),
        destructive: hsl(4.0, 75.0, 62.0),
        destructive_foreground: hsl(222.0, 30.0, 9.0),
        success: hsl(150.0, 45.0, 56.0),
        success_foreground: hsl(222.0, 30.0, 9.0),
        warning: hsl(38.0, 85.0, 62.0),
        warning_foreground: hsl(222.0, 30.0, 9.0),
        recording: hsl(4.0, 75.0, 62.0),
        border: hsl(222.0, 12.0, 22.0),
        input: hsl(222.0, 10.0, 40.0),
        ring: hsl(214.0, 80.0, 70.0),
        scrollbar: hsl(222.0, 10.0, 28.0),
    };

    pub fn for_mode(mode: ThemeMode) -> &'static Palette {
        if mode.is_dark() {
            &Self::DARK
        } else {
            &Self::LIGHT
        }
    }
}

/// Spacing scale (4 px steps, as Tailwind's 1, 2, 3, 4, 6, 8).
pub mod space {
    use gpui::{px, Pixels};

    pub const XS: Pixels = px(4.0);
    pub const SM: Pixels = px(8.0);
    pub const MD: Pixels = px(12.0);
    pub const LG: Pixels = px(16.0);
    pub const XL: Pixels = px(24.0);
    pub const XXL: Pixels = px(32.0);
}

/// Type families and sizes. The body text is 15 px with 1.55 line height.
pub mod typography {
    use gpui::{px, Pixels};

    /// User interface (Hanken Grotesk, bundled).
    pub const SANS: &str = "Hanken Grotesk";
    /// Headings and notes (Literata, bundled).
    pub const SERIF: &str = "Literata";

    pub const XS: Pixels = px(12.0);
    pub const SM: Pixels = px(13.0);
    pub const BASE: Pixels = px(15.0);
    pub const LG: Pixels = px(17.0);
    pub const XL: Pixels = px(20.0);
    pub const XXL: Pixels = px(26.0);
    pub const LINE_HEIGHT: f32 = 1.55;
}

/// Layout sizes of the shell.
pub mod layout {
    use gpui::{px, Pixels};

    pub const SIDEBAR_WIDTH: Pixels = px(256.0);
    pub const CONTENT_MAX_WIDTH: Pixels = px(760.0);
    pub const DEFAULT_WINDOW_WIDTH: Pixels = px(1100.0);
    pub const DEFAULT_WINDOW_HEIGHT: Pixels = px(720.0);
    pub const MIN_WINDOW_WIDTH: Pixels = px(640.0);
    pub const MIN_WINDOW_HEIGHT: Pixels = px(420.0);
}

/// Corner radius of controls (`calc(var(--radius) - 2px)`) and cards (`--radius`).
pub const RADIUS: Pixels = px(6.0);
pub const RADIUS_LG: Pixels = px(8.0);

/// A slightly stronger version of `color` for hover (`step` 1) and pressed
/// (`step` 2) states: darker on light surfaces, lighter on dark ones.
pub fn emphasize(color: Hsla, dark: bool, step: u8) -> Hsla {
    let delta = 0.05 * f32::from(step);
    let l = if dark {
        (color.l + delta).min(1.0)
    } else {
        (color.l - delta).max(0.0)
    };
    Hsla { l, ..color }
}

/// `TINGSKRIVER_APPEARANCE=light` or `dark` pins the appearance instead of
/// following the system (manual tests and screenshots).
pub const APPEARANCE_ENV: &str = "TINGSKRIVER_APPEARANCE";

/// The mode `TINGSKRIVER_APPEARANCE` asks for, if any.
pub fn forced_mode(value: Option<&str>) -> Option<ThemeMode> {
    match value?.trim().to_lowercase().as_str() {
        "light" | "lys" => Some(ThemeMode::Light),
        "dark" | "mørk" => Some(ThemeMode::Dark),
        _ => None,
    }
}

/// The theme mode for a system appearance, unless one is forced.
pub fn mode_for(appearance: WindowAppearance) -> ThemeMode {
    forced_mode(std::env::var(APPEARANCE_ENV).ok().as_deref())
        .unwrap_or_else(|| ThemeMode::from(appearance))
}

/// Load gpui-component's theme for `mode` and put the Tingskriver tokens on
/// top of it.
pub fn apply(mode: ThemeMode, cx: &mut App) {
    Theme::change(mode, None, cx);
    let p = *Palette::for_mode(mode);
    let dark = mode.is_dark();
    Theme::update(cx, |theme| {
        theme.font_family = typography::SANS.into();
        theme.font_size = typography::BASE;
        theme.radius = RADIUS;
        theme.radius_lg = RADIUS_LG;
        theme.shadow = false;

        let c = &mut theme.colors;
        c.background = p.background;
        c.foreground = p.foreground;
        c.border = p.border;
        c.input = p.input;
        c.ring = p.ring;
        c.caret = p.foreground;
        c.selection = p.primary.opacity(0.25);
        c.accent = p.accent;
        c.accent_foreground = p.accent_foreground;
        c.muted = p.muted;
        c.muted_foreground = p.muted_foreground;
        c.popover = p.popover;
        c.popover_foreground = p.popover_foreground;
        c.overlay = p.foreground.opacity(0.4);
        c.window_border = p.border;
        c.drag_border = p.ring;

        c.primary = p.primary;
        c.primary_foreground = p.primary_foreground;
        c.primary_hover = emphasize(p.primary, dark, 1);
        c.primary_active = emphasize(p.primary, dark, 2);
        c.secondary = p.secondary;
        c.secondary_foreground = p.secondary_foreground;
        c.secondary_hover = emphasize(p.secondary, dark, 1);
        c.secondary_active = emphasize(p.secondary, dark, 2);
        c.danger = p.destructive;
        c.danger_foreground = p.destructive_foreground;
        c.danger_hover = emphasize(p.destructive, dark, 1);
        c.danger_active = emphasize(p.destructive, dark, 2);
        c.success = p.success;
        c.success_foreground = p.success_foreground;
        c.success_hover = emphasize(p.success, dark, 1);
        c.success_active = emphasize(p.success, dark, 2);
        c.warning = p.warning;
        c.warning_foreground = p.warning_foreground;
        c.warning_hover = emphasize(p.warning, dark, 1);
        c.warning_active = emphasize(p.warning, dark, 2);
        c.info = p.primary;
        c.info_foreground = p.primary_foreground;
        c.info_hover = emphasize(p.primary, dark, 1);
        c.info_active = emphasize(p.primary, dark, 2);
        c.link = p.primary;
        c.link_hover = emphasize(p.primary, dark, 1);
        c.link_active = emphasize(p.primary, dark, 2);

        c.button_primary = p.primary;
        c.button_primary_foreground = p.primary_foreground;
        c.button_primary_hover = emphasize(p.primary, dark, 1);
        c.button_primary_active = emphasize(p.primary, dark, 2);

        c.sidebar = p.card;
        c.sidebar_foreground = p.card_foreground;
        c.sidebar_border = p.border;
        c.sidebar_accent = p.accent;
        c.sidebar_accent_foreground = p.accent_foreground;
        c.sidebar_primary = p.primary;
        c.sidebar_primary_foreground = p.primary_foreground;
        c.title_bar = p.card;
        c.title_bar_border = p.border;
        c.list = p.background;
        c.list_hover = p.accent;
        c.list_active = p.accent;
        c.list_active_border = p.ring;
        c.scrollbar_thumb = p.scrollbar;
        c.scrollbar_thumb_hover = p.muted_foreground;
    });
}

/// Follow the window's (that is, the system's) appearance.
pub fn sync_with_window(window: &Window, cx: &mut App) {
    apply(mode_for(window.appearance()), cx);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WCAG 2 relative luminance.
    fn luminance(color: Hsla) -> f32 {
        let rgb = gpui::Rgba::from(color);
        let channel = |c: f32| {
            if c <= 0.039_28 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(rgb.r) + 0.7152 * channel(rgb.g) + 0.0722 * channel(rgb.b)
    }

    fn contrast(a: Hsla, b: Hsla) -> f32 {
        let (la, lb) = (luminance(a), luminance(b));
        (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
    }

    fn text_pairs(p: &Palette) -> Vec<(&'static str, Hsla, Hsla)> {
        vec![
            ("foreground/background", p.foreground, p.background),
            ("card", p.card_foreground, p.card),
            ("popover", p.popover_foreground, p.popover),
            ("muted text on background", p.muted_foreground, p.background),
            ("muted text on card", p.muted_foreground, p.card),
            ("primary", p.primary_foreground, p.primary),
            ("secondary", p.secondary_foreground, p.secondary),
            ("accent", p.accent_foreground, p.accent),
            ("destructive", p.destructive_foreground, p.destructive),
            ("success", p.success_foreground, p.success),
            ("warning", p.warning_foreground, p.warning),
            ("primary text on background", p.primary, p.background),
        ]
    }

    #[test]
    fn text_meets_wcag_aa_in_both_modes() {
        for (mode, palette) in [("lys", &Palette::LIGHT), ("mørk", &Palette::DARK)] {
            for (name, fg, bg) in text_pairs(palette) {
                let ratio = contrast(fg, bg);
                assert!(
                    ratio >= 4.5,
                    "{mode}: {name} has contrast {ratio:.2}, below 4.5"
                );
            }
        }
    }

    #[test]
    fn hover_states_stay_readable() {
        for (dark, p) in [(false, &Palette::LIGHT), (true, &Palette::DARK)] {
            for step in [1, 2] {
                let hover = emphasize(p.primary, dark, step);
                assert!(
                    contrast(p.primary_foreground, hover) >= 4.5,
                    "dark={dark} step={step}"
                );
            }
        }
    }

    #[test]
    fn emphasize_moves_away_from_the_surface() {
        let base = Palette::LIGHT.primary;
        assert!(emphasize(base, false, 1).l < base.l);
        assert!(emphasize(base, true, 1).l > base.l);
        assert!(emphasize(hsl(0.0, 0.0, 99.0), true, 2).l <= 1.0);
        assert!(emphasize(hsl(0.0, 0.0, 2.0), false, 2).l >= 0.0);
    }

    #[test]
    fn css_values_convert_to_gpui_ranges() {
        let c = hsl(216.0, 58.0, 30.0);
        assert!((c.h - 0.6).abs() < 1e-6);
        assert!((c.s - 0.58).abs() < 1e-6);
        assert!((c.l - 0.30).abs() < 1e-6);
    }

    #[test]
    fn appearance_maps_to_mode() {
        assert_eq!(ThemeMode::from(WindowAppearance::Dark), ThemeMode::Dark);
        assert_eq!(
            ThemeMode::from(WindowAppearance::VibrantDark),
            ThemeMode::Dark
        );
        assert_eq!(ThemeMode::from(WindowAppearance::Light), ThemeMode::Light);
        assert!(Palette::for_mode(ThemeMode::Dark) == &Palette::DARK);
    }

    #[test]
    fn appearance_can_be_forced() {
        assert_eq!(forced_mode(Some("dark")), Some(ThemeMode::Dark));
        assert_eq!(forced_mode(Some(" Mørk ")), Some(ThemeMode::Dark));
        assert_eq!(forced_mode(Some("light")), Some(ThemeMode::Light));
        assert_eq!(forced_mode(Some("system")), None);
        assert_eq!(forced_mode(None), None);
    }
}
