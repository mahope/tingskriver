//! Indstillinger > Transskription, as in the Tauri app
//! (`components/TranscriptSettings.tsx:101-128`, `ParakeetModelManager.tsx`,
//! `WhisperModelManager.tsx`), plus the GPUI language group (#209,
//! `pages::settings::language_section`) and the entry to Måling
//! (docs/design-paritet.md 2.10).
//!
//! The engine choice and the model cards are the Tauri ones: Parakeet and
//! Whisper, a card per model with "Hent", progress with "Annullér", "Slet",
//! and the chosen model saved in `transcript_settings`. The logic is
//! `crate::models`; opening this tab only reads the models folder, a
//! download starts when the user presses "Hent" (#174).

use gpui::{
    div, prelude::*, AnyElement, App, Context, ElementId, Entity, FontWeight, SharedString, Window,
};
use gpui_component::{dialog::DialogFooter, h_flex, v_flex, Disableable, WindowExt};
use gpui_kit_assets::IconName;
use tingskriver_core::strings::models as model_text;
use tingskriver_core::strings::{self, settings_tabs as text};

use crate::controls::{self, Segment, Variant};
use crate::models::{self, Busy, Check, Engine, Key, Load, Models, Row, State, Status};
use crate::pages::settings::{alert, field_label, settings_section, tab_column};
use crate::theme::{self, space, typography};
use crate::ui::{heading, muted, palette, AppModel, Level, Page, Shell};

/// `border-2` of a model card.
const CARD_BORDER: gpui::Pixels = gpui::px(2.0);
/// `h-2` of the progress bar.
const BAR_HEIGHT: gpui::Pixels = gpui::px(8.0);
/// `w-2 h-2` of the "Klar" dot.
const DOT: gpui::Pixels = gpui::px(8.0);
/// `bg-primary/10` of the selected card.
const SELECTED_TINT: f32 = 0.1;

impl Shell {
    pub(crate) fn transcription_settings_tab(&self, cx: &mut Context<Self>) -> AnyElement {
        let models = models::global(cx);
        if let Some(models) = &models {
            // Reads the models folder; never downloads.
            if models.read(cx).needs_load() {
                models.update(cx, |models, cx| models.load(cx));
            }
        }
        let engine_body = match &models {
            Some(models) => {
                let busy = models_busy(&self.model, cx);
                engine_body(models, &self.model, busy, cx)
            }
            None => muted(model_text::MODELS_NO_HOST, cx).into_any_element(),
        };

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
                engine_body,
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

/// What uses the models right now (recording, dictation, a job). Also
/// asked again when a delete is confirmed.
pub(crate) fn models_busy(app: &Entity<AppModel>, cx: &App) -> Option<Busy> {
    let model = app.read(cx);
    let phase = model
        .recorder
        .as_ref()
        .map(|recorder| recorder.state().phase)
        .unwrap_or_default();
    let job_running = model.meeting_jobs.iter().any(|job| job.status == "running");
    models::busy_reason(
        phase,
        tingskriver_core::dictation::service::is_dictating(),
        job_running,
    )
}

fn engine_label(engine: Engine) -> &'static str {
    match engine {
        Engine::Parakeet => text::TRANSCRIPTION_PARAKEET,
        Engine::Whisper => text::TRANSCRIPTION_WHISPER,
    }
}

/// The engine choice, the model cards and what belongs to no card.
fn engine_body(
    models: &Entity<Models>,
    app: &Entity<AppModel>,
    busy: Option<Busy>,
    cx: &App,
) -> AnyElement {
    let state = &models.read(cx).state;
    let view = state.view;
    let chooser = {
        let models = models.clone();
        controls::segmented(
            "transcription-engine",
            model_text::MODEL_ENGINE_LABEL,
            Engine::ALL
                .iter()
                .map(|engine| Segment::new(engine_label(*engine)))
                .collect(),
            Engine::ALL.iter().position(|e| *e == view).unwrap_or(0),
            move |index, _, cx| {
                let engine = Engine::ALL[index];
                models.update(cx, |models, cx| models.set_view(engine, cx));
            },
            cx,
        )
    };
    v_flex()
        .gap(space::LG)
        .child(
            v_flex()
                .gap(space::SM)
                .child(
                    div()
                        .text_size(typography::SM)
                        .child(field_label(text::TRANSCRIPTION_ENGINE)),
                )
                .child(chooser),
        )
        .child(model_list(models, app, state, busy, cx))
        .into_any_element()
}

fn model_list(
    models: &Entity<Models>,
    app: &Entity<AppModel>,
    state: &State,
    busy: Option<Busy>,
    cx: &App,
) -> AnyElement {
    let palette = palette(cx);
    match &state.load {
        Load::Idle | Load::Loading => {
            return div()
                .id("models-loading")
                .role(gpui::Role::Status)
                .child(muted(model_text::MODELS_LOADING, cx))
                .into_any_element();
        }
        Load::Failed(why) => {
            let background = palette.background;
            return v_flex()
                .id("models-failed")
                .role(gpui::Role::Alert)
                .gap(space::XS)
                .p(space::LG)
                .rounded(theme::RADIUS_LG)
                .border_1()
                .border_color(theme::over(palette.destructive, background, 0.3))
                .bg(theme::over(palette.destructive, background, 0.1))
                .text_color(palette.destructive)
                .child(div().text_size(typography::SM).child(
                    if why == model_text::MODELS_NO_HOST {
                        why.clone()
                    } else {
                        model_text::MODELS_LOAD_FAILED.to_string()
                    },
                ))
                .when(why != model_text::MODELS_NO_HOST, |this| {
                    this.child(div().text_size(typography::XS).child(why.clone()))
                })
                .into_any_element();
        }
        Load::Ready => {}
    }

    let view = state.view;
    let selected_here = state
        .selected
        .as_ref()
        .filter(|(engine, _)| *engine == view)
        .cloned();
    let cards = state
        .rows_of(view)
        .map(|row| {
            let selected = selected_here.as_ref().is_some_and(|key| *key == row.key());
            model_card(models, app, row, selected, busy, cx)
        })
        .collect::<Vec<_>>();

    // Under the cards: what is in use, or why nothing is.
    let footer = if state.selected_unreadable {
        Some(alert(
            "models-selected-unreadable",
            text::SAVED_SETTING_UNREADABLE,
            cx,
        ))
    } else if let Some(provider) = &state.saved_other_provider {
        Some(alert(
            "models-saved-other-provider",
            &model_text::model_saved_other_provider(provider),
            cx,
        ))
    } else {
        selected_here
            .as_ref()
            .and_then(|key| state.row(key))
            .map(|row| {
                if row.status() == Status::Ready {
                    div()
                        .id("models-using")
                        .text_size(typography::XS)
                        .text_color(palette.muted_foreground)
                        .text_center()
                        .child(model_text::model_using(&row.name()))
                        .into_any_element()
                } else {
                    alert(
                        "models-selected-missing",
                        model_text::MODEL_SELECTED_MISSING,
                        cx,
                    )
                }
            })
    };

    v_flex()
        .id("models-list")
        .role(gpui::Role::List)
        .gap(space::MD)
        .when(!state.any_ready(), |this| {
            this.child(
                div()
                    .id("models-none")
                    .role(gpui::Role::Note)
                    .child(muted(model_text::MODELS_NONE_INSTALLED, cx)),
            )
        })
        .children(cards)
        .when_some(state.notice.clone(), |this, notice| {
            this.child(
                div()
                    .id("models-notice")
                    .role(gpui::Role::Status)
                    .aria_label(SharedString::from(notice.clone()))
                    .child(muted(&notice, cx)),
            )
        })
        .children(footer)
        .into_any_element()
}

/// `prefix` and the model: unique per card.
fn eid(prefix: &'static str, id: &SharedString) -> ElementId {
    ElementId::NamedChild(std::sync::Arc::new(ElementId::from(prefix)), id.clone())
}

fn key_id(key: &Key) -> SharedString {
    SharedString::from(format!("{}-{}", key.0.provider(), key.1))
}

/// One model: Tauri's `ModelCard` (`rounded-lg border-2 p-4`) plus size,
/// source, licence, revision and the checksum status.
fn model_card(
    models: &Entity<Models>,
    app: &Entity<AppModel>,
    row: &Row,
    selected: bool,
    busy: Option<Busy>,
    cx: &App,
) -> AnyElement {
    let palette = palette(cx);
    let key = row.key();
    let id = key_id(&key);
    let status = row.status();
    let name = row.name();
    let ready = status == Status::Ready;
    let working = matches!(status, Status::Downloading(_) | Status::Cancelling);
    let background = palette.background;

    let (border, fill) = if selected && ready {
        (
            palette.primary,
            theme::over(palette.primary, background, SELECTED_TINT),
        )
    } else if ready {
        (palette.border, palette.card)
    } else {
        (palette.border, background)
    };

    let title = h_flex()
        .gap(space::SM)
        .child(div().font_weight(FontWeight::SEMIBOLD).child(name.clone()))
        .when(selected && ready, |this| {
            this.child(
                h_flex()
                    .id(eid("model-selected", &id))
                    .role(gpui::Role::Note)
                    .aria_label(SharedString::from(model_text::MODEL_SELECTED))
                    .gap(space::XS)
                    .px(space::SM)
                    .py(theme::settings_tabs::BADGE_PADDING_Y)
                    .rounded(theme::RADIUS_FULL)
                    .bg(palette.primary)
                    .text_color(palette.primary_foreground)
                    .text_size(typography::XS)
                    .font_weight(FontWeight::MEDIUM)
                    .child("✓")
                    .child(model_text::MODEL_SELECTED),
            )
        });

    let actions = card_actions(models, app, row, &status, selected, busy, &name, &id, cx);

    let details = model_text::model_details(
        &model_text::model_size(row.size_mb()),
        row.meta.source.as_deref(),
        row.meta.license.as_deref(),
        row.meta.revision.as_deref(),
    );
    let check_line = match (row.check, row.meta.verifiable) {
        (Check::Passed, _) => Some(model_text::MODEL_CHECK_PASSED),
        (Check::Checking, _) => Some(model_text::MODEL_CHECK_CHECKING),
        (Check::Mismatch, _) => Some(model_text::MODEL_CHECK_FAILED),
        (Check::Unknown, true) if ready => Some(model_text::MODEL_CHECK_UNKNOWN),
        (Check::Unknown, false) if ready => Some(model_text::MODEL_CHECK_UNSUPPORTED),
        _ => None,
    };

    let will_download = matches!(status, Status::Missing).then(|| {
        model_text::model_will_download(
            row.meta
                .source
                .as_deref()
                .unwrap_or(model_text::MODEL_UNKNOWN),
            &model_text::model_size(row.size_mb()),
        )
    });

    v_flex()
        .id(eid("model-card", &id))
        .role(gpui::Role::ListItem)
        .aria_label(SharedString::from(name.clone()))
        .w_full()
        .p(space::LG)
        .gap(space::SM)
        .rounded(theme::RADIUS_LG)
        .border(CARD_BORDER)
        .border_color(border)
        .bg(fill)
        .child(
            h_flex()
                .items_start()
                .justify_between()
                .gap(space::LG)
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap(space::XS)
                        .child(h_flex().gap(space::SM).child(title).when(
                            row.recommended(),
                            |this| {
                                this.child(
                                    div()
                                        .px(space::SM)
                                        .py(theme::settings_tabs::BADGE_PADDING_Y)
                                        .rounded(theme::RADIUS_FULL)
                                        .bg(palette.primary)
                                        .text_color(palette.primary_foreground)
                                        .text_size(typography::XS)
                                        .font_weight(FontWeight::MEDIUM)
                                        .child(model_text::MODEL_RECOMMENDED),
                                )
                            },
                        ))
                        .child(muted(&row.tagline(), cx)),
                )
                .child(actions),
        )
        .child(
            v_flex()
                .gap(space::XXS)
                .text_size(typography::XS)
                .text_color(palette.muted_foreground)
                .child(div().child(details))
                .children(check_line.map(|line| {
                    div()
                        .id(eid("model-check", &id))
                        .text_color(if row.check == Check::Mismatch {
                            palette.destructive
                        } else if row.check == Check::Passed {
                            palette.success
                        } else {
                            palette.muted_foreground
                        })
                        .child(line)
                }))
                .children(will_download.map(|line| div().child(line))),
        )
        .when(working, |this| {
            this.child(progress_block(models, row, &status, &name, &id, cx))
        })
        .when_some(
            card_message(row, &status, busy),
            |this, (message, is_error)| {
                this.child(if is_error {
                    alert_line(eid("model-error", &id), &message, cx)
                } else {
                    div()
                        .id(eid("model-note", &id))
                        .text_size(typography::XS)
                        .text_color(palette.muted_foreground)
                        .child(message)
                        .into_any_element()
                })
            },
        )
        .into_any_element()
}

/// The line under a card: a failure, the corruption notice, or why a model
/// in use cannot be deleted. The bool is "this is an error".
fn card_message(row: &Row, status: &Status, busy: Option<Busy>) -> Option<(String, bool)> {
    match status {
        Status::Failed(why) => {
            let mut message = why.clone();
            if row.found.base == models::Base::Ready {
                message.push(' ');
                message.push_str(model_text::MODEL_KEPT_EXISTING);
            }
            Some((message, true))
        }
        Status::Corrupted => Some((model_text::MODEL_CORRUPTED.to_string(), true)),
        Status::Cancelling => row.error.clone().map(|why| (why, false)),
        Status::Ready => match (&row.error, busy) {
            (Some(why), _) => Some((why.clone(), true)),
            (None, Some(busy)) => Some((busy.explanation().to_string(), false)),
            (None, None) => None,
        },
        _ => None,
    }
}

fn alert_line(id: ElementId, message: &str, cx: &App) -> AnyElement {
    div()
        .id(id)
        .role(gpui::Role::Alert)
        .aria_label(SharedString::from(message.to_string()))
        .text_size(typography::XS)
        .text_color(palette(cx).destructive)
        .child(message.to_string())
        .into_any_element()
}

/// The right-hand side of a card: "Klar" and "Slet", "Hent", "Prøv igen" or
/// "Slet" with "Hent igen", as Tauri, plus "Vælg" and "Kontrollér" so the
/// card can be used from the keyboard.
#[allow(clippy::too_many_arguments)]
fn card_actions(
    models: &Entity<Models>,
    app: &Entity<AppModel>,
    row: &Row,
    status: &Status,
    selected: bool,
    busy: Option<Busy>,
    name: &str,
    id: &SharedString,
    cx: &App,
) -> AnyElement {
    let palette = palette(cx);
    let key = row.key();
    let labelled = |verb: &str| SharedString::from(model_text::model_action_label(verb, name));
    let download = |label: &'static str, variant: Variant| {
        let models = models.clone();
        let key = key.clone();
        controls::small_button(eid("model-download", id), variant)
            .label(label)
            .accessibility_label(labelled(label))
            .on_click(move |_, _, cx| {
                let key = key.clone();
                models.update(cx, |models, cx| models.download(key, cx));
            })
    };
    let delete = |label: &'static str, variant: Variant, blocked: Option<Busy>| {
        let models = models.clone();
        let app = app.clone();
        let key = key.clone();
        let name = name.to_string();
        let button = controls::small_button(eid("model-delete", id), variant)
            .label(label)
            .accessibility_label(labelled(model_text::MODEL_DELETE_LABEL))
            .disabled(blocked.is_some());
        match blocked {
            // The reason is also written under the card.
            Some(busy) => button.tooltip(busy.explanation()),
            None => {
                button
                    .tooltip(model_text::MODEL_DELETE_LABEL)
                    .on_click(move |_, window, cx| {
                        confirm_delete(
                            models.clone(),
                            app.clone(),
                            key.clone(),
                            name.clone(),
                            selected,
                            window,
                            cx,
                        )
                    })
            }
        }
    };

    match status {
        Status::Ready => h_flex()
            .gap(space::SM)
            .child(
                h_flex()
                    .gap(space::XS2)
                    .text_color(palette.success)
                    .child(
                        div()
                            .size(DOT)
                            .rounded(theme::RADIUS_FULL)
                            .bg(palette.success),
                    )
                    .child(
                        div()
                            .text_size(typography::XS)
                            .font_weight(FontWeight::MEDIUM)
                            .child(model_text::MODEL_READY),
                    ),
            )
            .when(!selected, |this| {
                let models = models.clone();
                let key = key.clone();
                this.child(
                    controls::small_button(eid("model-choose", id), Variant::Outline)
                        .label(model_text::MODEL_CHOOSE)
                        .accessibility_label(labelled(model_text::MODEL_CHOOSE))
                        .on_click(move |_, _, cx| {
                            let key = key.clone();
                            models.update(cx, |models, cx| models.select(key, cx));
                        }),
                )
            })
            .when(row.check != Check::Checking, |this| {
                let models = models.clone();
                let key = key.clone();
                this.child(
                    controls::small_button(eid("model-verify", id), Variant::Ghost)
                        .label(model_text::MODEL_CHECK)
                        .accessibility_label(labelled(model_text::MODEL_CHECK_LABEL))
                        .on_click(move |_, _, cx| {
                            let key = key.clone();
                            models.update(cx, |models, cx| models.verify(key, cx));
                        }),
                )
            })
            .child(delete(model_text::MODEL_DELETE, Variant::Ghost, busy))
            .into_any_element(),
        Status::Missing => {
            download(model_text::MODEL_DOWNLOAD, Variant::Primary).into_any_element()
        }
        Status::Failed(_) => download(model_text::MODEL_RETRY, Variant::Danger).into_any_element(),
        Status::Corrupted => h_flex()
            .gap(space::SM)
            .child(delete(model_text::MODEL_DELETE, Variant::Secondary, busy))
            .child(download(model_text::MODEL_DOWNLOAD_AGAIN, Variant::Primary))
            .into_any_element(),
        Status::Downloading(_) | Status::Cancelling => div().into_any_element(),
    }
}

/// The full-width bar of a running download with "Annullér".
fn progress_block(
    models: &Entity<Models>,
    row: &Row,
    status: &Status,
    name: &str,
    id: &SharedString,
    cx: &App,
) -> AnyElement {
    let palette = palette(cx);
    let (percent, cancelling) = match status {
        Status::Downloading(progress) => (progress.percent, false),
        _ => (0, true),
    };
    let key = row.key();
    let total = row.size_mb();
    let done = total * f64::from(percent) / 100.0;
    v_flex()
        .mt(space::XS)
        .pt(space::MD)
        .border_t_1()
        .border_color(palette.border)
        .gap(space::SM)
        .child(
            h_flex()
                .justify_between()
                .child(
                    h_flex()
                        .gap(space::SM)
                        .text_size(typography::SM)
                        .text_color(palette.primary)
                        .child(div().font_weight(FontWeight::MEDIUM).child(if cancelling {
                            model_text::MODEL_CANCELLING
                        } else {
                            model_text::MODEL_DOWNLOADING
                        }))
                        .when(!cancelling, |this| {
                            this.child(
                                div()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(model_text::model_percent(percent)),
                            )
                        }),
                )
                .child(if cancelling {
                    div()
                        .text_size(typography::XS)
                        .text_color(palette.muted_foreground)
                        .font_weight(FontWeight::MEDIUM)
                        .child(model_text::MODEL_CANCEL_STARTED)
                        .into_any_element()
                } else {
                    let models = models.clone();
                    controls::small_button(eid("model-cancel", id), Variant::Ghost)
                        .label(model_text::MODEL_CANCEL)
                        .accessibility_label(SharedString::from(model_text::model_action_label(
                            model_text::MODEL_CANCEL_LABEL,
                            name,
                        )))
                        .on_click(move |_, _, cx| {
                            let key = key.clone();
                            models.update(cx, |models, cx| models.cancel(key, cx));
                        })
                        .into_any_element()
                }),
        )
        .child(
            div()
                .id(eid("model-progress", id))
                .role(gpui::Role::ProgressIndicator)
                .aria_label(SharedString::from(model_text::model_progress_label(
                    name, percent,
                )))
                .w_full()
                .h(BAR_HEIGHT)
                .rounded(theme::RADIUS_FULL)
                .bg(palette.muted)
                .overflow_hidden()
                .child(
                    div()
                        .h_full()
                        .w(gpui::relative(f32::from(percent) / 100.0))
                        .rounded(theme::RADIUS_FULL)
                        .bg(palette.primary),
                ),
        )
        .child(
            div()
                .text_size(typography::XS)
                .text_color(palette.muted_foreground)
                .child(format!(
                    "{} / {}",
                    model_text::model_size(done),
                    model_text::model_size(total)
                )),
        )
        .into_any_element()
}

/// "Slet …?" as the meeting delete dialog. Deleting needs this answer.
fn confirm_delete(
    models: Entity<Models>,
    app: Entity<AppModel>,
    key: Key,
    name: String,
    selected: bool,
    window: &mut Window,
    cx: &mut App,
) {
    let muted_color = palette(cx).muted_foreground;
    let question = model_text::model_delete_confirm(&name, selected);
    window.open_dialog(cx, move |dialog, _, _| {
        let models = models.clone();
        let app = app.clone();
        let key = key.clone();
        dialog
            .w(theme::sidebar::CONFIRM_DIALOG_WIDTH)
            .title(
                div()
                    .font_family(typography::SERIF)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_size(typography::XL)
                    .child(model_text::MODEL_DELETE_TITLE),
            )
            .child(
                div()
                    .pb(space::XL)
                    .text_color(muted_color)
                    .child(question.clone()),
            )
            .footer(
                DialogFooter::new()
                    .gap(space::LG)
                    .child(
                        controls::button("model-delete-cancel", Variant::Ghost)
                            .label(strings::DIALOG_CANCEL)
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        controls::button("model-delete-confirm", Variant::Danger)
                            .label(model_text::MODEL_DELETE)
                            .on_click(move |_, window, cx| {
                                window.close_dialog(cx);
                                let key = key.clone();
                                // Asked again: a recording may have started
                                // while the dialog was open.
                                let busy = models_busy(&app, cx);
                                models.update(cx, |models, cx| models.delete(key, busy, cx));
                            }),
                    ),
            )
    });
}
