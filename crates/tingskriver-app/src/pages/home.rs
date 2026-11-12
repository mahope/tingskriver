//! The home screen (Tauri `app/page.tsx`, `TranscriptPanel.tsx`,
//! `RecordingControls.tsx`; `docs/screenshots/tom-tilstand-*.png` and
//! `optagelse-*.png`): the hero "Klar til dit næste møde", or the transcript
//! of a recording, and the floating recording pill (`recording`).
//!
//! Above the hero: a meeting that sleep or logoff ended, interrupted
//! recordings to restore, and why an import did not open a screen.

use gpui::{div, prelude::*, AnyElement, App, Context, FontWeight, SharedString};
use gpui_component::{h_flex, v_flex, Icon};
use gpui_kit_assets::IconName;
use tingskriver_core::audio::power::InterruptNotice;
use tingskriver_core::strings;

use crate::controls::{self, Variant};
use crate::theme::{self, icon_size, layout, recording, sidebar as size, space, typography};
use crate::ui::{dismiss_interrupted, heading, palette, paragraph, Level, Shell};

impl Shell {
    pub(crate) fn home_page(&self, cx: &mut Context<Self>) -> AnyElement {
        let palette = palette(cx);
        let mut notices = Vec::new();
        if let Some(interrupted) = self.model.read(cx).interrupted.clone() {
            // Sleep or logoff ended a recording (#179): shown until dismissed.
            notices.push(interrupted_card(&interrupted, cx));
        }
        if let Some(recovery) = self
            .meetings
            .update(cx, |meetings, cx| meetings.recovery_card(cx))
        {
            notices.push(recovery);
        }
        if let Some(notice) = self.home_notice {
            notices.push(
                div()
                    .id("home-notice")
                    .role(gpui::Role::Status)
                    .aria_label(SharedString::from(notice))
                    .px(space::LG)
                    .py(space::MD)
                    .rounded(theme::RADIUS_LG)
                    .border_1()
                    .border_color(palette.border)
                    .bg(palette.background)
                    .child(paragraph(notice, cx))
                    .into_any_element(),
            );
        }
        let body = if self.home.shows_transcript() {
            // Notices stay reachable above a running transcript.
            v_flex()
                .size_full()
                .when(!notices.is_empty(), |this| {
                    this.child(
                        v_flex()
                            .flex_none()
                            .w_full()
                            .max_w(layout::TRANSCRIPT_MAX)
                            .mx_auto()
                            .px(recording::TRANSCRIPT_PADDING_X)
                            .pt(recording::TRANSCRIPT_TOP)
                            .gap(space::MD)
                            .children(notices),
                    )
                })
                .child(div().flex_1().min_h_0().child(self.transcript_view(cx)))
                .into_any_element()
        } else {
            let hero = v_flex()
                .max_w(layout::ONBOARDING_TEXT_MAX)
                .items_center()
                .text_center()
                .child(heading(strings::HOME_TITLE, Level::Hero))
                .child(
                    div()
                        .mt(space::MD)
                        .text_size(typography::MD)
                        .text_color(palette.muted_foreground)
                        .child(strings::HOME_LEAD),
                )
                .child(
                    div()
                        .mt(space::SM)
                        .text_size(typography::SM)
                        .text_color(palette.muted_foreground)
                        // "Tryk på \"Start optagelse\"" only while it can.
                        .child(if self.home.can_record {
                            strings::HOME_HINT
                        } else {
                            strings::RECORDING_NOT_IN_APP
                        }),
                );
            v_flex()
                .id("home")
                .size_full()
                .overflow_y_scroll()
                .px(space::XXL)
                .when(!notices.is_empty(), |this| {
                    this.pt(space::XXL).child(
                        v_flex()
                            .w_full()
                            .max_w(layout::TRANSCRIPT_MAX)
                            .mx_auto()
                            .gap(space::MD)
                            .children(notices),
                    )
                })
                .child(
                    v_flex()
                        .flex_1()
                        .items_center()
                        .justify_center()
                        .pb(size::HOME_BOTTOM_SPACE)
                        .child(hero),
                )
                .into_any_element()
        };
        let overlay = matches!(
            self.home.saving,
            Some(crate::recording::Saving::Finishing | crate::recording::Saving::SavingMeeting)
        );
        div()
            .relative()
            .size_full()
            .child(body)
            .child(
                h_flex()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom(if overlay {
                        recording::OVERLAY_BOTTOM
                    } else {
                        size::PILL_BOTTOM
                    })
                    .px(space::XXL)
                    .justify_center()
                    .child(self.recording_pill(cx)),
            )
            .into_any_element()
    }
}

/// A meeting that sleep or logoff ended (#179), in the style of the
/// recording status bar with a warning border. GPUI only: Tauri reports it
/// with toasts.
pub(crate) fn interrupted_card(notice: &InterruptNotice, cx: &App) -> AnyElement {
    let palette = palette(cx);
    let lines = notice.lines();
    v_flex()
        .id("interrupted-notice")
        .role(gpui::Role::Alert)
        .aria_label(SharedString::from(strings::INTERRUPTED_TITLE))
        .gap(space::XS)
        .px(space::LG)
        .py(space::MD)
        .rounded(theme::RADIUS_LG)
        .border_1()
        .border_color(theme::over(
            palette.warning,
            palette.background,
            recording::ALERT_BORDER,
        ))
        .bg(palette.background)
        .text_size(typography::SM)
        .child(
            h_flex()
                .gap(space::SM)
                .child(
                    Icon::from(IconName::TriangleAlert)
                        .size(icon_size::SM)
                        .text_color(palette.warning),
                )
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette.foreground)
                        .child(strings::INTERRUPTED_TITLE),
                ),
        )
        .children(lines.into_iter().map(|line| {
            div()
                .pl(icon_size::SM + space::SM)
                .text_color(palette.muted_foreground)
                .child(line)
        }))
        .child(
            div().pt(space::SM).pl(icon_size::SM + space::SM).child(
                controls::small_button("interrupted-dismiss", Variant::Outline)
                    .label(strings::INTERRUPTED_DISMISS)
                    .on_click(|_, _, cx| dismiss_interrupted(cx)),
            ),
        )
        .into_any_element()
}
