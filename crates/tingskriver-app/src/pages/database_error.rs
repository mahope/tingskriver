//! The full-window page shown when the database cannot be opened.

use gpui::{div, prelude::*, AnyElement, Context};
use gpui_component::{h_flex, v_flex, ActiveTheme, Icon};
use gpui_kit_assets::IconName;
use tingskriver_core::strings;

use crate::controls::{self, Variant};
use crate::startup::{self, DbFailure};
use crate::theme::{self, icon_size, layout, space, typography};
use crate::ui::{heading, load_database, muted, palette, paragraph, Level, Shell};

impl Shell {
    pub(crate) fn database_error_page(
        &self,
        failure: &DbFailure,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let palette = palette(cx);
        let model = self.model.read(cx);
        let log_dir = model.log_dir();
        let log_line = match &model.log_path {
            Some(path) => strings::log_file(&path.display().to_string()),
            None => strings::LOG_FAILED.to_string(),
        };
        v_flex()
            .id("database-error")
            .size_full()
            .items_center()
            .justify_center()
            .p(space::XXL)
            .overflow_y_scroll()
            .child(
                v_flex()
                    .max_w(layout::NOTICE_MAX)
                    .gap(space::MD)
                    .child(
                        Icon::from(IconName::TriangleAlert)
                            .size(icon_size::ERROR)
                            .text_color(palette.destructive),
                    )
                    .child(heading(strings::DATABASE_FAILED, Level::Section))
                    .child(paragraph(&failure.summary, cx))
                    .child(paragraph(&failure.advice, cx))
                    .child(muted(
                        &strings::database(&failure.database_path.display().to_string()),
                        cx,
                    ))
                    .child(muted(&log_line, cx))
                    .child(
                        div()
                            .p(space::SM)
                            .rounded(theme::RADIUS)
                            .bg(palette.muted)
                            .font_family(cx.theme().mono_font_family.clone())
                            .text_size(typography::XS)
                            .text_color(palette.muted_foreground)
                            .child(failure.detail.clone()),
                    )
                    .child(
                        h_flex()
                            .pt(space::SM)
                            .gap(space::SM)
                            .child(
                                controls::button("db-error-open-log", Variant::Primary)
                                    .label(strings::OPEN_LOG)
                                    .icon(IconName::FolderOpen)
                                    .on_click(move |_, _, _| {
                                        if let Some(dir) = &log_dir {
                                            startup::open_folder(dir);
                                        }
                                    }),
                            )
                            .child(
                                controls::button("db-error-retry", Variant::Outline)
                                    .label(strings::RETRY)
                                    .icon(IconName::RotateCcw)
                                    .on_click(|_, _, cx| load_database(cx)),
                            ),
                    ),
            )
            .into_any_element()
    }
}
