//! The recording screen of the home page (Tauri `RecordingControls.tsx`,
//! `RecordingStatusBar.tsx`, `TranscriptPanel.tsx`,
//! `VirtualizedTranscriptView.tsx`, `StatusOverlays.tsx`;
//! `docs/screenshots/optagelse-*.png`): the status bar, the transcript lines,
//! the floating pill and the saving overlays.
//!
//! The pieces draw a [`HomeState`], which [`HomeState::from_recorder`] makes
//! from the state of `tingskriver_host::Recorder`. The recorder reads core
//! (a snapshot at startup, core's events, time and levels while recording),
//! so a recording started from the tray, MCP or before the window opened
//! shows here too. The buttons ask the recorder; nothing here assumes that a
//! click worked until core says so.
//!
//! No transcript is made while recording (the audio is saved first and
//! transcribed after "Stop og gem"), so a running recording shows what is
//! happening instead of transcript lines.

use gpui::{
    canvas, div, fill, point, prelude::*, px, relative, size as gpui_size, AnyElement, App, Bounds,
    ClipboardItem, Context, FontWeight, Hsla, Pixels, SharedString, StyledText,
};
use gpui_component::{
    h_flex, notification::Notification, spinner::Spinner, v_flex, Disableable as _, Icon,
    IconName as ComponentIcon, Sizable as _, WindowExt as _,
};
use gpui_kit_assets::IconName;
use tingskriver_core::strings;

use std::sync::Arc;

use tingskriver_host::recorder::{self as host, Notice, Phase, RecorderState};
use tingskriver_host::Recorder;

use crate::controls::{self, Pill, Variant};
use crate::theme::{self, icon_size, layout, recording as size, shadow, space, typography};
use crate::ui::{palette, Shell};

/// What the home page shows besides the idle hero.
#[derive(Debug, Clone, Default)]
pub struct HomeState {
    /// This app can record (the host and its recorder run).
    pub can_record: bool,
    /// "Start optagelse" was pressed and core is getting ready.
    pub starting: bool,
    /// A recording runs or is paused.
    pub recording: Option<Recording>,
    /// The recording has stopped and is being saved.
    pub saving: Option<Saving>,
    /// Lines of the recording that is running or being saved.
    pub lines: Vec<Line>,
    /// Starting failed (the alert under the pill).
    pub start_error: Option<StartError>,
}

impl HomeState {
    /// The transcript view replaces the idle hero.
    pub fn shows_transcript(&self) -> bool {
        self.recording.is_some() || self.saving.is_some() || !self.lines.is_empty()
    }

    /// The sidebar shows "Optagelse i gang".
    pub fn is_recording(&self) -> bool {
        self.recording.is_some()
    }

    /// A recording runs or the last one is still being saved: nothing else
    /// may start one, and interrupted recordings are not offered (Tauri skips
    /// its recovery check then).
    pub fn is_busy(&self) -> bool {
        self.starting || self.recording.is_some() || self.saving.is_some()
    }

    /// Going from `self` to `next`, the last recording has been saved and
    /// nothing records now: its raw tracks are gone, so the scan for
    /// interrupted recordings is due again.
    pub fn becomes_free(&self, next: &HomeState) -> bool {
        self.is_busy() && !next.is_busy()
    }

    /// The screen for the recorder's state.
    pub fn from_recorder(state: &RecorderState) -> HomeState {
        let recording = state.phase.is_recording().then(|| Recording {
            paused: state.phase == Phase::Paused,
            seconds: state.seconds,
            microphone: state.microphone.clone(),
            system: state.system.clone(),
            notice: state.notice.as_ref().map(|notice| match notice {
                host::DeviceNotice::Switched(device) => DeviceNotice::Switched(device.clone()),
                host::DeviceNotice::SwitchFailed(device) => {
                    DeviceNotice::SwitchFailed(device.clone())
                }
                host::DeviceNotice::Lost(device) => DeviceNotice::Lost(device.clone()),
            }),
            storage_error: state.storage_error.clone(),
            levels: state.levels,
        });
        let saving = match state.phase {
            Phase::Stopping => Some(Saving::Stopping),
            Phase::SavingMeeting => Some(Saving::SavingMeeting),
            Phase::Finishing => Some(Saving::Finishing),
            _ => None,
        };
        HomeState {
            can_record: true,
            starting: state.phase == Phase::Starting,
            recording,
            saving,
            lines: Vec::new(),
            start_error: state.start_error.map(|error| match error {
                host::StartError::Microphone => StartError::Microphone,
                host::StartError::System => StartError::System,
                host::StartError::Permission => StartError::Permission,
                host::StartError::Other => StartError::Other,
            }),
        }
    }
}

/// A running recording, as the status bar and the pill show it.
#[derive(Debug, Clone)]
pub struct Recording {
    pub paused: bool,
    /// Recorded time in seconds (pauses excluded).
    pub seconds: u64,
    /// The microphone in use; `None` is the system default.
    pub microphone: Option<String>,
    /// The system audio device; `None` is the system default.
    pub system: Option<String>,
    pub notice: Option<DeviceNotice>,
    /// The audio is not being saved ("Lyden gemmes ikke: disken er fuld").
    pub storage_error: Option<String>,
    /// Level per bar, 0 to 1: microphone, both, system audio.
    pub levels: [f32; 3],
}

/// A microphone change during the recording (`RecordingStateContext.tsx`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceNotice {
    /// Switched to this device; information.
    Switched(String),
    /// Could not switch to this device; warning.
    SwitchFailed(String),
    /// This device was lost and could not be restored; warning, and only
    /// system audio is recorded.
    Lost(String),
}

impl DeviceNotice {
    fn is_warning(&self) -> bool {
        !matches!(self, DeviceNotice::Switched(_))
    }

    fn message(&self) -> String {
        match self {
            DeviceNotice::Switched(device) => strings::recording_device_switched(device),
            DeviceNotice::SwitchFailed(device) => strings::recording_device_switch_failed(device),
            DeviceNotice::Lost(device) => strings::recording_device_lost(device),
        }
    }
}

/// The stages after "Stop og gem".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Saving {
    /// The pill shows "Gemmer optagelsen…".
    Stopping,
    /// Overlay "Gør transskriptionen færdig…" instead of the pill.
    Finishing,
    /// Overlay "Gemmer mødet…" instead of the pill.
    SavingMeeting,
}

/// Why a recording could not start (`RecordingControls.tsx:120-136`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartError {
    Microphone,
    System,
    Permission,
    Other,
}

impl StartError {
    fn texts(self) -> (&'static str, &'static str) {
        match self {
            StartError::Microphone => (
                strings::RECORDING_ERROR_MICROPHONE_TITLE,
                strings::RECORDING_ERROR_MICROPHONE,
            ),
            StartError::System => (
                strings::RECORDING_ERROR_SYSTEM_TITLE,
                strings::RECORDING_ERROR_SYSTEM,
            ),
            StartError::Permission => (
                strings::RECORDING_ERROR_PERMISSION_TITLE,
                strings::RECORDING_ERROR_PERMISSION,
            ),
            StartError::Other => (
                strings::RECORDING_ERROR_START_TITLE,
                strings::RECORDING_ERROR_START,
            ),
        }
    }
}

/// One transcript line.
#[derive(Debug, Clone)]
pub struct Line {
    /// Start in the recording, in seconds.
    pub seconds: u64,
    pub text: String,
    /// The recognizer is unsure: a dotted underline (`dotted_text`).
    pub low_confidence: bool,
}

/// `mm:ss`, or `h:mm:ss` from one hour (`formatClock` in Tauri).
pub fn clock(seconds: u64) -> String {
    let (hours, minutes, secs) = (seconds / 3600, (seconds % 3600) / 60, seconds % 60);
    if hours > 0 {
        format!("{}:{:02}:{:02}", hours, minutes, secs)
    } else {
        format!("{:02}:{:02}", minutes, secs)
    }
}

/// The clipboard text of "Kopiér": `[MM:SS] text` per line, minutes not
/// wrapped at the hour (`copyTranscript` in `TranscriptContext.tsx`).
pub fn copy_text(lines: &[Line]) -> String {
    lines
        .iter()
        .map(|line| {
            format!(
                "[{:02}:{:02}] {}",
                line.seconds / 60,
                line.seconds % 60,
                line.text
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

impl Shell {
    /// The recorder, when the host runs (not in screenshot fixtures).
    pub(crate) fn recorder(&self, cx: &App) -> Option<Arc<Recorder>> {
        self.model.read(cx).recorder.clone()
    }

    /// "Start optagelse", "Ny optagelse", the tray and the shortcut. The
    /// recorder ignores it while a recording runs or is being saved.
    pub(crate) fn start_recording(&mut self, cx: &mut Context<Self>) {
        if let Some(recorder) = self.recorder(cx) {
            recorder.start_recording();
        }
    }

    /// Draw the recorder's state from now on: the current state at once,
    /// then every change. Ends with the shell.
    pub(crate) fn follow_recorder(&mut self, cx: &mut Context<Self>) {
        let Some(recorder) = self.recorder(cx) else {
            return;
        };
        let mut states = recorder.subscribe();
        self.home = HomeState::from_recorder(&states.borrow_and_update());
        cx.spawn(async move |shell, cx| {
            while states.changed().await.is_ok() {
                let home = HomeState::from_recorder(&states.borrow_and_update());
                let updated = shell.update(cx, |shell, cx| {
                    if shell.home.becomes_free(&home) {
                        // Scans while recording or saving list the meeting's
                        // raw tracks as interrupted (finalize deletes them
                        // only when the audio is saved): scan again first.
                        shell
                            .meetings
                            .update(cx, |meetings, cx| meetings.forget_recovery(cx));
                    }
                    shell.home = home;
                    cx.notify();
                });
                if updated.is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    /// The transcript column: "Kopiér", the status bar and the lines.
    pub(crate) fn transcript_view(&self, cx: &mut Context<Self>) -> AnyElement {
        let palette = palette(cx);
        let state = &self.home;
        let live = state.recording.as_ref().map(|r| !r.paused);
        let last = state.lines.len().saturating_sub(1);
        let lines: Vec<_> = state
            .lines
            .iter()
            .enumerate()
            .map(|(index, line)| {
                // The newest line is still being written while recording.
                let streaming = state.recording.is_some() && index == last;
                transcript_line(index, line, streaming, cx)
            })
            .collect();
        v_flex()
            .id("recording-transcript")
            .size_full()
            .overflow_y_scroll()
            .pt(size::TRANSCRIPT_TOP)
            .pb(size::TRANSCRIPT_BOTTOM)
            .child(
                v_flex()
                    .w_full()
                    .max_w(layout::TRANSCRIPT_MAX)
                    .mx_auto()
                    .px(size::TRANSCRIPT_PADDING_X)
                    .when(!state.lines.is_empty(), |this| {
                        this.child(
                            h_flex().mb(space::SM).justify_end().child(
                                controls::small_button("recording-copy", Variant::Outline)
                                    .icon(Icon::from(ComponentIcon::Copy).size(icon_size::SM))
                                    .label(strings::RECORDING_COPY)
                                    .tooltip(strings::RECORDING_COPY_TITLE)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        cx.write_to_clipboard(ClipboardItem::new_string(
                                            copy_text(&this.home.lines),
                                        ));
                                        window.push_notification(
                                            Notification::success(strings::RECORDING_COPIED),
                                            cx,
                                        );
                                    })),
                            ),
                        )
                    })
                    .when_some(state.recording.as_ref(), |this, recording| {
                        this.child(self.status_bar(recording, cx))
                    })
                    .child(
                        v_flex()
                            .id("recording-lines")
                            .pt(space::LG)
                            .role(gpui::Role::Log)
                            .aria_label(SharedString::from(strings::RECORDING_TRANSCRIPT_LABEL))
                            .children(lines),
                    )
                    .when_some(
                        state.recording.as_ref().filter(|_| state.lines.is_empty()),
                        |this, recording| this.child(empty_transcript(recording.paused, cx)),
                    )
                    .when(live == Some(true) && !state.lines.is_empty(), |this| {
                        this.child(
                            h_flex()
                                .mt(space::LG)
                                .gap(space::SM)
                                .text_size(typography::SM)
                                .text_color(palette.muted_foreground)
                                .child(dot(size::LISTENING_DOT, palette.primary))
                                .child(strings::RECORDING_LISTENING),
                        )
                    }),
            )
            .into_any_element()
    }

    /// The card above the transcript: state, time, devices and a notice when
    /// the microphone changed (`RecordingStatusBar.tsx`).
    fn status_bar(&self, recording: &Recording, cx: &mut Context<Self>) -> AnyElement {
        let palette = palette(cx);
        let lost = matches!(recording.notice, Some(DeviceNotice::Lost(_)));
        let microphone = match (&recording.microphone, lost) {
            (_, true) => strings::RECORDING_MICROPHONE_LOST.to_string(),
            (Some(name), false) => name.clone(),
            (None, false) => strings::RECORDING_DEFAULT_MICROPHONE.to_string(),
        };
        let system = recording
            .system
            .clone()
            .unwrap_or_else(|| strings::RECORDING_DEFAULT_SYSTEM.to_string());
        let (state, dot_color) = if recording.paused {
            (strings::RECORDING_PAUSED, palette.warning)
        } else {
            (strings::RECORDING_ACTIVE, palette.recording)
        };
        let clock = clock(recording.seconds);
        let status = h_flex()
            .id("recording-status")
            .role(gpui::Role::Status)
            .aria_label(SharedString::from(strings::recording_status_label(
                state,
                &clock,
                &microphone,
                &system,
            )))
            .flex_wrap()
            .gap_x(space::LG)
            .gap_y(space::XS)
            .text_size(typography::SM)
            .child(
                h_flex()
                    .gap(space::SM)
                    .child(dot(size::STATUS_DOT, dot_color))
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(palette.foreground)
                            .child(state),
                    )
                    .child(div().text_color(palette.muted_foreground).child(clock)),
            )
            // One line; long device names are cut with an ellipsis
            // (Tauri `min-w-0 truncate`).
            .child(
                h_flex()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_color(palette.muted_foreground)
                    .child(
                        div()
                            .flex_none()
                            .child(format!("{} ", strings::RECORDING_MICROPHONE)),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_color(palette.foreground)
                            .child(microphone),
                    )
                    .child(div().flex_none().child("  ·  "))
                    .child(
                        div()
                            .flex_none()
                            .child(format!("{} ", strings::RECORDING_SYSTEM)),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_color(palette.foreground)
                            .child(system),
                    ),
            );
        v_flex()
            .mb(space::MD)
            .px(space::LG)
            .py(space::MD)
            .rounded(theme::RADIUS_LG)
            .border_1()
            .border_color(palette.border)
            .bg(palette.background)
            .child(status)
            .when_some(recording.storage_error.as_ref(), |this, error| {
                this.child(storage_notice(error, cx))
            })
            .when_some(recording.notice.as_ref(), |this, notice| {
                this.child(device_notice(notice, cx))
            })
            .into_any_element()
    }

    /// The floating pill: "Start optagelse", or "Pause"/"Genoptag", "Stop og
    /// gem" and the level bars, or "Gemmer optagelsen…". The saving overlays
    /// replace it. A start error shows under it.
    pub(crate) fn recording_pill(&self, cx: &mut Context<Self>) -> AnyElement {
        let palette = palette(cx);
        let state = &self.home;
        if let Some(stage @ (Saving::Finishing | Saving::SavingMeeting)) = state.saving {
            let text = match stage {
                Saving::Finishing => strings::RECORDING_FINISHING,
                _ => strings::RECORDING_SAVING_MEETING,
            };
            return h_flex()
                .id("recording-overlay")
                .role(gpui::Role::Status)
                .aria_label(SharedString::from(text))
                .gap(space::SM)
                .px(space::LG)
                .py(space::SM)
                .rounded(theme::RADIUS_LG)
                .border_1()
                .border_color(palette.border)
                .bg(palette.card)
                .shadow(shadow::md())
                .child(
                    Spinner::new()
                        .color(palette.primary)
                        .with_size(icon_size::SM),
                )
                .child(
                    div()
                        .text_size(typography::SM)
                        .text_color(palette.foreground)
                        .child(text),
                )
                .into_any_element();
        }
        let pill = h_flex()
            .gap(space::SM)
            .px(theme::sidebar::PILL_PADDING_X)
            .py(space::SM)
            .rounded(theme::RADIUS_FULL)
            .border_1()
            .border_color(palette.border)
            .bg(palette.card)
            .shadow(shadow::md());
        let pill = if state.saving == Some(Saving::Stopping) {
            pill.child(
                h_flex()
                    .id("recording-saving")
                    .role(gpui::Role::Status)
                    .aria_label(SharedString::from(strings::RECORDING_SAVING))
                    .h(layout::CONTROL_HEIGHT)
                    .gap(space::SM)
                    .px(space::MD)
                    .child(
                        Spinner::new()
                            .color(palette.primary)
                            .with_size(icon_size::MD),
                    )
                    .child(
                        div()
                            .text_size(typography::SM)
                            .text_color(palette.muted_foreground)
                            .child(strings::RECORDING_SAVING),
                    ),
            )
        } else if let Some(recording) = &state.recording {
            let (icon, label) = if recording.paused {
                (ComponentIcon::Play, strings::RECORDING_RESUME)
            } else {
                (ComponentIcon::Pause, strings::RECORDING_PAUSE)
            };
            pill.child(
                controls::pill_button("recording-pause", Pill::Pause, cx)
                    .icon(Icon::from(icon).size(icon_size::SM))
                    .label(label)
                    .disabled(!state.can_record)
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(recorder) = this.recorder(cx) {
                            recorder.toggle_pause();
                        }
                    })),
            )
            .child(
                controls::pill_button("recording-stop", Pill::Stop, cx)
                    .accessibility_label(strings::RECORDING_STOP)
                    .child(
                        div()
                            .size(size::STOP_SQUARE)
                            .rounded(size::STOP_SQUARE_RADIUS)
                            .bg(palette.background),
                    )
                    .child(strings::RECORDING_STOP)
                    .disabled(!state.can_record)
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(recorder) = this.recorder(cx) {
                            recorder.stop_recording();
                        }
                    })),
            )
            .child(level_bars(recording, cx))
        } else if state.starting {
            // "Gør klar…" until core says the recording runs or failed.
            pill.child(
                controls::pill_button("home-start", Pill::Start, cx)
                    .label(strings::RECORDING_STARTING)
                    .loading(true)
                    .disabled(true),
            )
        } else {
            pill.child(
                controls::pill_button("home-start", Pill::Start, cx)
                    .icon(Icon::from(IconName::Mic).size(icon_size::SM))
                    .label(strings::START_RECORDING)
                    // Without the host (no data folder) nothing can record,
                    // and the hero says why (`pages::home`).
                    .disabled(!state.can_record)
                    .on_click(cx.listener(|this, _, _, cx| this.start_recording(cx))),
            )
        };
        v_flex()
            .items_center()
            .gap(space::SM)
            .child(pill)
            .when_some(state.start_error, |this, error| {
                this.child(self.start_error(error, cx))
            })
            .into_any_element()
    }

    /// The destructive alert under the pill (`RecordingControls.tsx:416-431`).
    fn start_error(&self, error: StartError, cx: &mut Context<Self>) -> AnyElement {
        let palette = palette(cx);
        let (title, message) = error.texts();
        v_flex()
            .id("recording-start-error")
            .role(gpui::Role::Alert)
            .aria_label(SharedString::from(title))
            .relative()
            .mt(space::SM)
            .w_full()
            .max_w(layout::ONBOARDING_TEXT_MAX)
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
            .child(
                h_flex()
                    .items_start()
                    .gap(space::MD)
                    .child(
                        Icon::from(IconName::CircleAlert)
                            .size(icon_size::MD)
                            .text_color(palette.destructive),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .pr(size::DISMISS)
                            .child(
                                div()
                                    .mb(space::SM)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(palette.destructive)
                                    .child(title),
                            )
                            .children(message.split('\n').enumerate().map(|(index, line)| {
                                div()
                                    .min_w_0()
                                    .when(index > 0, |this| this.ml(space::SM))
                                    .text_color(palette.foreground)
                                    .child(line.to_string())
                            })),
                    ),
            )
            .child(
                div().absolute().top(space::SM).right(space::SM).child(
                    controls::icon_button(
                        "recording-error-dismiss",
                        IconName::X,
                        strings::RECORDING_ERROR_DISMISS,
                        Variant::Ghost,
                    )
                    .size(size::DISMISS)
                    .text_color(palette.destructive)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.home.start_error = None;
                        if let Some(recorder) = this.recorder(cx) {
                            recorder.dismiss_start_error();
                        }
                        cx.notify();
                    })),
                ),
            )
            .into_any_element()
    }
}

/// A round dot of `diameter`.
fn dot(diameter: gpui::Pixels, color: Hsla) -> impl IntoElement {
    div()
        .flex_none()
        .size(diameter)
        .rounded(theme::RADIUS_FULL)
        .bg(color)
}

/// A recording without lines (Tauri `VirtualizedTranscriptView.tsx` empty
/// state): a dot, what is happening and what to do. Tauri says "Lytter efter
/// tale …", but no transcript is made while recording, so this says that the
/// audio is saved and transcribed after "Stop og gem".
fn empty_transcript(paused: bool, cx: &App) -> AnyElement {
    let palette = palette(cx);
    let (color, title, hint) = if paused {
        (
            palette.warning,
            strings::RECORDING_EMPTY_PAUSED,
            strings::RECORDING_EMPTY_PAUSED_HINT,
        )
    } else {
        (
            palette.primary,
            strings::RECORDING_EMPTY_RECORDING,
            strings::RECORDING_EMPTY_RECORDING_HINT,
        )
    };
    v_flex()
        .id("recording-empty")
        .mt(size::EMPTY_TOP)
        .items_center()
        .text_color(palette.muted_foreground)
        .child(div().mb(space::MD).child(dot(size::EMPTY_DOT, color)))
        .child(div().text_size(typography::SM).child(title))
        .child(div().mt(space::XS).text_size(typography::XS).child(hint))
        .into_any_element()
}

/// One line: the timestamp column and the text. The newest line while
/// recording sits on a `secondary` background; an unsure line is underlined.
fn transcript_line(index: usize, line: &Line, streaming: bool, cx: &App) -> AnyElement {
    let palette = palette(cx);
    let text = div()
        .text_size(typography::MD)
        .line_height(relative(size::LINE_HEIGHT))
        .text_color(palette.foreground)
        .child(if line.low_confidence {
            dotted_text(&line.text, palette.muted_foreground)
        } else {
            div().child(line.text.clone()).into_any_element()
        });
    h_flex()
        .id(("recording-line", index))
        .when(line.low_confidence, |this| {
            this.tooltip(|window, cx| {
                gpui_component::tooltip::Tooltip::new(strings::RECORDING_LOW_CONFIDENCE)
                    .build(window, cx)
            })
        })
        .items_start()
        .gap(space::SM)
        .mb(space::MD)
        .child(
            div()
                .flex_none()
                .mt(size::TIMESTAMP_NUDGE)
                .text_size(typography::SM)
                .text_color(palette.muted_foreground)
                .child(clock(line.seconds)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .when(streaming, |this| {
                    this.px(space::MD)
                        .py(space::SM)
                        .rounded(theme::RADIUS_LG)
                        .bg(palette.secondary)
                })
                .child(text),
        )
        .into_any_element()
}

/// X positions of the dots of a dotted underline from `start` to `end`:
/// one pixel dots, one pixel apart (`decoration-dotted` at 1 px).
fn dot_positions(start: f32, end: f32) -> impl Iterator<Item = f32> {
    let count = ((end - start) / f32::from(size::DOT_PERIOD))
        .floor()
        .max(0.0) as usize;
    (0..count).map(move |n| start + n as f32 * f32::from(size::DOT_PERIOD))
}

/// Text with Tauri's dotted underline (`underline decoration-dotted`). gpui's
/// text decoration is only solid or wavy, so the dots are painted under each
/// wrapped line from the text layout.
fn dotted_text(text: &str, color: Hsla) -> AnyElement {
    let styled = StyledText::new(text.to_string());
    let layout = styled.layout().clone();
    let text = text.to_string();
    div()
        .relative()
        .child(styled)
        .child(
            canvas(
                |_, _, _| (),
                move |_, _, window, _| {
                    let line_height = layout.line_height();
                    // (line top, first x, end x) per wrapped line.
                    let mut lines: Vec<(Pixels, Pixels, Pixels)> = Vec::new();
                    for (index, ch) in text.char_indices() {
                        let Some(at) = layout.position_for_index(index) else {
                            continue;
                        };
                        let advance = layout
                            .position_for_index(index + ch.len_utf8())
                            .filter(|next| next.y == at.y)
                            .map_or(line_height * 0.4, |next| next.x - at.x);
                        match lines.last_mut() {
                            Some((top, _, end)) if *top == at.y => *end = at.x + advance,
                            _ => lines.push((at.y, at.x, at.x + advance)),
                        }
                    }
                    for (top, start, end) in lines {
                        let y = top + line_height - size::DOT_OFFSET;
                        for x in dot_positions(f32::from(start), f32::from(end)) {
                            window.paint_quad(fill(
                                Bounds::new(
                                    point(px(x), y),
                                    gpui_size(size::UNDERLINE, size::UNDERLINE),
                                ),
                                color,
                            ));
                        }
                    }
                },
            )
            .absolute()
            .size_full(),
        )
        .into_any_element()
}

/// The device notice inside the status bar (`RecordingStatusBar.tsx:70-87`).
fn device_notice(notice: &DeviceNotice, cx: &mut Context<Shell>) -> AnyElement {
    let palette = palette(cx);
    let warning = notice.is_warning();
    let (tone, border, icon) = if warning {
        (palette.warning, size::ALERT_BORDER, IconName::TriangleAlert)
    } else {
        (
            palette.primary,
            size::INFO_BORDER,
            IconName::from(ComponentIcon::Info),
        )
    };
    let message = notice.message();
    h_flex()
        .id("recording-device-notice")
        .role(gpui::Role::Status)
        .aria_label(SharedString::from(message.clone()))
        .mt(space::MD)
        .items_start()
        .gap(space::SM)
        .px(space::MD)
        .py(space::SM)
        .rounded(theme::RADIUS)
        .border_1()
        .border_color(theme::over(tone, palette.background, border))
        .bg(theme::over(tone, palette.background, size::NOTICE_TINT))
        .text_size(typography::SM)
        .text_color(palette.foreground)
        .child(
            div()
                .flex_none()
                .mt(space::XXS)
                .child(Icon::from(icon).size(icon_size::SM).text_color(tone)),
        )
        .child(div().flex_1().min_w_0().child(message))
        .child(
            div().flex_none().my(-space::XS).mr(-space::XS).child(
                controls::icon_button(
                    "recording-notice-dismiss",
                    IconName::X,
                    strings::RECORDING_NOTICE_DISMISS,
                    Variant::Ghost,
                )
                .size(size::DISMISS)
                .text_color(palette.muted_foreground)
                .on_click(cx.listener(|this, _, _, cx| {
                    if let Some(recording) = this.home.recording.as_mut() {
                        recording.notice = None;
                    }
                    if let Some(recorder) = this.recorder(cx) {
                        recorder.dismiss_notice();
                    }
                    cx.notify();
                })),
            ),
        )
        .into_any_element()
}

/// "Lyden gemmes ikke: disken er fuld" inside the status bar, as a warning
/// that stays while it is true (Tauri shows a toast that stays).
fn storage_notice(message: &str, cx: &App) -> AnyElement {
    let palette = palette(cx);
    h_flex()
        .id("recording-storage-notice")
        .role(gpui::Role::Alert)
        .aria_label(SharedString::from(message.to_string()))
        .mt(space::MD)
        .items_start()
        .gap(space::SM)
        .px(space::MD)
        .py(space::SM)
        .rounded(theme::RADIUS)
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
        .text_color(palette.foreground)
        .child(
            div().flex_none().mt(space::XXS).child(
                Icon::from(IconName::CircleAlert)
                    .size(icon_size::SM)
                    .text_color(palette.destructive),
            ),
        )
        .child(div().flex_1().min_w_0().child(message.to_string()))
        .into_any_element()
}

/// Three level bars (microphone, both, system audio): `recording` while
/// recording, 4 px `warning` dots at 60 % when paused. The heights follow
/// core's measured peaks (Tauri draws random heights) and never animate on
/// their own, so they are calm with reduced motion too.
fn level_bars(recording: &Recording, cx: &App) -> AnyElement {
    let palette = palette(cx);
    h_flex()
        .mx(space::MD)
        .gap(space::XS)
        .h(layout::CONTROL_HEIGHT)
        .items_center()
        .children(recording.levels.iter().map(|level| {
            let height = if recording.paused {
                size::LEVEL_BAR_PAUSED
            } else {
                (size::LEVEL_BAR_MAX * level.clamp(0.0, 1.0)).max(size::LEVEL_BAR_PAUSED)
            };
            div()
                .w(size::LEVEL_BAR_WIDTH)
                .h(height)
                .rounded(theme::RADIUS_FULL)
                .bg(if recording.paused {
                    palette.warning
                } else {
                    palette.recording
                })
                .when(recording.paused, |this| {
                    this.opacity(size::LEVEL_PAUSED_OPACITY)
                })
        }))
        .into_any_element()
}

/// Show a message from the recorder: a notification in the main window, a
/// fresh meeting list, or the finished meeting.
pub(crate) fn show_notice(notice: Notice, cx: &mut App) {
    let notification = match notice {
        Notice::Error {
            title,
            text,
            sticky,
        } => {
            log::info!("Recording notice: {title}");
            Some(
                match text {
                    Some(text) => Notification::error(text).title(title),
                    None => Notification::error(title),
                }
                .autohide(!sticky),
            )
        }
        Notice::Warning { text, sticky } => Some(Notification::warning(text).autohide(!sticky)),
        Notice::Success(text) => Some(Notification::success(text)),
        Notice::MeetingSaved(_) => {
            crate::ui::meetings_changed(cx);
            None
        }
        Notice::OpenMeeting(id) => {
            crate::ui::open_meeting(id, cx);
            None
        }
    };
    if let Some(notification) = notification {
        crate::ui::notify_main_window(notification, cx);
    }
}

#[cfg(feature = "screenshot-tests")]
pub(crate) fn fixture_recording(paused: bool, notice: Option<DeviceNotice>) -> Recording {
    let microphone = match &notice {
        Some(DeviceNotice::Switched(name)) => Some(name.clone()),
        _ => None,
    };
    Recording {
        paused,
        seconds: 12 * 60 + 34,
        microphone,
        system: None,
        notice,
        storage_error: None,
        levels: [0.58, 0.76, 0.58],
    }
}

/// The home state of a screenshot fixture: what the recorder reports in
/// each stage (no transcript lines: none are made while recording).
#[cfg(feature = "screenshot-tests")]
pub(crate) fn fixture(variant: crate::ui::FixtureVariant) -> HomeState {
    use crate::ui::FixtureVariant as V;
    let mut state = HomeState {
        can_record: true,
        ..HomeState::default()
    };
    match variant {
        V::Recording => state.recording = Some(fixture_recording(false, None)),
        V::RecordingPaused => state.recording = Some(fixture_recording(true, None)),
        V::RecordingDeviceSwitched => {
            state.recording = Some(fixture_recording(
                false,
                Some(DeviceNotice::Switched("Headset (USB)".into())),
            ))
        }
        V::RecordingSaving => state.saving = Some(Saving::Stopping),
        V::RecordingFinishing => state.saving = Some(Saving::Finishing),
        V::RecordingStartError => state.start_error = Some(StartError::Microphone),
        _ => {}
    }
    state
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dotted_underline_has_a_dot_every_two_pixels() {
        let dots: Vec<f32> = super::dot_positions(10.0, 17.0).collect();
        assert_eq!(dots, vec![10.0, 12.0, 14.0]);
        assert_eq!(super::dot_positions(5.0, 5.0).count(), 0);
        assert_eq!(super::dot_positions(9.0, 4.0).count(), 0);
    }

    #[test]
    fn clock_matches_tauri() {
        assert_eq!(clock(0), "00:00");
        assert_eq!(clock(754), "12:34");
        assert_eq!(clock(3600 + 62), "1:01:02");
    }

    #[test]
    fn copy_keeps_minutes_past_the_hour() {
        let lines = [
            Line {
                seconds: 3,
                text: "Hej".into(),
                low_confidence: false,
            },
            Line {
                seconds: 3725,
                text: "Farvel".into(),
                low_confidence: true,
            },
        ];
        assert_eq!(copy_text(&lines), "[00:03] Hej\n[62:05] Farvel");
    }

    #[test]
    fn device_notices_use_the_tauri_texts() {
        let switched = DeviceNotice::Switched("Headset (USB)".into());
        assert!(!switched.is_warning());
        assert_eq!(
            switched.message(),
            "Mikrofonen er skiftet til Headset (USB). Optagelsen fortsætter som normalt."
        );
        assert!(DeviceNotice::Lost("X".into()).is_warning());
        assert!(DeviceNotice::SwitchFailed("X".into()).is_warning());
    }

    #[test]
    fn idle_state_without_a_host_cannot_record() {
        let state = HomeState::default();
        assert!(!state.can_record);
        assert!(!state.shows_transcript());
        assert!(!state.is_recording());
        assert!(!state.is_busy());
    }

    fn recorder(phase: Phase) -> RecorderState {
        RecorderState {
            phase,
            seconds: 754,
            levels: [0.5, 0.75, 0.25],
            microphone: Some("Headset (USB)".into()),
            system: Some("Højttalere".into()),
            ..Default::default()
        }
    }

    #[test]
    fn the_recovery_scan_is_redone_when_the_recorder_becomes_free() {
        let home = |phase| HomeState::from_recorder(&recorder(phase));
        assert!(home(Phase::Finishing).becomes_free(&home(Phase::Idle)));
        assert!(home(Phase::SavingMeeting).becomes_free(&home(Phase::Idle)));
        assert!(home(Phase::Starting).becomes_free(&home(Phase::Idle)));
        assert!(!home(Phase::Idle).becomes_free(&home(Phase::Idle)));
        assert!(!home(Phase::Finishing).becomes_free(&home(Phase::Starting)));
        assert!(!home(Phase::Recording).becomes_free(&home(Phase::Paused)));
    }

    #[test]
    fn the_screen_follows_the_recorder_through_a_meeting() {
        let idle = HomeState::from_recorder(&RecorderState::default());
        assert!(idle.can_record && !idle.is_busy() && !idle.shows_transcript());

        let starting = HomeState::from_recorder(&recorder(Phase::Starting));
        assert!(starting.starting && starting.is_busy() && !starting.shows_transcript());

        let running = HomeState::from_recorder(&recorder(Phase::Recording));
        let recording = running.recording.as_ref().unwrap();
        assert!(!recording.paused);
        assert_eq!(clock(recording.seconds), "12:34");
        assert_eq!(recording.levels, [0.5, 0.75, 0.25]);
        assert_eq!(recording.microphone.as_deref(), Some("Headset (USB)"));
        assert!(running.is_recording() && running.shows_transcript());

        let paused = HomeState::from_recorder(&recorder(Phase::Paused));
        assert!(paused.recording.unwrap().paused);

        for (phase, saving) in [
            (Phase::Stopping, Saving::Stopping),
            (Phase::SavingMeeting, Saving::SavingMeeting),
            (Phase::Finishing, Saving::Finishing),
        ] {
            let state = HomeState::from_recorder(&recorder(phase));
            assert_eq!(state.saving, Some(saving));
            assert!(state.recording.is_none() && state.is_busy());
        }
    }

    #[test]
    fn errors_and_notices_from_the_recorder_use_the_tauri_texts() {
        let state = HomeState::from_recorder(&RecorderState {
            start_error: Some(host::StartError::Permission),
            ..Default::default()
        });
        assert_eq!(
            state.start_error.unwrap().texts(),
            ("Tilladelse mangler", strings::RECORDING_ERROR_PERMISSION)
        );
        let state = HomeState::from_recorder(&RecorderState {
            phase: Phase::Recording,
            notice: Some(host::DeviceNotice::Lost("Headset (USB)".into())),
            storage_error: Some("Lyden gemmes ikke: disken er fuld".into()),
            ..Default::default()
        });
        let recording = state.recording.unwrap();
        assert_eq!(
            recording.notice,
            Some(DeviceNotice::Lost("Headset (USB)".into()))
        );
        assert_eq!(
            recording.storage_error.as_deref(),
            Some("Lyden gemmes ikke: disken er fuld")
        );
    }
}
