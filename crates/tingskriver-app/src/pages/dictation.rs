//! The Diktering page: a placeholder text and the keyboard test field
//! (`probe`) for the manual Windows input tests.

use gpui::{div, prelude::*, AnyElement, Context};
use gpui_component::{input::Textarea, v_flex};
use tingskriver_core::strings;

use crate::controls;
use crate::probe;
use crate::theme::{layout, space, typography};
use crate::ui::{muted, page, palette, paragraph, section_title, Shell};

impl Shell {
    pub(crate) fn dictation_page(&self, cx: &mut Context<Self>) -> AnyElement {
        let palette = palette(cx);
        let typed = format!(
            "{}\n{}",
            self.probe_line.read(cx).value(),
            self.probe_text.read(cx).value()
        );
        let described = probe::describe(&typed);
        let warning = probe::composition_warning(&typed);
        let body = v_flex()
            .gap(space::LG)
            .child(paragraph(strings::DICTATION_LATER, cx))
            .child(
                v_flex()
                    .gap(space::SM)
                    .child(section_title(strings::PROBE))
                    .child(muted(strings::PROBE_HELP, cx))
                    .child(controls::input(&self.probe_line))
                    .child(Textarea::new(&self.probe_text))
                    .when_some(warning, |this, warning| {
                        this.child(
                            div()
                                .text_size(typography::SM)
                                .text_color(palette.destructive)
                                .child(warning),
                        )
                    })
                    .child(
                        div()
                            .text_size(typography::SM)
                            .text_color(palette.muted_foreground)
                            .child(if described.is_empty() {
                                strings::SPECIAL_EMPTY.to_string()
                            } else {
                                strings::special_characters(&described.join(", "))
                            }),
                    ),
            );
        page(
            strings::DICTATION,
            None,
            false,
            layout::DICTATION_MAX,
            body.into_any_element(),
            cx,
        )
    }
}
