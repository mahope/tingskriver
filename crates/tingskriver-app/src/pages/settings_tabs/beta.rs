//! Indstillinger > Beta, as in the Tauri app (`components/BetaSettings.tsx`,
//! texts from `types/betaFeatures.ts`): the warning banner, one card per
//! feature and the info box. The GPUI voiceprint explanation is a second
//! card (docs/design-paritet.md 2.5: an off-by-default experiment).
//!
//! The import switch is Tauri's `importAndRetranscribe` (on by default). It
//! hides and shows "Importér lyd" (`import`). Voiceprints have no switch
//! yet; the card says they are off.

use gpui::{div, prelude::*, AnyElement, App, Context, FontWeight};
use gpui_component::{h_flex, v_flex, Disableable};
use gpui_kit_assets::IconName;
use tingskriver_core::strings::{self, settings_tabs as text};

use super::{card_title, settings_card, tinted_box};
use crate::controls;
use crate::pages::settings::tab_column;
use crate::theme::{self, settings_tabs as size, space, typography};
use crate::ui::{muted, palette, Shell};

impl Shell {
    pub(crate) fn beta_settings_tab(&self, cx: &mut Context<Self>) -> AnyElement {
        let palette = palette(cx);
        let import_on = self.import.read(cx).audio_enabled();
        let import_view = self.import.downgrade();
        let can_save = self.model.read(cx).host.is_some();

        let banner = tinted_box(
            "beta-banner",
            palette.warning,
            Some(IconName::CircleAlert),
            v_flex()
                .child(
                    div()
                        .font_weight(FontWeight::MEDIUM)
                        .child(text::BETA_BANNER_TITLE),
                )
                .child(div().mt(space::XS).child(text::BETA_BANNER))
                .into_any_element(),
            cx,
        );

        let import = settings_card(
            "beta-import",
            vec![h_flex()
                .justify_between()
                .gap(space::XL)
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap(space::SM)
                        .child(feature_title(text::BETA_IMPORT, cx))
                        .child(muted(text::BETA_IMPORT_HELP, cx)),
                )
                .child(
                    div().flex_none().child(
                        controls::switch("beta-import-switch")
                            .checked(import_on)
                            .disabled(!can_save)
                            .accessibility_label(text::BETA_IMPORT)
                            .on_click(move |value, _, cx| {
                                let on = *value;
                                let _ = import_view.update(cx, |view, cx| view.save_beta(on, cx));
                            }),
                    ),
                )
                .into_any_element()],
            cx,
        );

        let voiceprint = settings_card(
            "beta-voiceprint",
            vec![
                feature_title(strings::VOICEPRINT_TITLE, cx),
                v_flex()
                    .mt(space::SM)
                    .gap(space::SM)
                    .children(voiceprint_lines().iter().map(|line| muted(line, cx)))
                    .into_any_element(),
            ],
            cx,
        );

        let info = tinted_box(
            "beta-info",
            palette.primary,
            None,
            div().child(text::BETA_INFO).into_any_element(),
            cx,
        );

        // Tauri: `space-y-6`.
        tab_column(vec![v_flex()
            .id("settings-beta")
            .role(gpui::Role::Group)
            .gap(space::XL)
            .child(banner)
            .child(import)
            .child(voiceprint)
            .child(info)
            .into_any_element()])
    }
}

/// A feature's title row: flask icon, `h3 text-lg font-semibold` and the
/// "Beta" badge (`bg-warning/15 text-warning rounded-full text-xs`).
fn feature_title(title: &str, cx: &App) -> AnyElement {
    let palette = palette(cx);
    h_flex()
        .gap(space::SM)
        .child(card_title(title, Some(IconName::FlaskConical), cx))
        .child(
            div()
                .px(space::SM)
                .py(size::BADGE_PADDING_Y)
                .rounded(theme::RADIUS_FULL)
                .bg(theme::over(palette.warning, palette.card, size::BADGE_TINT))
                .text_color(palette.warning)
                .text_size(typography::XS)
                .font_weight(FontWeight::MEDIUM)
                .child(text::BETA_BADGE),
        )
        .into_any_element()
}

/// The voiceprint explanation, Danish first and English below on the same
/// page (moved from the Generelt-era settings page). Voiceprints are off by
/// default and there is no switch yet, so the status line says off.
fn voiceprint_lines() -> Vec<String> {
    use tingskriver_core::voiceprint::texts;
    let paragraphs = texts::explanation();
    let mut lines = vec![
        strings::VOICEPRINT_STATUS_OFF.to_string(),
        strings::VOICEPRINT_NO_SWITCH.to_string(),
    ];
    lines.extend(paragraphs.iter().map(|p| p.da.to_string()));
    lines.push(texts::EN_TITLE.to_string());
    lines.push(texts::EN_STATUS_OFF.to_string());
    lines.extend(paragraphs.iter().map(|p| p.en.to_string()));
    lines
}
