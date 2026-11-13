//! "Gendan afbrudte møder" (Tauri `TranscriptRecovery.tsx`): interrupted
//! recordings on the left, the chosen one on the right, and Annullér, Slet
//! and Gendan. The home screen shows a card that opens it, and it opens by
//! itself once per session when interrupted recordings are found (as Tauri
//! does), never while a recording runs or is being saved.
//!
//! Tauri's dialog reads transcript drafts from the webview's IndexedDB.
//! Core's recovery reads the raw audio tracks on disk (`recover::scan`), so
//! the right pane shows what the audio holds (length, tracks, when it was
//! last saved) instead of a transcript excerpt. Restoring encodes the audio
//! and saves the meeting (`recover::restore`); the transcript is then made
//! in the background like after a normal stop, and the meeting opens. "Slet"
//! deletes the raw tracks (`recover::discard`) after Tauri's confirmation.

use gpui::{
    div, prelude::*, relative, AnyElement, App, Context, Entity, FontWeight, SharedString, Window,
};
use gpui_component::{
    button::{Button, ButtonVariants as _},
    h_flex,
    notification::Notification,
    v_flex, Disableable as _, Icon, Selectable as _, WindowExt as _,
};
use gpui_kit_assets::IconName;
use tingskriver_core::audio::finalize::AudioSaveStatus;
use tingskriver_core::audio::track_writer::TrackKind;
use tingskriver_core::{format_da, recover, strings};

use crate::controls::{self, Variant};
use crate::startup::DbState;
use crate::theme::{self, icon_size, layout, recording as size, space, typography};
use crate::ui::{palette, AppModel, Shell};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Busy {
    Restoring,
    Deleting,
}

pub(crate) struct RecoveryView {
    model: Entity<AppModel>,
    recordings: Vec<recover::Recording>,
    selected: Option<String>,
    busy: Option<Busy>,
    /// "Slet" was pressed once: ask before deleting (Tauri `confirm`).
    confirm_delete: bool,
}

/// "Mikrofon og systemlyd", "Kun mikrofon", "Kun systemlyd".
fn tracks_text(tracks: &[TrackKind]) -> &'static str {
    let microphone = tracks.contains(&TrackKind::Microphone);
    let system = tracks.contains(&TrackKind::System);
    match (microphone, system) {
        (true, false) => strings::RECOVERY_TRACKS_MICROPHONE,
        (false, true) => strings::RECOVERY_TRACKS_SYSTEM,
        _ => strings::RECOVERY_TRACKS_BOTH,
    }
}

/// "Sidst gemt 30. okt., 14.02" in local time.
fn last_written_text(recording: &recover::Recording) -> String {
    let at = chrono::DateTime::from_timestamp_millis(recording.last_written as i64)
        .map(|at| at.with_timezone(&chrono::Local).naive_local());
    match at {
        Some(at) => strings::recovery_last_written(&format_da::day_month_clock(at)),
        None => String::new(),
    }
}

/// "12 min lyd".
fn duration_text(recording: &recover::Recording) -> String {
    strings::recovery_duration(&format_da::duration(
        recording.duration_seconds.max(0.0) as u64
    ))
}

/// The toast after restoring (`recoveryDescription` without a transcript).
fn restored_text(status: AudioSaveStatus) -> &'static str {
    if status == AudioSaveStatus::Partial {
        strings::RECOVERY_RESTORED_PARTIAL
    } else {
        strings::RECOVERY_RESTORED_AUDIO
    }
}

impl RecoveryView {
    pub(crate) fn new(model: Entity<AppModel>, recordings: Vec<recover::Recording>) -> Self {
        // The first one is chosen, as Tauri does.
        let selected = recordings.first().map(|r| r.id.clone());
        RecoveryView {
            model,
            recordings,
            selected,
            busy: None,
            confirm_delete: false,
        }
    }

    fn selected(&self) -> Option<&recover::Recording> {
        let id = self.selected.as_deref()?;
        self.recordings.iter().find(|r| r.id == id)
    }

    fn select(&mut self, id: String, cx: &mut Context<Self>) {
        if self.busy.is_none() {
            self.selected = Some(id);
            self.confirm_delete = false;
            cx.notify();
        }
    }

    fn restore(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.selected.clone() else {
            return;
        };
        if self.busy.is_some() {
            return;
        }
        let model = self.model.read(cx);
        let db = match &model.db {
            DbState::Ready(db) => Some(db.clone()),
            DbState::NotCreated => None,
            _ => return,
        };
        let data_dir = model.app_data_dir.clone();
        let recorder = model.recorder.clone();
        let task = model.runtime.spawn(async move {
            let db = match db {
                Some(db) => db,
                None => {
                    let dir =
                        data_dir.ok_or_else(|| anyhow::anyhow!(strings::MEASURE_NO_FOLDER))?;
                    tingskriver_core::database::manager::DatabaseManager::new_from_app_data_dir(
                        &dir,
                    )
                    .await?
                }
            };
            let root =
                tingskriver_core::audio::recording_preferences::get_default_recordings_folder();
            let ffmpeg =
                tokio::task::spawn_blocking(tingskriver_core::audio::ffmpeg::find_ffmpeg_path)
                    .await?;
            let restored = recover::restore(&root, &id, db.pool(), ffmpeg.as_deref()).await?;
            let folder = root.join(&id).to_string_lossy().into_owned();
            Ok::<_, anyhow::Error>((db, restored, folder))
        });
        self.busy = Some(Busy::Restoring);
        self.confirm_delete = false;
        cx.notify();
        cx.spawn_in(window, async move |view, cx| {
            let result = task.await;
            let _ = view.update_in(cx, |this, window, cx| {
                this.busy = None;
                match result {
                    Ok(Ok((db, restored, folder))) => {
                        this.model.update(cx, |model, cx| {
                            if !matches!(model.db, DbState::Ready(_)) {
                                model.db = DbState::Ready(db);
                            }
                            cx.notify();
                        });
                        if let Some(recorder) = recorder {
                            recorder.transcribe_in_background(restored.meeting_id.clone(), folder);
                        }
                        window.close_dialog(cx);
                        window.push_notification(
                            Notification::success(restored_text(restored.audio.status))
                                .title(strings::RECOVERY_RESTORED),
                            cx,
                        );
                        crate::ui::open_meeting(restored.meeting_id, cx);
                    }
                    Ok(Err(error)) => {
                        log::warn!("Restoring an interrupted recording failed: {error:#}");
                        window.push_notification(
                            Notification::error(strings::RECOVERY_RESTORE_FAILED),
                            cx,
                        );
                    }
                    Err(_) => window.push_notification(
                        Notification::error(strings::RECOVERY_RESTORE_FAILED),
                        cx,
                    ),
                }
                crate::ui::meetings_changed(cx);
                cx.notify();
            });
        })
        .detach();
    }

    fn delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.selected.clone() else {
            return;
        };
        if self.busy.is_some() {
            return;
        }
        if !self.confirm_delete {
            self.confirm_delete = true;
            cx.notify();
            return;
        }
        self.confirm_delete = false;
        self.busy = Some(Busy::Deleting);
        cx.notify();
        let task = self.model.read(cx).runtime.spawn({
            let id = id.clone();
            async move {
                let root =
                    tingskriver_core::audio::recording_preferences::get_default_recordings_folder();
                recover::discard(&root, &id).await
            }
        });
        cx.spawn_in(window, async move |view, cx| {
            let result = task.await;
            let _ = view.update_in(cx, |this, window, cx| {
                this.busy = None;
                match result {
                    Ok(Ok(())) => {
                        this.recordings.retain(|r| r.id != id);
                        this.selected = this.recordings.first().map(|r| r.id.clone());
                        window.push_notification(
                            Notification::success(strings::RECOVERY_DELETED),
                            cx,
                        );
                        if this.recordings.is_empty() {
                            window.close_dialog(cx);
                        }
                    }
                    other => {
                        if let Ok(Err(error)) = other {
                            log::warn!("Deleting an interrupted recording failed: {error:#}");
                        }
                        window.push_notification(
                            Notification::error(strings::RECOVERY_DELETE_FAILED),
                            cx,
                        );
                    }
                }
                crate::ui::meetings_changed(cx);
                cx.notify();
            });
        })
        .detach();
    }

    fn recovery_list(&self, cx: &mut Context<Self>) -> AnyElement {
        let palette = palette(cx);
        let rows = self
            .recordings
            .iter()
            .enumerate()
            .map(|(index, recording)| {
                let selected = self.selected.as_deref() == Some(recording.id.as_str());
                let id = recording.id.clone();
                Button::new(("recovery-row", index))
                    .ghost()
                    .selected(selected)
                    .w_full()
                    .h_auto()
                    .min_h(layout::CONTROL_HEIGHT)
                    .p(space::MD)
                    .rounded(theme::RADIUS_LG)
                    .border_1()
                    .border_color(if selected {
                        palette.primary
                    } else {
                        gpui::transparent_black()
                    })
                    .when(selected, |this| {
                        this.bg(theme::over(
                            palette.primary,
                            palette.background,
                            size::NOTICE_TINT,
                        ))
                    })
                    .disabled(self.busy.is_some())
                    .child(
                        h_flex()
                            .w_full()
                            .items_start()
                            .justify_between()
                            .gap(space::SM)
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .items_start()
                                    .child(
                                        div()
                                            .w_full()
                                            .truncate()
                                            .text_size(typography::SM)
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(palette.foreground)
                                            .child(recording.id.clone()),
                                    )
                                    .child(
                                        div()
                                            .mt(space::XS)
                                            .text_size(typography::XS)
                                            .text_color(palette.muted_foreground)
                                            .child(last_written_text(recording)),
                                    )
                                    .child(
                                        div()
                                            .mt(space::XS)
                                            .text_size(typography::XS)
                                            .text_color(palette.muted_foreground)
                                            .child(duration_text(recording)),
                                    ),
                            )
                            .child(
                                div().flex_none().child(
                                    Icon::from(IconName::CircleCheck)
                                        .size(icon_size::SM)
                                        .text_color(palette.success),
                                ),
                            ),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.select(id.clone(), cx)))
            });
        v_flex()
            .w(relative(size::RECOVERY_LIST_FRACTION))
            .flex_none()
            .min_h_0()
            .child(
                div()
                    .mb(space::SM)
                    .text_size(typography::SM)
                    .font_weight(FontWeight::MEDIUM)
                    .child(strings::RECOVERY_LIST),
            )
            .child(
                v_flex()
                    .id("recovery-list")
                    .role(gpui::Role::List)
                    .aria_label(SharedString::from(strings::RECOVERY_LIST))
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p(space::SM)
                    .gap(space::SM)
                    .rounded(theme::RADIUS_LG)
                    .border_1()
                    .border_color(palette.border)
                    .children(rows),
            )
            .into_any_element()
    }

    fn recovery_preview(&self, cx: &mut Context<Self>) -> AnyElement {
        let palette = palette(cx);
        let body = match self.selected() {
            Some(recording) => v_flex()
                .size_full()
                .child(
                    v_flex()
                        .p(space::LG)
                        .border_b_1()
                        .border_color(palette.border)
                        .bg(palette.muted)
                        .child(
                            div()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(recording.id.clone()),
                        )
                        .child(
                            div()
                                .mt(space::XS)
                                .text_size(typography::SM)
                                .text_color(palette.muted_foreground)
                                .child(last_written_text(recording)),
                        )
                        .child(
                            h_flex()
                                .mt(space::SM)
                                .gap(space::LG)
                                .flex_wrap()
                                .text_size(typography::SM)
                                .child(duration_text(recording))
                                .child(tracks_text(&recording.tracks))
                                .child(
                                    h_flex()
                                        .gap(space::XS)
                                        .text_color(palette.success)
                                        .child(
                                            Icon::from(IconName::CircleCheck)
                                                .size(icon_size::SM)
                                                .text_color(palette.success),
                                        )
                                        .child(strings::RECOVERY_HAS_AUDIO),
                                ),
                        ),
                )
                .child(
                    div()
                        .flex_1()
                        .p(space::LG)
                        .text_size(typography::SM)
                        .text_color(palette.muted_foreground)
                        .child(strings::RECOVERY_NO_TRANSCRIPT),
                ),
            None => v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .text_color(palette.muted_foreground)
                .child(strings::RECOVERY_SELECT),
        };
        v_flex()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .child(
                div()
                    .mb(space::SM)
                    .text_size(typography::SM)
                    .font_weight(FontWeight::MEDIUM)
                    .child(strings::RECOVERY_PREVIEW),
            )
            .child(
                div()
                    .id("recovery-preview")
                    .role(gpui::Role::Region)
                    .aria_label(SharedString::from(strings::RECOVERY_PREVIEW))
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .rounded(theme::RADIUS_LG)
                    .border_1()
                    .border_color(palette.border)
                    .child(body),
            )
            .into_any_element()
    }

    fn footer(&self, cx: &mut Context<Self>) -> AnyElement {
        let palette = palette(cx);
        let busy = self.busy.is_some();
        let none = self.selected.is_none();
        let (delete_icon, delete_label) = match self.busy {
            Some(Busy::Deleting) => (IconName::CircleX, strings::RECOVERY_DELETING),
            _ => (IconName::Trash, strings::RECOVERY_DELETE),
        };
        let restore_label = if self.busy == Some(Busy::Restoring) {
            strings::RECOVERY_RESTORING
        } else {
            strings::RECOVERY_RESTORE
        };
        v_flex()
            .gap(space::MD)
            .when(self.confirm_delete, |this| {
                this.child(
                    div()
                        .id("recovery-confirm-delete")
                        .role(gpui::Role::Alert)
                        .aria_label(SharedString::from(strings::RECOVERY_CONFIRM_DELETE))
                        .px(space::LG)
                        .py(space::MD)
                        .rounded(theme::RADIUS_LG)
                        .border_1()
                        .border_color(theme::over(
                            palette.destructive,
                            palette.background,
                            size::ALERT_BORDER,
                        ))
                        .bg(theme::over(
                            palette.destructive,
                            palette.background,
                            size::NOTICE_TINT,
                        ))
                        .text_size(typography::SM)
                        .child(strings::RECOVERY_CONFIRM_DELETE),
                )
            })
            .child(
                h_flex()
                    .justify_end()
                    .gap(space::SM)
                    .child(
                        controls::button("recovery-cancel", Variant::Outline)
                            .label(strings::RECOVERY_CANCEL)
                            .disabled(busy)
                            .on_click(cx.listener(|this, _, window, cx| {
                                if this.confirm_delete {
                                    this.confirm_delete = false;
                                    cx.notify();
                                } else {
                                    window.close_dialog(cx);
                                }
                            })),
                    )
                    .child(
                        controls::button("recovery-delete", Variant::Danger)
                            .icon(Icon::from(delete_icon).size(icon_size::SM))
                            .label(delete_label)
                            .disabled(busy || none)
                            .on_click(cx.listener(|this, _, window, cx| this.delete(window, cx))),
                    )
                    .child(
                        controls::button("recovery-restore", Variant::Primary)
                            .icon(Icon::from(IconName::CircleCheck).size(icon_size::SM))
                            .label(restore_label)
                            .loading(self.busy == Some(Busy::Restoring))
                            .disabled(busy || none)
                            .on_click(cx.listener(|this, _, window, cx| this.restore(window, cx))),
                    ),
            )
            .into_any_element()
    }
}

impl Render for RecoveryView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = palette(cx);
        let height = (window.viewport_size().height * size::RECOVERY_HEIGHT_FRACTION)
            .min(size::RECOVERY_HEIGHT_MAX);
        v_flex()
            .id("recovery-dialog")
            .h(height)
            .gap(space::LG)
            .child(
                div()
                    .text_size(typography::SM)
                    .text_color(palette.muted_foreground)
                    .child(strings::recovery_found(self.recordings.len())),
            )
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .items_stretch()
                    .gap(space::LG)
                    .child(self.recovery_list(cx))
                    .child(self.recovery_preview(cx)),
            )
            .child(self.footer(cx))
    }
}

/// Open "Gendan afbrudte møder" for these recordings.
pub(crate) fn open(
    model: Entity<AppModel>,
    recordings: Vec<recover::Recording>,
    window: &mut Window,
    cx: &mut App,
) {
    if recordings.is_empty() {
        return;
    }
    let view = cx.new(|_| RecoveryView::new(model, recordings));
    window.open_dialog(cx, move |dialog, _, _| {
        dialog
            .w(size::RECOVERY_WIDTH)
            .title(
                div()
                    .font_family(typography::SERIF)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_size(typography::XXL)
                    .child(strings::RECOVERY_TITLE),
            )
            .child(view.clone())
    });
}

impl Shell {
    /// The card above the hero while interrupted recordings exist: what
    /// happened and a button that opens the dialog.
    pub(crate) fn recovery_card(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let count = self.meetings.read(cx).recovery().len();
        if count == 0 {
            return None;
        }
        let palette = palette(cx);
        Some(
            v_flex()
                .id("meeting-recovery")
                .role(gpui::Role::Status)
                .aria_label(SharedString::from(strings::MEETING_RECOVERY))
                .gap(space::SM)
                .p(space::LG)
                .rounded(theme::RADIUS_LG)
                .border_1()
                .border_color(palette.border)
                .bg(palette.card)
                .child(crate::ui::section_title(strings::MEETING_RECOVERY))
                .child(
                    div()
                        .text_size(typography::SM)
                        .text_color(palette.muted_foreground)
                        .child(strings::recovery_found(count)),
                )
                .child(
                    h_flex().child(
                        controls::button("recovery-open", Variant::Outline)
                            .label(strings::RECOVERY_OPEN)
                            .on_click(
                                cx.listener(|this, _, window, cx| this.open_recovery(window, cx)),
                            ),
                    ),
                )
                .into_any_element(),
        )
    }

    pub(crate) fn open_recovery(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.recovery_offered = true;
        let recordings = self.meetings.read(cx).recovery().to_vec();
        open(self.model.clone(), recordings, window, cx);
    }

    /// Open the dialog by itself once per session, when interrupted
    /// recordings are found and nothing records (Tauri `page.tsx`).
    pub(crate) fn offer_recovery(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.recovery_offered
            || self.home.is_busy()
            || !self.home.can_record
            || self.meetings.read(cx).recovery().is_empty()
        {
            return;
        }
        self.recovery_offered = true;
        let shell = cx.entity().downgrade();
        window.defer(cx, move |window, cx| {
            let _ = shell.update(cx, |shell, cx| shell.open_recovery(window, cx));
        });
    }
}

/// Two synthetic interrupted recordings for the screenshot fixture.
#[cfg(feature = "screenshot-tests")]
pub(crate) fn fixture_recordings() -> Vec<recover::Recording> {
    // Fixed instants; the text is drawn in the machine's local time zone,
    // which is UTC on CI and under Xvfb.
    vec![
        recover::Recording {
            id: "Møde 12. nov. 2026 09.00".into(),
            duration_seconds: 47.0 * 60.0,
            last_written: 1_794_478_020_000,
            tracks: vec![TrackKind::Microphone, TrackKind::System],
        },
        recover::Recording {
            id: "Møde 10. nov. 2026 13.30".into(),
            duration_seconds: 8.0 * 60.0 + 20.0,
            last_written: 1_794_318_000_000,
            tracks: vec![TrackKind::Microphone],
        },
    ]
}

/// Open the fixture dialog over the home screen.
#[cfg(feature = "screenshot-tests")]
pub(crate) fn open_fixture(model: Entity<AppModel>, window: &mut Window, cx: &mut App) {
    open(model, fixture_recordings(), window, cx);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recording(tracks: Vec<TrackKind>, seconds: f64) -> recover::Recording {
        recover::Recording {
            id: "Møde".into(),
            duration_seconds: seconds,
            last_written: 0,
            tracks,
        }
    }

    #[test]
    fn the_audio_is_described_in_danish() {
        assert_eq!(
            tracks_text(&[TrackKind::Microphone, TrackKind::System]),
            "Mikrofon og systemlyd"
        );
        assert_eq!(tracks_text(&[TrackKind::Microphone]), "Kun mikrofon");
        assert_eq!(tracks_text(&[TrackKind::System]), "Kun systemlyd");
        assert_eq!(
            duration_text(&recording(vec![TrackKind::Microphone], 47.0 * 60.0)),
            "47 min lyd"
        );
        assert_eq!(
            duration_text(&recording(vec![TrackKind::Microphone], -1.0)),
            "0 s lyd"
        );
        assert!(last_written_text(&recording(vec![], 0.0)).starts_with("Sidst gemt "));
    }

    #[test]
    fn restoring_reports_partial_audio_like_tauri() {
        assert_eq!(
            restored_text(AudioSaveStatus::Partial),
            "Lyden er delvist gendannet. Dele af optagelsen kunne ikke reddes."
        );
        assert_eq!(
            restored_text(AudioSaveStatus::Ok),
            "Lyden er gendannet og gemt i mødets mappe."
        );
    }
}
