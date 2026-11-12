//! The Om page: build metadata, update status, copying diagnostics and the
//! bundled legal documents (`about`).

use gpui::{div, prelude::*, AnyElement, Context};
use gpui_component::v_flex;
use tingskriver_core::strings;

use crate::controls::{self, Variant};
use crate::theme::{layout, space};
use crate::ui::{page, paragraph, section_title, Shell};

impl Shell {
    pub(crate) fn about_page(&self, cx: &mut Context<Self>) -> AnyElement {
        let body =
            v_flex()
                .gap(space::LG)
                .child(paragraph(
                    &self
                        .model
                        .read(cx)
                        .about_metadata
                        .clone()
                        .unwrap_or_else(crate::about::metadata),
                    cx,
                ))
                .child(section_title(strings::ABOUT_UPDATES))
                .child(paragraph(&crate::about::update_status().label(), cx))
                .child(
                    controls::button("about-copy", Variant::Outline)
                        .track_focus(&self.about_copy_focus)
                        .label(strings::ABOUT_COPY)
                        .loading(self.diagnosis_status == Some(strings::ABOUT_COPYING))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.diagnosis_status = Some(strings::ABOUT_COPYING);
                            cx.notify();
                            let task = this
                                .model
                                .read(cx)
                                .runtime
                                .spawn(crate::about::diagnostics());
                            cx.spawn(async move |this, cx| {
                                let status = match task.await {
                                    Ok(report) => {
                                        cx.update(|cx| {
                                            cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                                                report,
                                            ))
                                        });
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
                        })),
                )
                .when_some(self.diagnosis_status, |body, status| {
                    body.child(
                        div()
                            .id("about-copy-status")
                            .role(gpui::Role::Status)
                            .child(status),
                    )
                })
                .child(
                    controls::button("about-privacy", Variant::Outline)
                        .label(strings::ABOUT_PRIVACY)
                        .on_click(|_, _, cx| {
                            cx.open_url(
                                "https://github.com/mahope/tingskriver/blob/main/PRIVACY_POLICY.md",
                            );
                        }),
                )
                .child(
                    controls::button("about-contact", Variant::Outline)
                        .label(strings::ABOUT_CONTACT)
                        .on_click(|_, _, cx| {
                            cx.open_url("https://mahoje.dk");
                        }),
                )
                .children(crate::about::DOCUMENTS.iter().enumerate().map(
                    |(index, (title, text))| {
                        v_flex().gap(space::SM).child(section_title(title)).child(
                            div()
                                .id(("about-document", index))
                                .h(gpui::px(240.))
                                .flex_shrink_0()
                                .overflow_y_scroll()
                                .track_scroll(&self.about_document_scroll[index])
                                .on_key_down(cx.listener(
                                    move |this, event: &gpui::KeyDownEvent, _, cx| {
                                        let scroll = &this.about_document_scroll[index];
                                        let mut offset = scroll.offset();
                                        let page = scroll.bounds().size.height;
                                        offset.y = match event.keystroke.key.as_str() {
                                            "down" => offset.y - gpui::px(32.),
                                            "up" => offset.y + gpui::px(32.),
                                            "pagedown" => offset.y - page,
                                            "pageup" => offset.y + page,
                                            "home" => gpui::px(0.),
                                            "end" => -scroll.max_offset().y,
                                            _ => return,
                                        };
                                        offset.y =
                                            offset.y.clamp(-scroll.max_offset().y, gpui::px(0.));
                                        scroll.set_offset(offset);
                                        cx.stop_propagation();
                                        cx.notify();
                                    },
                                ))
                                .tab_index(0)
                                .role(gpui::Role::Document)
                                .aria_label(*title)
                                .child(paragraph(text, cx)),
                        )
                    },
                ));
        page(
            strings::ABOUT,
            None,
            false,
            layout::SETTINGS_SECTION_MAX,
            body.into_any_element(),
            cx,
        )
    }
}
