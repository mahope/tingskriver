//! Indstillinger > Transskription, as in the Tauri app
//! (`components/TranscriptSettings.tsx:101-128`), plus the GPUI language
//! group (#209, `pages::settings::language_section`) and the entry to Måling
//! (docs/design-paritet.md 2.10).
//!
//! Choosing the engine and downloading models (`ParakeetModelManager.tsx`,
//! `WhisperModelManager.tsx`) is not in this app yet: the select shows the
//! saved engine read-only, with the saved model and the reason.

use gpui::{div, prelude::*, AnyElement, Context};
use gpui_component::{h_flex, v_flex};
use gpui_kit_assets::IconName;
use tingskriver_core::strings::{self, settings_tabs as text};

use super::{disabled_select, load_saved_models};
use crate::controls::{self, Variant};
use crate::pages::settings::{field_label, settings_section, tab_column};
use crate::theme::{space, typography};
use crate::ui::{heading, muted, Level, Page, Shell};

impl Shell {
    pub(crate) fn transcription_settings_tab(&self, cx: &mut Context<Self>) -> AnyElement {
        load_saved_models(self, cx);
        let saved_models = &self.model.read(cx).settings.saved_models;
        let unreadable = saved_models.transcription_unreadable;
        let saved = saved_models.transcription.clone();
        // Tauri's default engine is Parakeet (`TranscriptSettings.tsx`); a
        // failed read must not look like that default.
        let engine = match &saved {
            Some((id, _)) => text::transcription_engine(id).to_string(),
            None if unreadable => text::SAVED_SETTING_UNREADABLE.to_string(),
            None => text::TRANSCRIPTION_PARAKEET.to_string(),
        };
        let model = saved
            .map(|(_, model)| model)
            .filter(|model| !model.is_empty());

        let engine_body = v_flex()
            .gap(space::SM)
            .child(
                div()
                    .text_size(typography::SM)
                    .child(field_label(text::TRANSCRIPTION_ENGINE)),
            )
            .child(disabled_select(
                "transcription-engine",
                text::TRANSCRIPTION_ENGINE,
                &engine,
                text::TRANSCRIPTION_ENGINE_LATER,
                cx,
            ))
            .children(model.map(|m| muted(&text::transcription_model_current(&m), cx)))
            .child(
                div()
                    .id("transcription-later")
                    .role(gpui::Role::Note)
                    .child(muted(text::TRANSCRIPTION_ENGINE_LATER, cx)),
            );

        let measurement = h_flex().child(
            controls::button("open-measurement", Variant::Outline)
                .label(text::MEASURE_ENTRY)
                .icon(IconName::ChevronRight)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.page = Page::Measurement;
                    cx.notify();
                })),
        );

        tab_column(vec![
            settings_section(
                "settings-transcription",
                Some(heading(text::TRANSCRIPTION_TITLE, Level::Section).into_any_element()),
                Some(text::TRANSCRIPTION_HELP),
                engine_body.into_any_element(),
                true,
                cx,
            ),
            self.language_section(cx).into_any_element(),
            settings_section(
                "settings-measurement",
                Some(heading(strings::MEASURE_TITLE, Level::Section).into_any_element()),
                Some(text::MEASURE_ENTRY_HELP),
                measurement.into_any_element(),
                false,
                cx,
            ),
        ])
    }
}
