//! Review of AI proposals for a meeting's notes and summary (#205).
//!
//! The band "N ventende ændringer", a diff per proposal and the buttons
//! Anvend / Afvis / Fortryd. Nothing here writes notes or summary directly:
//! every change goes through `ProposalStore::apply/undo`, and only from a
//! button click. The view is meant to sit in the meeting document (#51); until
//! that page exists on `main` it is not mounted in `ui.rs`.

use crate::{
    theme::{self, space, typography, Palette},
    ui::section_title,
};
use gpui::{div, prelude::*, AnyElement, Context, FontWeight, Window};
use gpui_component::{
    button::{Button, ButtonVariants},
    h_flex, v_flex, ActiveTheme,
};
use tingskriver_core::{
    proposals::{DiffKind, Proposal, ProposalError, ProposalStatus, ProposalStore, UserConfirmed},
    strings,
};
use tokio::runtime::Handle;

/// At most this many already-handled proposals are listed below the pending ones.
const HISTORY_LIMIT: usize = 5;
/// The diff scrolls inside its card above this height.
const DIFF_MAX_HEIGHT: gpui::Pixels = gpui::px(320.0);

pub enum ReviewState {
    Loading,
    Failed,
    Loaded(Vec<Proposal>),
}

pub struct ProposalReview {
    store: Option<ProposalStore>,
    meeting_id: String,
    runtime: Handle,
    state: ReviewState,
    /// The last error, in Danish, shown above the list.
    message: Option<String>,
    busy: bool,
}

#[derive(Clone, Copy)]
enum Action {
    Apply,
    Reject,
    Undo,
}

impl ProposalReview {
    pub fn new(
        store: ProposalStore,
        meeting_id: String,
        runtime: Handle,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut view = Self::with_state(Some(store), meeting_id, runtime, ReviewState::Loading);
        view.reload(cx);
        view
    }

    /// A view with fixed contents, for mock data and previews. Buttons do nothing.
    pub fn with_state(
        store: Option<ProposalStore>,
        meeting_id: String,
        runtime: Handle,
        state: ReviewState,
    ) -> Self {
        Self {
            store,
            meeting_id,
            runtime,
            state,
            message: None,
            busy: false,
        }
    }

    pub fn reload(&mut self, cx: &mut Context<Self>) {
        let Some(store) = self.store.clone() else {
            return;
        };
        let meeting_id = self.meeting_id.clone();
        let task = self
            .runtime
            .spawn(async move { store.list(&meeting_id).await });
        cx.spawn(async move |view, cx| {
            let result = task.await;
            let _ = view.update(cx, |view, cx| {
                view.state = match result {
                    Ok(Ok(list)) => ReviewState::Loaded(list),
                    _ => ReviewState::Failed,
                };
                cx.notify();
            });
        })
        .detach();
    }

    fn run(&mut self, action: Action, id: String, cx: &mut Context<Self>) {
        let Some(store) = self.store.clone() else {
            return;
        };
        if self.busy {
            return;
        }
        self.busy = true;
        self.message = None;
        cx.notify();
        let task = self.runtime.spawn(async move {
            // The token exists only because a button was clicked.
            let confirmed = UserConfirmed::from_ui_click();
            match action {
                Action::Apply => store.apply(&id, confirmed).await,
                Action::Reject => store.reject(&id).await,
                Action::Undo => store.undo(&id, confirmed).await,
            }
        });
        cx.spawn(async move |view, cx| {
            let result: Result<Proposal, ProposalError> = task
                .await
                .unwrap_or_else(|_| Err(ProposalError::Storage("task ended".into())));
            let _ = view.update(cx, |view, cx| {
                view.busy = false;
                if let Err(error) = result {
                    view.message = Some(error.user_message());
                }
                view.reload(cx);
                cx.notify();
            });
        })
        .detach();
    }

    fn card(&self, index: usize, proposal: &Proposal, cx: &mut Context<Self>) -> AnyElement {
        let palette = Palette::for_mode(cx.theme().mode);
        let id = proposal.id.clone();
        let pending = proposal.status == ProposalStatus::Pending;
        let applied = proposal.status == ProposalStatus::Applied;
        let lines = proposal.diff_lines();
        let mut diff = v_flex()
            .id(("proposal-diff", index))
            .max_h(DIFF_MAX_HEIGHT)
            .overflow_y_scroll()
            .rounded(theme::RADIUS)
            .border_1()
            .border_color(palette.border);
        if lines.is_empty() {
            diff = diff.child(div().p(space::SM).child(strings::PROPOSAL_NO_LINES));
        }
        for line in lines {
            // The symbol and the label make the change readable without colour.
            let (symbol, tint) = match line.kind {
                DiffKind::Insert => (strings::PROPOSAL_SYMBOL_ADDED, Some(palette.success)),
                DiffKind::Delete => (strings::PROPOSAL_SYMBOL_REMOVED, Some(palette.destructive)),
                DiffKind::Equal => (strings::PROPOSAL_SYMBOL_UNCHANGED, None),
            };
            diff = diff.child(
                h_flex()
                    .gap(space::SM)
                    .px(space::SM)
                    .when_some(tint, |row, color| row.bg(color.opacity(0.14)))
                    .child(
                        div()
                            .w(space::LG)
                            .flex_none()
                            .font_weight(FontWeight::BOLD)
                            .child(symbol),
                    )
                    .child(div().flex_1().min_w_0().child(line.text)),
            );
        }
        let title = strings::proposal_title(proposal.field.label(), proposal.status.label());
        v_flex()
            .gap(space::SM)
            .p(space::LG)
            .rounded(theme::RADIUS_LG)
            .border_1()
            .border_color(palette.border)
            .bg(palette.card)
            .child(section_title(&title))
            .child(
                div()
                    .text_size(typography::SM)
                    .text_color(palette.muted_foreground)
                    .child(strings::proposal_source(proposal.source.kind.label())),
            )
            .when_some(proposal.status_reason.clone(), |card, reason| {
                card.child(
                    div()
                        .text_size(typography::SM)
                        .text_color(palette.warning)
                        .child(reason),
                )
            })
            .child(diff)
            .child(
                div()
                    .text_size(typography::XS)
                    .text_color(palette.muted_foreground)
                    .child(strings::PROPOSAL_LEGEND),
            )
            .when(pending || applied, |card| {
                let mut buttons = h_flex().gap(space::SM);
                if pending {
                    let (apply_id, reject_id) = (id.clone(), id.clone());
                    buttons = buttons
                        .child(
                            Button::new(("proposal-apply", index))
                                .primary()
                                .label(strings::PROPOSAL_APPLY)
                                .on_click(cx.listener(move |view, _, _, cx| {
                                    view.run(Action::Apply, apply_id.clone(), cx)
                                })),
                        )
                        .child(
                            Button::new(("proposal-reject", index))
                                .label(strings::PROPOSAL_REJECT)
                                .on_click(cx.listener(move |view, _, _, cx| {
                                    view.run(Action::Reject, reject_id.clone(), cx)
                                })),
                        );
                } else {
                    let undo_id = id.clone();
                    buttons = buttons.child(
                        Button::new(("proposal-undo", index))
                            .label(strings::PROPOSAL_UNDO)
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.run(Action::Undo, undo_id.clone(), cx)
                            })),
                    );
                }
                card.child(buttons)
            })
            .into_any_element()
    }
}

impl Render for ProposalReview {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = Palette::for_mode(cx.theme().mode);
        let mut root = v_flex().gap(space::LG);
        if let Some(message) = &self.message {
            root = root.child(
                div()
                    .text_size(typography::SM)
                    .text_color(palette.destructive)
                    .child(message.clone()),
            );
        }
        let list = match &self.state {
            ReviewState::Loading => {
                return root
                    .child(
                        div()
                            .text_color(palette.muted_foreground)
                            .child(strings::PROPOSALS_LOADING),
                    )
                    .into_any_element()
            }
            ReviewState::Failed => {
                return root
                    .child(
                        div()
                            .text_color(palette.destructive)
                            .child(strings::PROPOSALS_LOAD_FAILED),
                    )
                    .child(
                        Button::new("proposals-retry")
                            .label(strings::RETRY)
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.state = ReviewState::Loading;
                                view.reload(cx);
                                cx.notify();
                            })),
                    )
                    .into_any_element()
            }
            ReviewState::Loaded(list) => list.clone(),
        };
        let pending: Vec<_> = list
            .iter()
            .filter(|p| p.status == ProposalStatus::Pending)
            .collect();
        let handled: Vec<_> = list
            .iter()
            .filter(|p| p.status != ProposalStatus::Pending)
            .take(HISTORY_LIMIT)
            .collect();
        if pending.is_empty() && handled.is_empty() {
            return root
                .child(section_title(strings::PROPOSALS_EMPTY))
                .child(
                    div()
                        .max_w(theme::layout::EMPTY_TEXT_MAX_WIDTH)
                        .text_size(typography::SM)
                        .text_color(palette.muted_foreground)
                        .child(strings::PROPOSALS_EMPTY_HELP),
                )
                .into_any_element();
        }
        if !pending.is_empty() {
            root = root
                .child(section_title(&strings::proposals_banner(pending.len())))
                .child(
                    div()
                        .text_size(typography::SM)
                        .text_color(palette.muted_foreground)
                        .child(strings::PROPOSALS_REVIEW_HELP),
                );
        }
        for (index, proposal) in pending.into_iter().chain(handled).enumerate() {
            root = root.child(self.card(index, proposal, cx));
        }
        root.into_any_element()
    }
}
