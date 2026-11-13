//! Indstillinger > Opsummering, as in the Tauri app
//! (`components/SummaryModelSettings.tsx:125-148`,
//! `SummaryLanguageSettings.tsx:19-26`, the provider part of
//! `ModelSettingsModal.tsx:812-884`): three cards.
//!
//! Automatic summaries run when a recording stops, and this app does not
//! record yet; the language quick picks and choosing the model are not
//! ported. The cards are shown as in Tauri with disabled controls, the
//! saved model read-only, and the reason in each card.

use gpui::{div, prelude::*, AnyElement, Context};
use gpui_component::{h_flex, v_flex, Disableable};
use gpui_kit_assets::IconName;
use tingskriver_core::strings::settings_tabs as text;

use super::{card_title, disabled_select, load_saved_models, settings_card};
use crate::controls;
use crate::pages::settings::{field_label, tab_column};
use crate::theme::{space, typography};
use crate::ui::{muted, Shell};

impl Shell {
    pub(crate) fn summary_settings_tab(&self, cx: &mut Context<Self>) -> AnyElement {
        load_saved_models(self, cx);
        let saved_models = &self.model.read(cx).settings.saved_models;
        let unreadable = saved_models.summary_unreadable;
        let saved = saved_models.summary.clone();

        let later = |id: &'static str, why: &str| {
            div()
                .id(id)
                .role(gpui::Role::Note)
                .mt(space::MD)
                .child(muted(why, cx))
                .into_any_element()
        };

        let auto = settings_card(
            "summary-auto",
            vec![
                h_flex()
                    .justify_between()
                    .gap(space::XL)
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap(space::SM)
                            .child(card_title(text::SUMMARY_AUTO, None, cx))
                            .child(muted(text::SUMMARY_AUTO_HELP, cx)),
                    )
                    .child(
                        div().flex_none().child(
                            // Tauri's default is on (ModelSettingsModal.tsx:143);
                            // its saved value cannot be read here.
                            controls::switch("summary-auto-switch")
                                .checked(true)
                                .disabled(true)
                                .accessibility_label(text::SUMMARY_AUTO),
                        ),
                    )
                    .into_any_element(),
                later("summary-auto-later", text::SUMMARY_AUTO_LATER),
            ],
            cx,
        );

        let language = settings_card(
            "summary-language",
            vec![
                card_title(text::SUMMARY_LANGUAGE, Some(IconName::Globe), cx),
                div()
                    .mt(space::SM)
                    .child(muted(text::SUMMARY_LANGUAGE_HELP, cx))
                    .into_any_element(),
                later("summary-language-later", text::SUMMARY_LANGUAGE_LATER),
            ],
            cx,
        );

        let provider = match &saved {
            Some((id, _)) => text::summary_provider(id).to_string(),
            None if unreadable => text::SAVED_SETTING_UNREADABLE.to_string(),
            None => text::SUMMARY_PROVIDER_PLACEHOLDER.to_string(),
        };
        let model = match saved.map(|(_, model)| model).filter(|m| !m.is_empty()) {
            Some(model) => model,
            None if unreadable => text::SAVED_SETTING_UNREADABLE.to_string(),
            None => text::SUMMARY_MODEL_PLACEHOLDER.to_string(),
        };
        let model_card = settings_card(
            "summary-model",
            vec![
                card_title(text::SUMMARY_MODEL, None, cx),
                div()
                    .mt(space::SM)
                    .mb(space::XL)
                    .child(muted(text::SUMMARY_MODEL_HELP, cx))
                    .into_any_element(),
                v_flex()
                    .gap(space::SM)
                    .child(
                        div()
                            .text_size(typography::SM)
                            .child(field_label(text::SUMMARY_MODEL)),
                    )
                    .child(muted(text::SUMMARY_MODEL_PICK, cx))
                    .child(disabled_select(
                        "summary-provider",
                        text::SUMMARY_PROVIDER,
                        &provider,
                        text::SUMMARY_MODEL_LATER,
                        cx,
                    ))
                    .child(disabled_select(
                        "summary-model-name",
                        text::SUMMARY_MODEL,
                        &model,
                        text::SUMMARY_MODEL_LATER,
                        cx,
                    ))
                    .into_any_element(),
                later("summary-model-later", text::SUMMARY_MODEL_LATER),
            ],
            cx,
        );

        // Tauri: `flex flex-col gap-4`, no tab title.
        tab_column(vec![v_flex()
            .id("settings-summary")
            .role(gpui::Role::Group)
            .gap(space::LG)
            .child(auto)
            .child(language)
            .child(model_card)
            .into_any_element()])
    }
}
