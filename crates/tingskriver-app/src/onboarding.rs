//! First start (Tauri `components/onboarding/*`;
//! `docs/screenshots/onboarding-*.png`): shown in place of the sidebar and
//! the page until onboarding is done. Step 1 welcomes, step 2 fetches the
//! transcription and summary models through core
//! (`tingskriver_core::onboarding`) and finishes once the transcription
//! model is on disk; the summary model may carry on in the background.
//!
//! Tauri's third step, macOS permissions, is left out: this app runs on
//! Windows (and Linux for tests).

use gpui::{
    div, prelude::*, relative, AnyElement, App, Context, Entity, FontWeight, Global, SharedString,
    Window,
};
use gpui_component::{
    h_flex, notification::Notification, spinner::Spinner, v_flex, Icon, Sizable, WindowExt,
};
use gpui_kit_assets::IconName;
use tingskriver_core::host::HostHandle;
use tingskriver_core::onboarding::{self as core, Change, Download, Model, Status};
use tingskriver_core::strings::onboarding as text;
use tokio::runtime::Handle;
use tokio::sync::mpsc::UnboundedReceiver;

use crate::controls::{self, Variant};
use crate::startup::DbState;
use crate::theme::{self, icon_size, onboarding as size, over, space, typography};
use crate::ui::{heading, palette, AppModel, Level};

/// The two visible steps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Welcome,
    Download,
}

impl Step {
    fn number(self) -> usize {
        match self {
            Step::Welcome => 1,
            Step::Download => 2,
        }
    }
}

pub struct Onboarding {
    model: Entity<AppModel>,
    /// Core's connection to the app; without it nothing can be fetched.
    host: Option<HostHandle>,
    runtime: Handle,
    /// False once onboarding is done (or was done before this start).
    visible: bool,
    step: Step,
    transcription: Download,
    summary: Download,
    /// The recommended summary model, once known.
    summary_model: Option<String>,
    /// Downloads begin when step 2 opens, once.
    started: bool,
    /// "Færdig" was pressed and the database is being set up.
    completing: bool,
}

struct OnboardingGlobal(Option<Entity<Onboarding>>);

impl Global for OnboardingGlobal {}

/// The onboarding view while it should replace the sidebar and the page.
pub(crate) fn shown(cx: &App) -> Option<Entity<Onboarding>> {
    cx.try_global::<OnboardingGlobal>()
        .and_then(|global| global.0.clone())
        .filter(|view| view.read(cx).visible)
}

/// Decide at startup whether onboarding is needed and route core's download
/// events to it. `host` is `None` without an application data folder; the
/// app then shows its database error instead.
pub fn init(
    model: Entity<AppModel>,
    host: Option<(
        HostHandle,
        UnboundedReceiver<tingskriver_host::bridge::Event>,
    )>,
    cx: &mut App,
) {
    let Some((host, mut events)) = host else {
        cx.set_global(OnboardingGlobal(None));
        return;
    };
    // A tiny settings file, read once before the first window, like the
    // language settings in `AppModel::new`.
    if core::is_completed(&*host) {
        cx.set_global(OnboardingGlobal(None));
        return;
    }
    log::info!("Onboarding not completed: showing it");
    let runtime = model.read(cx).runtime.clone();
    let view = cx.new(|_| Onboarding::new(model, Some(host), runtime, true));
    cx.set_global(OnboardingGlobal(Some(view.clone())));
    let weak = view.downgrade();
    cx.spawn(async move |cx| {
        while let Some(event) = events.recv().await {
            let Some(view) = weak.upgrade() else { break };
            view.update(cx, |view, cx| {
                let summary_model = view.summary_model.as_deref();
                if let Some((model, change)) =
                    core::parse_event(&event.name, &event.payload_json, summary_model)
                {
                    view.card(model).apply(change);
                    cx.notify();
                }
            });
        }
    })
    .detach();
}

/// A fixed onboarding screen for the screenshot fixtures (`None` hides it),
/// set before the shell is built.
#[cfg(feature = "screenshot-tests")]
pub(crate) fn set_fixture(step: Option<Step>, model: Entity<AppModel>, cx: &mut App) {
    let view = step.map(|step| {
        let runtime = model.read(cx).runtime.clone();
        cx.new(|_| {
            let mut view = Onboarding::new(model, None, runtime, true);
            if step == Step::Download {
                // The moment of `onboarding-model-lys.png`: the transcription
                // model has just started, the summary model waits.
                view.step = Step::Download;
                view.started = true;
                view.transcription.start();
            }
            view
        })
    });
    cx.set_global(OnboardingGlobal(view));
}

impl Onboarding {
    fn new(
        model: Entity<AppModel>,
        host: Option<HostHandle>,
        runtime: Handle,
        visible: bool,
    ) -> Self {
        Onboarding {
            model,
            host,
            runtime,
            visible,
            step: Step::Welcome,
            transcription: Download::waiting(core::TRANSCRIPTION_TOTAL_MB),
            summary: Download::waiting(0.0),
            summary_model: None,
            started: false,
            completing: false,
        }
    }

    fn card(&mut self, model: Model) -> &mut Download {
        match model {
            Model::Transcription => &mut self.transcription,
            Model::Summary => &mut self.summary,
        }
    }

    /// "Kom i gang": open step 2 and start both downloads.
    fn begin(&mut self, cx: &mut Context<Self>) {
        self.step = Step::Download;
        if !self.started {
            self.started = true;
            self.start_transcription(true, cx);
            self.start_summary(cx);
        }
        cx.notify();
    }

    /// Prepare the engine and fetch the transcription model unless it is
    /// already there. `first` checks for an existing model first.
    fn start_transcription(&mut self, first: bool, cx: &mut Context<Self>) {
        let Some(host) = self.host.clone() else {
            return;
        };
        if self.transcription.status == Status::Downloading && !first {
            return;
        }
        self.transcription.start();
        let task = self.runtime.spawn(async move {
            if core::prepare_transcription(&host).await? {
                return Ok(true);
            }
            core::download_transcription(host).await.map(|_| false)
        });
        cx.spawn(async move |this, cx| {
            let result = task.await.unwrap_or_else(|e| Err(e.to_string()));
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(true) => this.transcription.apply(Change::Completed),
                    // The events have moved the card already.
                    Ok(false) => {}
                    Err(error) => {
                        if this.transcription.status != Status::Cancelled {
                            this.transcription.fail(error);
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Find the recommended summary model and fetch it unless it is ready.
    fn start_summary(&mut self, cx: &mut Context<Self>) {
        let Some(host) = self.host.clone() else {
            return;
        };
        let known = self.summary_model.clone();
        let runtime = self.runtime.clone();
        cx.spawn(async move |this, cx| {
            let model = match known {
                Some(model) => model,
                None => runtime
                    .spawn(core::recommended_summary_model())
                    .await
                    .unwrap_or_else(|_| core::FALLBACK_SUMMARY_MODEL.to_string()),
            };
            let ready = runtime
                .spawn(core::summary_ready(host.clone(), model.clone()))
                .await
                .unwrap_or(false);
            let started = this.update(cx, |this, cx| {
                let total = core::summary_model_size_mb(&model);
                this.summary_model = Some(model.clone());
                this.summary = if ready {
                    Download::completed(total)
                } else {
                    let mut download = Download::waiting(total);
                    download.start();
                    download
                };
                cx.notify();
                !ready
            });
            if !matches!(started, Ok(true)) {
                return;
            }
            let result = runtime
                .spawn(core::download_summary(host, model))
                .await
                .unwrap_or_else(|e| Err(e.to_string()));
            if let Err(error) = result {
                // A cancel is reported by its own event.
                if !error.starts_with("CANCELLED:") {
                    let _ = this.update(cx, |this, cx| {
                        this.summary.fail(error);
                        cx.notify();
                    });
                }
            }
        })
        .detach();
    }

    fn retry(&mut self, model: Model, cx: &mut Context<Self>) {
        if self.host.is_none() {
            return;
        }
        match model {
            Model::Transcription => self.start_transcription(false, cx),
            Model::Summary => {
                if self.summary.status != Status::Downloading {
                    self.start_summary(cx);
                }
            }
        }
        cx.notify();
    }

    /// "Færdig": set up the database, mark onboarding as done and show the
    /// app. A summary model still downloading carries on.
    fn finish(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(host) = self.host.clone() else {
            return;
        };
        if self.completing || !self.transcription.is_done() {
            return;
        }
        self.completing = true;
        cx.notify();
        let database = match &self.model.read(cx).db {
            DbState::Ready(db) => Some(db.clone()),
            _ => None,
        };
        let summary_model = self
            .summary_model
            .clone()
            .unwrap_or_else(|| core::FALLBACK_SUMMARY_MODEL.to_string());
        let task = self
            .runtime
            .spawn(core::complete(host, database, summary_model));
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await.unwrap_or_else(|e| Err(e.to_string()));
            let _ = this.update_in(cx, |this, window, cx| {
                this.completing = false;
                match result {
                    Ok(db) => {
                        if !this.summary.is_done() {
                            window.push_notification(
                                Notification::info(text::BACKGROUND_TEXT)
                                    .title(text::BACKGROUND_TITLE),
                                cx,
                            );
                        }
                        this.visible = false;
                        this.model.update(cx, |model, cx| {
                            if !matches!(model.db, DbState::Ready(_)) {
                                model.db = DbState::Ready(db);
                            }
                            cx.notify();
                        });
                    }
                    Err(error) => {
                        log::error!("Onboarding could not be completed: {error}");
                        window.push_notification(
                            Notification::error(text::COMPLETE_FAILED_TEXT)
                                .title(text::COMPLETE_FAILED_TITLE),
                            cx,
                        );
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
}

impl Render for Onboarding {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = palette(cx);
        let (title, lead, body) = match self.step {
            Step::Welcome => (text::WELCOME_TITLE, text::WELCOME_LEAD, self.welcome(cx)),
            Step::Download => (
                text::DOWNLOAD_TITLE,
                text::DOWNLOAD_LEAD,
                self.downloads(cx),
            ),
        };
        div()
            .id("onboarding")
            .size_full()
            .bg(palette.background)
            .overflow_y_scroll()
            .child(
                v_flex()
                    .w_full()
                    .max_w(size::FRAME_MAX)
                    .mx_auto()
                    .px(space::XL)
                    .py(space::XXL)
                    .child(progress(self.step, cx))
                    .child(
                        v_flex()
                            .mb(space::XL)
                            .gap(space::MD)
                            .items_center()
                            .text_center()
                            .child(heading(title, Level::Hero))
                            .child(
                                div()
                                    .max_w(theme::layout::ONBOARDING_TEXT_MAX)
                                    .text_size(typography::MD)
                                    .line_height(relative(typography::LINE_HEIGHT))
                                    .text_color(palette.muted_foreground)
                                    .child(lead),
                            ),
                    )
                    .child(body),
            )
    }
}

impl Onboarding {
    fn welcome(&self, cx: &mut Context<Self>) -> AnyElement {
        let palette = palette(cx);
        v_flex()
            .items_center()
            .gap(space::LG)
            .child(
                div()
                    .max_w(theme::layout::ONBOARDING_TEXT_MAX)
                    .text_center()
                    .text_size(typography::SM)
                    .text_color(palette.muted_foreground)
                    .child(text::WELCOME_NOTE),
            )
            .child(
                div().w_full().max_w(size::BUTTON_MAX).child(
                    controls::button("onboarding-start", Variant::Primary)
                        .w_full()
                        .label(text::WELCOME_START)
                        .on_click(cx.listener(|this, _, _, cx| this.begin(cx))),
                ),
            )
            .into_any_element()
    }

    fn downloads(&self, cx: &mut Context<Self>) -> AnyElement {
        let palette = palette(cx);
        let summary_size = self
            .summary_model
            .as_deref()
            .map(core::summary_size_label)
            .unwrap_or_default();
        let ready = self.transcription.is_done();
        let label = if self.completing {
            text::PREPARING
        } else if !ready {
            text::FETCHING_MODEL
        } else {
            text::FINISH
        };
        v_flex()
            .items_center()
            .gap(space::XL)
            .child(
                v_flex()
                    .w_full()
                    .max_w(size::CARDS_MAX)
                    .gap(space::LG)
                    .child(self.download_card(
                        Model::Transcription,
                        text::TRANSCRIPTION_CARD,
                        IconName::Mic,
                        &self.transcription,
                        text::TRANSCRIPTION_SIZE,
                        text::UNIT_MB,
                        cx,
                    ))
                    .child(self.download_card(
                        Model::Summary,
                        text::SUMMARY_CARD,
                        IconName::Sparkles,
                        &self.summary,
                        &summary_size,
                        text::UNIT_MIB,
                        cx,
                    )),
            )
            .when(ready && !self.summary.is_done(), |this| {
                this.child(
                    h_flex()
                        .id("onboarding-summary-note")
                        .role(gpui::Role::Status)
                        .w_full()
                        .max_w(size::CARDS_MAX)
                        .items_start()
                        .gap(space::MD)
                        .p(space::LG)
                        .rounded(theme::RADIUS_LG)
                        .bg(palette.secondary)
                        .text_size(typography::SM)
                        .text_color(palette.foreground)
                        .child(
                            Icon::from(IconName::Download)
                                .size(icon_size::MD)
                                .text_color(palette.muted_foreground)
                                .flex_none(),
                        )
                        .child(
                            v_flex()
                                .gap(space::XS)
                                .child(
                                    div()
                                        .font_weight(FontWeight::MEDIUM)
                                        .child(text::SUMMARY_CONTINUE_TITLE),
                                )
                                .child(text::SUMMARY_CONTINUE_TEXT),
                        ),
                )
            })
            .child({
                // Tauri's `disabled:opacity-60` with a Loader2: a loading
                // button is inert and keeps the primary colors (the
                // disabled style would turn it gray).
                let busy = self.completing || !ready;
                div()
                    .w_full()
                    .max_w(size::BUTTON_MAX)
                    .when(busy, |this| this.opacity(size::BUSY_OPACITY))
                    .child(
                        controls::button("onboarding-finish", Variant::Primary)
                            .w_full()
                            .when(busy, |this| {
                                this.icon(Icon::from(IconName::LoaderCircle).size(icon_size::SM))
                                    .loading_icon(IconName::LoaderCircle)
                            })
                            .label(label)
                            .loading(busy)
                            .on_click(cx.listener(|this, _, window, cx| this.finish(window, cx))),
                    )
            })
            .into_any_element()
    }

    #[allow(clippy::too_many_arguments)]
    fn download_card(
        &self,
        model: Model,
        title: &'static str,
        icon: IconName,
        state: &Download,
        model_size: &str,
        unit: &'static str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let palette = palette(cx);
        // Element ids: one per card, without formatted text.
        let id = match model {
            Model::Transcription => 0usize,
            Model::Summary => 1,
        };
        let status: AnyElement = match state.status {
            Status::Waiting => status_text(text::WAITING, palette.muted_foreground),
            Status::Downloading => div()
                .id(("onboarding-spinner", id))
                .role(gpui::Role::Status)
                .aria_label(SharedString::from(text::DOWNLOADING))
                .child(
                    Spinner::new()
                        .icon(IconName::LoaderCircle)
                        .with_size(icon_size::MD)
                        .color(palette.foreground),
                )
                .into_any_element(),
            Status::Completed => h_flex()
                .id(("onboarding-done", id))
                .role(gpui::Role::Status)
                .aria_label(SharedString::from(text::DONE))
                .size(size::CHECK_CIRCLE)
                .justify_center()
                .rounded(theme::RADIUS_FULL)
                .bg(over(palette.success, palette.card, size::SUCCESS_TINT))
                .child(
                    Icon::from(IconName::Check)
                        .size(icon_size::SM)
                        .text_color(palette.success),
                )
                .into_any_element(),
            Status::Error => status_text(text::FAILED, palette.destructive),
            Status::Cancelled => status_text(text::CANCELLED, palette.muted_foreground),
        };
        let header = h_flex()
            .justify_between()
            .child(
                h_flex()
                    .gap(space::MD)
                    .child(
                        h_flex()
                            .flex_none()
                            .size(size::ICON_CIRCLE)
                            .justify_center()
                            .rounded(theme::RADIUS_FULL)
                            .bg(palette.secondary)
                            .child(
                                Icon::from(icon)
                                    .size(icon_size::MD)
                                    .text_color(palette.muted_foreground),
                            ),
                    )
                    .child(
                        v_flex()
                            .child(
                                div()
                                    .font_family(typography::SERIF)
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(palette.foreground)
                                    .child(title),
                            )
                            .when(!model_size.is_empty(), |this| {
                                this.child(
                                    div()
                                        .text_size(typography::SM)
                                        .text_color(palette.muted_foreground)
                                        .child(model_size.to_string()),
                                )
                            }),
                    ),
            )
            .child(status);
        let bar = matches!(state.status, Status::Downloading | Status::Completed).then(|| {
            v_flex()
                .gap(space::SM)
                .child(
                    div()
                        .id(("onboarding-progress", id))
                        .role(gpui::Role::ProgressIndicator)
                        .aria_label(SharedString::from(text::progress_label(title)))
                        .w_full()
                        .h(size::BAR_HEIGHT)
                        .rounded(theme::RADIUS_FULL)
                        .bg(palette.muted)
                        .overflow_hidden()
                        .child(
                            div()
                                .h_full()
                                .w(relative(state.progress as f32 / 100.0))
                                .rounded(theme::RADIUS_FULL)
                                .bg(palette.primary),
                        ),
                )
                .child(
                    h_flex()
                        .justify_between()
                        .text_size(typography::SM)
                        .child(
                            div()
                                .text_color(palette.muted_foreground)
                                .child(text::amount(state.downloaded_mb, state.total_mb, unit)),
                        )
                        .child(
                            h_flex()
                                .gap(space::SM)
                                .when(state.speed_mbps > 0.0, |this| {
                                    this.child(
                                        div()
                                            .text_color(palette.muted_foreground)
                                            .child(text::speed(state.speed_mbps, unit)),
                                    )
                                })
                                .child(
                                    div()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(palette.foreground)
                                        .child(text::percent(state.progress)),
                                ),
                        ),
                )
        });
        let failed = matches!(state.status, Status::Error | Status::Cancelled).then(|| {
            v_flex()
                .id(("onboarding-error", id))
                .role(gpui::Role::Alert)
                .mt(space::SM)
                .p(space::MD)
                .rounded(theme::RADIUS)
                .border_1()
                .border_color(over(palette.destructive, palette.card, size::ERROR_BORDER))
                .bg(over(palette.destructive, palette.card, size::ERROR_TINT))
                .child(
                    div()
                        .text_size(typography::SM)
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette.destructive)
                        .child(if state.status == Status::Cancelled {
                            text::DOWNLOAD_CANCELLED
                        } else {
                            text::DOWNLOAD_FAILED
                        }),
                )
                .when_some(state.error.clone(), |this, error| {
                    this.child(
                        div()
                            .mt(space::XS)
                            .text_size(typography::XS)
                            .text_color(palette.destructive)
                            .child(error),
                    )
                })
                .child(
                    div().mt(space::MD).w_full().child(
                        controls::button(("onboarding-retry", id), Variant::Primary)
                            .w_full()
                            .icon(Icon::from(IconName::RefreshCw).size(icon_size::SM))
                            .label(text::RETRY)
                            .on_click(cx.listener(move |this, _, _, cx| this.retry(model, cx))),
                    ),
                )
        });
        v_flex()
            .p(size::CARD_PADDING)
            .rounded(size::CARD_RADIUS)
            .border_1()
            .border_color(palette.border)
            .bg(palette.card)
            .child(header.when(bar.is_some(), |this| this.mb(space::LG)))
            .children(bar)
            .children(failed)
            .into_any_element()
    }
}

fn status_text(label: &'static str, color: gpui::Hsla) -> AnyElement {
    div()
        .text_size(typography::SM)
        .text_color(color)
        .child(label)
        .into_any_element()
}

/// "Trin 1 af 2" and one segment per step, filled up to the current one.
fn progress(step: Step, cx: &App) -> AnyElement {
    let palette = palette(cx);
    let current = step.number();
    v_flex()
        .mb(space::XL)
        .items_center()
        .gap(space::SM)
        .child(
            div()
                .id("onboarding-step")
                .role(gpui::Role::Status)
                .text_size(typography::SM)
                .text_color(palette.muted_foreground)
                .child(text::step_of(current, core::STEPS)),
        )
        .child(
            h_flex()
                .gap(space::XS2)
                .children((1..=core::STEPS).map(|n| {
                    div()
                        .w(size::SEGMENT_WIDTH)
                        .h(size::SEGMENT_HEIGHT)
                        .rounded(theme::RADIUS_FULL)
                        .bg(if n <= current {
                            palette.primary
                        } else {
                            palette.muted
                        })
                })),
        )
        .into_any_element()
}
