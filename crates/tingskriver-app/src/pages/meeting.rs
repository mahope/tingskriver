//! The open meeting as a document (Tauri `MeetingDetails/MeetingDocument.tsx`,
//! `MeetingHeader.tsx`, `SummaryPanel.tsx`, `TranscriptPanel.tsx`): title and
//! time, MCP sharing, the summary toolbar, pending AI proposals, the summary as
//! read-only text and the transcript folded in below. The whole page scrolls
//! as one surface (docs/design-paritet.md, 2.4 and Job 4).
//!
//! Making, stopping and editing the summary lives in `meeting/summary.rs`
//! (the editor is a plain Markdown text field; Tauri uses BlockNote), and
//! "Forbedr" (retranscription) in `meeting/improve.rs`. Delivery is another
//! job's and still shows its note.

mod improve;
mod summary;

use std::rc::Rc;

use gpui::{
    div, list as virtual_list, prelude::*, relative, AnyElement, App, ClipboardItem, Context,
    Entity, FontStyle, FontWeight, ListAlignment, ListState, Pixels, ScrollHandle, SharedString,
    Subscription, Task, Window,
};
use gpui_component::{
    button::{Button, ButtonRounded, ButtonVariants},
    checkbox::Checkbox,
    h_flex,
    input::Textarea,
    menu::{DropdownMenu, PopupMenu, PopupMenuItem},
    spinner::Spinner,
    v_flex, Disableable, Icon, Sizable, WindowExt,
};
use gpui_kit_assets::IconName;
use tingskriver_core::{
    meeting_document::{self, Block, BlockKind, Document, SummaryStatus},
    meeting_list, meeting_summary,
    proposals::ProposalStore,
    strings::{self, meeting_summary as summary_text},
};

use crate::controls::{self, Variant};
use crate::meetings::MeetingsView;
use crate::proposals::{ProposalReview, ProposalsApplied};
use crate::startup::DbState;
use crate::summary_run::Run;
use crate::theme::{self, document as size, icon_size, layout, space, typography, Palette};
use crate::ui::{heading, load_database, AppModel, Level, Shell};

/// Loading state of the open meeting.
pub(crate) enum DocState {
    Loading,
    Failed,
    Ready(Rc<Loaded>),
}

/// A loaded meeting with its summary already split into blocks.
pub(crate) struct Loaded {
    document: Document,
    blocks: Vec<Block>,
}

impl Loaded {
    pub(crate) fn new(document: Document) -> Self {
        let blocks = document
            .summary
            .markdown
            .as_deref()
            .map(meeting_document::summary_blocks)
            .unwrap_or_default();
        Loaded { document, blocks }
    }
}

/// The meeting document view. The shell keeps one for the open meeting and
/// replaces it when another meeting is opened.
pub(crate) struct MeetingView {
    model: Entity<AppModel>,
    meetings: Entity<MeetingsView>,
    pub(crate) meeting_id: String,
    /// The sidebar status last seen; a change (summary done, transcription
    /// finished) reloads the document.
    status: Option<meeting_list::Status>,
    pub(crate) state: DocState,
    pub(crate) transcript_open: bool,
    toggle_focus: gpui::FocusHandle,
    transcript: ListState,
    pub(crate) scroll: ScrollHandle,
    proposals: Option<Entity<ProposalReview>>,
    /// Width of the content area, set by the shell each frame (the window
    /// minus the sidebar); drives the toolbar's label collapse.
    pub(crate) available: Pixels,
    /// Show times in UTC instead of local time (screenshot fixtures, so
    /// goldens do not depend on the machine's time zone).
    utc: bool,
    load: Option<Task<()>>,
    /// Making, stopping and what to say about it (`summary.rs`).
    run: Run,
    /// A loop reads the stored summary while a run goes.
    watching: bool,
    /// The summary text field, while editing.
    editor: Option<summary::Editor>,
    /// Language and template for the next summary.
    choices: summary::Choices,
    /// The "Forbedr" dialog and the retranscription it started.
    improve: improve::Improve,
    _subscriptions: Vec<Subscription>,
}

impl MeetingView {
    pub(crate) fn new(
        model: Entity<AppModel>,
        meetings: Entity<MeetingsView>,
        meeting_id: String,
        cx: &mut Context<Self>,
    ) -> Self {
        let status = meetings
            .read(cx)
            .selected_meeting()
            .filter(|m| m.data.id == meeting_id)
            .map(|m| m.status);
        let subscriptions = vec![cx.observe(&meetings, |this, meetings, cx| {
            let status = meetings
                .read(cx)
                .selected_meeting()
                .filter(|m| m.data.id == this.meeting_id)
                .map(|m| m.status);
            if status.is_some() && status != this.status {
                this.status = status;
                this.reload(cx);
            }
            cx.notify();
        })];
        let mut subscriptions = subscriptions;
        let proposals = match &model.read(cx).db {
            DbState::Ready(db) => {
                let store = ProposalStore::new(db.pool().clone());
                let runtime = model.read(cx).runtime.clone();
                let id = meeting_id.clone();
                let review = cx.new(|cx| ProposalReview::new(store, id, runtime, cx));
                // An applied or undone proposal changed the summary: show it.
                subscriptions.push(cx.subscribe(&review, |this, _, _: &ProposalsApplied, cx| {
                    this.load(false, cx)
                }));
                Some(review)
            }
            _ => None,
        };
        let mut view = MeetingView {
            model,
            meetings,
            meeting_id,
            status,
            state: DocState::Loading,
            transcript_open: false,
            toggle_focus: cx.focus_handle(),
            transcript: ListState::new(0, ListAlignment::Top, space::XXL),
            scroll: ScrollHandle::new(),
            proposals,
            available: layout::DOCUMENT_MAX,
            utc: false,
            load: None,
            run: Run::default(),
            watching: false,
            editor: None,
            choices: summary::Choices::with_template(meeting_summary::DEFAULT_TEMPLATE),
            improve: improve::Improve::default(),
            _subscriptions: subscriptions,
        };
        view.reload(cx);
        view.load_choices(cx);
        view
    }

    /// A view with fixed contents for the screenshot fixtures: no database,
    /// no loading, buttons that need storage do nothing.
    #[cfg(feature = "screenshot-tests")]
    pub(crate) fn fixture(
        model: Entity<AppModel>,
        meetings: Entity<MeetingsView>,
        document: Document,
        proposals: Option<Entity<ProposalReview>>,
        transcript_open: bool,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut view = MeetingView {
            model,
            meetings,
            meeting_id: document.id.clone(),
            status: None,
            state: DocState::Loading,
            transcript_open,
            toggle_focus: cx.focus_handle(),
            transcript: ListState::new(0, ListAlignment::Top, space::XXL),
            scroll: ScrollHandle::new(),
            proposals,
            available: layout::DOCUMENT_MAX,
            utc: true,
            load: None,
            run: Run::default(),
            watching: false,
            editor: None,
            choices: summary::Choices::fixture(),
            improve: improve::Improve::default(),
            _subscriptions: Vec::new(),
        };
        view.set_loaded(Loaded::new(document));
        if transcript_open {
            // Show the transcript as the reference image does.
            view.scroll.scroll_to_bottom();
        }
        view
    }

    fn loaded(&self) -> Option<&Rc<Loaded>> {
        match &self.state {
            DocState::Ready(loaded) => Some(loaded),
            _ => None,
        }
    }

    fn set_loaded(&mut self, loaded: Loaded) {
        // Keep the reading position when only the summary changed.
        let unchanged = matches!(&self.state, DocState::Ready(old)
            if old.document.transcript == loaded.document.transcript);
        if !unchanged {
            self.transcript.reset(loaded.document.transcript.len());
        }
        self.state = DocState::Ready(Rc::new(loaded));
    }

    fn reload(&mut self, cx: &mut Context<Self>) {
        self.load(true, cx);
    }

    /// Read the document again, and the proposals too when `proposals`.
    fn load(&mut self, proposals: bool, cx: &mut Context<Self>) {
        let DbState::Ready(db) = &self.model.read(cx).db else {
            return;
        };
        let pool = db.pool().clone();
        let id = self.meeting_id.clone();
        let task = self
            .model
            .read(cx)
            .runtime
            .spawn(async move { meeting_document::load(&pool, &id).await });
        self.load = Some(cx.spawn(async move |view, cx| {
            let result = task.await;
            let _ = view.update(cx, |view, cx| {
                match result {
                    Ok(Ok(Some(document))) => {
                        view.set_loaded(Loaded::new(document));
                        view.after_load(cx);
                    }
                    // Deleted while open: the sidebar closes the page.
                    Ok(Ok(None)) => {}
                    Ok(Err(error)) => {
                        log::warn!("Could not load the meeting document: {error:#}");
                        view.state = DocState::Failed;
                    }
                    Err(_) => view.state = DocState::Failed,
                }
                if let Some(review) = view.proposals.as_ref().filter(|_| proposals) {
                    review.update(cx, |review, cx| review.reload(cx));
                }
                cx.notify();
            });
        }));
    }

    fn palette(cx: &App) -> &'static Palette {
        crate::ui::palette(cx)
    }

    /// The article's text width: the content area minus the page padding,
    /// at most the document column.
    fn content_width(&self) -> Pixels {
        let column = if self.available < layout::DOCUMENT_MAX {
            self.available
        } else {
            layout::DOCUMENT_MAX
        };
        column - space::XXL * 2.0
    }

    fn header(&self, title: &str, loaded: Option<&Loaded>, cx: &mut Context<Self>) -> AnyElement {
        let palette = Self::palette(cx);
        let id = self.meeting_id.clone();
        let meetings = self.meetings.clone();
        let mut header = v_flex()
            .child(
                h_flex()
                    .items_start()
                    .gap(space::SM)
                    .child(div().min_w_0().child(heading(title, Level::Document)))
                    .child(
                        div().mt(space::XS2).child(
                            controls::small_icon_button(
                                "rename-meeting",
                                IconName::Pencil,
                                strings::DOC_RENAME,
                            )
                            .on_click(move |_, window, cx| {
                                let id = id.clone();
                                meetings.update(cx, |list, cx| list.open_rename(id, window, cx));
                            }),
                        ),
                    ),
            )
            .when_some(loaded, |header, loaded| {
                header.child(
                    div()
                        .mt(space::SM)
                        .text_size(typography::SM)
                        .text_color(palette.muted_foreground)
                        .child(if self.utc {
                            meeting_document::date_line(loaded.document.created_at, &chrono::Utc)
                        } else {
                            meeting_document::date_line(loaded.document.created_at, &chrono::Local)
                        }),
                )
            })
            .when_some(
                loaded.and_then(|loaded| self.header_status(loaded)),
                |header, status| {
                    let failed = status == meeting_list::Status::Failed;
                    header.child(
                        div()
                            .id("meeting-job-status")
                            .role(if failed {
                                gpui::Role::Alert
                            } else {
                                gpui::Role::Status
                            })
                            .mt(space::XS)
                            .text_size(typography::SM)
                            .text_color(if failed {
                                palette.destructive
                            } else {
                                palette.muted_foreground
                            })
                            .child(status.text()),
                    )
                },
            )
            .child(self.sharing(cx));
        if let Some(loaded) = loaded {
            header = header.child(div().mt(space::LG).child(self.toolbar(loaded, cx)));
        }
        header.into_any_element()
    }

    /// The meeting's job status from the sidebar when it is in progress or
    /// went wrong (the old page's status line). Not while the summary panel
    /// already says it: writing, or the summary's own error.
    fn job_status(&self) -> Option<meeting_list::Status> {
        self.status.filter(|status| status.short().is_some())
    }

    fn header_status(&self, loaded: &Loaded) -> Option<meeting_list::Status> {
        let summary = loaded.document.summary.status;
        self.job_status().filter(|status| match status {
            meeting_list::Status::Summarizing => false,
            meeting_list::Status::Failed => summary != SummaryStatus::Failed,
            _ => true,
        })
    }

    /// "Må deles med AI-værktøjer" (Tauri `MeetingSharing`): the meeting's
    /// MCP sharing, moved here from the old Møder page.
    fn sharing(&self, cx: &mut Context<Self>) -> AnyElement {
        let palette = Self::palette(cx);
        let state = self.model.read(cx);
        let listed = match &state.sharing_meetings {
            Some(Ok(meetings)) => meetings.iter().find(|m| m.id == self.meeting_id),
            _ => None,
        };
        let shared = listed.is_some_and(|m| m.shared);
        let ready = listed.is_some() && !state.sharing_pending;
        let model = self.model.clone();
        let id = self.meeting_id.clone();
        let mut block = v_flex().mt(space::LG).gap(space::XS).child(
            h_flex().min_h(layout::CONTROL_HEIGHT).child(
                Checkbox::new("meeting-shared")
                    .label(strings::DOC_SHARED)
                    .text_size(typography::MD)
                    .checked(shared)
                    .disabled(!ready)
                    .on_click(move |checked, _, cx| set_shared(&model, id.clone(), *checked, cx)),
            ),
        );
        let note = |text: &str| {
            div()
                .text_size(typography::SM)
                .text_color(palette.muted_foreground)
                .child(text.to_string())
        };
        match &state.sharing_meetings {
            Some(Err(_)) => {
                block = block.child(
                    div()
                        .id("meeting-sharing-load-error")
                        .role(gpui::Role::Alert)
                        .text_color(palette.destructive)
                        .child(strings::DOC_SHARING_LOAD_FAILED),
                )
            }
            Some(Ok(_)) if listed.is_none() => {
                block = block.child(note(strings::MCP_MEETING_NOT_LISTED)).child(
                    controls::small_button("refresh-mcp-sharing", Variant::Outline)
                        .label(strings::MCP_REFRESH)
                        .disabled(state.sharing_pending)
                        .on_click(|_, _, cx| load_database(cx)),
                )
            }
            _ => {}
        }
        if state.mcp_message == strings::DOC_SHARING_SAVE_FAILED {
            block = block.child(
                div()
                    .id("meeting-sharing-error")
                    .role(gpui::Role::Alert)
                    .text_color(palette.destructive)
                    .child(strings::DOC_SHARING_SAVE_FAILED),
            );
        }
        block.into_any_element()
    }

    /// The two joined button groups under the title (Tauri
    /// `SummaryGeneratorButtonGroup` and `SummaryUpdaterButtonGroup`).
    fn toolbar(&self, loaded: &Loaded, cx: &mut Context<Self>) -> AnyElement {
        let palette = Self::palette(cx);
        let width = self.content_width();
        let labels = width >= size::LABELS_MIN;
        let language_label = width >= size::LANGUAGE_LABEL_MIN;
        let summary = &loaded.document.summary;
        let has_summary = summary.has_content();
        let writing = summary.status == SummaryStatus::Running || self.run.is_active();
        let view = cx.entity().downgrade();
        let mut row = h_flex().flex_wrap().gap(space::SM).min_w_0();
        if !loaded.document.transcript.is_empty() {
            let generate = if self.run.is_running() {
                // Tauri: a destructive "Stop" while the summary is written.
                segment(
                    "summary-stop",
                    IconName::Square,
                    Some(summary_text::SUMMARY_STOP),
                    summary_text::SUMMARY_STOP_TITLE,
                )
                .text_color(palette.destructive)
                .disabled(self.run.stopping)
                .on_click(cx.listener(|this, _, _, cx| this.stop(cx)))
            } else if writing {
                segment(
                    "summary-generate",
                    IconName::LoaderCircle,
                    Some(summary_text::SUMMARY_WORKING),
                    summary_text::SUMMARY_WORKING,
                )
                .text_color(palette.primary)
                .disabled(true)
            } else {
                segment(
                    "summary-generate",
                    IconName::Sparkles,
                    Some(if has_summary {
                        strings::DOC_REGENERATE
                    } else {
                        strings::DOC_GENERATE
                    }),
                    if has_summary {
                        summary_text::SUMMARY_REGENERATE_TITLE
                    } else {
                        summary_text::SUMMARY_GENERATE_TITLE
                    },
                )
                .text_color(palette.primary)
                .on_click(cx.listener(|this, _, _, cx| this.generate(cx)))
            };
            let language_name = self
                .choices
                .language
                .as_deref()
                .map(meeting_summary::language_label)
                .unwrap_or_else(|| strings::DOC_LANGUAGE_AUTO.to_string());
            let (current, recents) = (self.choices.language.clone(), self.choices.recents.clone());
            let language_view = view.clone();
            let language = segment(
                "summary-language",
                IconName::Languages,
                None,
                strings::DOC_LANGUAGE_LABEL,
            )
            .when(language_label, |button| button.label(language_name))
            .dropdown_caret(true)
            .disabled(writing)
            .dropdown_menu(move |menu, _, _| {
                language_menu(menu, &language_view, current.as_deref(), &recents)
            });
            let model = segment(
                "summary-model",
                IconName::Settings,
                labels.then_some(strings::DOC_MODEL),
                summary_text::SUMMARY_MODEL_TITLE,
            )
            .on_click(|_, _, cx| crate::ui::open_summary_settings(cx));
            let mut segments = vec![
                generate.into_any_element(),
                language.into_any_element(),
                model.into_any_element(),
            ];
            if !self.choices.templates.is_empty() {
                let templates = self.choices.templates.clone();
                let chosen = self.choices.template.clone();
                let template_view = view.clone();
                segments.push(
                    segment(
                        "summary-template",
                        IconName::FileText,
                        labels.then_some(strings::DOC_TEMPLATE),
                        summary_text::SUMMARY_TEMPLATE_TITLE,
                    )
                    .disabled(writing)
                    .dropdown_menu(move |menu, _, _| {
                        template_menu(menu, &template_view, &templates, &chosen)
                    })
                    .into_any_element(),
                );
            }
            row = row.child(
                button_group("summary-actions", strings::DOC_ACTIONS, segments, cx)
                    .border_l_0()
                    .child(
                        // Tauri tints the first segment's border (`border-primary/40`).
                        div()
                            .absolute()
                            .top_0()
                            .left_0()
                            .bottom_0()
                            .w(size::HAIRLINE)
                            .bg(palette.primary.opacity(size::GENERATE_BORDER_ALPHA)),
                    ),
            );
        }
        if has_summary && !writing {
            let mut segments = Vec::new();
            if let Some(editor) = &self.editor {
                let dirty = editor.is_dirty(cx);
                segments.push(
                    segment(
                        "summary-save",
                        IconName::Save,
                        Some(if editor.saving {
                            summary_text::SUMMARY_SAVING
                        } else if dirty {
                            summary_text::SUMMARY_SAVE_DIRTY
                        } else {
                            strings::DOC_SAVE
                        }),
                        summary_text::SUMMARY_SAVE_TITLE,
                    )
                    .when(dirty, |button| button.text_color(palette.primary))
                    .disabled(editor.saving)
                    .on_click(cx.listener(|this, _, _, cx| this.save_edit(cx)))
                    .into_any_element(),
                );
                segments.push(
                    segment(
                        "summary-edit-cancel",
                        IconName::X,
                        labels.then_some(summary_text::SUMMARY_EDIT_CANCEL),
                        summary_text::SUMMARY_EDIT_CANCEL_TITLE,
                    )
                    .disabled(editor.saving)
                    .on_click(cx.listener(|this, _, _, cx| this.cancel_edit(cx)))
                    .into_any_element(),
                );
            } else {
                segments.push(
                    segment(
                        "summary-edit",
                        IconName::Pencil,
                        Some(summary_text::SUMMARY_EDIT),
                        summary_text::SUMMARY_EDIT_TITLE,
                    )
                    .on_click(cx.listener(|this, _, window, cx| this.begin_edit(window, cx)))
                    .into_any_element(),
                );
            }
            let document = loaded.document.clone();
            segments.push(
                segment(
                    "summary-copy",
                    IconName::Copy,
                    labels.then_some(strings::DOC_COPY),
                    strings::DOC_COPY_SUMMARY,
                )
                .on_click(move |_, window, cx| {
                    match meeting_document::summary_for_copy(
                        &document,
                        chrono::Utc::now(),
                        &chrono::Local,
                    ) {
                        Some(text) => {
                            cx.write_to_clipboard(ClipboardItem::new_string(text));
                            window.push_notification(strings::DOC_SUMMARY_COPIED, cx);
                        }
                        None => window.push_notification(strings::DOC_NO_SUMMARY_TO_COPY, cx),
                    }
                })
                .into_any_element(),
            );
            row = row.child(button_group(
                "summary-updater",
                strings::DOC_SUMMARY_ACTIONS,
                segments,
                cx,
            ));
        }
        row.into_any_element()
    }

    /// The summary area: writing, empty state, or the rendered Markdown with
    /// the error box after a failed regeneration (Tauri `SummaryPanel`).
    fn summary(&self, loaded: &Loaded, cx: &mut Context<Self>) -> AnyElement {
        let palette = Self::palette(cx);
        let summary = &loaded.document.summary;
        if summary.status == SummaryStatus::Running || self.run.is_active() {
            return v_flex()
                .id("summary-running")
                .role(gpui::Role::Status)
                .items_center()
                .py(space::XXXL + space::XL)
                .child(
                    div().mb(space::LG).child(
                        Spinner::new()
                            .with_size(icon_size::ERROR)
                            .color(palette.primary),
                    ),
                )
                .child(
                    div()
                        .text_color(palette.muted_foreground)
                        .child(strings::DOC_SUMMARY_RUNNING),
                )
                .child(
                    div()
                        .mt(space::XS)
                        .text_size(typography::SM)
                        .text_color(palette.muted_foreground)
                        .child(strings::DOC_SUMMARY_RUNNING_HELP),
                )
                .into_any_element();
        }
        // The page's own failure comes first: a start that was refused (no
        // transcript, a model that is not ready) has no stored error.
        let failure = self.run.error.clone().or_else(|| {
            (summary.status == SummaryStatus::Failed)
                .then(|| summary.error.clone().unwrap_or_default())
        });
        let failed = failure.map(|error| {
            v_flex()
                .id("summary-error")
                .role(gpui::Role::Alert)
                .gap(space::XS)
                .max_w(layout::ONBOARDING_TEXT_MAX)
                .px(space::MD)
                .py(space::SM)
                .rounded(theme::RADIUS)
                .bg(palette.destructive.opacity(size::ERROR_TINT))
                .text_size(typography::SM)
                .text_color(palette.destructive)
                .child(
                    div()
                        .font_weight(FontWeight::MEDIUM)
                        .child(strings::DOC_SUMMARY_FAILED),
                )
                .when(!error.trim().is_empty(), |this| this.child(error))
        });
        if !summary.has_content() {
            // Tauri `EmptyStateSummary`.
            return v_flex()
                .items_center()
                .p(space::XXL)
                .child(
                    Icon::from(IconName::FileQuestionMark)
                        .size(size::EMPTY_ICON)
                        .text_color(palette.muted_foreground),
                )
                .child(
                    div()
                        .mt(space::LG)
                        .mb(space::SM)
                        .font_family(typography::SERIF)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_size(typography::LG)
                        .child(strings::DOC_EMPTY_TITLE),
                )
                .child(
                    div()
                        .mb(space::XL)
                        .max_w(layout::ONBOARDING_TEXT_MAX)
                        .text_center()
                        .text_size(typography::SM)
                        .text_color(palette.muted_foreground)
                        .child(strings::DOC_EMPTY_TEXT),
                )
                .when_some(failed, |this, failed| {
                    this.child(div().mb(space::LG).child(failed))
                })
                .child(
                    controls::button("summary-empty-generate", Variant::Primary)
                        .icon(Icon::from(IconName::Sparkles).size(icon_size::SM))
                        .label(
                            if summary.status == SummaryStatus::Failed || self.run.error.is_some() {
                                strings::DOC_SUMMARY_RETRY
                            } else {
                                strings::DOC_GENERATE
                            },
                        )
                        .disabled(loaded.document.transcript.is_empty())
                        .on_click(cx.listener(|this, _, _, cx| this.generate(cx)))
                        .self_center(),
                )
                .into_any_element();
        }
        if let Some(editor) = &self.editor {
            return v_flex()
                .id("summary-editor")
                .gap(space::SM)
                .child(
                    Textarea::new(&editor.input)
                        .aria_label(summary_text::SUMMARY_EDITOR_LABEL)
                        .h(size::EDITOR_HEIGHT),
                )
                .child(
                    div()
                        .text_size(typography::SM)
                        .text_color(palette.muted_foreground)
                        .child(summary_text::SUMMARY_EDITOR_HELP),
                )
                .when_some(editor.error.clone(), |this, error| {
                    this.child(
                        div()
                            .id("summary-editor-error")
                            .role(gpui::Role::Alert)
                            .text_size(typography::SM)
                            .text_color(palette.destructive)
                            .child(error),
                    )
                })
                .into_any_element();
        }
        v_flex()
            .id("summary")
            .max_w(size::SUMMARY_TEXT_MAX)
            .gap(size::BLOCK_GAP)
            .children(
                loaded
                    .blocks
                    .iter()
                    .enumerate()
                    .map(|(index, block)| summary_block(block, index == 0, palette)),
            )
            .when_some(failed, |this, failed| {
                this.child(div().mt(space::LG).child(failed))
            })
            .into_any_element()
    }

    /// Where the summary goes after approval (Tauri `DeliveryApproval`).
    fn delivery(&self, cx: &mut Context<Self>) -> AnyElement {
        let palette = Self::palette(cx);
        v_flex()
            .mt(space::XXL)
            .pt(space::LG)
            .gap(space::SM)
            .border_t_1()
            .border_color(palette.border)
            .child(strings::DOC_DELIVERY_TITLE)
            .child(
                div()
                    .text_size(typography::SM)
                    .text_color(palette.muted_foreground)
                    .child(strings::DOC_DELIVERY_LATER),
            )
            .into_any_element()
    }

    /// Pending AI proposals, as a card between the toolbar and the summary.
    /// Hidden while loading and when there are none.
    fn proposals_band(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let review = self.proposals.clone()?;
        if !review.read(cx).has_items() {
            return None;
        }
        let palette = Self::palette(cx);
        Some(
            div()
                .id("proposals-band")
                .mt(space::XL)
                .max_h(size::PROPOSALS_MAX)
                .overflow_y_scroll()
                .px(space::LG)
                .py(space::MD)
                .rounded(theme::RADIUS_LG)
                .border_1()
                .border_color(palette.border)
                .bg(palette.card)
                .child(review)
                .into_any_element(),
        )
    }

    /// The folded transcript (Tauri `MeetingDocument` section and
    /// `TranscriptPanel`).
    fn transcript_section(
        &self,
        loaded: &Rc<Loaded>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let palette = Self::palette(cx);
        let open = self.transcript_open;
        let count = loaded.document.transcript.len();
        let toggle = div()
            .id("transcript-toggle")
            .track_focus(&self.toggle_focus)
            .role(gpui::Role::Button)
            .aria_label(SharedString::from(strings::DOC_TRANSCRIPT))
            .aria_expanded(open)
            .w_full()
            .min_h(layout::CONTROL_HEIGHT)
            // The 2 px focus border is always there (transparent) and taken
            // from the padding, so focus does not move the row.
            .px(space::XS - size::FOCUS_BORDER)
            .border(size::FOCUS_BORDER)
            .border_color(gpui::transparent_black())
            .flex()
            .items_center()
            .rounded(theme::RADIUS)
            .hover(|this| this.bg(palette.accent.opacity(theme::LIST_HOVER_ALPHA)))
            .when(self.toggle_focus.is_focused(window), |this| {
                this.border_color(palette.ring)
            })
            .child(
                h_flex()
                    .w_full()
                    .gap(space::SM)
                    .child(
                        Icon::from(if open {
                            IconName::ChevronDown
                        } else {
                            IconName::ChevronRight
                        })
                        .size(icon_size::SM)
                        .text_color(palette.muted_foreground),
                    )
                    .child(
                        div()
                            .text_size(typography::MD)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(palette.foreground)
                            .child(strings::DOC_TRANSCRIPT),
                    )
                    .child(
                        div()
                            .text_size(typography::SM)
                            .text_color(palette.muted_foreground)
                            .child(strings::transcript_lines(count)),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_size(typography::SM)
                            .text_color(palette.muted_foreground)
                            .child(if open {
                                strings::DOC_HIDE
                            } else {
                                strings::DOC_SHOW
                            }),
                    ),
            )
            .on_click(cx.listener(|this, _, _, cx| this.toggle_transcript(cx)))
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, _, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    this.toggle_transcript(cx);
                    cx.stop_propagation();
                }
            }));
        let mut section = v_flex()
            .id("transcript-section")
            .aria_label(SharedString::from(strings::DOC_TRANSCRIPT))
            .mt(size::TRANSCRIPT_GAP)
            .pt(space::LG)
            .border_t_1()
            .border_color(palette.border)
            .child(toggle);
        if open {
            let height = window.viewport_size().height * size::TRANSCRIPT_PANEL_SHARE;
            section = section.child(
                v_flex()
                    .id("transcript-panel")
                    .mt(space::MD)
                    .h(height)
                    .overflow_hidden()
                    .rounded(theme::RADIUS_LG)
                    .border_1()
                    .border_color(palette.border)
                    .bg(palette.card)
                    .child(self.transcript_toolbar(loaded, cx))
                    .child(self.transcript_lines(loaded, cx)),
            );
        }
        section.into_any_element()
    }

    fn toggle_transcript(&mut self, cx: &mut Context<Self>) {
        self.transcript_open = !self.transcript_open;
        cx.notify();
    }

    fn transcript_toolbar(&self, loaded: &Rc<Loaded>, cx: &mut Context<Self>) -> AnyElement {
        let palette = Self::palette(cx);
        let labels = self.content_width() >= size::TRANSCRIPT_LABELS_MIN;
        let document = loaded.document.clone();
        let empty = document.transcript.is_empty();
        let copy = segment(
            "transcript-copy",
            IconName::Copy,
            labels.then_some(strings::DOC_COPY),
            if empty {
                strings::DOC_NO_TRANSCRIPT_TO_COPY
            } else {
                strings::DOC_COPY_TRANSCRIPT
            },
        )
        .disabled(empty)
        .on_click(move |_, window, cx| {
            let text = meeting_document::transcript_for_copy(&document, &chrono::Local);
            cx.write_to_clipboard(ClipboardItem::new_string(text));
            window.push_notification(strings::DOC_TRANSCRIPT_COPIED, cx);
        });
        let mut segments = vec![copy.into_any_element()];
        let folder = loaded
            .document
            .folder_path
            .clone()
            .map(std::path::PathBuf::from);
        let mut no_audio = None;
        match folder {
            Some(folder) => {
                segments.push(
                    segment(
                        "transcript-folder",
                        IconName::FolderOpen,
                        labels.then_some(strings::DOC_RECORDING),
                        strings::DOC_OPEN_RECORDING,
                    )
                    .on_click(move |_, _, _| crate::startup::open_folder(&folder))
                    .into_any_element(),
                );
                segments.push(
                    segment(
                        "transcript-improve",
                        IconName::RefreshCw,
                        labels.then_some(strings::DOC_IMPROVE),
                        summary_text::IMPROVE_TITLE,
                    )
                    .disabled(self.improve.is_running())
                    .on_click(cx.listener(|this, _, window, cx| this.open_improve(window, cx)))
                    .into_any_element(),
                );
            }
            None => no_audio = Some(strings::DOC_NO_AUDIO),
        }
        h_flex()
            .justify_end()
            .gap(space::SM)
            .px(space::MD)
            .py(space::SM)
            .border_b_1()
            .border_color(palette.border)
            .when_some(no_audio, |this, text| {
                this.child(
                    div()
                        .text_size(typography::SM)
                        .text_color(palette.muted_foreground)
                        .child(text),
                )
            })
            .child(button_group(
                "transcript-actions",
                strings::DOC_TRANSCRIPT_ACTIONS,
                segments,
                cx,
            ))
            .into_any_element()
    }

    fn transcript_lines(&self, loaded: &Rc<Loaded>, cx: &mut Context<Self>) -> AnyElement {
        let palette = Self::palette(cx);
        if loaded.document.transcript.is_empty() {
            // Still being transcribed, or the transcription failed: say that
            // rather than "no speech was recognised".
            let (title, help) = match self.job_status() {
                Some(
                    status @ (meeting_list::Status::Recording
                    | meeting_list::Status::Pending
                    | meeting_list::Status::Transcribing
                    | meeting_list::Status::Failed),
                ) => (status.short().unwrap_or_default(), status.text()),
                _ => (
                    strings::DOC_TRANSCRIPT_EMPTY,
                    strings::DOC_TRANSCRIPT_EMPTY_HELP,
                ),
            };
            return v_flex()
                .flex_1()
                .items_center()
                .mt(space::XXL)
                .child(
                    div()
                        .font_weight(FontWeight::MEDIUM)
                        .text_size(typography::MD)
                        .child(title),
                )
                .child(
                    div()
                        .mt(space::XS)
                        .text_size(typography::SM)
                        .text_color(palette.muted_foreground)
                        .child(help),
                )
                .into_any_element();
        }
        let loaded = loaded.clone();
        virtual_list(self.transcript.clone(), move |index, _, cx| {
            let palette = crate::ui::palette(cx);
            let Some(line) = loaded.document.transcript.get(index) else {
                return div().into_any_element();
            };
            h_flex()
                .items_start()
                .gap(space::SM)
                .px(space::LG)
                .when(index == 0, |this| this.pt(space::SM))
                .pb(space::MD)
                .child(
                    div()
                        .flex_none()
                        .min_w(size::TIME_COLUMN)
                        .mt(space::XXS)
                        .text_size(typography::SM)
                        .text_color(palette.muted_foreground)
                        .child(line.time_label()),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(typography::MD)
                        .line_height(relative(size::TRANSCRIPT_LINE_HEIGHT))
                        .child(line.text.clone()),
                )
                .into_any_element()
        })
        .flex_1()
        .min_h_0()
        .into_any_element()
    }
}

impl Render for MeetingView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let title = self
            .meetings
            .read(cx)
            .selected_meeting()
            .filter(|m| m.data.id == self.meeting_id)
            .map(|m| m.data.title.clone())
            .or_else(|| match &self.state {
                DocState::Ready(loaded) => Some(loaded.document.title.clone()),
                _ => None,
            })
            .unwrap_or_default();
        let palette = Self::palette(cx);
        let loaded = match &self.state {
            DocState::Ready(loaded) => Some(loaded.clone()),
            _ => None,
        };
        let mut article = v_flex()
            .w_full()
            .max_w(layout::DOCUMENT_MAX)
            .px(space::XXL)
            .pt(space::XXXL)
            .pb(size::BOTTOM_SPACE)
            .child(self.header(&title, loaded.as_deref(), cx));
        match &loaded {
            None => {
                let body = match self.state {
                    DocState::Failed => div()
                        .id("meeting-load-error")
                        .mt(space::XL)
                        .role(gpui::Role::Alert)
                        .text_color(palette.destructive)
                        .child(strings::DOC_LOAD_FAILED)
                        .into_any_element(),
                    _ => div()
                        .mt(space::XL)
                        .child(crate::ui::loading(strings::DOC_LOADING, cx))
                        .into_any_element(),
                };
                article = article.child(body);
            }
            Some(loaded) => {
                let summary = &loaded.document.summary;
                if let Some(band) = self.proposals_band(cx) {
                    article = article.child(band);
                }
                article = article.child(div().mt(space::XL).child(self.summary(loaded, cx)));
                // Below the summary, as Tauri's `DeliveryApproval`. A note
                // until delivery (queue worker, targets) runs in this app.
                if summary.has_content()
                    && summary.status != SummaryStatus::Running
                    && !self.run.is_active()
                {
                    article = article.child(self.delivery(cx));
                }
                article = article.child(self.transcript_section(loaded, window, cx));
            }
        }
        div()
            .id("meeting")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .child(h_flex().w_full().justify_center().child(article))
    }
}

/// One summary block in BlockNote's look: Literata headings, 16 px text on
/// 28 px lines, square bullets, numbers and task boxes in a 24 px column.
fn summary_block(block: &Block, first: bool, palette: &Palette) -> AnyElement {
    let text = spans(block, palette);
    let body = |depth: u8, marker: AnyElement, text: gpui::StyledText| {
        h_flex()
            .items_start()
            .pl(size::LIST_INDENT * f32::from(depth))
            .child(
                h_flex()
                    .flex_none()
                    .w(size::LIST_INDENT)
                    .h(typography::MD * size::SUMMARY_LINE_HEIGHT)
                    .justify_center()
                    .child(marker),
            )
            .child(div().flex_1().min_w_0().child(text))
            .into_any_element()
    };
    match &block.kind {
        BlockKind::Heading(level) => {
            let font_size = match level {
                1 => size::HEADING_1,
                2 => typography::XXXL,
                _ => typography::XL,
            };
            div()
                .when(!first, |this| this.mt(size::HEADING_GAP))
                .font_family(typography::SERIF)
                .font_weight(FontWeight::SEMIBOLD)
                .text_size(font_size)
                .line_height(relative(typography::LINE_HEIGHT_TIGHT))
                .text_color(palette.foreground)
                .child(text)
                .into_any_element()
        }
        BlockKind::Bullet(depth) => body(
            *depth,
            div()
                .size(size::BULLET)
                .bg(palette.foreground)
                .into_any_element(),
            text,
        ),
        BlockKind::Numbered(number, depth) => body(
            *depth,
            div()
                .text_color(palette.foreground)
                .child(strings::numbered_item(*number))
                .into_any_element(),
            text,
        ),
        BlockKind::Task(done, depth) => body(
            *depth,
            Icon::from(if *done {
                IconName::SquareCheck
            } else {
                IconName::Square
            })
            .size(icon_size::SM)
            .text_color(palette.muted_foreground)
            .into_any_element(),
            text,
        ),
        BlockKind::Quote => div()
            .pl(space::LG)
            .border_l_2()
            .border_color(palette.border)
            .text_color(palette.muted_foreground)
            .child(text)
            .into_any_element(),
        BlockKind::Rule => div()
            .my(space::SM)
            .h(size::HAIRLINE)
            .bg(palette.border)
            .into_any_element(),
        BlockKind::Paragraph => div().child(text).into_any_element(),
    }
}

/// The styled runs of a block as one wrapping text.
fn spans(block: &Block, palette: &Palette) -> gpui::StyledText {
    let mut text = String::new();
    let mut highlights = Vec::new();
    for span in &block.spans {
        let start = text.len();
        text.push_str(&span.text);
        let mut style = gpui::HighlightStyle::default();
        if span.bold {
            style.font_weight = Some(FontWeight::SEMIBOLD);
        }
        if span.italic {
            style.font_style = Some(FontStyle::Italic);
        }
        if span.code {
            style.background_color = Some(palette.muted);
        }
        if span.bold || span.italic || span.code {
            highlights.push((start..text.len(), style));
        }
    }
    gpui::StyledText::new(text).with_highlights(highlights)
}

/// One segment of a joined button group: 32 px, 12 px text, a 16 px icon,
/// and the label hidden in narrow containers (the tooltip names it then).
fn segment(
    id: &'static str,
    icon: IconName,
    label: Option<&'static str>,
    tooltip: &'static str,
) -> Button {
    let button = Button::new(id)
        .ghost()
        .icon(Icon::from(icon).size(icon_size::SM))
        .h(layout::CONTROL_HEIGHT_SM)
        .px(space::MD)
        .gap(space::SM)
        .text_size(typography::XS)
        .font_weight(FontWeight::MEDIUM)
        .rounded(ButtonRounded::None)
        .tooltip(tooltip)
        .accessibility_label(label.unwrap_or(tooltip));
    match label {
        Some(label) => button.label(label),
        None => button,
    }
}

/// Joined outline buttons (Tauri `ButtonGroup`): one `input` border with
/// 6 px outer corners, a divider between segments and `shadow-sm`.
fn button_group(
    id: &'static str,
    label: &'static str,
    segments: Vec<AnyElement>,
    cx: &App,
) -> gpui::Stateful<gpui::Div> {
    let palette = crate::ui::palette(cx);
    h_flex()
        .id(id)
        .role(gpui::Role::Group)
        .aria_label(SharedString::from(label))
        .relative()
        .flex_none()
        .overflow_hidden()
        .rounded(theme::RADIUS)
        .border_1()
        .border_color(palette.input)
        .bg(palette.background)
        .shadow(theme::shadow::sm())
        .children(segments.into_iter().enumerate().map(|(index, button)| {
            div()
                .when(index > 0, |this| {
                    this.border_l_1().border_color(palette.input)
                })
                .child(button)
        }))
}

/// The language menu: Automatisk, the quick picks, then every language.
fn language_menu(
    menu: PopupMenu,
    view: &gpui::WeakEntity<MeetingView>,
    current: Option<&str>,
    recents: &[String],
) -> PopupMenu {
    let item = |label: String, code: Option<String>| {
        let view = view.clone();
        PopupMenuItem::new(label)
            .checked(current == code.as_deref())
            .on_click(move |_, _, cx| {
                let code = code.clone();
                let _ = view.update(cx, |this, cx| this.set_language(code, cx));
            })
    };
    let mut menu = menu
        .max_h(size::MENU_MAX)
        .scrollable(true)
        .item(item(strings::DOC_LANGUAGE_AUTO.to_string(), None));
    if !recents.is_empty() {
        menu = menu.separator();
        for code in recents {
            menu = menu.item(item(
                meeting_summary::language_label(code),
                Some(code.clone()),
            ));
        }
    }
    menu = menu.separator();
    for (code, label) in meeting_summary::LANGUAGES {
        if !recents.iter().any(|recent| recent == code) {
            menu = menu.item(item((*label).to_string(), Some((*code).to_string())));
        }
    }
    menu
}

/// The template menu (Tauri `DropdownMenu`, the description as the tooltip).
fn template_menu(
    menu: PopupMenu,
    view: &gpui::WeakEntity<MeetingView>,
    templates: &[meeting_summary::Template],
    chosen: &str,
) -> PopupMenu {
    let mut menu = menu.max_h(size::MENU_MAX).scrollable(true);
    for template in templates {
        let (view, id) = (view.clone(), template.id.clone());
        menu = menu.item(
            PopupMenuItem::new(template.name.clone())
                .checked(template.id == chosen)
                .on_click(move |_, _, cx| {
                    let id = id.clone();
                    let _ = view.update(cx, |this, cx| this.set_template(id, cx));
                }),
        );
    }
    menu
}

/// Save "Må deles med AI-værktøjer" for one meeting.
fn set_shared(model: &Entity<AppModel>, id: String, shared: bool, cx: &mut App) {
    let task = model.update(cx, |state, cx| {
        if state.sharing_pending {
            return None;
        }
        let DbState::Ready(db) = &state.db else {
            return None;
        };
        let pool = db.pool().clone();
        state.sharing_pending = true;
        cx.notify();
        Some(state.runtime.spawn(async move {
            let result = tingskriver_core::mcp::set_shared(&pool, &id, shared).await;
            (id, result)
        }))
    });
    let Some(task) = task else {
        return;
    };
    let model = model.clone();
    cx.spawn(async move |cx| {
        let result = task.await;
        model.update(cx, |state, cx| {
            state.sharing_pending = false;
            if let Ok((id, Ok(()))) = result {
                if let Some(Ok(meetings)) = &mut state.sharing_meetings {
                    if let Some(meeting) = meetings.iter_mut().find(|m| m.id == id) {
                        meeting.shared = shared;
                    }
                }
                state.mcp_message = strings::MCP_SAVED.into();
            } else {
                state.mcp_message = strings::DOC_SHARING_SAVE_FAILED.into();
            }
            cx.notify();
        });
    })
    .detach();
}

impl Shell {
    /// The open meeting's document; creates the view when another meeting
    /// was opened.
    pub(crate) fn meeting_page(&mut self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(id) = self
            .meetings
            .read(cx)
            .selected_meeting()
            .map(|m| m.data.id.clone())
        else {
            return div().into_any_element();
        };
        let current = self
            .meeting_view
            .as_ref()
            .is_some_and(|view| view.read(cx).meeting_id == id);
        if !current {
            let (model, meetings) = (self.model.clone(), self.meetings.clone());
            self.meeting_view = Some(cx.new(|cx| MeetingView::new(model, meetings, id, cx)));
        }
        let sidebar = if self.sidebar_collapsed {
            layout::SIDEBAR_COLLAPSED_WIDTH
        } else {
            layout::SIDEBAR_WIDTH
        };
        let available = window.viewport_size().width - sidebar;
        let view = self.meeting_view.clone().expect("meeting view");
        view.update(cx, |view, _| view.available = available);
        view.into_any_element()
    }

    /// A fixed meeting document for the screenshot fixtures.
    #[cfg(feature = "screenshot-tests")]
    pub(crate) fn open_fixture_meeting(
        &mut self,
        kind: FixtureMeeting,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let document = fixture_document(kind);
        let id = document.id.clone();
        self.meetings
            .update(cx, |meetings, _| meetings.selected = Some(id.clone()));
        self.model.update(cx, |model, _| {
            model.sharing_meetings = Some(Ok(vec![tingskriver_core::mcp::SharingMeeting {
                id: id.clone(),
                title: document.title.clone(),
                shared: false,
            }]));
        });
        let proposals = (kind == FixtureMeeting::Proposals).then(|| {
            let runtime = self.model.read(cx).runtime.clone();
            cx.new(|_| {
                ProposalReview::with_state(
                    None,
                    id.clone(),
                    runtime,
                    crate::proposals::ReviewState::Loaded(vec![fixture_proposal(&id)]),
                )
            })
        });
        let (model, meetings) = (self.model.clone(), self.meetings.clone());
        let open = kind == FixtureMeeting::Transcript;
        let view =
            cx.new(|cx| MeetingView::fixture(model, meetings, document, proposals, open, cx));
        view.update(cx, |view, cx| match kind {
            // The summary text field, as after "Rediger".
            FixtureMeeting::Editing => view.begin_edit(window, cx),
            // A run found in the database: "Stop" instead of "Lav om".
            FixtureMeeting::Generating => {
                if let Some(loaded) = view.loaded().cloned() {
                    let _ = view.run.observe(&loaded.document.summary);
                }
            }
            _ => {}
        });
        if kind == FixtureMeeting::Improving {
            let dialog = view.clone();
            window.defer(cx, move |window, cx| {
                dialog.update(cx, |view, cx| view.open_improve(window, cx))
            });
        }
        self.meeting_view = Some(view);
    }
}

/// The meeting document fixtures (docs/screenshots/moedeside-*.png).
#[cfg(feature = "screenshot-tests")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixtureMeeting {
    /// "Ugentligt statusmøde" with its summary (`moedeside-*`).
    Summary,
    /// The same, scrolled to the open transcript (`moedeside-transskript-*`).
    Transcript,
    /// "Kickoff med Havnens Bageri" without a summary
    /// (`moedeside-uden-opsummering-*`).
    NoSummary,
    /// A failed summary without previous text.
    SummaryFailed,
    /// The summary with a pending AI proposal above it.
    Proposals,
    /// The summary text field open (`meeting-editing`).
    Editing,
    /// The summary being rewritten, with "Stop" (`meeting-generating`).
    Generating,
    /// The "Transskribér mødet igen" dialog over the summary
    /// (`meeting-improve`).
    Improving,
}

#[cfg(feature = "screenshot-tests")]
fn fixture_document(kind: FixtureMeeting) -> Document {
    use meeting_document::{Line, Summary};
    let at = |value: &str| {
        chrono::DateTime::parse_from_rfc3339(value)
            .expect("fixture time")
            .to_utc()
    };
    let transcript = [
        (
            3.0,
            "Godmorgen alle sammen, så går vi i gang med ugens status.",
        ),
        (
            9.0,
            "Bookingsiden er næsten klar, men testen mangler stadig.",
        ),
        (17.0, "Kan vi flytte lanceringen til tirsdag?"),
        (24.0, "Ja, det giver testen tid nok."),
        (
            31.0,
            "Vi bruger den eksisterende betalingsløsning i første omgang.",
        ),
        (40.0, "Kunden var glad for prototypen."),
        (48.0, "Maria sender prøveadgang inden onsdag."),
        (55.0, "Jonas kigger på mobilvisningen af kalenderen."),
        (63.0, "Sofie booker lokale til workshoppen."),
        (70.0, "Næste statusmøde er fredag kl. 9.30."),
    ]
    .into_iter()
    .map(|(start, text)| Line {
        start: Some(start),
        timestamp: String::new(),
        text: text.into(),
    })
    .collect();
    let markdown = "# Kort fortalt\nTeamet gennemgik status på de tre åbne spor og blev enige om at udskyde lanceringen af den nye bookingside til efter weekenden. Kundens feedback på prototypen var overvejende positiv.\n\n# Beslutninger\n- Lanceringen af bookingsiden flyttes til tirsdag, så testen når at blive færdig.\n- Vi bruger den eksisterende betalingsløsning i første omgang.\n- Næste statusmøde holdes fredag kl. 9.30.\n\n# Handlepunkter\n- Maria sender prøveadgang til kunden inden onsdag.\n- Jonas retter mobilvisningen af kalenderen.\n- Sofie booker lokale til workshoppen i november.\n\n# Åbne spørgsmål\n- Skal ventelisten have sin egen bekræftelsesmail?\n- Hvem har ansvaret for opdatering af priserne?";
    let (id, title, created, summary) = match kind {
        FixtureMeeting::NoSummary => (
            "fixture-2",
            "Kickoff med Havnens Bageri",
            "2026-10-29T13:05:00Z",
            Summary::none(),
        ),
        FixtureMeeting::Generating => (
            "fixture-1",
            "Ugentligt statusmøde",
            "2026-10-29T09:30:00Z",
            Summary {
                status: SummaryStatus::Running,
                markdown: Some(markdown.into()),
                error: None,
            },
        ),
        FixtureMeeting::SummaryFailed => (
            "fixture-2",
            "Kickoff med Havnens Bageri",
            "2026-10-29T13:05:00Z",
            Summary {
                status: SummaryStatus::Failed,
                markdown: None,
                error: Some("Modellen svarede ikke inden for tidsgrænsen.".into()),
            },
        ),
        _ => (
            "fixture-1",
            "Ugentligt statusmøde",
            "2026-10-29T09:30:00Z",
            Summary {
                status: SummaryStatus::Completed,
                markdown: Some(markdown.into()),
                error: None,
            },
        ),
    };
    Document {
        id: id.into(),
        title: title.into(),
        created_at: at(created),
        folder_path: Some("fixtures/meetings/statusmoede".into()),
        summary,
        transcript,
    }
}

#[cfg(feature = "screenshot-tests")]
fn fixture_proposal(meeting_id: &str) -> tingskriver_core::proposals::Proposal {
    use tingskriver_core::proposals::{
        Proposal, ProposalField, ProposalSource, ProposalStatus, SourceKind,
    };
    let base = "# Handlepunkter\n- Maria sender prøveadgang til kunden inden onsdag.";
    let proposed = "# Handlepunkter\n- Maria sender prøveadgang til kunden inden tirsdag.";
    Proposal {
        id: "fixture-proposal".into(),
        meeting_id: meeting_id.into(),
        field: ProposalField::Summary,
        base_updated_at: None,
        base_content: base.into(),
        proposed_content: proposed.into(),
        diff: String::new(),
        source: ProposalSource::new(SourceKind::Mcp),
        status: ProposalStatus::Pending,
        status_reason: None,
        applied_content: None,
        created_at: "2026-10-29T10:00:00Z".into(),
        resolved_at: None,
    }
}
