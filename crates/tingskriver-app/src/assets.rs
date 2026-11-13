//! Bundled fonts and icons.
//!
//! The fonts are static instances of the variable fonts the Tauri UI uses
//! (see `assets/fonts/OFL.txt`); GPUI loads TTF, not woff2. Icons come from
//! gpui-kit-assets: the component defaults plus the lucide icons of the
//! Tauri UI that the defaults lack, and the Tingskriver logo mark.

use std::borrow::Cow;

use gpui::{App, AssetSource, SharedString};

/// Every bundled font file, embedded in the binary.
pub const FONTS: &[(&str, &[u8])] = &[
    (
        "HankenGrotesk-Regular.ttf",
        include_bytes!("../assets/fonts/HankenGrotesk-Regular.ttf"),
    ),
    (
        "HankenGrotesk-Medium.ttf",
        include_bytes!("../assets/fonts/HankenGrotesk-Medium.ttf"),
    ),
    (
        "HankenGrotesk-SemiBold.ttf",
        include_bytes!("../assets/fonts/HankenGrotesk-SemiBold.ttf"),
    ),
    (
        "HankenGrotesk-Bold.ttf",
        include_bytes!("../assets/fonts/HankenGrotesk-Bold.ttf"),
    ),
    (
        "Literata-Regular.ttf",
        include_bytes!("../assets/fonts/Literata-Regular.ttf"),
    ),
    (
        "Literata-SemiBold.ttf",
        include_bytes!("../assets/fonts/Literata-SemiBold.ttf"),
    ),
    (
        "Literata-Italic.ttf",
        include_bytes!("../assets/fonts/Literata-Italic.ttf"),
    ),
];

/// Characters every bundled font must draw: Danish letters, the vowels that
/// dead keys produce on a Danish keyboard, and AltGr symbols.
pub const REQUIRED_CHARACTERS: &str =
    "æøåÆØÅ éèêëÉ áàâäÁ íìîï óòôöÓÖ úùûüÜ ñÑ ~^¨´` @£$€{[]}|\\ «»„“”–§½";

/// Register the bundled fonts with GPUI's text system.
pub fn register_fonts(cx: &mut App) {
    let fonts = FONTS
        .iter()
        .map(|(_, bytes)| Cow::Borrowed(*bytes))
        .collect();
    if let Err(error) = cx.text_system().add_fonts(fonts) {
        log::error!("Could not register the bundled fonts: {error:#}");
    }
}

gpui_kit_assets::icon_assets!(
    ShellIcons,
    [
        Mic,
        Calendar,
        Settings,
        Bot,
        CircleAlert,
        TriangleAlert,
        FolderOpen,
        RotateCcw,
        LoaderCircle,
        Inbox,
        // Lucide icons of the Tauri UI that gpui-component's defaults lack
        // (docs/design-paritet.md, Job 1). Lucide 1.x calls trash-2 `Trash`.
        AudioLines,
        Upload,
        X,
        Pencil,
        Trash,
        Sparkles,
        Sparkle,
        Languages,
        Database,
        FlaskConical,
        Monitor,
        Keyboard,
        CircleCheckBig,
        // Job 2: "Møder" in the collapsed sidebar.
        NotebookPen,
        // Job 9: the note under the onboarding download cards; Job 8: the
        // update dialog ("Opdatering klar").
        Download,
        // Job 4: the meeting document (Gem, empty summary, task items).
        Save,
        FileQuestionMark,
        SquareCheck,
        // Job 6: "Systemlyd" on the Optagelser settings tab.
        Speaker,
    ]
);

/// The Tingskriver logo mark (the quill with a writing line from
/// `frontend/public/tingskriver-mark.svg`), drawn in the text color.
pub const LOGO_MARK: &str = "icons/tingskriver-mark.svg";
const LOGO_MARK_SVG: &[u8] = include_bytes!("../assets/icons/tingskriver-mark.svg");

/// The icons of the shell plus gpui-component's default icons.
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        if path == LOGO_MARK {
            return Ok(Some(Cow::Borrowed(LOGO_MARK_SVG)));
        }
        if let Some(data) = ShellIcons.load(path)? {
            return Ok(Some(data));
        }
        gpui_kit_assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> gpui::Result<Vec<SharedString>> {
        let mut paths = ShellIcons.list(path)?;
        if LOGO_MARK.starts_with(path) {
            paths.push(LOGO_MARK.into());
        }
        paths.extend(gpui_kit_assets::Assets.list(path)?);
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_font_draws_danish_text() {
        for (name, bytes) in FONTS {
            let face = ttf_parser::Face::parse(bytes, 0)
                .unwrap_or_else(|e| panic!("{name} is not a valid font: {e}"));
            let missing: String = REQUIRED_CHARACTERS
                .chars()
                .filter(|c| !c.is_whitespace() && face.glyph_index(*c).is_none())
                .collect();
            assert!(missing.is_empty(), "{name} has no glyph for {missing:?}");
        }
    }

    #[test]
    fn fonts_are_static_and_named_for_one_family() {
        use ttf_parser::name_id;
        for (name, bytes) in FONTS {
            let face = ttf_parser::Face::parse(bytes, 0).unwrap();
            assert!(!face.is_variable(), "{name} is still a variable font");
            let family = face
                .names()
                .into_iter()
                .filter(|n| {
                    n.name_id == name_id::TYPOGRAPHIC_FAMILY || n.name_id == name_id::FAMILY
                })
                .filter_map(|n| n.to_string())
                .min_by_key(|s| s.len())
                .unwrap();
            let expected = if name.starts_with("Hanken") {
                crate::theme::typography::SANS
            } else {
                crate::theme::typography::SERIF
            };
            assert_eq!(family, expected, "{name}");
        }
    }

    #[test]
    fn weights_match_file_names() {
        for (name, bytes) in FONTS {
            let face = ttf_parser::Face::parse(bytes, 0).unwrap();
            let expected = match name.split(['-', '.']).nth(1).unwrap() {
                "Regular" | "Italic" => 400,
                "Medium" => 500,
                "SemiBold" => 600,
                "Bold" => 700,
                other => panic!("unexpected style {other}"),
            };
            assert_eq!(face.weight().to_number(), expected, "{name}");
            assert_eq!(face.is_italic(), name.contains("Italic"), "{name}");
        }
    }

    #[test]
    fn shell_icons_load_and_defaults_remain() {
        use gpui_kit_assets::IconNamed;
        let icon = gpui_kit_assets::IconName::Mic.path();
        assert!(Assets.load(&icon).unwrap().is_some());
        let default_icon = gpui_component::IconName::Check.path();
        assert!(Assets.load(&default_icon).unwrap().is_some());
    }

    /// Every icon the design port needs (docs/design-paritet.md, Job 1)
    /// loads, whether it comes from the defaults or from `ShellIcons`.
    #[test]
    fn design_icons_and_logo_mark_load() {
        use gpui_kit_assets::IconName;
        let icons = [
            IconName::AudioLines,
            IconName::FileText,
            IconName::Upload,
            IconName::Search,
            IconName::X,
            IconName::ChevronLeft,
            IconName::ChevronRight,
            IconName::Pencil,
            IconName::Trash,
            IconName::Copy,
            IconName::FolderOpen,
            IconName::RefreshCw,
            IconName::Sparkles,
            IconName::Sparkle,
            IconName::Languages,
            IconName::Square,
            IconName::Pause,
            IconName::ArrowLeft,
            IconName::Settings2,
            IconName::Database,
            IconName::FlaskConical,
            IconName::Monitor,
            IconName::Sun,
            IconName::Moon,
            IconName::Keyboard,
            IconName::Info,
            IconName::CircleCheckBig,
            IconName::Download,
            IconName::Speaker,
        ];
        for icon in icons {
            let path = icon.path();
            assert!(Assets.load(&path).unwrap().is_some(), "{path} is missing");
        }
        let mark = Assets.load(LOGO_MARK).unwrap().expect("logo mark");
        assert!(std::str::from_utf8(&mark).unwrap().contains("currentColor"));
        assert!(Assets
            .list("icons/")
            .unwrap()
            .iter()
            .any(|p| p == LOGO_MARK));
    }
}
