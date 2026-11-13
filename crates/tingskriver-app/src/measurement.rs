//! Import and compare a user's sample, without using provider fallback chains.
//!
//! Måling is a sub-page of Indstillinger > Transskription with the settings
//! header ("Tilbage" returns to the tab), Tauri section style, a checkbox
//! row per provider and model with the tariff inline, and the results as a
//! card (docs/design-paritet.md 2.10, Job 6).
use crate::{
    controls::{self, Variant},
    pages::settings::field_label,
    theme::{self, layout, settings as settings_size, settings_tabs as size, shadow, space},
    ui::{heading, muted, palette, paragraph, Level, Shell},
};
use gpui::{div, prelude::*, AnyElement, Context, Entity, SharedString, Window};
use gpui_component::{
    checkbox::Checkbox,
    h_flex,
    input::{InputState, Textarea, TextareaState},
    v_flex, Disableable,
};
use gpui_kit_assets::IconName;
use std::{path::PathBuf, sync::Arc};
use tingskriver_core::{
    measurement::{self, Rate, Report, Sample, Selection},
    providers::{secrets::CredentialManager, ProviderService, ProvidersConfig},
    strings,
};
use tokio::runtime::Handle;

struct Choice {
    selection: Selection,
    label: String,
    selected: bool,
    rate: Entity<InputState>,
}

pub struct MeasurementView {
    dir: Option<PathBuf>,
    runtime: Handle,
    path: Entity<InputState>,
    reference: Entity<TextareaState>,
    config: ProvidersConfig,
    choices: Vec<Choice>,
    running: bool,
    message: String,
    report: Option<Report>,
}

impl MeasurementView {
    pub fn new(
        dir: Option<PathBuf>,
        runtime: Handle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let path = cx.new(|cx| InputState::new(window, cx).placeholder(strings::MEASURE_PATH_HINT));
        let reference = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder(strings::MEASURE_REFERENCE_HINT)
                .rows(5)
        });
        let mut view = Self {
            dir,
            runtime,
            path,
            reference,
            config: ProvidersConfig::default(),
            choices: Vec::new(),
            running: false,
            message: String::new(),
            report: None,
        };
        view.reload(window, cx);
        view
    }

    fn reload(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.running {
            return;
        }
        let result = self
            .dir
            .as_ref()
            .ok_or_else(|| strings::MEASURE_NO_FOLDER.to_string())
            .and_then(|dir| ProviderService::new(dir, Arc::new(CredentialManager)).load());
        self.apply_config(result, window, cx);
    }

    fn apply_config(
        &mut self,
        result: Result<Option<ProvidersConfig>, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok(config) => {
                self.config = config.unwrap_or_default();
                self.choices = measurement::candidates(&self.config)
                    .into_iter()
                    .map(|selection| {
                        let provider = self
                            .config
                            .provider(&selection.provider)
                            .expect("candidate provider");
                        let location = if provider.kind.is_remote() {
                            strings::MEASURE_REMOTE
                        } else {
                            strings::MEASURE_LOCAL
                        };
                        let rate = cx.new(|cx| {
                            InputState::new(window, cx).placeholder(strings::MEASURE_RATE_HINT)
                        });
                        Choice {
                            label: strings::measure_model_label(
                                &provider.name,
                                &selection.model,
                                location,
                            ),
                            selection,
                            selected: false,
                            rate,
                        }
                    })
                    .collect();
                self.message.clear();
            }
            Err(message) => {
                self.choices.clear();
                self.message = message;
            }
        }
        cx.notify();
    }

    #[cfg(feature = "screenshot-tests")]
    pub(crate) fn fixture(runtime: Handle, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut view = Self::new(None, runtime, window, cx);
        view.apply_config(Ok(Some(ProvidersConfig::default())), window, cx);
        view.path.update(cx, |input, cx| {
            input.set_value("fixtures/proeve.wav", window, cx)
        });
        view.reference.update(cx, |input, cx| {
            input.set_value("Vi mødes på torsdag klokken ti.", window, cx)
        });
        view
    }

    fn start(&mut self, cx: &mut Context<Self>) {
        if self.running {
            return;
        }
        let Some(dir) = self.dir.clone() else {
            self.message = strings::MEASURE_NO_FOLDER.into();
            cx.notify();
            return;
        };
        let selected: Result<Vec<_>, String> = self
            .choices
            .iter()
            .filter(|c| c.selected)
            .map(|choice| {
                let mut selection = choice.selection.clone();
                let price = choice.rate.read(cx).value().to_string();
                if !price.trim().is_empty() {
                    let price = price
                        .trim()
                        .replace(',', ".")
                        .parse::<f64>()
                        .map_err(|_| strings::MEASURE_INVALID_RATE.to_string())?;
                    selection.rate = Some(Rate::new(price, "DKK")?);
                }
                Ok(selection)
            })
            .collect();
        let selected = match selected {
            Ok(s) => s,
            Err(e) => {
                self.message = e;
                cx.notify();
                return;
            }
        };
        if selected.is_empty() {
            self.message = strings::MEASURE_CHOOSE.into();
            cx.notify();
            return;
        }
        let config = self.config.clone();
        let path = PathBuf::from(self.path.read(cx).value().trim());
        let reference = self.reference.read(cx).value().to_string();
        self.running = true;
        self.report = None;
        self.message = strings::MEASURE_RUNNING.into();
        cx.notify();
        let task = self.runtime.spawn(async move {
            // Disk reads and model inference stay outside GPUI's event loop.
            let sample = Sample::load(&path)?;
            let service = ProviderService::new(&dir, Arc::new(CredentialManager));
            let report = measurement::run(
                &config,
                &service,
                &selected,
                &sample,
                &reference,
                &dir.join("models"),
            )
            .await?;
            let saved = report.save(&dir);
            Ok::<_, String>((report, saved))
        });
        cx.spawn(async move |view, cx| {
            let result = task
                .await
                .unwrap_or_else(|_| Err(strings::MEASURE_INTERRUPTED.into()));
            let _ = view.update(cx, |view, cx| {
                view.running = false;
                match result {
                    Ok((report, saved)) => {
                        view.message = match saved {
                            Ok(_) => strings::MEASURE_SAVED.into(),
                            Err(e) => e,
                        };
                        view.report = Some(report);
                    }
                    Err(message) => view.message = message,
                }
                cx.notify();
            });
        })
        .detach();
    }
}

impl Render for MeasurementView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = palette(cx);
        let sample = v_flex()
            .gap(space::LG)
            .child(muted(strings::MEASURE_HELP, cx))
            .child(
                v_flex()
                    .gap(space::SM)
                    .child(field_label(strings::MEASURE_PATH))
                    .child(controls::input(&self.path).disabled(self.running)),
            )
            .child(
                v_flex()
                    .gap(space::SM)
                    .child(field_label(strings::MEASURE_REFERENCE))
                    .child(Textarea::new(&self.reference).disabled(self.running)),
            );

        // One checkbox row per provider and model, the tariff inline.
        let rows = self.choices.iter().enumerate().map(|(index, choice)| {
            h_flex()
                .id(("measure-row", index))
                .w_full()
                .min_h(layout::CONTROL_HEIGHT)
                .py(space::SM)
                .gap(space::LG)
                .justify_between()
                .border_b_1()
                .border_color(palette.border)
                .child(
                    div().flex_1().min_w_0().child(
                        Checkbox::new(("measure-choice", index))
                            .label(choice.label.clone())
                            .checked(choice.selected)
                            .disabled(self.running)
                            .on_click(cx.listener(move |view, checked: &bool, _, cx| {
                                view.choices[index].selected = *checked;
                                cx.notify();
                            })),
                    ),
                )
                .child(
                    div()
                        .flex_none()
                        .w(size::RATE_INPUT_WIDTH)
                        .child(controls::input(&choice.rate).disabled(self.running)),
                )
        });
        let models = v_flex()
            .gap(space::LG)
            .child(muted(strings::MEASURE_PRIVACY, cx))
            .child(muted(strings::MEASURE_RATE, cx))
            .child(v_flex().children(rows))
            .child(
                h_flex()
                    .gap(space::SM)
                    .child(
                        controls::button("start-measurement", Variant::Primary)
                            .label(strings::MEASURE_START)
                            .disabled(self.running)
                            .on_click(cx.listener(|view, _, _, cx| view.start(cx))),
                    )
                    .child(
                        controls::button("reload-measurement", Variant::Outline)
                            .label(strings::MEASURE_RELOAD)
                            .icon(IconName::RefreshCw)
                            .disabled(self.running)
                            .on_click(cx.listener(|view, _, window, cx| view.reload(window, cx))),
                    ),
            )
            .child(
                div()
                    .id("measure-status")
                    // No message: no role and no label, so a screen reader
                    // does not find an empty status.
                    .when(!self.message.is_empty(), |this| {
                        this.role(gpui::Role::Status)
                            .aria_label(SharedString::from(self.message.clone()))
                            .child(paragraph(&self.message, cx))
                    }),
            );

        let mut results = v_flex().gap(space::LG);
        if let Some(report) = &self.report {
            let rows = report.results.iter().enumerate().map(|(index, row)| {
                let score = row
                    .score
                    .map(|s| strings::measure_errors(&s.errors().to_string()))
                    .unwrap_or_else(|| "–".into());
                let wer = row
                    .wer
                    .map(|w| strings::measure_wer(&format!("{:.1}", w * 100.0).replace('.', ",")))
                    .unwrap_or_else(|| strings::measure_wer("–"));
                let latency = row
                    .latency_ms
                    .map(|ms| strings::measure_ms(&ms.to_string()))
                    .unwrap_or_else(|| "–".into());
                let price = row
                    .rate
                    .as_ref()
                    .map(|r| {
                        strings::measure_price(
                            &format!("{:.2}", r.per_hour).replace('.', ","),
                            &r.currency,
                        )
                    })
                    .unwrap_or_else(|| strings::MEASURE_PRICE_UNKNOWN.into());
                v_flex()
                    .px(space::LG)
                    .py(space::MD)
                    .gap(space::XS)
                    .when(index > 0, |this| {
                        this.border_t_1().border_color(palette.border)
                    })
                    .child(field_label(&format!("{}: {}", row.provider, row.model)))
                    .child(paragraph(
                        &strings::measure_result_line(&wer, &score, &latency, &price),
                        cx,
                    ))
                    .when(!row.message.is_empty(), |this| {
                        this.child(muted(&row.message, cx))
                    })
            });
            results = results.child(
                v_flex()
                    .id("measure-results")
                    .role(gpui::Role::Group)
                    .bg(palette.card)
                    .text_color(palette.card_foreground)
                    .border_1()
                    .border_color(palette.border)
                    .rounded(theme::RADIUS_LG)
                    .shadow(shadow::sm())
                    .children(rows),
            );
        }
        let dir = self.dir.clone();
        results = results.child(muted(strings::MEASURE_HISTORY, cx)).child(
            h_flex().child(
                controls::button("open-measurements", Variant::Outline)
                    .label(strings::MEASURE_OPEN)
                    .icon(IconName::FolderOpen)
                    .on_click(move |_, _, _| {
                        if let Some(dir) = &dir {
                            crate::startup::open_folder(&dir.join("measurements"));
                        }
                    }),
            ),
        );

        let section = |id: &'static str, title: Option<&str>, body: gpui::Div, first: bool| {
            v_flex()
                .id(id)
                .role(gpui::Role::Group)
                .when(!first, |this| {
                    this.border_t_1()
                        .border_color(palette.border)
                        .pt(space::XXL)
                })
                .when_some(title, |this, title| {
                    this.child(div().mb(space::LG).child(heading(title, Level::Section)))
                })
                .child(body)
        };
        v_flex()
            .w_full()
            .max_w(layout::SETTINGS_SECTION_MAX)
            .gap(settings_size::SECTION_GAP)
            .child(section("measure-sample", None, sample, true))
            .child(section(
                "measure-models",
                Some(strings::MEASURE_MODELS),
                models,
                false,
            ))
            .child(section(
                "measure-results-section",
                Some(strings::MEASURE_RESULTS),
                results,
                false,
            ))
    }
}

impl Shell {
    /// Måling as a sub-page of Indstillinger > Transskription
    /// (docs/design-paritet.md 2.10): the settings header with "Tilbage" to
    /// that tab, and the view below in the settings column.
    pub(crate) fn measurement_page(&self, cx: &mut Context<Shell>) -> AnyElement {
        let header = self.sub_page_header(
            "measurement-back",
            strings::MEASURE_TITLE,
            |shell, cx| {
                let (page, tab) = crate::pages::settings::MEASUREMENT_BACK;
                shell.settings.select_tab(tab);
                shell.page = page;
                cx.notify();
            },
            cx,
        );
        v_flex()
            .id("page-measurement")
            .size_full()
            .child(header)
            .child(
                v_flex()
                    .id("measurement-scroll")
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .overflow_y_scroll()
                    .child(
                        v_flex()
                            .w_full()
                            .max_w(layout::SETTINGS_MAX)
                            .px(space::XXL)
                            .pt(space::XL)
                            .pb(space::XXL)
                            .child(self.measurement.clone()),
                    ),
            )
            .into_any_element()
    }
}
