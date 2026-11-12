//! The recording screen of the home page (Tauri `RecordingControls.tsx`,
//! `RecordingStatusBar.tsx`, `TranscriptPanel.tsx`,
//! `VirtualizedTranscriptView.tsx`, `StatusOverlays.tsx`;
//! `docs/screenshots/optagelse-*.png`): the status bar, the transcript lines,
//! the floating pill and the saving overlays.
//!
//! The pieces draw a [`HomeState`]. Starting, pausing and stopping a meeting
//! recording is not wired to core in this app yet: core's recording commands
//! only save the audio, and turning a stopped recording into a meeting is
//! still done by the Tauri frontend (`useRecordingStop.ts`). Until that moves
//! to core, [`HomeState::can_record`] is false, "Start optagelse" is disabled
//! and says why, and nothing fills `recording`. Screenshot fixtures fill it.

use gpui::{
    div, prelude::*, relative, AnyElement, App, ClipboardItem, Context, FontWeight, Hsla,
    SharedString,
};
use gpui_component::{
    h_flex, notification::Notification, spinner::Spinner, v_flex, Disableable as _, Icon,
    IconName as ComponentIcon, Sizable as _, WindowExt as _,
};
use gpui_kit_assets::IconName;
use tingskriver_core::strings;

use crate::controls::{self, Pill, Variant};
use crate::theme::{self, icon_size, layout, recording as size, shadow, space, typography};
use crate::ui::{palette, Shell};

/// What the home page shows besides the idle hero.
#[derive(Debug, Clone, Default)]
pub struct HomeState {
    /// This app can start a meeting recording. False until recording is
    /// wired to core (module docs).
    pub can_record: bool,
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
    /// Input level per bar, 0 to 1 (three bars).
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
    /// The recognizer is unsure: dotted underline in Tauri, a solid muted
    /// underline here (gpui has no dotted text decoration).
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
            // Not wired to core yet (module docs): disabled like "Start
            // optagelse" until it is.
            pill.child(
                controls::pill_button("recording-pause", Pill::Pause, cx)
                    .icon(Icon::from(icon).size(icon_size::SM))
                    .label(label)
                    .disabled(!state.can_record),
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
                    .disabled(!state.can_record),
            )
            .child(level_bars(recording, cx))
        } else {
            pill.child(
                controls::pill_button("home-start", Pill::Start, cx)
                    .icon(Icon::from(IconName::Mic).size(icon_size::SM))
                    .label(strings::START_RECORDING)
                    // Not wired to core yet: disabled, and the hero says why
                    // (module docs).
                    .disabled(!state.can_record),
            )
        };
        // While recording cannot start here, the hero says why instead of
        // "Tryk på \"Start optagelse\"" (`pages::home`).
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

/// A recording without lines yet (Tauri `VirtualizedTranscriptView.tsx`
/// empty state): a dot, what is happening and what to do.
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
            strings::RECORDING_EMPTY_LISTENING,
            strings::RECORDING_EMPTY_LISTENING_HINT,
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
        .when(line.low_confidence, |this| {
            this.underline()
                .text_decoration_color(palette.muted_foreground)
                .text_decoration_1()
        })
        .child(line.text.clone());
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
                    cx.notify();
                })),
            ),
        )
        .into_any_element()
}

/// Three level bars: `recording` while recording, 4 px `warning` dots at
/// 60 % when paused. The heights follow the input level and never animate
/// on their own, so they are calm with reduced motion too.
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

/// Synthetic content for the screenshot fixtures (the lines of
/// `docs/screenshots/optagelse-i-gang-*.png`; no meeting data).
#[cfg(feature = "screenshot-tests")]
pub(crate) fn fixture_lines() -> Vec<Line> {
    let line = |seconds, text: &str, low_confidence| Line {
        seconds,
        text: text.into(),
        low_confidence,
    };
    vec![
        line(
            3,
            "Godmorgen alle sammen, så går vi i gang med ugens status.",
            false,
        ),
        line(
            22,
            "Jeg starter med bookingsiden. Vi er tæt på, men testen er ikke helt færdig.",
            false,
        ),
        line(
            41,
            "Kan vi nå det til fredag, eller skal vi rykke lanceringen?",
            false,
        ),
        line(
            60,
            "Jeg tror, det bliver tirsdag, hvis vi vil være sikre på mobilvisningen.",
            false,
        ),
        line(
            79,
            "Det er fint for mig. Kunden har alligevel ikke travlt før næste uge.",
            true,
        ),
    ]
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
        levels: [0.58, 0.76, 0.58],
    }
}

/// The home state of a screenshot fixture.
#[cfg(feature = "screenshot-tests")]
pub(crate) fn fixture(variant: crate::ui::FixtureVariant) -> HomeState {
    use crate::ui::FixtureVariant as V;
    let mut state = HomeState::default();
    // The recording fixtures show what the app looks like once recording is
    // wired to core; a start error or a running recording cannot happen
    // while it cannot record.
    let idle = matches!(
        variant,
        V::Plain | V::Meetings | V::SidebarCollapsed | V::OnboardingWelcome | V::OnboardingModel
    );
    state.can_record = !idle;
    match variant {
        V::Plain
        | V::Meetings
        | V::SidebarCollapsed
        | V::OnboardingWelcome
        | V::OnboardingModel => {}
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
    }
    if state.shows_transcript() {
        state.lines = fixture_lines();
    }
    state
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn idle_state_cannot_record_yet() {
        let state = HomeState::default();
        assert!(!state.can_record);
        assert!(!state.shows_transcript());
        assert!(!state.is_recording());
    }
}
