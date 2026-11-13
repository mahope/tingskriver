//! "Aflever referatet til" below the summary (Tauri
//! `delivery/DeliveryApproval.tsx`): pick a target, tick the consent box and
//! approve. Approving snapshots the meeting through core and queues it on the
//! host's one delivery service; nothing is sent by merely making a summary.
//!
//! Like Tauri it reads the targets when the meeting opens and again every few
//! seconds, so a target made in Indstillinger shows up at once. The queue
//! itself is shown in Indstillinger.

use std::time::Duration;

use gpui::{div, prelude::*, Context, Entity, SharedString, Task, Window};
use gpui_component::{
    checkbox::Checkbox,
    h_flex,
    select::{Select, SelectState},
    v_flex, Disableable, IndexPath,
};
use tingskriver_core::delivery::{self, Delivery, JobStatus, Target};
use tingskriver_core::strings;

use super::settings_delivery::{fetch, Status};
use crate::controls::{self, Variant};
use crate::startup::DbState;
use crate::theme::{layout, space, typography};
use crate::ui::{muted, palette, paragraph, AppModel};

/// How often the targets are read again while a meeting is open.
const REFRESH: Duration = Duration::from_secs(5);

type Choices = SelectState<Vec<SharedString>>;

/// The job Tauri finds first: an approval already requested (by an MCP
/// client) for this meeting and target, which the click then answers instead
/// of queueing a second delivery.
pub(crate) fn pending_request<'a>(
    jobs: &'a [JobStatus],
    meeting_id: &str,
    target_id: &str,
) -> Option<&'a JobStatus> {
    jobs.iter().find(|job| {
        job.status == "awaiting_approval"
            && job.meeting_id.as_deref() == Some(meeting_id)
            && job.target_id == target_id
    })
}

/// Queue the approved snapshot: answer the pending request if there is one,
/// otherwise add a new job. Core refuses while the privacy settings block
/// delivery and for a meeting without consent (`private`).
pub(crate) async fn approve(
    service: &Delivery,
    bundle: delivery::Bundle,
    meeting_id: &str,
    target_id: &str,
    private: bool,
) -> Result<String, delivery::Error> {
    let jobs = service.jobs().await?;
    match pending_request(&jobs, meeting_id, target_id) {
        Some(job) => {
            service
                .approve_request(&job.id, meeting_id, target_id, bundle, private)
                .await
        }
        None => service.enqueue(target_id, bundle, true, private).await,
    }
}

/// Whether the button can be pressed (Tauri's `disabled` expression).
pub(crate) fn can_approve(
    busy: bool,
    unsaved: bool,
    target: Option<&str>,
    consent: bool,
    status: Option<&Status>,
) -> bool {
    !busy
        && !unsaved
        && target.is_some()
        && consent
        && status.is_some_and(|s| !s.policy.local_only && !s.policy.private)
}

/// The approval block of one open meeting.
pub(crate) struct DeliveryApproval {
    model: Entity<AppModel>,
    meeting_id: String,
    /// The visible summary (Markdown) that an approval sends.
    summary: String,
    /// Edits not saved yet: approval waits (Tauri's `disabled` prop).
    unsaved: bool,
    status: Option<Status>,
    load_failed: bool,
    select: Option<Entity<Choices>>,
    /// The targets the select was last filled with.
    shown: Vec<(String, String)>,
    consent: bool,
    busy: bool,
    message: &'static str,
    error: Option<&'static str>,
    _refresh: Task<()>,
}

impl DeliveryApproval {
    pub(crate) fn new(model: Entity<AppModel>, meeting_id: String, cx: &mut Context<Self>) -> Self {
        let refresh = cx.spawn(async move |this, cx| loop {
            let Ok(read) = this.update(cx, |this, cx| this.read(cx)) else {
                return;
            };
            let result = match read {
                Some(task) => Some(task.await),
                None => None,
            };
            let alive = this.update(cx, |this, cx| {
                this.loaded(result);
                cx.notify();
            });
            if alive.is_err() {
                return;
            }
            cx.background_executor().timer(REFRESH).await;
        });
        DeliveryApproval {
            model,
            meeting_id,
            summary: String::new(),
            unsaved: false,
            status: None,
            load_failed: false,
            select: None,
            shown: Vec::new(),
            consent: false,
            busy: false,
            message: "",
            error: None,
            _refresh: refresh,
        }
    }

    /// A block with a fixed queue for the screenshot fixtures.
    #[cfg(feature = "screenshot-tests")]
    pub(crate) fn fixture(
        model: Entity<AppModel>,
        meeting_id: String,
        status: Status,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut view = DeliveryApproval::new(model, meeting_id, cx);
        view._refresh = Task::ready(());
        view.status = Some(status);
        view
    }

    /// The summary an approval sends: what the page shows now.
    pub(crate) fn set_summary(&mut self, summary: &str) {
        if self.summary != summary {
            self.summary = summary.to_string();
        }
    }

    /// Edits not yet saved block approval, as Tauri's `disabled`.
    #[allow(dead_code)]
    pub(crate) fn set_unsaved(&mut self, unsaved: bool) {
        self.unsaved = unsaved;
    }

    /// Start reading the queue; `None` while the host has not opened it.
    fn read(&self, cx: &mut Context<Self>) -> Option<Task<Result<Status, delivery::Error>>> {
        let model = self.model.read(cx);
        let service = model.settings.delivery.service()?;
        let runtime = model.runtime.clone();
        Some(cx.background_spawn(async move {
            runtime
                .spawn(async move { fetch(&service).await })
                .await
                .unwrap_or(Err(delivery::Error::Storage))
        }))
    }

    fn loaded(&mut self, result: Option<Result<Status, delivery::Error>>) {
        match result {
            Some(Ok(status)) => {
                self.load_failed = false;
                self.status = Some(status);
            }
            // Keep what is shown after a failed read, as Tauri does.
            _ if self.status.is_none() => self.load_failed = true,
            _ => {}
        }
    }

    fn target_id(&self, cx: &gpui::App) -> Option<String> {
        let index = self.select.as_ref()?.read(cx).selected_index(cx)?.row;
        self.shown.get(index).map(|(id, _)| id.clone())
    }

    /// Fill the select from the targets, keeping the chosen one if it is
    /// still there (Tauri's `setTarget` callback).
    fn sync_select(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let targets: Vec<(String, String)> = self
            .status
            .as_ref()
            .map(|s| {
                s.targets
                    .iter()
                    .map(|t: &Target| (t.id.clone(), t.name.clone()))
                    .collect()
            })
            .unwrap_or_default();
        if targets.is_empty() {
            self.shown.clear();
            return;
        }
        let names: Vec<SharedString> = targets
            .iter()
            .map(|(_, name)| SharedString::from(name.clone()))
            .collect();
        match self.select.clone() {
            None => {
                self.select =
                    Some(cx.new(|cx| SelectState::new(names, Some(IndexPath::new(0)), window, cx)));
            }
            Some(select) if targets != self.shown => {
                let current = self.target_id(cx);
                let keep = current
                    .and_then(|id| targets.iter().position(|(t, _)| *t == id))
                    .unwrap_or(0);
                select.update(cx, |state, cx| {
                    state.set_items(names, window, cx);
                    state.set_selected_index(Some(IndexPath::new(keep)), window, cx);
                });
            }
            Some(_) => {}
        }
        self.shown = targets;
    }

    fn approve(&mut self, cx: &mut Context<Self>) {
        let Some(target) = self.target_id(cx) else {
            return;
        };
        let model = self.model.read(cx);
        let (Some(service), DbState::Ready(db)) = (model.settings.delivery.service(), &model.db)
        else {
            self.error = Some(strings::DOC_DELIVERY_FAILED);
            cx.notify();
            return;
        };
        let pool = db.pool().clone();
        let runtime = model.runtime.clone();
        let meeting_id = self.meeting_id.clone();
        let summary = self.summary.clone();
        // Tauri sends `private: !consent`; core refuses a private meeting.
        let private = !self.consent;
        self.busy = true;
        self.message = "";
        self.error = None;
        cx.notify();
        let work = runtime.spawn(async move {
            let bundle = delivery::meeting::snapshot(&pool, &meeting_id, summary).await?;
            approve(&service, bundle, &meeting_id, &target, private).await?;
            fetch(&service).await
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                match result {
                    Ok(Ok(status)) => {
                        this.status = Some(status);
                        this.message = strings::DOC_DELIVERY_QUEUED;
                        // A new approval needs a new consent.
                        this.consent = false;
                    }
                    // Fixed text only: never the target, address or content.
                    _ => this.error = Some(strings::DOC_DELIVERY_FAILED),
                }
                cx.notify();
            });
        })
        .detach();
    }
}

impl Render for DeliveryApproval {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = palette(cx);
        let frame = || {
            v_flex()
                .mt(space::XXL)
                .pt(space::LG)
                .gap(space::SM)
                .border_t_1()
                .border_color(palette.border)
        };
        if self.load_failed {
            return frame()
                .child(
                    div()
                        .id("delivery-approval-load-error")
                        .role(gpui::Role::Alert)
                        .text_color(palette.destructive)
                        .child(strings::DOC_DELIVERY_LOAD_FAILED),
                )
                .into_any_element();
        }
        // Tauri shows nothing until the first read, then a note without targets.
        let Some(status) = self.status.clone() else {
            return div().into_any_element();
        };
        if status.targets.is_empty() {
            return frame()
                .child(muted(strings::DOC_DELIVERY_NO_TARGETS, cx))
                .into_any_element();
        }
        self.sync_select(window, cx);
        let target = self.target_id(cx);
        let blocked = status.policy.local_only || status.policy.private;
        let enabled = can_approve(
            self.busy,
            self.unsaved,
            target.as_deref(),
            self.consent,
            Some(&status),
        );
        let select = self.select.clone().map(|state| {
            Select::new(&state)
                .accessibility_label(strings::DOC_DELIVERY_TITLE)
                .disabled(self.busy)
                .w_full()
                .h(layout::CONTROL_HEIGHT)
        });
        let line: SharedString = if self.busy {
            strings::DOC_DELIVERY_QUEUEING.into()
        } else {
            self.message.into()
        };
        frame()
            .child(div().child(strings::DOC_DELIVERY_TITLE))
            .children(select)
            .child(
                h_flex().min_h(layout::CONTROL_HEIGHT).child(
                    Checkbox::new("delivery-consent")
                        .label(strings::DOC_DELIVERY_CONSENT)
                        .checked(self.consent)
                        .disabled(self.busy)
                        .on_click(cx.listener(|this, value: &bool, _, cx| {
                            this.consent = *value;
                            cx.notify();
                        })),
                ),
            )
            .when(blocked, |this| {
                this.child(paragraph(strings::DOC_DELIVERY_BLOCKED, cx))
            })
            .when(self.unsaved, |this| {
                this.child(paragraph(strings::DOC_DELIVERY_UNSAVED, cx))
            })
            .child(
                h_flex().child(
                    controls::button("delivery-approve", Variant::Primary)
                        .label(strings::DOC_DELIVERY_APPROVE)
                        .disabled(!enabled)
                        .on_click(cx.listener(|this, _, _, cx| this.approve(cx))),
                ),
            )
            .child(
                div()
                    .id("delivery-approval-status")
                    .role(gpui::Role::Status)
                    .aria_label(line.clone())
                    .min_h(typography::BASE)
                    .child(line),
            )
            .when_some(self.error, |this, error| {
                this.child(
                    div()
                        .id("delivery-approval-error")
                        .role(gpui::Role::Alert)
                        .text_color(palette.destructive)
                        .child(error),
                )
            })
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tingskriver_core::delivery::{Destination, Policy};
    use tingskriver_core::providers::secrets::MemoryKeyStore;

    fn job(id: &str, target: &str, status: &str, meeting: Option<&str>) -> JobStatus {
        JobStatus {
            id: id.into(),
            target_id: target.into(),
            status: status.into(),
            attempts: 0,
            next_attempt: 0,
            error: None,
            meeting_id: meeting.map(Into::into),
        }
    }

    fn status(local_only: bool, private: bool) -> Status {
        Status {
            policy: Policy {
                local_only,
                private,
            },
            ..Status::empty()
        }
    }

    #[test]
    fn only_a_request_for_this_meeting_and_target_is_answered() {
        let jobs = vec![
            job("a", "t1", "pending", Some("m1")),
            job("b", "t1", "awaiting_approval", Some("m2")),
            job("c", "t2", "awaiting_approval", Some("m1")),
            job("d", "t1", "awaiting_approval", Some("m1")),
        ];
        assert_eq!(pending_request(&jobs, "m1", "t1").unwrap().id, "d");
        assert!(pending_request(&jobs, "m3", "t1").is_none());
    }

    #[test]
    fn approval_needs_a_target_consent_saved_edits_and_an_open_policy() {
        let open = status(false, false);
        assert!(can_approve(false, false, Some("t"), true, Some(&open)));
        assert!(!can_approve(true, false, Some("t"), true, Some(&open)));
        assert!(!can_approve(false, true, Some("t"), true, Some(&open)));
        assert!(!can_approve(false, false, None, true, Some(&open)));
        assert!(!can_approve(false, false, Some("t"), false, Some(&open)));
        assert!(!can_approve(false, false, Some("t"), true, None));
        for blocked in [status(true, false), status(false, true)] {
            assert!(!can_approve(false, false, Some("t"), true, Some(&blocked)));
        }
    }

    async fn service(dir: &std::path::Path) -> Delivery {
        let service = Delivery::open(dir, Arc::new(MemoryKeyStore::default()))
            .await
            .unwrap();
        service
            .set_policy(Policy {
                local_only: false,
                private: false,
            })
            .await
            .unwrap();
        let folder = dir.join("ud");
        std::fs::create_dir_all(&folder).unwrap();
        service
            .save_target(
                Target {
                    id: delivery::new_target_id(),
                    name: "Arkiv".into(),
                    destination: Destination::Folder { path: folder },
                },
                None,
            )
            .await
            .unwrap();
        service
    }

    #[tokio::test]
    async fn an_approval_queues_one_job_and_a_private_meeting_none() {
        let dir = tempfile::tempdir().unwrap();
        let service = service(dir.path()).await;
        let target = service.targets().await.unwrap().remove(0).id;
        let bundle = delivery::Bundle::test_sample();
        // Without consent the meeting is private: refused, nothing queued.
        assert!(approve(&service, bundle.clone(), "m1", &target, true)
            .await
            .is_err());
        assert!(service.jobs().await.unwrap().is_empty());
        // With consent exactly one pending job is queued.
        approve(&service, bundle.clone(), "m1", &target, false)
            .await
            .unwrap();
        let jobs = service.jobs().await.unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].status, "pending");
    }

    #[tokio::test]
    async fn a_policy_change_blocks_an_approval() {
        let dir = tempfile::tempdir().unwrap();
        let service = service(dir.path()).await;
        let target = service.targets().await.unwrap().remove(0).id;
        service
            .set_policy(Policy {
                local_only: true,
                private: false,
            })
            .await
            .unwrap();
        let result = approve(
            &service,
            delivery::Bundle::test_sample(),
            "m1",
            &target,
            false,
        )
        .await;
        assert!(result.is_err());
        assert!(service.jobs().await.unwrap().is_empty());
    }
}
