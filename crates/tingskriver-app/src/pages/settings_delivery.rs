//! Aflevering af referat on the Generelt tab, as in Tauri
//! (`delivery/DeliverySettings.tsx`): the privacy checkboxes, the targets
//! with "Test mål" and "Fjern mål", the add-target form and the queue.
//!
//! The queue is the host's instance (`Host::delivery`), so it is opened once
//! per process. It stays empty while the Tauri app runs, which then owns the
//! queue; the block shows Tauri's "could not be read" line and "Opdatér".

use std::{
    future::Future,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

use gpui::{div, prelude::*, AnyElement, App, Context, Entity, SharedString, Subscription, Window};
use gpui_component::{
    checkbox::Checkbox,
    dialog::DialogFooter,
    h_flex,
    input::InputState,
    select::{Select, SelectEvent, SelectState},
    v_flex, Disableable, IndexPath, WindowExt,
};
use tingskriver_core::delivery::{
    self, Delivery, Destination, Error, JobStatus, Policy, Target, WebhookFormat,
};
use tingskriver_core::strings;
use tokio::sync::OnceCell;

use super::settings::{alert, field_label};
use crate::controls::{self, Variant};
use crate::theme::{layout, space, typography};
use crate::ui::{muted, palette, paragraph, AppModel, Shell};

/// How often the queue is read again, as Tauri's 15 s timer.
const POLL: Duration = Duration::from_secs(15);
/// How often to check whether Generelt is shown (and the queue open).
const WATCH: Duration = Duration::from_secs(1);

/// Policy, targets and jobs, as Tauri's `delivery_status`.
#[derive(Clone)]
pub(crate) struct Status {
    pub(crate) policy: Policy,
    pub(crate) targets: Vec<Target>,
    pub(crate) jobs: Vec<JobStatus>,
}

impl Status {
    /// Nothing is set up yet: the closed default policy, no targets or jobs.
    pub(crate) fn empty() -> Self {
        Status {
            policy: Policy::default(),
            targets: Vec::new(),
            jobs: Vec::new(),
        }
    }

    /// Whether the privacy settings block delivery (Tauri's `blocked`).
    fn blocked(&self) -> bool {
        self.policy.local_only || self.policy.private
    }

    /// Changes only, so the 15 s poll does not redraw an unchanged page.
    fn fingerprint(&self) -> String {
        serde_json::json!([self.policy, self.targets, self.jobs]).to_string()
    }
}

/// The delivery state shared by every window (part of `SettingsModel`).
#[derive(Default)]
pub(crate) struct DeliveryModel {
    service: Option<Arc<OnceCell<Delivery>>>,
    pub(crate) status: Option<Status>,
    fingerprint: String,
    busy: bool,
    error: Option<&'static str>,
    message: &'static str,
}

impl std::fmt::Debug for DeliveryModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeliveryModel")
            .field("loaded", &self.status.is_some())
            .field("busy", &self.busy)
            .field("error", &self.error)
            .finish_non_exhaustive()
    }
}

impl DeliveryModel {
    /// The host's queue, once it is open (the meeting page approves into it).
    pub(crate) fn service(&self) -> Option<Delivery> {
        self.service.as_ref().and_then(|s| s.get()).cloned()
    }

    /// A loaded, empty queue for screenshot fixtures.
    #[cfg(feature = "screenshot-tests")]
    pub(crate) fn fixture() -> Self {
        let mut model = DeliveryModel {
            service: Some(Arc::new(OnceCell::new())),
            ..Default::default()
        };
        model.set_status(Status::empty());
        model
    }

    /// Take the result of a background read; `None` means the queue is not
    /// open. A success clears the "could not be read" line; a failure shows
    /// it only while nothing is loaded. Returns whether to redraw.
    fn loaded(
        &mut self,
        result: Option<Result<Result<Status, Error>, tokio::task::JoinError>>,
    ) -> bool {
        match result {
            Some(Ok(Ok(status))) => {
                let cleared = self.error == Some(strings::DELIVERY_LOAD_FAILED);
                if cleared {
                    self.error = None;
                }
                self.set_status(status) || cleared
            }
            _ if self.status.is_none() && self.error.is_none() => {
                self.error = Some(strings::DELIVERY_LOAD_FAILED);
                true
            }
            _ => false,
        }
    }

    fn set_status(&mut self, status: Status) -> bool {
        let fingerprint = status.fingerprint();
        let changed = self.status.is_none() || fingerprint != self.fingerprint;
        self.fingerprint = fingerprint;
        self.status = Some(status);
        changed
    }
}

pub(crate) async fn fetch(service: &Delivery) -> Result<Status, Error> {
    Ok(Status {
        policy: service.policy().await?,
        targets: service.targets().await?,
        jobs: service.jobs().await?,
    })
}

/// Give the settings page the host's queue and keep its status fresh while
/// Generelt is shown. Tauri reads only while `DeliverySettings` is mounted:
/// once on mount, then every 15 s. Elsewhere this only looks at the page.
pub(crate) fn start(model: Entity<AppModel>, service: Arc<OnceCell<Delivery>>, cx: &mut App) {
    model.update(cx, |m, _| m.settings.delivery.service = Some(service));
    cx.spawn(async move |cx| {
        // When the queue was last read; `None` while Generelt is hidden, so
        // showing it again reads at once.
        let mut last_read: Option<Instant> = None;
        loop {
            if !cx.update(|cx| crate::ui::settings_general_shown(cx)) {
                last_read = None;
            } else if last_read.is_none_or(|at| at.elapsed() >= POLL) {
                let (runtime, service) = model.read_with(cx, |m, _| {
                    (
                        m.runtime.clone(),
                        m.settings
                            .delivery
                            .service
                            .as_ref()
                            .and_then(|s| s.get())
                            .cloned(),
                    )
                });
                let result = match service {
                    Some(service) => {
                        last_read = Some(Instant::now());
                        Some(runtime.spawn(async move { fetch(&service).await }).await)
                    }
                    // Not open (yet): the Tauri app owns the queue, or it
                    // could not be opened. Look again in a second.
                    None => None,
                };
                model.update(cx, |m, cx| {
                    if m.settings.delivery.loaded(result) {
                        cx.notify();
                    }
                });
            }
            cx.background_executor().timer(WATCH).await;
        }
    })
    .detach();
}

/// Run one action on the queue, then read the status again. `done` is the
/// line shown afterwards; `on_saved` runs in the window after a success.
fn act<W, F>(
    model: Entity<AppModel>,
    done: &'static str,
    work: W,
    window: &mut Window,
    cx: &mut App,
    on_saved: impl FnOnce(&mut Window, &mut App) + 'static,
) where
    W: FnOnce(Delivery) -> F + Send + 'static,
    F: Future<Output = Result<(), Error>> + Send + 'static,
{
    let (runtime, service, busy) = model.read_with(cx, |m, _| {
        let delivery = &m.settings.delivery;
        (
            m.runtime.clone(),
            delivery.service.as_ref().and_then(|s| s.get()).cloned(),
            delivery.busy,
        )
    });
    if busy {
        return;
    }
    let Some(service) = service else {
        model.update(cx, |m, cx| {
            m.settings.delivery.error = Some(strings::DELIVERY_LOAD_FAILED);
            cx.notify();
        });
        return;
    };
    model.update(cx, |m, cx| {
        let delivery = &mut m.settings.delivery;
        delivery.busy = true;
        delivery.error = None;
        delivery.message = "";
        cx.notify();
    });
    let task = runtime.spawn(async move {
        work(service.clone()).await?;
        fetch(&service).await
    });
    window
        .spawn(cx, async move |cx| {
            let result = task.await;
            let saved = matches!(result, Ok(Ok(_)));
            let _ = cx.update(|window, cx| {
                model.update(cx, |m, cx| {
                    let delivery = &mut m.settings.delivery;
                    delivery.busy = false;
                    match result {
                        Ok(Ok(status)) => {
                            delivery.set_status(status);
                            delivery.message = done;
                        }
                        // Fixed text only: never the target, URL or SSH output.
                        _ => delivery.error = Some(strings::DELIVERY_ACTION_FAILED),
                    }
                    cx.notify();
                });
                if saved {
                    on_saved(window, cx);
                }
            });
        })
        .detach();
}

type Choices = SelectState<Vec<&'static str>>;

const KINDS: [&str; 3] = [
    strings::DELIVERY_KIND_FOLDER,
    strings::DELIVERY_KIND_WEBHOOK,
    strings::DELIVERY_KIND_SSH,
];
const FOLDER: usize = 0;
const WEBHOOK: usize = 1;
const SSH: usize = 2;

const FORMATS: [(WebhookFormat, &str); 4] = [
    (WebhookFormat::Json, strings::DELIVERY_FORMAT_JSON),
    (WebhookFormat::Slack, strings::DELIVERY_FORMAT_SLACK),
    (WebhookFormat::Discord, strings::DELIVERY_FORMAT_DISCORD),
    (WebhookFormat::Teams, strings::DELIVERY_FORMAT_TEAMS),
];

const DEFAULT_PORT: &str = "22";

/// The add-target form's controls.
#[derive(Clone)]
struct Fields {
    kind: Entity<Choices>,
    format: Entity<Choices>,
    name: Entity<InputState>,
    path: Entity<InputState>,
    url: Entity<InputState>,
    host: Entity<InputState>,
    port: Entity<InputState>,
    user: Entity<InputState>,
    key_path: Entity<InputState>,
    remote_dir: Entity<InputState>,
}

impl Fields {
    fn kind(&self, cx: &App) -> usize {
        self.kind
            .read(cx)
            .selected_index(cx)
            .map_or(FOLDER, |i| i.row.min(SSH))
    }

    fn text(input: &Entity<InputState>, cx: &App) -> String {
        input.read(cx).value().trim().to_string()
    }

    /// The target the form describes, or `None` if the port is not a number
    /// (everything else is checked by core's `Target::validate`).
    fn target(&self, cx: &App) -> Option<Target> {
        let text = |input| Self::text(input, cx);
        let destination = match self.kind(cx) {
            WEBHOOK => {
                let at = self
                    .format
                    .read(cx)
                    .selected_index(cx)
                    .map_or(0, |i| i.row.min(FORMATS.len() - 1));
                Destination::Webhook {
                    format: FORMATS[at].0,
                }
            }
            SSH => Destination::Ssh {
                host: text(&self.host),
                port: text(&self.port).parse().ok()?,
                user: text(&self.user),
                key_path: PathBuf::from(text(&self.key_path)),
                remote_dir: text(&self.remote_dir),
            },
            _ => Destination::Folder {
                path: PathBuf::from(text(&self.path)),
            },
        };
        Some(Target {
            id: delivery::new_target_id(),
            name: text(&self.name),
            destination,
        })
    }

    /// Empty the form after a save, as Tauri's `form.reset()`.
    fn reset(&self, window: &mut Window, cx: &mut App) {
        for input in [
            &self.name,
            &self.path,
            &self.url,
            &self.host,
            &self.user,
            &self.key_path,
            &self.remote_dir,
        ] {
            input.update(cx, |input, cx| input.set_value("", window, cx));
        }
        self.port
            .update(cx, |input, cx| input.set_value(DEFAULT_PORT, window, cx));
    }
}

/// Per-window state of the delivery block (part of `SettingsView`).
pub(crate) struct DeliveryForm {
    fields: Fields,
    _subscriptions: Vec<Subscription>,
}

impl DeliveryForm {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Shell>) -> Self {
        let choices = |items: Vec<&'static str>, window: &mut Window, cx: &mut Context<Shell>| {
            cx.new(|cx| SelectState::new(items, Some(IndexPath::new(0)), window, cx))
        };
        let kind = choices(KINDS.to_vec(), window, cx);
        let format = choices(FORMATS.iter().map(|(_, l)| *l).collect(), window, cx);
        let mut input = |placeholder: Option<&'static str>, masked: bool| {
            cx.new(|cx| {
                let state = InputState::new(window, cx).masked(masked);
                match placeholder {
                    Some(text) => state.placeholder(text),
                    None => state,
                }
            })
        };
        let fields = Fields {
            name: input(None, false),
            path: input(None, false),
            // The address holds the webhook's token: never shown in clear.
            url: input(None, true),
            host: input(None, false),
            port: input(Some(DEFAULT_PORT), false),
            user: input(None, false),
            key_path: input(None, false),
            remote_dir: input(None, false),
            kind,
            format,
        };
        fields
            .port
            .update(cx, |input, cx| input.set_value(DEFAULT_PORT, window, cx));
        // The fields under the type follow the chosen type.
        let subscriptions = vec![cx.subscribe_in(
            &fields.kind,
            window,
            |_: &mut Shell, _, _: &SelectEvent<Vec<&'static str>>, _, cx| cx.notify(),
        )];
        DeliveryForm {
            fields,
            _subscriptions: subscriptions,
        }
    }
}

/// Save the form as a new target. The webhook address is cleared from the
/// form even if saving fails, as in Tauri.
fn save(fields: Fields, model: Entity<AppModel>, window: &mut Window, cx: &mut App) {
    let Some(target) = fields.target(cx) else {
        model.update(cx, |m, cx| {
            let delivery = &mut m.settings.delivery;
            delivery.message = "";
            delivery.error = Some(strings::DELIVERY_ACTION_FAILED);
            cx.notify();
        });
        return;
    };
    let url = (fields.kind(cx) == WEBHOOK).then(|| Fields::text(&fields.url, cx));
    fields
        .url
        .update(cx, |input, cx| input.set_value("", window, cx));
    act(
        model,
        strings::DELIVERY_SAVED,
        move |service| async move { service.save_target(target, url).await },
        window,
        cx,
        move |window, cx| fields.reset(window, cx),
    );
}

/// "Fjern målet …?" as Tauri's `window.confirm`, styled as the meeting
/// delete dialog.
fn confirm_remove(target: Target, model: Entity<AppModel>, window: &mut Window, cx: &mut App) {
    let muted_color = palette(cx).muted_foreground;
    let question = strings::delivery_remove_confirm(&target.name);
    window.open_dialog(cx, move |dialog, _, _| {
        let model = model.clone();
        let id = target.id.clone();
        dialog
            .w(crate::theme::sidebar::CONFIRM_DIALOG_WIDTH)
            .title(
                div()
                    .font_family(typography::SERIF)
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_size(typography::XL)
                    .child(strings::DELIVERY_REMOVE),
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
                        controls::button("delivery-remove-cancel", Variant::Ghost)
                            .label(strings::DIALOG_CANCEL)
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        controls::button("delivery-remove-confirm", Variant::Danger)
                            .label(strings::DELIVERY_REMOVE)
                            .on_click(move |_, window, cx| {
                                window.close_dialog(cx);
                                let id = id.clone();
                                act(
                                    model.clone(),
                                    "",
                                    move |service| async move { service.delete_target(&id).await },
                                    window,
                                    cx,
                                    |_, _| {},
                                );
                            }),
                    ),
            )
    });
}

fn status_label(status: &str) -> &str {
    match status {
        "awaiting_approval" => strings::DELIVERY_STATUS_AWAITING,
        "pending" => strings::DELIVERY_STATUS_PENDING,
        "running" => strings::DELIVERY_STATUS_RUNNING,
        "completed" => strings::DELIVERY_STATUS_COMPLETED,
        "failed" => strings::DELIVERY_STATUS_FAILED,
        other => other,
    }
}

/// "Navn: Venter på aflevering. Antal forsøg: 2."
fn job_line(job: &JobStatus, targets: &[Target]) -> String {
    let target = targets
        .iter()
        .find(|t| t.id == job.target_id)
        .map_or(strings::DELIVERY_REMOVED_TARGET, |t| t.name.as_str());
    strings::DELIVERY_JOB
        .replace("{target}", target)
        .replace("{status}", status_label(&job.status))
        .replace("{attempts}", &job.attempts.to_string())
}

/// Local time as Tauri's `toLocaleString('da-DK')`.
fn local_time(seconds: i64) -> String {
    chrono::DateTime::from_timestamp(seconds, 0)
        .map(|t| {
            t.with_timezone(&chrono::Local)
                .format(strings::DELIVERY_TIME_FORMAT)
                .to_string()
        })
        .unwrap_or_default()
}

/// An 18 px semibold title inside the block (`text-lg font-semibold`).
fn group_title(text: &str) -> impl IntoElement {
    div()
        .text_size(typography::LG)
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .child(text.to_string())
}

/// A labelled form control (`label > span + input`).
fn labelled(label: &str, control: impl IntoElement) -> impl IntoElement {
    v_flex()
        .gap(space::SM)
        .child(div().child(label.to_string()))
        .child(control)
}

impl Shell {
    /// The Aflevering af referat block's controls (the section title and
    /// help text come from `settings_section`).
    pub(crate) fn delivery_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        let state = &self.model.read(cx).settings.delivery;
        let busy = state.busy;
        let mut body = v_flex().gap(space::XL);
        // Without a host (no app data folder) nothing ever reads the queue.
        let error = state.error.or(state
            .service
            .is_none()
            .then_some(strings::DELIVERY_LOAD_FAILED));
        if let Some(error) = error {
            body = body.child(alert("delivery-error", error, cx));
        }
        let line: SharedString = if busy {
            strings::DELIVERY_WORKING.into()
        } else {
            state.message.into()
        };
        body = body.child(
            div()
                .id("delivery-status")
                .role(gpui::Role::Status)
                .aria_label(line.clone())
                .min_h(space::XL)
                .child(paragraph(&line, cx)),
        );
        let Some(status) = state.status.clone() else {
            let model = self.model.clone();
            return body
                .child(
                    h_flex().child(
                        controls::button("delivery-refresh", Variant::Outline)
                            .label(strings::DELIVERY_REFRESH)
                            .disabled(busy)
                            .on_click(move |_, window, cx| {
                                act(
                                    model.clone(),
                                    "",
                                    |_| async { Ok(()) },
                                    window,
                                    cx,
                                    |_, _| {},
                                )
                            }),
                    ),
                )
                .into_any_element();
        };
        let blocked = status.blocked();
        body.child(self.delivery_privacy(&status, busy))
            .child(self.delivery_targets(&status, busy, blocked, cx))
            .child(self.delivery_form(busy, cx))
            .child(self.delivery_queue(&status, busy, blocked, cx))
            .into_any_element()
    }

    fn delivery_privacy(&self, status: &Status, busy: bool) -> AnyElement {
        let policy = status.policy;
        let mut group = v_flex()
            .id("delivery-privacy")
            .role(gpui::Role::Group)
            .aria_label(SharedString::from(strings::DELIVERY_PRIVACY))
            .gap(space::SM)
            .child(field_label(strings::DELIVERY_PRIVACY));
        for (index, (label, checked)) in [
            (strings::DELIVERY_LOCAL_ONLY, policy.local_only),
            (strings::DELIVERY_PRIVATE, policy.private),
        ]
        .into_iter()
        .enumerate()
        {
            let model = self.model.clone();
            group = group.child(
                h_flex().min_h(layout::CONTROL_HEIGHT).child(
                    Checkbox::new(("delivery-policy", index))
                        .label(label)
                        .checked(checked)
                        .disabled(busy)
                        .on_click(move |value, window, cx| {
                            let mut next = policy;
                            if index == 0 {
                                next.local_only = *value;
                            } else {
                                next.private = *value;
                            }
                            act(
                                model.clone(),
                                "",
                                move |service| async move { service.set_policy(next).await },
                                window,
                                cx,
                                |_, _| {},
                            );
                        }),
                ),
            );
        }
        group.into_any_element()
    }

    fn delivery_targets(
        &self,
        status: &Status,
        busy: bool,
        blocked: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let palette = palette(cx);
        let mut list = v_flex()
            .gap(space::LG)
            .child(group_title(strings::DELIVERY_TARGETS));
        if status.targets.is_empty() {
            list = list.child(paragraph(strings::DELIVERY_NO_TARGETS, cx));
        }
        for (index, target) in status.targets.iter().enumerate() {
            let test_model = self.model.clone();
            let remove_model = self.model.clone();
            let test_id = target.id.clone();
            let remove_target = target.clone();
            list = list.child(
                h_flex()
                    .flex_wrap()
                    .gap(space::MD)
                    .pb(space::LG)
                    .border_b_1()
                    .border_color(palette.border)
                    .child(div().min_w_0().flex_1().child(paragraph(&target.name, cx)))
                    .child(
                        controls::button(("delivery-test", index), Variant::Outline)
                            .label(strings::DELIVERY_TEST)
                            .disabled(busy || blocked)
                            .on_click(move |_, window, cx| {
                                let id = test_id.clone();
                                act(
                                    test_model.clone(),
                                    strings::DELIVERY_TESTED,
                                    move |service| async move {
                                        service.test_target(&id).await.map(|_| ())
                                    },
                                    window,
                                    cx,
                                    |_, _| {},
                                );
                            }),
                    )
                    .child(
                        controls::button(("delivery-remove", index), Variant::Outline)
                            .label(strings::DELIVERY_REMOVE)
                            .disabled(busy)
                            .on_click(move |_, window, cx| {
                                confirm_remove(
                                    remove_target.clone(),
                                    remove_model.clone(),
                                    window,
                                    cx,
                                )
                            }),
                    ),
            );
        }
        list.into_any_element()
    }

    fn delivery_form(&self, busy: bool, cx: &mut Context<Self>) -> AnyElement {
        let fields = self.settings.delivery.fields.clone();
        let kind = fields.kind(cx);
        let input = |state: &Entity<InputState>, label: &str| {
            labelled(
                label,
                // `Input::h` only sizes multi-line inputs; the style sets
                // the 44 px of Tauri's `control`.
                gpui::Styled::h(controls::input(state), layout::CONTROL_HEIGHT).disabled(busy),
            )
        };
        let select = |state: &Entity<Choices>, label: &'static str| {
            labelled(
                label,
                Select::new(state)
                    .accessibility_label(label)
                    .disabled(busy)
                    .w_full()
                    .h(layout::CONTROL_HEIGHT),
            )
        };
        let mut form = v_flex()
            .id("delivery-form")
            .role(gpui::Role::Form)
            .aria_label(SharedString::from(strings::DELIVERY_ADD))
            .w_full()
            .gap(space::LG)
            .child(group_title(strings::DELIVERY_ADD))
            .child(input(&fields.name, strings::DELIVERY_NAME))
            .child(select(&fields.kind, strings::DELIVERY_KIND));
        form = match kind {
            WEBHOOK => form
                .child(input(&fields.url, strings::DELIVERY_URL))
                .child(select(&fields.format, strings::DELIVERY_FORMAT)),
            SSH => form
                .child(input(&fields.host, strings::DELIVERY_HOST))
                .child(input(&fields.port, strings::DELIVERY_PORT))
                .child(input(&fields.user, strings::DELIVERY_USER))
                .child(input(&fields.key_path, strings::DELIVERY_KEY_PATH))
                .child(input(&fields.remote_dir, strings::DELIVERY_REMOTE_DIR))
                .child(muted(strings::DELIVERY_SSH_HELP, cx)),
            _ => form.child(input(&fields.path, strings::DELIVERY_PATH)),
        };
        let model = self.model.clone();
        form.child(
            h_flex().child(
                controls::button("delivery-save", Variant::Primary)
                    .label(strings::DELIVERY_SAVE)
                    .disabled(busy)
                    .on_click(move |_, window, cx| save(fields.clone(), model.clone(), window, cx)),
            ),
        )
        .into_any_element()
    }

    fn delivery_queue(
        &self,
        status: &Status,
        busy: bool,
        blocked: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let palette = palette(cx);
        let mut queue = v_flex()
            .gap(space::LG)
            .child(group_title(strings::DELIVERY_QUEUE));
        if blocked {
            queue = queue.child(paragraph(strings::DELIVERY_PAUSED, cx));
        }
        if status.jobs.is_empty() {
            queue = queue.child(paragraph(strings::DELIVERY_NO_JOBS, cx));
        }
        for (index, job) in status.jobs.iter().enumerate() {
            let mut row = v_flex()
                .gap(space::SM)
                .pb(space::LG)
                .border_b_1()
                .border_color(palette.border)
                .child(paragraph(&job_line(job, &status.targets), cx));
            if let Some(error) = &job.error {
                row = row.child(div().text_color(palette.destructive).child(error.clone()));
            }
            if job.status == "pending" && !blocked && job.attempts > 0 {
                row = row.child(paragraph(
                    &strings::delivery_next_attempt(&local_time(job.next_attempt)),
                    cx,
                ));
            }
            if matches!(job.status.as_str(), "pending" | "failed") {
                let model = self.model.clone();
                let id = job.id.clone();
                row = row.child(
                    h_flex().child(
                        controls::button(("delivery-retry", index), Variant::Outline)
                            .label(strings::DELIVERY_RETRY)
                            .disabled(busy || blocked)
                            .on_click(move |_, window, cx| {
                                let id = id.clone();
                                act(
                                    model.clone(),
                                    "",
                                    move |service| async move { service.retry(&id).await },
                                    window,
                                    cx,
                                    |_, _| {},
                                );
                            }),
                    ),
                );
            }
            queue = queue.child(row);
        }
        queue.into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(target_id: &str, status: &str, attempts: i64) -> JobStatus {
        JobStatus {
            id: "job".into(),
            target_id: target_id.into(),
            status: status.into(),
            attempts,
            next_attempt: 0,
            error: None,
            meeting_id: None,
        }
    }

    #[test]
    fn a_job_names_its_target_and_status_as_in_tauri() {
        let targets = vec![Target {
            id: "a".into(),
            name: "Arkiv".into(),
            destination: Destination::Folder {
                path: PathBuf::from("/arkiv"),
            },
        }];
        assert_eq!(
            job_line(&job("a", "pending", 2), &targets),
            "Arkiv: Venter på aflevering. Antal forsøg: 2."
        );
        assert_eq!(
            job_line(&job("gone", "failed", 5), &targets),
            "Fjernet mål: Kræver handling. Antal forsøg: 5."
        );
    }

    #[test]
    fn the_default_policy_blocks_delivery() {
        // Core's missing policy is closed (Kun lokal), so a fresh install
        // shows the queue as paused.
        assert!(Status::empty().blocked());
        let open = Status {
            policy: Policy {
                local_only: false,
                private: false,
            },
            ..Status::empty()
        };
        assert!(!open.blocked());
    }

    #[test]
    fn the_poll_redraws_only_on_changes() {
        let mut model = DeliveryModel::default();
        assert!(model.set_status(Status::empty()));
        assert!(!model.set_status(Status::empty()));
        let mut changed = Status::empty();
        changed.jobs.push(job("a", "pending", 0));
        assert!(model.set_status(changed));
    }

    #[test]
    fn a_read_clears_only_the_load_error() {
        let mut model = DeliveryModel::default();
        // The queue is not open: the line shows before any click.
        assert!(model.loaded(None));
        assert_eq!(model.error, Some(strings::DELIVERY_LOAD_FAILED));
        assert!(!model.loaded(None), "no redraw for the same line");
        // A later read succeeds: the block appears without the stale line.
        assert!(model.loaded(Some(Ok(Ok(Status::empty())))));
        assert_eq!(model.error, None);
        assert!(model.status.is_some());
        // A failed read keeps what is loaded and adds no line.
        let failed = Err(Error::Storage);
        assert!(!model.loaded(Some(Ok(failed))));
        assert_eq!(model.error, None);
        // An action's error stays until the user acts again.
        model.error = Some(strings::DELIVERY_ACTION_FAILED);
        assert!(!model.loaded(Some(Ok(Ok(Status::empty())))));
        assert_eq!(model.error, Some(strings::DELIVERY_ACTION_FAILED));
    }
}
