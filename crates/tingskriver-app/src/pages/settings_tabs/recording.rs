//! Indstillinger > Optagelser, as in the Tauri app
//! (`components/RecordingSettings.tsx:145-218`, the device part of
//! `DeviceSelection.tsx:243-365`).
//!
//! The folder is core's recordings folder and "Åbn mappen" works. This app
//! does not record yet (`recording::HomeState::can_record`), so the start
//! reminder and the default devices would have no effect here: they are
//! shown as in Tauri but disabled, with the reason under the switch.

use gpui::{div, prelude::*, AnyElement, Context};
use gpui_component::{h_flex, v_flex, Disableable, Icon};
use gpui_kit_assets::IconName;
use tingskriver_core::strings::{self, settings_tabs as text};

use super::{disabled_select, subheading_regular};
use crate::controls::{self, Variant};
use crate::pages::settings::{field_label, open_recordings_folder, switch_row, tab_column};
use crate::theme::{icon_size, space, typography};
use crate::ui::{heading, muted, palette, Level, Shell};

impl Shell {
    pub(crate) fn recording_settings_tab(&self, cx: &mut Context<Self>) -> AnyElement {
        let model = self.model.read(cx);
        let folder = model.settings.recordings_dir.clone();
        let runtime = model.runtime.clone();
        let palette = palette(cx);

        let title = v_flex()
            .child(heading(text::RECORDING_TITLE, Level::Section))
            .child(div().mt(space::XS).child(muted(text::RECORDING_HELP, cx)));

        let open = folder.clone();
        let save_folder = v_flex()
            .gap(space::LG)
            .child(
                v_flex()
                    .child(
                        div()
                            .mb(space::XS)
                            .child(field_label(text::RECORDING_SAVED_IN)),
                    )
                    .child(
                        div()
                            .id("recording-folder")
                            .mb(space::MD)
                            .text_size(typography::SM)
                            .text_color(palette.muted_foreground)
                            .child(
                                folder
                                    .as_ref()
                                    .map(|dir| dir.display().to_string())
                                    .unwrap_or_else(|| text::RECORDING_DEFAULT_FOLDER.to_string()),
                            ),
                    )
                    .child(
                        h_flex().child(
                            controls::button("recording-open-folder", Variant::Outline)
                                .label(strings::STORAGE_OPEN)
                                .icon(IconName::FolderOpen)
                                .disabled(open.is_none())
                                .on_click(move |_, _, _| {
                                    if let Some(dir) = open.clone() {
                                        open_recordings_folder(&runtime, dir);
                                    }
                                }),
                        ),
                    ),
            )
            .child(muted(text::RECORDING_ALWAYS_SAVED, cx));

        let reminder = v_flex()
            .gap(space::SM)
            .child(switch_row(
                "recording-start-notice",
                text::RECORDING_START_NOTICE,
                Some(text::RECORDING_START_NOTICE_HINT),
                // Tauri's default is on (RecordingSettings.tsx:32); the
                // saved value lives in the current app's preferences.json.
                controls::switch("recording-start-notice-switch")
                    .checked(true)
                    .disabled(true)
                    .accessibility_label(text::RECORDING_START_NOTICE),
                cx,
            ))
            .child(
                div()
                    .id("recording-later")
                    .role(gpui::Role::Note)
                    .child(muted(text::RECORDING_NOTICE_LATER, cx)),
            );

        let device = |id: &'static str, icon: IconName, label: &str, value: &str| {
            v_flex()
                .gap(space::SM)
                .child(
                    h_flex()
                        .gap(space::SM)
                        .child(
                            Icon::new(icon)
                                .size(icon_size::SM)
                                .text_color(palette.muted_foreground),
                        )
                        .child(div().text_size(typography::SM).child(field_label(label))),
                )
                .child(disabled_select(
                    id,
                    label,
                    value,
                    text::RECORDING_SETTINGS_LATER,
                    cx,
                ))
        };
        let devices = v_flex()
            .border_t_1()
            .border_color(palette.border)
            .pt(space::XXL)
            .child(subheading_regular(text::RECORDING_DEVICES_TITLE))
            .child(
                div()
                    .mt(space::XS)
                    .mb(space::LG)
                    .child(muted(text::RECORDING_DEVICES_HELP, cx)),
            )
            .child(
                v_flex()
                    .gap(space::LG)
                    .child(
                        div()
                            .text_size(typography::SM)
                            .child(field_label(text::RECORDING_DEVICES)),
                    )
                    .child(
                        v_flex()
                            .gap(space::MD)
                            .child(device(
                                "recording-microphone",
                                IconName::Mic,
                                text::RECORDING_MICROPHONE,
                                text::RECORDING_MICROPHONE_DEFAULT,
                            ))
                            .child(device(
                                "recording-system",
                                IconName::Speaker,
                                text::RECORDING_SYSTEM,
                                text::RECORDING_SYSTEM_DEFAULT,
                            )),
                    ),
            );

        // Tauri: one `space-y-8` column, only the devices behind a border.
        tab_column(vec![v_flex()
            .id("settings-recording")
            .role(gpui::Role::Group)
            .gap(space::XXL)
            .child(title)
            .child(save_folder)
            .child(reminder)
            .child(devices)
            .into_any_element()])
    }
}
