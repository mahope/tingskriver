//! Import and compare a user's sample, without using provider fallback chains.
use crate::{
    controls::{self, Variant},
    theme::{layout, space},
    ui::{page, paragraph, section_title},
};
use gpui::{prelude::*, Context, Entity, Window};
use gpui_component::{
    input::{InputState, Textarea, TextareaState},
    v_flex, Disableable, Selectable,
};
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
        let mut body = v_flex()
            .gap(space::LG)
            .child(paragraph(strings::MEASURE_HELP, cx))
            .child(section_title(strings::MEASURE_PATH))
            .child(controls::input(&self.path).disabled(self.running))
            .child(section_title(strings::MEASURE_REFERENCE))
            .child(Textarea::new(&self.reference).disabled(self.running))
            .child(section_title(strings::MEASURE_MODELS))
            .child(paragraph(strings::MEASURE_PRIVACY, cx))
            .child(
                controls::button("reload-measurement", Variant::Outline)
                    .label(strings::MEASURE_RELOAD)
                    .disabled(self.running)
                    .on_click(cx.listener(|view, _, window, cx| view.reload(window, cx))),
            );
        for (index, choice) in self.choices.iter().enumerate() {
            body = body.child(
                v_flex()
                    .gap(space::SM)
                    .child(
                        controls::button(("measure-choice", index), Variant::Outline)
                            .label(format!(
                                "{} {}",
                                if choice.selected {
                                    strings::MEASURE_SELECTED
                                } else {
                                    strings::MEASURE_SELECT
                                },
                                choice.label
                            ))
                            .selected(choice.selected)
                            .disabled(self.running)
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.choices[index].selected = !view.choices[index].selected;
                                cx.notify();
                            })),
                    )
                    .child(paragraph(strings::MEASURE_RATE, cx))
                    .child(controls::input(&choice.rate).disabled(self.running)),
            );
        }
        body = body
            .child(
                controls::button("start-measurement", Variant::Primary)
                    .label(strings::MEASURE_START)
                    .disabled(self.running)
                    .on_click(cx.listener(|view, _, _, cx| view.start(cx))),
            )
            .child(paragraph(&self.message, cx));
        if let Some(report) = &self.report {
            body = body.child(section_title(strings::MEASURE_RESULTS));
            for row in &report.results {
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
                body = body.child(
                    v_flex()
                        .gap(space::SM)
                        .child(section_title(&format!("{}: {}", row.provider, row.model)))
                        .child(paragraph(&row.message, cx))
                        .child(paragraph(
                            &strings::measure_result_line(&wer, &score, &latency, &price),
                            cx,
                        )),
                );
            }
        }
        body = body.child(paragraph(strings::MEASURE_HISTORY, cx));
        let dir = self.dir.clone();
        body = body.child(
            controls::button("open-measurements", Variant::Outline)
                .label(strings::MEASURE_OPEN)
                .on_click(move |_, _, _| {
                    if let Some(dir) = &dir {
                        crate::startup::open_folder(&dir.join("measurements"));
                    }
                }),
        );
        page(
            strings::MEASURE_TITLE,
            None,
            false,
            layout::SETTINGS_SECTION_MAX,
            body.into_any_element(),
            cx,
        )
    }
}
