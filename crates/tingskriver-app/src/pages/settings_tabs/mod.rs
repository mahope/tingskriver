//! The settings tabs that have their own file (docs/design-paritet.md,
//! Jobs 6 and 7). The frame and Generelt are in `pages::settings`.

pub(crate) mod beta;
pub(crate) mod dictation;
pub(crate) mod recording;
pub(crate) mod summary;
pub(crate) mod transcription;

use gpui::{div, prelude::*, AnyElement, App, Context, FontWeight, SharedString};
use gpui_component::{h_flex, v_flex, Icon};
use gpui_kit_assets::IconName;

use crate::startup::DbState;
use crate::theme::{self, icon_size, layout, settings_tabs as size, shadow, space, typography};
use crate::ui::{palette, Shell};

/// A Tauri settings card: `bg-card rounded-lg border p-6 shadow-sm`
/// (Opsummering and Beta).
pub(super) fn settings_card(id: &'static str, children: Vec<AnyElement>, cx: &App) -> AnyElement {
    let palette = palette(cx);
    v_flex()
        .id(id)
        .role(gpui::Role::Group)
        .w_full()
        .bg(palette.card)
        .text_color(palette.card_foreground)
        .border_1()
        .border_color(palette.border)
        .rounded(theme::RADIUS_LG)
        .shadow(shadow::sm())
        .p(space::XL)
        .children(children)
        .into_any_element()
}

/// The title of a card: `h3 text-lg font-semibold`, serif like every h3 in
/// the Tauri app, with an optional muted icon in front.
pub(super) fn card_title(text: &str, icon: Option<IconName>, cx: &App) -> AnyElement {
    h_flex()
        .gap(space::SM)
        .when_some(icon, |this, icon| {
            this.child(
                Icon::new(icon)
                    .size(size::CARD_ICON)
                    .text_color(palette(cx).muted_foreground),
            )
        })
        .child(
            div()
                .font_family(typography::SERIF)
                .font_weight(FontWeight::SEMIBOLD)
                .text_size(typography::LG)
                .child(text.to_string()),
        )
        .into_any_element()
}

/// A sub-section title inside a tab: `h3 text-xl`, serif regular.
pub(super) fn subheading_regular(text: &str) -> AnyElement {
    div()
        .font_family(typography::SERIF)
        .text_size(typography::XL)
        .line_height(gpui::relative(typography::LINE_HEIGHT_TIGHT))
        .child(text.to_string())
        .into_any_element()
}

/// A select that this app cannot change yet: the Tauri trigger (36 px,
/// `input` border) at half opacity, showing the value Tauri shows by
/// default. It is not focusable; screen readers get the value and why.
pub(super) fn disabled_select(
    id: &'static str,
    label: &str,
    value: &str,
    why: &str,
    cx: &App,
) -> AnyElement {
    let palette = palette(cx);
    h_flex()
        .id(id)
        .role(gpui::Role::ComboBox)
        .aria_label(SharedString::from(label.to_string()))
        .aria_value(SharedString::from(value.to_string()))
        .aria_description(SharedString::from(why.to_string()))
        .w_full()
        .h(layout::INPUT_HEIGHT)
        .px(space::MD)
        .justify_between()
        .border_1()
        .border_color(palette.input)
        .rounded(theme::RADIUS)
        .bg(palette.background)
        .opacity(size::DISABLED_OPACITY)
        .cursor_not_allowed()
        .text_size(typography::SM)
        .child(value.to_string())
        .child(
            Icon::new(IconName::ChevronDown)
                .size(icon_size::SM)
                .text_color(palette.muted_foreground),
        )
        .into_any_element()
}

/// A rounded tinted box with a status text: the Beta banner and info box
/// (`bg-warning/10 border-warning/30`, `bg-primary/10 border-primary/30`).
pub(super) fn tinted_box(
    id: &'static str,
    color: gpui::Hsla,
    icon: Option<IconName>,
    body: AnyElement,
    cx: &App,
) -> AnyElement {
    let background = palette(cx).background;
    h_flex()
        .id(id)
        .role(gpui::Role::Note)
        .items_start()
        .gap(space::MD)
        .p(space::LG)
        .rounded(theme::RADIUS_LG)
        .border_1()
        .border_color(theme::over(color, background, size::TINT_BORDER))
        .bg(theme::over(color, background, size::TINT))
        .text_size(typography::SM)
        .text_color(color)
        .when_some(icon, |this, icon| {
            this.child(
                div()
                    .flex_none()
                    .mt(space::XXS)
                    .child(Icon::new(icon).size(size::CARD_ICON).text_color(color)),
            )
        })
        .child(div().flex_1().min_w_0().child(body))
        .into_any_element()
}

/// The saved transcription engine and summary model, read once from the
/// database so the read-only selects show what is set (part of
/// `SettingsModel`). Only provider and model are kept, never API keys.
#[derive(Debug, Default, Clone)]
pub(crate) struct SavedModels {
    /// Set by the first draw that asks; a `Cell` because drawing only
    /// reads the model.
    requested: std::cell::Cell<bool>,
    /// (engine id, model), e.g. ("parakeet", "parakeet-tdt-0.6b-v3").
    pub(crate) transcription: Option<(String, String)>,
    /// (provider id, model).
    pub(crate) summary: Option<(String, String)>,
    /// The last read of the transcription setting failed, so the tab must
    /// not show the default as if it were the saved choice.
    pub(crate) transcription_unreadable: bool,
    /// The same for the summary model.
    pub(crate) summary_unreadable: bool,
    /// Failed reads in a row; reset by a read that works.
    failures: u32,
}

/// First wait before reading the saved models again after a failed read (for
/// example a database that is locked during startup). It doubles each time.
const SAVED_MODELS_RETRY: std::time::Duration = std::time::Duration::from_secs(5);
/// After this many failed reads in a row the tab stops asking by itself; the
/// line "could not be read" stays until the app is restarted.
const SAVED_MODELS_MAX_FAILURES: u32 = 5;

/// How long to wait after the `failures`-th failed read in a row, or `None`
/// when the cap is reached: 5 s, 10 s, 20 s, 40 s, then give up.
fn retry_delay(failures: u32) -> Option<std::time::Duration> {
    if failures == 0 || failures >= SAVED_MODELS_MAX_FAILURES {
        return None;
    }
    Some(SAVED_MODELS_RETRY * 2u32.pow(failures - 1))
}

/// Read [`SavedModels`] once the database is open. Called while drawing the
/// Transskription and Opsummering tabs; the read runs on the tokio runtime.
pub(super) fn load_saved_models(shell: &Shell, cx: &mut Context<Shell>) {
    let model = shell.model.clone();
    let state = model.read(cx);
    let DbState::Ready(db) = &state.db else {
        return;
    };
    if state.settings.saved_models.requested.replace(true) {
        return;
    }
    let pool = db.pool().clone();
    let task = state.runtime.spawn(async move {
        use tingskriver_core::database::repositories::setting::SettingsRepository;
        let transcription = SettingsRepository::get_transcript_config(&pool)
            .await
            .map(|s| s.map(|s| (s.provider, s.model)))
            .map_err(|error| log::warn!("Could not read the transcription setting: {error}"));
        let summary = SettingsRepository::get_model_config(&pool)
            .await
            .map(|s| s.map(|s| (s.provider, s.model)))
            .map_err(|error| log::warn!("Could not read the summary model: {error}"));
        (transcription, summary)
    });
    cx.spawn(async move |_, cx| {
        let (transcription, summary) = match task.await {
            Ok(read) => read,
            Err(_) => (Err(()), Err(())),
        };
        let failed = transcription.is_err() || summary.is_err();
        let mut delay = None;
        model.update(cx, |m, cx| {
            let saved = &mut m.settings.saved_models;
            saved.failures = if failed { saved.failures + 1 } else { 0 };
            delay = retry_delay(saved.failures);
            saved.transcription_unreadable = transcription.is_err();
            saved.summary_unreadable = summary.is_err();
            if let Ok(value) = transcription {
                saved.transcription = value;
            }
            if let Ok(value) = summary {
                saved.summary = value;
            }
            cx.notify();
        });
        if let Some(delay) = delay {
            // Ask again on the next draw, but with backoff and a cap.
            cx.background_executor().timer(delay).await;
            model.update(cx, |m, cx| {
                m.settings.saved_models.requested.set(false);
                cx.notify();
            });
        }
    })
    .detach();
}

#[cfg(test)]
mod retry_tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn the_wait_doubles_and_then_stops() {
        let waits: Vec<_> = (1..=SAVED_MODELS_MAX_FAILURES)
            .map(retry_delay)
            .collect();
        assert_eq!(
            waits,
            vec![
                Some(Duration::from_secs(5)),
                Some(Duration::from_secs(10)),
                Some(Duration::from_secs(20)),
                Some(Duration::from_secs(40)),
                None,
            ]
        );
        assert_eq!(retry_delay(0), None, "a good read never retries");
    }
}
