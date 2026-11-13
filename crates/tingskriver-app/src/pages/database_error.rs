//! The full-window page shown when the database cannot be opened.

use gpui::{div, prelude::*, AnyElement, Context, FontWeight, Role};
use gpui_component::{h_flex, v_flex, ActiveTheme, Icon};
use gpui_kit_assets::IconName;
use tingskriver_core::strings;

use crate::controls::{self, Variant};
use crate::startup::{self, DbFailure};
use crate::theme::{icon_size, layout, space, typography};
use crate::ui::{heading, load_database, palette, paragraph, Level, Shell};

impl Shell {
    pub(crate) fn database_error_page(
        &self,
        failure: &DbFailure,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let palette = palette(cx);
        let model = self.model.read(cx);
        let log_dir = model.log_dir();
        let log_path = model.log_path.clone();
        let backup_dir = failure.backup_path.clone();
        // `DatabaseGate.tsx`: a block of facts, each a label above a mono
        // value. No log file means no row, only the sentence that says why.
        let fact = |label: &'static str, value: String| {
            v_flex()
                .gap(space::XXS)
                .child(
                    div()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette.foreground)
                        .child(label),
                )
                .child(
                    div()
                        .font_family(cx.theme().mono_font_family.clone())
                        .text_size(typography::XS)
                        .child(value),
                )
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
                    .id("database-error-body")
                    .role(Role::Alert)
                    .w_full()
                    .max_w(layout::NOTICE_MAX)
                    .gap(space::LG)
                    .child(
                        Icon::from(IconName::TriangleAlert)
                            .size(icon_size::ERROR)
                            .text_color(palette.destructive),
                    )
                    .child(heading(&failure.summary, Level::Section))
                    .child(paragraph(&failure.advice, cx))
                    .child(
                        v_flex()
                            .gap(space::SM)
                            .text_size(typography::SM)
                            .text_color(palette.muted_foreground)
                            .child(fact(
                                strings::DATABASE_FILE_LABEL,
                                failure.database_path.display().to_string(),
                            ))
                            .when_some(backup_dir.as_ref(), |this, dir| {
                                this.child(fact(
                                    strings::DATABASE_BACKUP_LABEL,
                                    dir.display().to_string(),
                                ))
                            })
                            .map(|this| match &log_path {
                                Some(path) => this.child(fact(
                                    strings::DATABASE_LOG_LABEL,
                                    path.display().to_string(),
                                )),
                                None => this.child(div().child(strings::LOG_FAILED)),
                            })
                            .child(fact(
                                strings::DATABASE_TECHNICAL_LABEL,
                                failure.detail.clone(),
                            )),
                    )
                    .child(
                        h_flex()
                            .flex_wrap()
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
                            .when_some(backup_dir, |this, dir| {
                                this.child(
                                    controls::button("db-error-open-backup", Variant::Outline)
                                        .label(strings::DATABASE_OPEN_BACKUP)
                                        .icon(IconName::FolderOpen)
                                        .on_click(move |_, _, _| startup::open_folder(&dir)),
                                )
                            })
                            // Not in Tauri: the GPUI app can try again
                            // without a restart.
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
