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

/// Spacing scale: Tailwind's 4 px grid (0.5, 1, 1.5, 2, 3, 4, 5, 6, 8, 10).
pub mod space {
    use gpui::{px, Pixels};

    /// `gap-0.5`, the padding of a segmented control.
    pub const XXS: Pixels = px(2.0);
    pub const XS: Pixels = px(4.0);
    /// `py-1.5` / `mt-1.5`.
    pub const XS2: Pixels = px(6.0);
    pub const SM: Pixels = px(8.0);
    pub const MD: Pixels = px(12.0);
    pub const LG: Pixels = px(16.0);
    /// `px-5`, the recording pill buttons.
    pub const XL2: Pixels = px(20.0);
    pub const XL: Pixels = px(24.0);
    pub const XXL: Pixels = px(32.0);
    /// `pt-10`, the top of the meeting document.
    pub const XXXL: Pixels = px(40.0);
}

/// Type families and sizes, the Tailwind steps the Tauri app uses. The body
/// text is 15 px with 1.55 line height; `MD` is Tailwind's `text-base` (16).
pub mod typography {
    use gpui::{px, Pixels};

    /// User interface (Hanken Grotesk, bundled).
    pub const SANS: &str = "Hanken Grotesk";
    /// Headings and notes (Literata, bundled).
    pub const SERIF: &str = "Literata";

    /// `text-xs`.
    pub const XS: Pixels = px(12.0);
    /// `text-sm`: buttons, inputs, help text.
    pub const SM: Pixels = px(14.0);
    /// Body text (`body { font-size: 15px }`).
    pub const BASE: Pixels = px(15.0);
    /// `text-base`.
    pub const MD: Pixels = px(16.0);
    /// `text-lg`: block titles (h3).
    pub const LG: Pixels = px(18.0);
    /// `text-xl`: the wordmark, sub-section titles.
    pub const XL: Pixels = px(20.0);
    /// `text-2xl`: settings section titles.
    pub const XXL: Pixels = px(24.0);
    /// `text-3xl`: page titles.
    pub const XXXL: Pixels = px(30.0);
    /// `text-4xl`: the meeting title.
    pub const DISPLAY: Pixels = px(36.0);
    pub const LINE_HEIGHT: f32 = 1.55;
    /// Headings (Tailwind's `leading-tight`).
    pub const LINE_HEIGHT_TIGHT: f32 = 1.25;
}

/// Layout sizes of the shell, the controls and each page's content column.
pub mod layout {
    use gpui::{px, Pixels};

    pub const SIDEBAR_WIDTH: Pixels = px(256.0);
    /// The collapsed sidebar (`w-16`).
    pub const SIDEBAR_COLLAPSED_WIDTH: Pixels = px(64.0);
    pub const DEFAULT_WINDOW_WIDTH: Pixels = px(1100.0);
    /// As the Tauri window (`tauri.conf.json`).
    pub const DEFAULT_WINDOW_HEIGHT: Pixels = px(700.0);
    pub const MIN_WINDOW_WIDTH: Pixels = px(640.0);
    pub const MIN_WINDOW_HEIGHT: Pixels = px(420.0);

    /// Default button (`h-11`), also the minimum touch target.
    pub const CONTROL_HEIGHT: Pixels = px(44.0);
    /// Small button (`size="sm"`, `h-8`).
    pub const CONTROL_HEIGHT_SM: Pixels = px(32.0);
    /// Icon button (`size="icon"`, `h-9 w-9`).
    pub const ICON_BUTTON: Pixels = px(36.0);
    /// Icon button in list rows (`h-8 w-8`).
    pub const ICON_BUTTON_SM: Pixels = px(32.0);
    /// Text input (`h-9`) and segmented control items (`min-h-[36px]`).
    pub const INPUT_HEIGHT: Pixels = px(36.0);

    /// Home transcript (`max-w-[44rem]`).
    pub const TRANSCRIPT_MAX: Pixels = px(704.0);
    /// Meeting document (`max-w-[46rem]`).
    pub const DOCUMENT_MAX: Pixels = px(736.0);
    /// Settings frame (`max-w-6xl`).
    pub const SETTINGS_MAX: Pixels = px(1152.0);
    /// A settings section (`max-w-2xl`).
    pub const SETTINGS_SECTION_MAX: Pixels = px(672.0);
    /// The Diktering page (`max-w-3xl`).
    pub const DICTATION_MAX: Pixels = px(768.0);
    /// Onboarding and home hero text (`max-w-md`).
    pub const ONBOARDING_TEXT_MAX: Pixels = px(448.0);
    /// Error pages, such as the database failure (`max-w-xl`).
    pub const NOTICE_MAX: Pixels = px(576.0);
    /// Running text inside an empty state.
    pub const EMPTY_TEXT_MAX_WIDTH: Pixels = px(420.0);
}

/// Icon sizes beyond the component defaults (lucide icons, as in Tauri).
pub mod icon_size {
    use gpui::{px, Pixels};

    /// Icons in buttons and rows (`[&_svg]:size-4`).
    pub const SM: Pixels = px(16.0);
    /// Icons in the collapsed sidebar and the settings "Tilbage" link.
    pub const MD: Pixels = px(20.0);
    pub const EMPTY_STATE: Pixels = px(28.0);
    pub const ERROR: Pixels = px(32.0);
}

// ---- Job 2 (sidebar shell, docs/design-paritet.md): added constants ----

/// The sidebar of the Tauri app (`Sidebar/index.tsx`, `Logo.tsx`).
pub mod sidebar {
    use gpui::{px, Pixels};

    /// The logo mark next to the wordmark (`h-6 w-6`).
    pub const LOGO_MARK: Pixels = px(24.0);
    /// The logo mark in the collapsed column (`h-7 w-7`).
    pub const LOGO_MARK_COLLAPSED: Pixels = px(28.0);
    /// The round collapse handle on the right edge (`h-6 w-6`).
    pub const HANDLE: Pixels = px(24.0);
    /// The handle's distance from the top (`top-5`).
    pub const HANDLE_TOP: Pixels = px(20.0);
    /// The pulsing dot of "Optagelse i gang" (`h-2.5 w-2.5`).
    pub const RECORDING_DOT: Pixels = px(10.0);
    /// "Optagelse i gang" is drawn on a `recording` tint. Tauri uses 10 %
    /// (`bg-recording/10`), which leaves the dark-mode text at 4.42:1; 8 %
    /// keeps it above WCAG AA.
    pub const RECORDING_TINT: f32 = 0.08;
    /// The home recording pill sits 48 px above the window edge (`bottom-12`).
    pub const PILL_BOTTOM: Pixels = px(48.0);
    /// Space kept free under the home hero for the pill (`pb-32`).
    pub const HOME_BOTTOM_SPACE: Pixels = px(128.0);
    /// The pill's inner padding (`px-2.5 py-2`).
    pub const PILL_PADDING_X: Pixels = px(10.0);
    /// Line heights of `text-sm` (20) and `text-xs` (16) in the list rows.
    pub const SM_LINE: Pixels = px(20.0);
    pub const XS_LINE: Pixels = px(16.0);
    /// The clear button inside the search field.
    pub const SEARCH_CLEAR: Pixels = px(24.0);
    /// "Omdøb møde" (`sm:max-w-[425px]`) and "Bekræft sletning" (`max-w-md`).
    pub const RENAME_DIALOG_WIDTH: Pixels = px(425.0);
    pub const CONFIRM_DIALOG_WIDTH: Pixels = px(448.0);
}

// ---- end of Job 2 block ----

// ---- Job 3 (home and recording screen, docs/design-paritet.md) ----

/// The recording screen of the Tauri app (`RecordingControls.tsx`,
/// `RecordingStatusBar.tsx`, `VirtualizedTranscriptView.tsx`,
/// `StatusOverlays.tsx`).
pub mod recording {
    use gpui::{px, Pixels};

    /// The transcript column's padding (`pt-6`, `pb-32`, `px-6`).
    pub const TRANSCRIPT_TOP: Pixels = px(24.0);
    pub const TRANSCRIPT_BOTTOM: Pixels = px(128.0);
    pub const TRANSCRIPT_PADDING_X: Pixels = px(24.0);
    /// The state dot in the status bar (`h-2.5 w-2.5`).
    pub const STATUS_DOT: Pixels = px(10.0);
    /// The "Lytter …" dot (`w-2 h-2`).
    pub const LISTENING_DOT: Pixels = px(8.0);
    /// Transcript text: `text-base` with `leading-[1.65]`.
    pub const LINE_HEIGHT: f32 = 1.65;
    /// The timestamp sits 3 px lower to share the text's baseline (`mt-[3px]`).
    pub const TIMESTAMP_NUDGE: Pixels = px(3.0);
    /// The low-confidence underline (`underline-offset-4`, 1 px).
    pub const UNDERLINE: Pixels = px(1.0);
    /// The filled stop square: lucide `Square` (18 of 24 units) at 14 px.
    pub const STOP_SQUARE: Pixels = px(10.5);
    pub const STOP_SQUARE_RADIUS: Pixels = px(2.0);
    /// The level bars next to the pill buttons (`w-1`, `gap-1`, `mx-3`).
    pub const LEVEL_BAR_WIDTH: Pixels = px(4.0);
    /// Bar heights relative to the 44 px pill row (`58%`, `76%`, `58%`).
    pub const LEVEL_BAR_MAX: Pixels = px(44.0);
    /// Paused bars are 4 px dots at 60 % opacity.
    pub const LEVEL_BAR_PAUSED: Pixels = px(4.0);
    pub const LEVEL_PAUSED_OPACITY: f32 = 0.6;
    /// Notice tints: `bg-primary/10 border-primary/30` (information),
    /// `bg-warning/10 border-warning/40` (warning), `bg-destructive/10
    /// border-destructive/40` (start error).
    pub const NOTICE_TINT: f32 = 0.1;
    pub const INFO_BORDER: f32 = 0.3;
    pub const ALERT_BORDER: f32 = 0.4;
    /// The 32 px dismiss button of a notice (`h-8 w-8`).
    pub const DISMISS: Pixels = px(32.0);
    /// The saving overlays sit 16 px above the window edge (`bottom-4`).
    pub const OVERLAY_BOTTOM: Pixels = px(16.0);
}

// ---- end of Job 3 block ----

/// Tailwind's shadows (`shadow-sm`, `shadow`, `shadow-md`), the same in both
/// modes as in the Tauri app.
pub mod shadow {
    use gpui::{hsla, px, BoxShadow};

    fn black(alpha: f32) -> gpui::Hsla {
        hsla(0.0, 0.0, 0.0, alpha)
    }

    /// `shadow-sm`: outline and secondary buttons, inputs.
    pub fn sm() -> Vec<BoxShadow> {
        vec![BoxShadow::new(px(0.0), px(1.0), black(0.05)).blur_radius(px(2.0))]
    }

    /// `shadow`: the primary button.
    pub fn base() -> Vec<BoxShadow> {
        vec![
            BoxShadow::new(px(0.0), px(1.0), black(0.1)).blur_radius(px(3.0)),
            BoxShadow::new(px(0.0), px(1.0), black(0.1))
                .blur_radius(px(2.0))
                .spread_radius(px(-1.0)),
        ]
    }

    /// `shadow-md`: the recording pill and status toasts.
    pub fn md() -> Vec<BoxShadow> {
        vec![
            BoxShadow::new(px(0.0), px(4.0), black(0.1))
                .blur_radius(px(6.0))
                .spread_radius(px(-1.0)),
            BoxShadow::new(px(0.0), px(2.0), black(0.1))
                .blur_radius(px(4.0))
                .spread_radius(px(-2.0)),
        ]
    }
}

/// Corner radii: `rounded-sm` (kbd chips), `rounded-md` (controls,
/// `calc(var(--radius) - 2px)`), `rounded-lg` (cards, `--radius`) and
/// `rounded-full` (pills, switches, dots).
pub const RADIUS_SM: Pixels = px(4.0);
pub const RADIUS: Pixels = px(6.0);
pub const RADIUS_LG: Pixels = px(8.0);
pub const RADIUS_FULL: Pixels = px(9999.0);

/// Hover and pressed strengths, as the Tauri classes: `bg-primary/90`
/// (hover) and a pressed step at 80 %; secondary hovers at `/80`.
pub const HOVER_ALPHA: f32 = 0.9;
pub const PRESSED_ALPHA: f32 = 0.8;
/// Destructive buttons stop at the hover strength when pressed: at 0.8 the
/// dark-mode text would fall to 3.9:1, below WCAG AA.
pub const DANGER_PRESSED_ALPHA: f32 = 0.9;
pub const SECONDARY_HOVER_ALPHA: f32 = 0.8;
pub const SECONDARY_PRESSED_ALPHA: f32 = 0.7;
/// List rows hover at `bg-accent/60`.
pub const LIST_HOVER_ALPHA: f32 = 0.6;

/// `color` at `alpha` painted on `surface`, as CSS draws `bg-primary/90`
/// (blended in sRGB). The result is opaque.
pub fn over(color: Hsla, surface: Hsla, alpha: f32) -> Hsla {
    let a = alpha.clamp(0.0, 1.0);
    let (c, s) = (gpui::Rgba::from(color), gpui::Rgba::from(surface));
    let mix = |x: f32, y: f32| x * a + y * (1.0 - a);
    Hsla::from(gpui::Rgba {
        r: mix(c.r, s.r),
        g: mix(c.g, s.g),
        b: mix(c.b, s.b),
        a: 1.0,
    })
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
    let bg = p.background;
    let hover = |color: Hsla| over(color, bg, HOVER_ALPHA);
    let pressed = |color: Hsla| over(color, bg, PRESSED_ALPHA);
    Theme::update(cx, |theme| {
        theme.font_family = typography::SANS.into();
        theme.font_size = typography::BASE;
        theme.radius = RADIUS;
        theme.radius_lg = RADIUS_LG;
        theme.shadow = true;

        let c = &mut theme.colors;
        c.background = p.background;
        c.foreground = p.foreground;
        c.border = p.border;
        c.input = p.input;
        c.ring = p.ring;
        c.caret = p.foreground;
        c.selection = p.primary.opacity(0.22);
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
        c.primary_hover = hover(p.primary);
        c.primary_active = pressed(p.primary);
        c.secondary = p.secondary;
        c.secondary_foreground = p.secondary_foreground;
        c.secondary_hover = over(p.secondary, bg, SECONDARY_HOVER_ALPHA);
        c.secondary_active = over(p.secondary, bg, SECONDARY_PRESSED_ALPHA);
        c.danger = p.destructive;
        c.danger_foreground = p.destructive_foreground;
        c.danger_hover = hover(p.destructive);
        c.danger_active = over(p.destructive, bg, DANGER_PRESSED_ALPHA);
        c.success = p.success;
        c.success_foreground = p.success_foreground;
        c.success_hover = hover(p.success);
        c.success_active = pressed(p.success);
        c.warning = p.warning;
        c.warning_foreground = p.warning_foreground;
        c.warning_hover = hover(p.warning);
        c.warning_active = pressed(p.warning);
        c.info = p.primary;
        c.info_foreground = p.primary_foreground;
        c.info_hover = hover(p.primary);
        c.info_active = pressed(p.primary);
        c.link = p.primary;
        c.link_hover = hover(p.primary);
        c.link_active = pressed(p.primary);

        // gpui-component's default button is Tauri's `outline` variant:
        // `input` border, background fill, hover `bg-accent`.
        c.button = p.background;
        c.button_foreground = p.foreground;
        c.button_hover = p.accent;
        c.button_active = p.accent;
        c.button_primary = p.primary;
        c.button_primary_foreground = p.primary_foreground;
        c.button_primary_hover = hover(p.primary);
        c.button_primary_active = pressed(p.primary);
        c.button_secondary = p.secondary;
        c.button_secondary_foreground = p.secondary_foreground;
        c.button_secondary_hover = over(p.secondary, bg, SECONDARY_HOVER_ALPHA);
        c.button_secondary_active = over(p.secondary, bg, SECONDARY_PRESSED_ALPHA);
        c.button_danger = p.destructive;
        c.button_danger_foreground = p.destructive_foreground;
        c.button_danger_hover = hover(p.destructive);
        c.button_danger_active = over(p.destructive, bg, DANGER_PRESSED_ALPHA);
        c.button_success = p.success;
        c.button_success_foreground = p.success_foreground;
        c.button_success_hover = hover(p.success);
        c.button_success_active = pressed(p.success);
        c.button_warning = p.warning;
        c.button_warning_foreground = p.warning_foreground;
        c.button_warning_hover = hover(p.warning);
        c.button_warning_active = pressed(p.warning);

        // Switch: `primary` when on, `muted` when off, thumb in the background color.
        c.switch = p.muted;
        c.switch_thumb = p.background;

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
        c.list_hover = over(p.accent, p.background, LIST_HOVER_ALPHA);
        c.list_active = p.accent;
        // The active meeting row is `bg-accent` without a border.
        c.list_active_border = p.accent;
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
            // Job 2: the sidebar is a card; rows hover at accent/60 and the
            // active row is accent, with the muted time line on both.
            ("muted text on accent", p.muted_foreground, p.accent),
            (
                "muted text on sidebar row hover",
                p.muted_foreground,
                over(p.accent, p.card, LIST_HOVER_ALPHA),
            ),
            (
                "recording text on the recording tint over card",
                p.recording,
                over(p.recording, p.card, sidebar::RECORDING_TINT),
            ),
            // Job 3: the recording screen. Device notices and the start
            // error are tints over the background; the live line is secondary.
            (
                "foreground on the information notice",
                p.foreground,
                over(p.primary, p.background, recording::NOTICE_TINT),
            ),
            (
                "foreground on the warning notice",
                p.foreground,
                over(p.warning, p.background, recording::NOTICE_TINT),
            ),
            (
                "foreground on the start error",
                p.foreground,
                over(p.destructive, p.background, recording::NOTICE_TINT),
            ),
            (
                "destructive title on the start error",
                p.destructive,
                over(p.destructive, p.background, recording::NOTICE_TINT),
            ),
            (
                "muted dismiss on the information notice",
                p.muted_foreground,
                over(p.primary, p.background, recording::NOTICE_TINT),
            ),
            (
                "muted dismiss on the warning notice",
                p.muted_foreground,
                over(p.warning, p.background, recording::NOTICE_TINT),
            ),
            ("foreground on the live line", p.foreground, p.secondary),
            ("stop pill", p.background, p.foreground),
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

    /// Every filled button keeps AA text contrast while hovered and pressed.
    #[test]
    fn hover_states_stay_readable() {
        for (mode, p) in [("lys", &Palette::LIGHT), ("mørk", &Palette::DARK)] {
            let fills = [
                ("primary", p.primary_foreground, p.primary, HOVER_ALPHA),
                (
                    "destructive",
                    p.destructive_foreground,
                    p.destructive,
                    HOVER_ALPHA,
                ),
                ("success", p.success_foreground, p.success, HOVER_ALPHA),
                ("warning", p.warning_foreground, p.warning, HOVER_ALPHA),
                (
                    "secondary",
                    p.secondary_foreground,
                    p.secondary,
                    SECONDARY_HOVER_ALPHA,
                ),
            ];
            for (name, fg, fill, hover) in fills {
                let pressed = match name {
                    "secondary" => SECONDARY_PRESSED_ALPHA,
                    "destructive" => DANGER_PRESSED_ALPHA,
                    _ => PRESSED_ALPHA,
                };
                for alpha in [hover, pressed] {
                    let ratio = contrast(fg, over(fill, p.background, alpha));
                    assert!(
                        ratio >= 4.5,
                        "{mode}: {name} at {alpha} has contrast {ratio:.2}, below 4.5"
                    );
                }
            }
            // Outline and ghost buttons hover on `accent`, list rows on accent/60.
            let list_hover = over(p.accent, p.background, LIST_HOVER_ALPHA);
            for (name, bg) in [("accent", p.accent), ("list hover", list_hover)] {
                let ratio = contrast(p.foreground, bg);
                assert!(ratio >= 4.5, "{mode}: text on {name} has {ratio:.2}");
            }
        }
    }

    #[test]
    fn over_blends_onto_the_surface() {
        let color = Palette::LIGHT.primary;
        let surface = Palette::LIGHT.background;
        let rgb = |c: Hsla| gpui::Rgba::from(c);
        let close = |a: Hsla, b: Hsla| {
            let (a, b) = (rgb(a), rgb(b));
            (a.r - b.r).abs() < 1e-3 && (a.g - b.g).abs() < 1e-3 && (a.b - b.b).abs() < 1e-3
        };
        assert!(close(over(color, surface, 1.0), color));
        assert!(close(over(color, surface, 0.0), surface));
        // Half way, each channel is the mean (CSS composites in sRGB).
        let half = rgb(over(color, surface, 0.5));
        let (c, s) = (rgb(color), rgb(surface));
        assert!((half.r - (c.r + s.r) / 2.0).abs() < 1e-3);
        assert!((half.b - (c.b + s.b) / 2.0).abs() < 1e-3);
        assert_eq!(half.a, 1.0);
        // Hover moves a dark fill towards a light surface and a light fill
        // towards a dark one, so it never disappears into the surface.
        assert!(over(color, surface, HOVER_ALPHA).l > color.l);
        let dark = Palette::DARK;
        assert!(over(dark.primary, dark.background, HOVER_ALPHA).l < dark.primary.l);
        // Out-of-range alphas are clamped.
        assert!(close(over(color, surface, 2.0), color));
    }

    #[test]
    fn type_scale_follows_tailwind() {
        use typography::*;
        let sizes = [XS, SM, BASE, MD, LG, XL, XXL, XXXL, DISPLAY].map(f32::from);
        assert_eq!(
            sizes,
            [12.0, 14.0, 15.0, 16.0, 18.0, 20.0, 24.0, 30.0, 36.0]
        );
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
