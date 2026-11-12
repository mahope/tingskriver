//! The sidebar of the main window, as the Tauri app
//! (`frontend/src/components/Sidebar/index.tsx`, `Logo.tsx`, `Info.tsx`):
//! logo mark and wordmark, "Ny optagelse", the search field, the meeting
//! list (`MeetingsView`), the footer rows and the 64 px collapsed column.
//!
//! Keyboard order follows the Tauri DOM without the logo: record, search,
//! list, footer, then the collapse handle.

use gpui::{div, prelude::*, svg, AnyElement, Context, ElementId, FontWeight, SharedString};
use gpui_component::{
    button::{Button, ButtonRounded, ButtonVariants},
    h_flex, v_flex, Icon,
};
use gpui_kit_assets::IconName;
use tingskriver_core::strings;

use crate::assets;
use crate::controls::{self, Variant};
use crate::theme::{self, icon_size, layout, shadow, sidebar as size, space, typography};
use crate::ui::{palette, Page, Shell};

/// Where a sidebar action takes the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Destination {
    Page(Page),
    /// "Ny optagelse": the home screen, with a notice while recording has
    /// not moved to this app yet.
    Record,
    /// "Importér transskript" / "Importér lyd": not in this app yet.
    Import,
}

impl Shell {
    /// Go somewhere from the sidebar. Leaves the open meeting.
    pub(crate) fn navigate(&mut self, to: Destination, cx: &mut Context<Self>) {
        self.meetings.update(cx, |meetings, cx| {
            meetings.selected = None;
            cx.notify();
        });
        self.home_notice = None;
        match to {
            Destination::Page(page) => self.page = page,
            // Say why recording cannot start here yet, so the click
            // visibly does something even on the home screen.
            Destination::Record => {
                self.page = Page::Home;
                if !self.home.can_record {
                    self.home_notice = Some(strings::RECORDING_NOT_IN_APP);
                }
            }
            Destination::Import => {
                self.page = Page::Home;
                self.home_notice = Some(strings::IMPORT_LATER);
            }
        }
        cx.notify();
    }

    fn on_nav(
        &self,
        to: Destination,
        cx: &mut Context<Self>,
    ) -> impl Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static {
        let shell = cx.entity().downgrade();
        move |_, _, cx| {
            let _ = shell.update(cx, |this, cx| this.navigate(to, cx));
        }
    }

    /// The sidebar column: 256 px, or 64 px when collapsed.
    pub(crate) fn sidebar(&self, cx: &mut Context<Self>) -> AnyElement {
        let palette = palette(cx);
        let column = v_flex()
            .id("sidebar")
            .role(gpui::Role::Navigation)
            .aria_label(SharedString::from(strings::SIDEBAR_LABEL))
            .flex_none()
            .h_full()
            .bg(palette.card)
            .border_r_1()
            .border_color(palette.border);
        if self.sidebar_collapsed {
            column
                .w(layout::SIDEBAR_COLLAPSED_WIDTH)
                .child(self.collapsed_column(cx))
                .into_any_element()
        } else {
            column
                .w(layout::SIDEBAR_WIDTH)
                .child(self.sidebar_header(cx))
                .child(div().flex_1().min_h_0().child(self.meetings.clone()))
                .child(self.sidebar_footer(cx))
                .into_any_element()
        }
    }

    /// The round chevron on the sidebar's right edge (`-right-3 top-5`). It
    /// is placed by the shell over the border, so it draws above the page.
    pub(crate) fn collapse_handle(&self, cx: &mut Context<Self>) -> AnyElement {
        let palette = palette(cx);
        let collapsed = self.sidebar_collapsed;
        let width = if collapsed {
            layout::SIDEBAR_COLLAPSED_WIDTH
        } else {
            layout::SIDEBAR_WIDTH
        };
        let label = if collapsed {
            strings::SIDEBAR_EXPAND
        } else {
            strings::SIDEBAR_COLLAPSE
        };
        div()
            .absolute()
            .top(size::HANDLE_TOP)
            .left(width - size::HANDLE / 2.)
            .child(
                Button::new("sidebar-collapse")
                    .icon(
                        Icon::from(if collapsed {
                            IconName::ChevronRight
                        } else {
                            IconName::ChevronLeft
                        })
                        .size(icon_size::SM),
                    )
                    .ghost()
                    .size(size::HANDLE)
                    .rounded(ButtonRounded::Size(theme::RADIUS_FULL))
                    .bg(palette.card)
                    .border_1()
                    .border_color(palette.border)
                    .text_color(palette.muted_foreground)
                    .accessibility_label(label)
                    .tooltip(label)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.sidebar_collapsed = !this.sidebar_collapsed;
                        cx.notify();
                    })),
            )
            .into_any_element()
    }

    fn logo(&self, collapsed: bool, cx: &mut Context<Self>) -> AnyElement {
        let palette = palette(cx);
        let mark = svg()
            .path(assets::LOGO_MARK)
            .size(if collapsed {
                size::LOGO_MARK_COLLAPSED
            } else {
                size::LOGO_MARK
            })
            .text_color(palette.primary);
        // The logo opens About, as in Tauri; it is not a tab stop, the ⓘ
        // button in the footer is the keyboard path.
        h_flex()
            .id("sidebar-logo")
            .role(gpui::Role::Button)
            .aria_label(SharedString::from(strings::ABOUT_APP))
            .min_h(layout::CONTROL_HEIGHT)
            .gap(space::SM)
            .rounded(theme::RADIUS)
            .when(collapsed, |this| {
                this.size(layout::CONTROL_HEIGHT).justify_center()
            })
            .when(!collapsed, |this| this.px(space::XS))
            .child(mark)
            .when(!collapsed, |this| {
                this.child(
                    div()
                        .font_family(typography::SERIF)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_size(typography::XL)
                        .text_color(palette.foreground)
                        .child(strings::APP_NAME),
                )
            })
            .on_click(self.on_nav(Destination::Page(Page::About), cx))
            .into_any_element()
    }

    /// "Ny optagelse", or "Optagelse i gang" while a meeting records.
    fn record_button(&self, cx: &mut Context<Self>) -> AnyElement {
        let palette = palette(cx);
        if self.meetings.read(cx).recording() || self.home.is_recording() {
            return h_flex()
                .id("sidebar-recording")
                .role(gpui::Role::Status)
                .aria_label(SharedString::from(strings::SIDEBAR_RECORDING))
                .w_full()
                .h(layout::CONTROL_HEIGHT)
                .justify_center()
                .gap(space::SM)
                .rounded(theme::RADIUS_LG)
                .bg(theme::over(
                    palette.recording,
                    palette.card,
                    size::RECORDING_TINT,
                ))
                .text_color(palette.recording)
                .text_size(typography::SM)
                .font_weight(FontWeight::SEMIBOLD)
                .child(
                    div()
                        .size(size::RECORDING_DOT)
                        .rounded(theme::RADIUS_FULL)
                        .bg(palette.recording),
                )
                .child(strings::SIDEBAR_RECORDING)
                .into_any_element();
        }
        controls::button("sidebar-record", Variant::Primary)
            .w_full()
            .rounded(ButtonRounded::Size(theme::RADIUS_LG))
            .font_weight(FontWeight::SEMIBOLD)
            .text_size(typography::SM)
            .icon(Icon::from(IconName::Mic).size(icon_size::SM))
            .label(strings::MEETING_NEW)
            .on_click(self.on_nav(Destination::Record, cx))
            .into_any_element()
    }

    fn sidebar_header(&self, cx: &mut Context<Self>) -> AnyElement {
        let search = self
            .meetings
            .update(cx, |meetings, cx| meetings.search_field(cx));
        v_flex()
            .flex_none()
            .px(space::LG)
            .pt(space::XL2)
            .pb(space::MD)
            .child(self.logo(false, cx))
            .child(div().mt(space::LG).child(self.record_button(cx)))
            .child(div().mt(space::MD).child(search))
            .into_any_element()
    }

    /// A 44 px footer row: 16 px icon and 14 px label, `bg-accent` when it
    /// is the current page.
    fn footer_row(
        &self,
        id: &'static str,
        icon: IconName,
        label: &'static str,
        active: bool,
        to: Destination,
        cx: &mut Context<Self>,
    ) -> Button {
        let palette = palette(cx);
        Button::new(id)
            .ghost()
            .w_full()
            .h(layout::CONTROL_HEIGHT)
            .px(space::MD)
            .rounded(ButtonRounded::Size(theme::RADIUS_LG))
            .text_size(typography::SM)
            .text_color(palette.foreground)
            .accessibility_label(label)
            .when(active, |this| this.bg(palette.accent))
            .child(
                h_flex()
                    .w_full()
                    .gap(space::SM)
                    .child(Icon::from(icon).size(icon_size::SM))
                    .child(label),
            )
            .on_click(self.on_nav(to, cx))
    }

    fn sidebar_footer(&self, cx: &mut Context<Self>) -> AnyElement {
        let palette = palette(cx);
        let page = self.page;
        let on_page = |p: Page| page == p;
        v_flex()
            .flex_none()
            .border_t_1()
            .border_color(palette.border)
            .p(space::SM)
            .gap(space::XXS)
            .child(self.footer_row(
                "sidebar-dictation",
                IconName::AudioLines,
                strings::DICTATION,
                on_page(Page::Dictation),
                Destination::Page(Page::Dictation),
                cx,
            ))
            .child(
                self.footer_row(
                    "sidebar-import-transcript",
                    IconName::FileText,
                    strings::IMPORT_TRANSCRIPT,
                    false,
                    Destination::Import,
                    cx,
                )
                .tooltip(strings::IMPORT_LATER),
            )
            .child(
                self.footer_row(
                    "sidebar-import-audio",
                    IconName::Upload,
                    strings::IMPORT_AUDIO,
                    false,
                    Destination::Import,
                    cx,
                )
                .tooltip(strings::IMPORT_LATER),
            )
            .child(
                h_flex()
                    .gap(space::XS)
                    .items_center()
                    .child(div().flex_1().min_w_0().child(self.footer_row(
                        "sidebar-settings",
                        IconName::Settings,
                        strings::SETTINGS,
                        on_page(Page::Settings),
                        Destination::Page(Page::Settings),
                        cx,
                    )))
                    .child(self.info_button(false, cx)),
            )
            .into_any_element()
    }

    /// ⓘ: opens Om (Tauri opens the About dialog here).
    fn info_button(&self, collapsed: bool, cx: &mut Context<Self>) -> Button {
        let palette = palette(cx);
        if collapsed {
            self.collapsed_item(
                "sidebar-about",
                IconName::Info,
                strings::ABOUT_APP,
                self.page == Page::About,
                cx,
            )
        } else {
            controls::icon_button(
                "sidebar-about",
                IconName::Info,
                strings::ABOUT_APP,
                Variant::Ghost,
            )
            .self_center()
            .text_color(palette.muted_foreground)
            .when(self.page == Page::About, |this| this.bg(palette.accent))
        }
        .on_click(self.on_nav(Destination::Page(Page::About), cx))
    }

    /// A 44 px icon square of the collapsed column with a 20 px icon and a
    /// tooltip. The icon is a child, not `Button::icon`, which would scale
    /// it with the button.
    fn collapsed_item(
        &self,
        id: impl Into<ElementId>,
        icon: IconName,
        label: &'static str,
        active: bool,
        cx: &mut Context<Self>,
    ) -> Button {
        let palette = palette(cx);
        square(id, icon, label, Variant::Ghost)
            .rounded(ButtonRounded::Size(theme::RADIUS_LG))
            .text_color(palette.muted_foreground)
            .when(active, |this| this.bg(palette.accent))
    }

    /// The 64 px column (`renderCollapsedIcons`): logo, record, Møder,
    /// import, Diktering, Indstillinger and ⓘ, each with a tooltip.
    fn collapsed_column(&self, cx: &mut Context<Self>) -> AnyElement {
        let palette = palette(cx);
        let recording = self.meetings.read(cx).recording();
        let record_label = if recording {
            strings::SIDEBAR_RECORDING
        } else {
            strings::MEETING_NEW
        };
        let record = square(
            "collapsed-record",
            if recording {
                IconName::Square
            } else {
                IconName::Mic
            },
            record_label,
            if recording {
                Variant::Ghost
            } else {
                Variant::Primary
            },
        )
        .rounded(ButtonRounded::Size(theme::RADIUS_FULL))
        .when(recording, |this| {
            this.bg(theme::over(
                palette.recording,
                palette.card,
                size::RECORDING_TINT,
            ))
            .text_color(palette.recording)
        })
        .when(!recording, |this| {
            this.on_click(self.on_nav(Destination::Record, cx))
        });
        let page = self.page;
        v_flex()
            .items_center()
            .gap(space::MD)
            .pt(space::LG)
            .child(self.logo(true, cx))
            .child(record)
            .child(
                self.collapsed_item(
                    "collapsed-meetings",
                    IconName::NotebookPen,
                    strings::MEETINGS,
                    false,
                    cx,
                )
                .accessibility_label(strings::SIDEBAR_SHOW_MEETINGS)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.sidebar_collapsed = false;
                    cx.notify();
                })),
            )
            .child(
                self.collapsed_item(
                    "collapsed-import-transcript",
                    IconName::FileText,
                    strings::IMPORT_TRANSCRIPT_TOOLTIP,
                    false,
                    cx,
                )
                .accessibility_label(strings::IMPORT_TRANSCRIPT)
                .on_click(self.on_nav(Destination::Import, cx)),
            )
            .child(
                self.collapsed_item(
                    "collapsed-import-audio",
                    IconName::Upload,
                    strings::IMPORT_AUDIO,
                    false,
                    cx,
                )
                .on_click(self.on_nav(Destination::Import, cx)),
            )
            .child(
                self.collapsed_item(
                    "collapsed-dictation",
                    IconName::AudioLines,
                    strings::DICTATION,
                    page == Page::Dictation,
                    cx,
                )
                .on_click(self.on_nav(Destination::Page(Page::Dictation), cx)),
            )
            .child(
                self.collapsed_item(
                    "collapsed-settings",
                    IconName::Settings,
                    strings::SETTINGS,
                    page == Page::Settings,
                    cx,
                )
                .on_click(self.on_nav(Destination::Page(Page::Settings), cx)),
            )
            .child(self.info_button(true, cx))
            .into_any_element()
    }
}

/// A 44 px square button with a 20 px icon (the collapsed column).
fn square(
    id: impl Into<ElementId>,
    icon: IconName,
    label: &'static str,
    variant: Variant,
) -> Button {
    let button = Button::new(id);
    let button = match variant {
        Variant::Primary => button.primary().shadow(shadow::base()),
        _ => button.ghost(),
    };
    button
        .child(Icon::from(icon).size(icon_size::MD))
        .accessibility_label(label)
        .tooltip(label)
        .size(layout::CONTROL_HEIGHT)
        .p_0()
}
