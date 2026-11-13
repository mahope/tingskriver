//! "Om Tingskriver" as a dialog over the current screen, and the update
//! dialog, as in the Tauri app (`frontend/src/components/Info.tsx`,
//! `About.tsx`, `UpdateDialog.tsx`, `UpdateNotification.tsx`).
//!
//! The Tauri layout comes first: logo mark, name, version, tagline, "Søg
//! efter opdateringer", "Det adskiller Tingskriver" and the footer links.
//! Below it, collapsed, is what only this app has: the build details with
//! the allowlisted diagnostics copy (`about::diagnostics`) and the bundled
//! legal documents.
//!
//! `Page::About` (`--page om`, the tray's "Om") is not a screen of its own:
//! the shell opens this dialog over the screen below it.

use std::cell::Cell;
use std::ops::Range;

use gpui::{
    div, prelude::*, px, relative, svg, AnyElement, App, Context, Entity, FocusHandle, FontWeight,
    HighlightStyle, InteractiveText, Role, ScrollHandle, SharedString, StyledText, UnderlineStyle,
    Window,
};
use gpui_component::{
    dialog::DialogFooter,
    h_flex,
    notification::Notification,
    scroll::{ScrollableElement as _, Scrollbar},
    v_flex, Icon, WindowExt,
};
use gpui_kit_assets::IconName;
use tingskriver_core::strings;

use crate::about::UpdateStatus;
use crate::assets;
use crate::controls::{self, Variant};
use crate::theme::{self, about_dialog as size, icon_size, layout, space, typography};
use crate::ui::{palette, AppModel};

const NOTICE_URL: &str = "https://github.com/mahope/tingskriver/blob/main/NOTICE.md";
const WEBSITE_URL: &str = "https://tingskriver.mahoje.dk";
const PRIVACY_URL: &str = "https://github.com/mahope/tingskriver/blob/main/PRIVACY_POLICY.md";
const CONTACT_URL: &str = "https://mahoje.dk";

/// How often the "Opdatering klar" notice looks at the background updater.
const WATCH_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60);

/// The state of the About dialog. It lives as long as the window, so a
/// diagnostics copy that finishes after the dialog closed has somewhere to
/// report to.
pub(crate) struct AboutView {
    model: Entity<AppModel>,
    check_focus: FocusHandle,
    document_scroll: Vec<ScrollHandle>,
    diagnosis_status: Option<&'static str>,
    checking: bool,
    /// The update a check found (Tauri's "Opdatering klar: version x").
    ready_version: Option<String>,
    build_open: bool,
    legal_open: bool,
}

impl AboutView {
    pub(crate) fn new(model: Entity<AppModel>, cx: &mut Context<Self>) -> Self {
        AboutView {
            model,
            check_focus: cx.focus_handle(),
            document_scroll: crate::about::DOCUMENTS
                .iter()
                .map(|_| ScrollHandle::new())
                .collect(),
            diagnosis_status: None,
            checking: false,
            ready_version: None,
            build_open: false,
            legal_open: false,
        }
    }

    /// "Søg efter opdateringer" (`About.tsx` `handleCheckForUpdates`): a
    /// found update opens the update dialog, anything else is a toast.
    fn check_updates(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.checking {
            return;
        }
        self.checking = true;
        cx.notify();
        let task = cx.background_spawn(async { crate::update::check_now() });
        cx.spawn_in(window, async move |this, cx| {
            let status = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.checking = false;
                let outcome = check_outcome(&status);
                if let CheckOutcome::Update(version) = &outcome {
                    this.ready_version = Some(version.clone());
                }
                cx.notify();
                match outcome {
                    CheckOutcome::Update(_) => {
                        let current = this.model.read(cx).display_version;
                        open_update(status, current, window, cx);
                    }
                    CheckOutcome::Latest => {
                        window.push_notification(Notification::success(strings::UPDATE_LATEST), cx)
                    }
                    CheckOutcome::Failed => window
                        .push_notification(Notification::error(strings::UPDATE_CHECK_FAILED), cx),
                    CheckOutcome::Message(text) => {
                        window.push_notification(Notification::info(text), cx)
                    }
                }
            });
        })
        .detach();
    }

    /// Copy the allowlisted diagnostics (`about::diagnostics`).
    fn copy_diagnostics(&mut self, cx: &mut Context<Self>) {
        self.diagnosis_status = Some(strings::ABOUT_COPYING);
        cx.notify();
        let task = self
            .model
            .read(cx)
            .runtime
            .spawn(crate::about::diagnostics());
        cx.spawn(async move |this, cx| {
            let status = match task.await {
                Ok(report) => {
                    cx.update(|cx| cx.write_to_clipboard(gpui::ClipboardItem::new_string(report)));
                    strings::ABOUT_COPIED
                }
                Err(_) => strings::ABOUT_COPY_FAILED,
            };
            let _ = this.update(cx, |this, cx| {
                this.diagnosis_status = Some(status);
                cx.notify();
            });
        })
        .detach();
    }

    /// Logo, name, version, tagline and the update button (`About.tsx:51-91`).
    fn hero(&self, cx: &mut Context<Self>) -> AnyElement {
        let palette = palette(cx);
        let version = self.model.read(cx).display_version;
        v_flex()
            .items_center()
            .text_center()
            .child(
                svg()
                    .path(assets::LOGO_MARK)
                    .size(size::LOGO)
                    .mb(space::MD)
                    .text_color(palette.primary),
            )
            .child(
                div()
                    .font_family(typography::SERIF)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_size(typography::XL)
                    .line_height(relative(typography::LINE_HEIGHT_TIGHT))
                    .child(strings::APP_NAME),
            )
            .child(
                div()
                    .text_color(palette.muted_foreground)
                    .child(strings::version(version)),
            )
            .child(div().mt(space::SM).child(strings::ABOUT_TAGLINE))
            .child(
                div().mt(space::LG).child(
                    controls::button("about-check-updates", Variant::Outline)
                        .track_focus(&self.check_focus)
                        .self_center()
                        .when(!self.checking, |button| {
                            button.icon(Icon::from(IconName::CircleCheckBig).size(icon_size::SM))
                        })
                        .loading(self.checking)
                        .label(if self.checking {
                            strings::SEARCHING
                        } else {
                            strings::UPDATE_CHECK
                        })
                        .on_click(
                            cx.listener(|this, _, window, cx| this.check_updates(window, cx)),
                        ),
                ),
            )
            .when_some(self.ready_version.clone(), |hero, version| {
                hero.child(
                    div()
                        .id("about-update-ready")
                        .role(Role::Status)
                        .mt(space::SM)
                        .text_color(palette.primary)
                        .child(strings::update_ready_line(&version)),
                )
            })
            .into_any_element()
    }

    /// "Det adskiller Tingskriver" (`About.tsx:93-113`).
    fn features(&self, cx: &App) -> AnyElement {
        let palette = palette(cx);
        let pairs = [
            (strings::ABOUT_LOCAL_TITLE, strings::ABOUT_LOCAL_TEXT),
            (strings::ABOUT_MODEL_TITLE, strings::ABOUT_MODEL_TEXT),
            (strings::ABOUT_PRICE_TITLE, strings::ABOUT_PRICE_TEXT),
            (strings::ABOUT_MEETINGS_TITLE, strings::ABOUT_MEETINGS_TEXT),
        ];
        v_flex()
            .gap(space::MD)
            .child(
                div()
                    .font_family(typography::SERIF)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_size(typography::MD)
                    .line_height(relative(typography::LINE_HEIGHT_TIGHT))
                    .child(strings::ABOUT_DIFFERENT),
            )
            .children(pairs.into_iter().map(|(term, text)| {
                v_flex()
                    .child(div().font_weight(FontWeight::MEDIUM).child(term))
                    .child(div().text_color(palette.muted_foreground).child(text))
            }))
            .into_any_element()
    }

    /// The open source note and the links (`About.tsx:115-124`).
    fn footer(&self, cx: &App) -> AnyElement {
        let palette = palette(cx);
        let text = strings::ABOUT_OPEN_SOURCE;
        let notice: Option<Range<usize>> = text
            .find(strings::ABOUT_NOTICE_LINK)
            .map(|start| start..start + strings::ABOUT_NOTICE_LINK.len());
        let underline = HighlightStyle {
            underline: Some(UnderlineStyle {
                thickness: size::LINK_UNDERLINE,
                color: Some(palette.muted_foreground),
                wavy: false,
            }),
            ..Default::default()
        };
        let note = InteractiveText::new(
            "about-open-source",
            StyledText::new(text).with_highlights(notice.clone().map(|range| (range, underline))),
        )
        .on_click(notice.into_iter().collect(), |_, _, cx| {
            cx.open_url(NOTICE_URL)
        });
        v_flex()
            .items_center()
            .text_center()
            .gap(space::XS)
            .pt(space::MD)
            .border_t_1()
            .border_color(palette.border)
            .text_size(typography::XS)
            .text_color(palette.muted_foreground)
            .child(note)
            .child(
                h_flex()
                    .gap(space::XS)
                    .child(footer_link(
                        "about-website",
                        strings::ABOUT_WEBSITE,
                        WEBSITE_URL,
                        cx,
                    ))
                    .child(strings::ABOUT_LINK_SEPARATOR)
                    .child(footer_link(
                        "about-github",
                        strings::ABOUT_GITHUB,
                        crate::update::REPO_URL,
                        cx,
                    )),
            )
            .into_any_element()
    }

    /// A collapsed section header for what only this app has.
    fn disclosure(
        &self,
        id: &'static str,
        title: &'static str,
        open: bool,
        toggle: fn(&mut Self),
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let palette = palette(cx);
        h_flex()
            .id(id)
            .role(Role::Button)
            .tab_index(0)
            .aria_label(title)
            .aria_expanded(open)
            .min_h(layout::CONTROL_HEIGHT)
            .px(space::SM)
            .gap(space::SM)
            .rounded(theme::RADIUS)
            .border_1()
            .border_color(gpui::transparent_black())
            .cursor_pointer()
            .hover(|style| style.bg(palette.accent))
            .focus_visible(|style| style.border_color(palette.ring))
            .child(
                Icon::from(if open {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                })
                .size(icon_size::SM)
                .text_color(palette.muted_foreground),
            )
            .child(div().font_weight(FontWeight::MEDIUM).child(title))
            .on_click(cx.listener(move |this, _, _, cx| {
                toggle(this);
                cx.notify();
            }))
            .into_any_element()
    }

    /// Version, build date, commit, channel, platform, update status and
    /// "Kopiér diagnoseoplysninger".
    fn build_details(&self, cx: &mut Context<Self>) -> AnyElement {
        let palette = palette(cx);
        let metadata = self
            .model
            .read(cx)
            .about_metadata
            .clone()
            .unwrap_or_else(crate::about::metadata);
        v_flex()
            .gap(space::MD)
            .px(space::SM)
            .pb(space::SM)
            .child(div().text_color(palette.muted_foreground).child(metadata))
            .child(
                v_flex()
                    .child(
                        div()
                            .font_weight(FontWeight::MEDIUM)
                            .child(strings::ABOUT_UPDATES),
                    )
                    .child(
                        div()
                            .text_color(palette.muted_foreground)
                            .child(crate::about::update_status().label()),
                    ),
            )
            .child(
                controls::button("about-copy", Variant::Outline)
                    .label(strings::ABOUT_COPY)
                    .loading(self.diagnosis_status == Some(strings::ABOUT_COPYING))
                    .on_click(cx.listener(|this, _, _, cx| this.copy_diagnostics(cx))),
            )
            .when_some(self.diagnosis_status, |body, status| {
                body.child(
                    div()
                        .id("about-copy-status")
                        .role(Role::Status)
                        .child(status),
                )
            })
            .into_any_element()
    }

    /// The bundled licence, third-party licences, notice and privacy
    /// policy, each in its own scroll box, and the contact link.
    fn legal(&self, cx: &mut Context<Self>) -> AnyElement {
        let palette = palette(cx);
        v_flex()
            .gap(space::LG)
            .px(space::SM)
            .pb(space::SM)
            .child(
                h_flex()
                    .gap(space::SM)
                    .flex_wrap()
                    .child(
                        controls::small_button("about-privacy", Variant::Outline)
                            .label(strings::ABOUT_PRIVACY)
                            .on_click(|_, _, cx| cx.open_url(PRIVACY_URL)),
                    )
                    .child(
                        controls::small_button("about-contact", Variant::Outline)
                            .label(strings::ABOUT_CONTACT)
                            .on_click(|_, _, cx| cx.open_url(CONTACT_URL)),
                    ),
            )
            .children(
                crate::about::DOCUMENTS
                    .iter()
                    .enumerate()
                    .map(|(index, (title, text))| {
                        v_flex()
                            .gap(space::XS)
                            .child(div().font_weight(FontWeight::MEDIUM).child(*title))
                            .child(
                                // The scrollbar sits over the box, so the
                                // long texts show that they scroll.
                                div()
                                    .relative()
                                    .h(size::DOCUMENT_HEIGHT)
                                    .flex_shrink_0()
                                    .child(
                                div()
                                    .id(("about-document", index))
                                    .size_full()
                                    .p(space::MD)
                                    .rounded(theme::RADIUS)
                                    .border_1()
                                    .border_color(palette.border)
                                    .bg(palette.card)
                                    .text_size(typography::XS)
                                    .text_color(palette.card_foreground)
                                    .overflow_y_scroll()
                                    .track_scroll(&self.document_scroll[index])
                                    .on_key_down(cx.listener(
                                        move |this, event: &gpui::KeyDownEvent, _, cx| {
                                            let scroll = &this.document_scroll[index];
                                            let mut offset = scroll.offset();
                                            let page = scroll.bounds().size.height;
                                            let line = size::DOCUMENT_SCROLL_LINE;
                                            offset.y = match event.keystroke.key.as_str() {
                                                "down" => offset.y - line,
                                                "up" => offset.y + line,
                                                "pagedown" => offset.y - page,
                                                "pageup" => offset.y + page,
                                                "home" => px(0.),
                                                "end" => -scroll.max_offset().y,
                                                _ => return,
                                            };
                                            offset.y =
                                                offset.y.clamp(-scroll.max_offset().y, px(0.));
                                            scroll.set_offset(offset);
                                            cx.stop_propagation();
                                            cx.notify();
                                        },
                                    ))
                                    .tab_index(0)
                                    .role(Role::Document)
                                    .aria_label(*title)
                                    .child(text.to_string()),
                                    )
                                    .child(Scrollbar::vertical(&self.document_scroll[index])),
                            )
                    }),
            )
            .into_any_element()
    }
}

impl Render for AboutView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = palette(cx);
        let max_height = window.viewport_size().height * size::MAX_HEIGHT_FRACTION;
        let extras = v_flex()
            .gap(space::XS)
            .pt(space::MD)
            .border_t_1()
            .border_color(palette.border)
            .child(self.disclosure(
                "about-build-toggle",
                strings::ABOUT_BUILD_DETAILS,
                self.build_open,
                |this| this.build_open = !this.build_open,
                cx,
            ))
            .when(self.build_open, |this| this.child(self.build_details(cx)))
            .child(self.disclosure(
                "about-legal-toggle",
                strings::ABOUT_LEGAL,
                self.legal_open,
                |this| this.legal_open = !this.legal_open,
                cx,
            ))
            .when(self.legal_open, |this| this.child(self.legal(cx)));
        v_flex()
            .id("about-dialog")
            .aria_label(SharedString::from(strings::ABOUT_APP))
            .w_full()
            .max_h(max_height)
            .overflow_y_scrollbar()
            .p(space::LG)
            .gap(space::XL)
            .font_family(typography::SANS)
            .text_size(typography::SM)
            .line_height(relative(typography::LINE_HEIGHT))
            .text_color(palette.foreground)
            .child(self.hero(cx))
            .child(self.features(cx))
            .child(self.footer(cx))
            .child(extras)
    }
}

/// An underlined footer link, 12 px muted (`underline hover:text-foreground`).
fn footer_link(
    id: &'static str,
    label: &'static str,
    url: &'static str,
    cx: &App,
) -> impl IntoElement {
    let palette = palette(cx);
    div()
        .id(id)
        .role(Role::Link)
        .tab_index(0)
        .aria_label(label)
        .underline()
        .rounded(theme::RADIUS_SM)
        .cursor_pointer()
        .hover(|style| style.text_color(palette.foreground))
        .focus_visible(|style| style.text_color(palette.foreground).bg(palette.accent))
        .on_click(move |_, _, cx| cx.open_url(url))
        .child(label)
}

/// Open "Om Tingskriver" over the current screen; does nothing while
/// another dialog is open.
pub(crate) fn open_about(view: Entity<AboutView>, window: &mut Window, cx: &mut App) {
    if window.has_active_dialog(cx) {
        return;
    }
    let focus = view.read(cx).check_focus.clone();
    let first = Cell::new(true);
    window.open_dialog(cx, move |dialog, window, cx| {
        if first.replace(false) {
            focus.focus(window, cx);
        }
        dialog
            .w(size::WIDTH)
            .p(space::XL)
            .centered()
            .child(view.clone())
    });
}

/// What "Søg efter opdateringer" shows for a check result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CheckOutcome {
    /// Open the update dialog for this version (Tauri: `info.available`).
    Update(String),
    /// "Du har den nyeste version" (success toast).
    Latest,
    /// The check failed (error toast).
    Failed,
    /// Anything else, as an info toast.
    Message(String),
}

pub(crate) fn check_outcome(status: &UpdateStatus) -> CheckOutcome {
    match status {
        UpdateStatus::ReadyAtRestart { version } | UpdateStatus::Downloading { version } => {
            CheckOutcome::Update(version.clone())
        }
        UpdateStatus::UpToDate => CheckOutcome::Latest,
        UpdateStatus::Failed { .. } => CheckOutcome::Failed,
        UpdateStatus::Inactive | UpdateStatus::Unavailable => {
            CheckOutcome::Message(strings::ABOUT_UPDATES_UNAVAILABLE.to_string())
        }
        other => CheckOutcome::Message(other.label()),
    }
}

/// The update dialog (`UpdateDialog.tsx`) for a found, downloading or
/// failed update. The app downloads updates itself and installs them at
/// the next start, so there is no "Hent og installér" step: the dialog
/// says where the update is and closes with "Luk".
pub(crate) fn open_update(
    status: UpdateStatus,
    current: &'static str,
    window: &mut Window,
    cx: &mut App,
) {
    let palette = palette(cx);
    let (icon, icon_color, title, detail, new_version) = match &status {
        UpdateStatus::ReadyAtRestart { version } => (
            IconName::Download,
            palette.primary,
            strings::UPDATE_READY,
            strings::update_ready_detail(version),
            Some(version.clone()),
        ),
        UpdateStatus::Downloading { version } => (
            IconName::LoaderCircle,
            palette.primary,
            strings::UPDATE_DOWNLOADING,
            strings::UPDATE_DOWNLOADING_DETAIL.to_string(),
            Some(version.clone()),
        ),
        UpdateStatus::Failed { .. } => (
            IconName::CircleAlert,
            palette.destructive,
            strings::UPDATE_FAILED,
            strings::UPDATE_FAILED_DETAIL.to_string(),
            None,
        ),
        _ => return,
    };
    let error = matches!(status, UpdateStatus::Failed { .. }).then(|| status.label());
    let ready = matches!(status, UpdateStatus::ReadyAtRestart { .. });
    window.open_dialog(cx, move |dialog, _, _| {
        let row = |label: &'static str, value: String, accent: bool| {
            h_flex()
                .justify_between()
                .child(div().text_color(palette.muted_foreground).child(label))
                .child(
                    div()
                        .font_weight(FontWeight::MEDIUM)
                        .when(accent, |this| this.text_color(palette.primary))
                        .child(value),
                )
        };
        dialog
            .w(size::UPDATE_WIDTH)
            .p(space::XL)
            .centered()
            .title(
                h_flex()
                    .gap(space::SM)
                    .child(Icon::from(icon).size(icon_size::MD).text_color(icon_color))
                    .child(crate::ui::section_title(title)),
            )
            .child(
                v_flex()
                    .font_family(typography::SANS)
                    .text_size(typography::SM)
                    .line_height(relative(typography::LINE_HEIGHT))
                    .text_color(palette.foreground)
                    .gap(space::LG)
                    .pb(space::LG)
                    .child(
                        div()
                            .text_color(palette.muted_foreground)
                            .child(detail.clone()),
                    )
                    .when_some(new_version.clone(), |body, version| {
                        body.child(
                            v_flex()
                                .gap(space::SM)
                                .child(row(strings::UPDATE_CURRENT, current.to_string(), false))
                                .child(row(strings::UPDATE_NEW, version, true)),
                        )
                    })
                    .when(ready, |body| {
                        body.child(
                            div()
                                .text_center()
                                .text_color(palette.muted_foreground)
                                .child(strings::UPDATE_AT_RESTART),
                        )
                    })
                    .when_some(error.clone(), |body, error| {
                        body.child(
                            div()
                                .id("update-error")
                                .role(Role::Alert)
                                .p(space::MD)
                                .rounded(theme::RADIUS_LG)
                                .border_1()
                                .border_color(palette.destructive.opacity(size::ERROR_BORDER))
                                .bg(theme::over(
                                    palette.destructive,
                                    palette.background,
                                    size::ERROR_TINT,
                                ))
                                .text_color(palette.destructive)
                                .child(error),
                        )
                    }),
            )
            .footer(
                DialogFooter::new().child(
                    controls::button("update-close", Variant::Outline)
                        .label(strings::UPDATE_CLOSE)
                        .on_click(|_, window, cx| window.close_dialog(cx)),
                ),
            )
    });
}

/// Show "Opdatering klar" with "Se detaljer" once the background updater
/// has a version ready (`UpdateNotification.tsx`). Polls the updater's
/// status; stops after the first notice, since the updater stops then too.
pub fn watch_updates(cx: &mut App) {
    if !crate::update::updates_active() {
        return;
    }
    cx.spawn(async move |cx| loop {
        cx.background_executor().timer(WATCH_INTERVAL).await;
        if let UpdateStatus::ReadyAtRestart { version } = crate::update::status() {
            cx.update(|cx| crate::ui::show_update_ready(version, cx));
            break;
        }
    })
    .detach();
}

/// The "Opdatering klar" notification for `version`.
pub(crate) fn ready_notification(version: String, current: &'static str) -> Notification {
    let details = version.clone();
    Notification::info(strings::update_ready_detail(&version))
        .title(strings::UPDATE_READY)
        .icon(Icon::from(IconName::Download))
        .action(move |_, _, _| {
            let version = details.clone();
            controls::small_button("update-details", Variant::Link)
                .label(strings::UPDATE_DETAILS)
                .on_click(move |_, window, cx| {
                    open_update(
                        UpdateStatus::ReadyAtRestart {
                            version: version.clone(),
                        },
                        current,
                        window,
                        cx,
                    )
                })
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_found_update_opens_the_dialog_and_the_rest_are_toasts() {
        let ready = UpdateStatus::ReadyAtRestart {
            version: "0.5.2".into(),
        };
        assert_eq!(check_outcome(&ready), CheckOutcome::Update("0.5.2".into()));
        assert_eq!(
            check_outcome(&UpdateStatus::Downloading {
                version: "0.5.2".into()
            }),
            CheckOutcome::Update("0.5.2".into())
        );
        assert_eq!(check_outcome(&UpdateStatus::UpToDate), CheckOutcome::Latest);
        assert_eq!(
            check_outcome(&UpdateStatus::Failed {
                detail: "offline".into()
            }),
            CheckOutcome::Failed
        );
        // A copy that does not update itself says so instead of "nyeste".
        for status in [UpdateStatus::Inactive, UpdateStatus::Unavailable] {
            assert_eq!(
                check_outcome(&status),
                CheckOutcome::Message(strings::ABOUT_UPDATES_UNAVAILABLE.into())
            );
        }
        assert_eq!(
            check_outcome(&UpdateStatus::NoReleaseFound),
            CheckOutcome::Message(UpdateStatus::NoReleaseFound.label())
        );
    }

    #[test]
    fn the_notice_link_points_into_the_open_source_note() {
        let start = strings::ABOUT_OPEN_SOURCE
            .find(strings::ABOUT_NOTICE_LINK)
            .expect("the note names NOTICE.md");
        assert!(start > 0);
    }
}
