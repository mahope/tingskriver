//! The home screen, idle (Tauri `app/page.tsx`, `TranscriptPanel.tsx`,
//! `RecordingControls.tsx`; `docs/screenshots/tom-tilstand-*.png`): the hero
//! "Klar til dit næste møde" and the floating "Start optagelse" pill.
//!
//! Recording has not moved to this app yet (docs/design-paritet.md, Job 3),
//! so the pill and "Ny optagelse" show why. Above the hero: a meeting that
//! sleep or logoff ended, interrupted recordings to restore, and that notice.

use gpui::{div, prelude::*, AnyElement, Context, SharedString};
use gpui_component::{h_flex, v_flex, Icon};
use gpui_kit_assets::IconName;
use tingskriver_core::strings;

use crate::controls::{self, Pill};
use crate::sidebar::Destination;
use crate::theme::{self, icon_size, layout, shadow, sidebar as size, space, typography};
use crate::ui::{heading, interrupted_card, palette, paragraph, Level, Shell};

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
                    .p(space::LG)
                    .rounded(theme::RADIUS_LG)
                    .border_1()
                    .border_color(palette.border)
                    .bg(palette.card)
                    .child(paragraph(notice, cx))
                    .into_any_element(),
            );
        }
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
                    .child(strings::HOME_HINT),
            );
        let pill = h_flex()
            .gap(space::SM)
            .px(size::PILL_PADDING_X)
            .py(space::SM)
            .rounded(theme::RADIUS_FULL)
            .border_1()
            .border_color(palette.border)
            .bg(palette.card)
            .shadow(shadow::md())
            .child(
                controls::pill_button("home-start", Pill::Start, cx)
                    .icon(Icon::from(IconName::Mic).size(icon_size::SM))
                    .label(strings::START_RECORDING)
                    .on_click(cx.listener(|this, _, _, cx| this.navigate(Destination::Record, cx))),
            );
        div()
            .relative()
            .size_full()
            .child(
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
                    ),
            )
            .child(
                h_flex()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom(size::PILL_BOTTOM)
                    .justify_center()
                    .child(pill),
            )
            .into_any_element()
    }
}
