//! The open meeting. A placeholder until the meeting document is ported
//! (docs/design-paritet.md, Job 4): title, time and job status, the audio
//! folder and the meeting's MCP sharing, which used to sit on the Møder page.

use gpui::{div, prelude::*, AnyElement, Context};
use gpui_component::{v_flex, Disableable, Selectable};
use tingskriver_core::strings;

use crate::controls::{self, Variant};
use crate::startup::DbState;
use crate::theme::{layout, space};
use crate::ui::{heading, load_database, loading, muted, paragraph, section_title, Level, Shell};

impl Shell {
    pub(crate) fn meeting_page(&self, cx: &mut Context<Self>) -> AnyElement {
        let meetings = self.meetings.read(cx);
        let Some(meeting) = meetings.selected_meeting() else {
            return div().into_any_element();
        };
        let id = meeting.data.id.clone();
        let title = meeting.data.title.clone();
        let when = [meeting.time.as_str(), meeting.duration.as_str()].join(" · ");
        let status = meeting.status.text();
        let folder = meeting
            .data
            .folder_path
            .clone()
            .map(std::path::PathBuf::from);
        let sharing = matches!(self.model.read(cx).db, DbState::Ready(_));
        let mut body = v_flex()
            .w_full()
            .max_w(layout::DOCUMENT_MAX)
            .gap(space::LG)
            .child(heading(&title, Level::Document))
            .child(muted(&when, cx))
            .child(paragraph(status, cx))
            .child(paragraph(strings::MEETING_DOCUMENT_LATER, cx));
        if let Some(folder) = folder {
            body = body.child(
                controls::button("open-meeting-folder", Variant::Outline)
                    .label(strings::MEETING_FOLDER)
                    .on_click(move |_, _, _| crate::startup::open_folder(&folder)),
            );
        }
        if sharing {
            body = body
                .child(div().pt(space::LG).child(section_title(strings::MCP_TITLE)))
                .child(self.meeting_sharing(&id, cx));
        }
        v_flex()
            .id("meeting")
            .size_full()
            .overflow_y_scroll()
            .px(space::XXL)
            .pt(space::XXXL)
            .pb(space::XXL)
            .child(body)
            .into_any_element()
    }

    /// MCP sharing of the open meeting (Tauri `MeetingSharing` in the
    /// meeting header). Moved here from the old Møder page.
    fn meeting_sharing(&self, meeting_id: &str, cx: &mut Context<Self>) -> AnyElement {
        let state = self.model.read(cx);
        let mut body = v_flex()
            .gap(space::SM)
            .child(paragraph(strings::MCP_PRIVACY, cx))
            .child(
                controls::button("refresh-mcp-sharing", Variant::Outline)
                    .label(strings::MCP_REFRESH)
                    .disabled(state.sharing_pending)
                    .on_click(|_, _, cx| load_database(cx)),
            )
            .child(
                div()
                    .id("mcp-result")
                    .role(gpui::Role::Status)
                    .aria_label(state.mcp_message.clone())
                    .child(paragraph(&state.mcp_message, cx)),
            );
        match &state.sharing_meetings {
            None => body = body.child(loading(strings::LOADING_MEETINGS, cx)),
            Some(Err(error)) => body = body.child(paragraph(error, cx)),
            Some(Ok(meetings)) => {
                for (index, meeting) in meetings
                    .iter()
                    .enumerate()
                    .filter(|(_, m)| m.id == meeting_id)
                {
                    let model = self.model.clone();
                    let id = meeting.id.clone();
                    let shared = !meeting.shared;
                    body = body.child(
                        v_flex()
                            .gap(space::SM)
                            .child(paragraph(&meeting.title, cx))
                            .child(
                                controls::button(("share-meeting", index), Variant::Outline)
                                    .label(if meeting.shared {
                                        strings::MCP_UNSHARE
                                    } else {
                                        strings::MCP_SHARE
                                    })
                                    .selected(meeting.shared)
                                    .disabled(state.sharing_pending)
                                    .on_click(move |_, _, cx| {
                                        let model = model.clone();
                                        let id = id.clone();
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
                                                let result = tingskriver_core::mcp::set_shared(
                                                    &pool, &id, shared,
                                                )
                                                .await;
                                                (id, result)
                                            }))
                                        });
                                        let Some(task) = task else {
                                            return;
                                        };
                                        cx.spawn(async move |cx| {
                                            let result = task.await;
                                            model.update(cx, |state, cx| {
                                                state.sharing_pending = false;
                                                if let Ok((id, Ok(()))) = result {
                                                    if let Some(Ok(meetings)) =
                                                        &mut state.sharing_meetings
                                                    {
                                                        if let Some(meeting) =
                                                            meetings.iter_mut().find(|m| m.id == id)
                                                        {
                                                            meeting.shared = shared;
                                                        }
                                                    }
                                                    state.mcp_message = strings::MCP_SAVED.into();
                                                } else {
                                                    state.mcp_message =
                                                        strings::MCP_SAVE_FAILED.into();
                                                }
                                                cx.notify();
                                            });
                                        })
                                        .detach();
                                    }),
                            ),
                    );
                }
            }
        }
        body.into_any_element()
    }
}
