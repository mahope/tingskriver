//! Indstillinger > Optagelser, as in the Tauri app
//! (`components/RecordingSettings.tsx`, the device part of
//! `DeviceSelection.tsx`, the reminder of `lib/recordingNotification.tsx`).
//!
//! Everything reads and writes what the recording itself reads, through the
//! same core functions as the Tauri commands:
//!
//! - the default microphone and system audio are
//!   `audio::recording_preferences` (`recording_preferences.json`), which
//!   `start_recording` loads when a recording begins;
//! - the start reminder is `show_recording_notification` in
//!   `preferences.json`, on unless switched off, and is shown by
//!   [`recording_changed`] when a recording starts;
//! - the folder is the saved one, and "Åbn mappen" opens it.
//!
//! The device choice is locked while a recording runs, as in Tauri. A saved
//! device that is no longer connected is reset to the default once the list
//! has been read, also as in Tauri.

use std::cell::Cell;
use std::rc::Rc;

use gpui::{
    div, prelude::*, AnyElement, AnyWindowHandle, App, AsyncApp, Context, Entity, FontWeight,
    SharedString, Subscription, Window,
};
use gpui_component::{
    checkbox::Checkbox,
    h_flex,
    notification::Notification,
    select::{Select, SelectEvent, SelectState},
    v_flex, Disableable, Icon, IndexPath, WindowExt,
};
use gpui_kit_assets::IconName;
use tingskriver_core::audio::recording_preferences::{self as stored, RecordingPreferences};
use tingskriver_core::audio::{AudioDevice, DeviceType};
use tingskriver_core::host::HostHandle;
use tingskriver_core::strings::{self, settings_tabs as text};

use super::{subheading_regular, tinted_box};
use crate::controls::{self, Variant};
use crate::pages::settings::{alert, field_label, open_recordings_folder, switch_row, tab_column};
use crate::theme::{icon_size, layout, space, typography};
use crate::ui::{heading, muted, palette, Level, Shell};

type Choices = SelectState<Vec<SharedString>>;

/// Tauri's value for a device in the saved preferences: the name with
/// " (input)" or " (output)" after it (`DeviceSelection.tsx:293`).
fn device_value(name: &str, input: bool) -> String {
    let kind = if input { " (input)" } else { " (output)" };
    [name, kind].concat()
}

/// The names of the connected devices, by direction.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Devices {
    inputs: Vec<String>,
    outputs: Vec<String>,
}

impl Devices {
    fn from_core(list: &[AudioDevice]) -> Self {
        let named = |wanted: DeviceType| {
            list.iter()
                .filter(|device| device.device_type == wanted)
                .map(|device| device.name.clone())
                .collect()
        };
        Devices {
            inputs: named(DeviceType::Input),
            outputs: named(DeviceType::Output),
        }
    }
}

/// Reset a saved device that is not connected to the default
/// (`DeviceSelection.tsx:98-128`). A direction with no devices at all is left
/// alone: an empty list is more likely a failed read than a missing device.
/// Returns whether anything changed.
fn prune(preferences: &mut RecordingPreferences, devices: &Devices) -> bool {
    let mut changed = false;
    let mut check = |saved: &mut Option<String>, names: &[String], input: bool| {
        let missing = saved.as_deref().is_some_and(|saved| {
            !names.is_empty() && !names.iter().any(|name| device_value(name, input) == saved)
        });
        if missing {
            *saved = None;
            changed = true;
        }
    };
    check(&mut preferences.preferred_mic_device, &devices.inputs, true);
    check(
        &mut preferences.preferred_system_device,
        &devices.outputs,
        false,
    );
    changed
}

/// What one select offers: the default first, then every connected device.
/// A saved device that is not in the list (the list could not be read) is
/// kept as a choice, so the select shows what is saved.
fn select_values(names: &[String], saved: Option<&str>, input: bool) -> Vec<Option<String>> {
    let mut values: Vec<Option<String>> = std::iter::once(None)
        .chain(names.iter().map(|name| Some(device_value(name, input))))
        .collect();
    if let Some(saved) = saved {
        if !values.iter().any(|v| v.as_deref() == Some(saved)) {
            values.insert(1, Some(saved.to_string()));
        }
    }
    values
}

/// The text of a choice: the device name, as in Tauri's `SelectItem`.
fn select_labels(values: &[Option<String>], default: &str) -> Vec<SharedString> {
    values
        .iter()
        .map(|value| match value {
            None => SharedString::from(default.to_string()),
            Some(value) => {
                let name = value
                    .strip_suffix(" (input)")
                    .or_else(|| value.strip_suffix(" (output)"))
                    .unwrap_or(value);
                SharedString::from(name.to_string())
            }
        })
        .collect()
}

fn index_of(values: &[Option<String>], saved: Option<&str>) -> usize {
    values
        .iter()
        .position(|value| value.as_deref() == saved)
        .unwrap_or(0)
}

/// What the tab shows of core: the saved preferences and the reminder.
#[derive(Debug, Clone)]
struct Saved {
    preferences: RecordingPreferences,
    notice: bool,
}

/// What a read of core gave, each part on its own as in Tauri: the page
/// works without the device list, and falls back when the preferences are
/// unreadable.
struct Fetched {
    preferences: Result<RecordingPreferences, String>,
    notice: Result<bool, String>,
    devices: Result<Devices, String>,
}

async fn fetch(host: HostHandle) -> Fetched {
    let preferences = stored::load_recording_preferences(&host)
        .await
        .map_err(|error| error.to_string());
    let notice = stored::recording_notice_enabled(&host);
    let devices = read_devices().await;
    Fetched {
        preferences,
        notice,
        devices,
    }
}

async fn read_devices() -> Result<Devices, String> {
    tingskriver_core::audio::list_audio_devices()
        .await
        .map(|list| Devices::from_core(&list))
        .map_err(|error| error.to_string())
}

/// Per-window state of the Optagelser tab (part of `SettingsView`).
pub(crate) struct RecordingTab {
    window: AnyWindowHandle,
    saved: Option<Saved>,
    preferences_unreadable: bool,
    devices: Option<Devices>,
    devices_failed: bool,
    loading: bool,
    refreshing: bool,
    saving: bool,
    notice_pending: bool,
    /// Read again the next time the tab is drawn (Tauri reloads on mount).
    stale: bool,
    /// Fixture data: never read from or written to core.
    frozen: bool,
    microphone: Entity<Choices>,
    system: Entity<Choices>,
    microphone_values: Vec<Option<String>>,
    system_values: Vec<Option<String>>,
    _subscriptions: Vec<Subscription>,
}

impl RecordingTab {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Shell>) -> Self {
        let mut select = |default: &'static str| {
            cx.new(|cx| SelectState::new(vec![SharedString::from(default)], None, window, cx))
        };
        let microphone = select(text::RECORDING_MICROPHONE_DEFAULT);
        let system = select(text::RECORDING_SYSTEM_DEFAULT);
        let subscriptions = [&microphone, &system]
            .into_iter()
            .map(|state| {
                cx.subscribe_in(
                    state,
                    window,
                    |shell: &mut Shell,
                     state,
                     event: &SelectEvent<Vec<SharedString>>,
                     window,
                     cx| {
                        let SelectEvent::Confirm(Some(_)) = event else {
                            return;
                        };
                        if let Some(index) = state.read(cx).selected_index(cx) {
                            shell.device_chosen(state.clone(), index.row, window, cx);
                        }
                    },
                )
            })
            .collect();
        RecordingTab {
            window: window.window_handle(),
            saved: None,
            preferences_unreadable: false,
            devices: None,
            devices_failed: false,
            loading: false,
            refreshing: false,
            saving: false,
            notice_pending: false,
            stale: true,
            frozen: false,
            microphone,
            system,
            microphone_values: vec![None],
            system_values: vec![None],
            _subscriptions: subscriptions,
        }
    }

    /// The tab was opened: read again, as Tauri reloads on mount.
    pub(crate) fn shown(&mut self) {
        self.stale = true;
    }

    /// Synthetic state for the screenshot fixtures: a saved microphone, two
    /// microphones and two outputs, the reminder on.
    #[cfg(feature = "screenshot-tests")]
    pub(crate) fn freeze_fixture(shell: &mut Shell, window: &mut Window, cx: &mut Context<Shell>) {
        let tab = &mut shell.settings.recording;
        tab.frozen = true;
        tab.stale = false;
        let mut preferences = RecordingPreferences::default();
        preferences.save_folder = std::path::PathBuf::from("fixtures/optagelser");
        preferences.preferred_mic_device = Some(device_value("Fixture-headset", true));
        tab.devices = Some(Devices {
            inputs: vec!["Fixture-mikrofon".into(), "Fixture-headset".into()],
            outputs: vec!["Fixture-højttalere".into(), "Fixture-headset".into()],
        });
        tab.saved = Some(Saved {
            preferences,
            notice: true,
        });
        tab.show_choices(window, cx);
    }

    /// Put the saved choices into the two selects.
    fn show_choices(&mut self, window: &mut Window, cx: &mut Context<Shell>) {
        let Some(saved) = &self.saved else {
            return;
        };
        let devices = self.devices.clone().unwrap_or_default();
        let preferences = &saved.preferences;
        self.microphone_values = select_values(
            &devices.inputs,
            preferences.preferred_mic_device.as_deref(),
            true,
        );
        self.system_values = select_values(
            &devices.outputs,
            preferences.preferred_system_device.as_deref(),
            false,
        );
        let mic_labels = select_labels(&self.microphone_values, text::RECORDING_MICROPHONE_DEFAULT);
        let system_labels = select_labels(&self.system_values, text::RECORDING_SYSTEM_DEFAULT);
        let mic_index = index_of(
            &self.microphone_values,
            preferences.preferred_mic_device.as_deref(),
        );
        let system_index = index_of(
            &self.system_values,
            preferences.preferred_system_device.as_deref(),
        );
        self.microphone.update(cx, |state, cx| {
            state.set_items(mic_labels, window, cx);
            state.set_selected_index(Some(IndexPath::new(mic_index)), window, cx);
        });
        self.system.update(cx, |state, cx| {
            state.set_items(system_labels, window, cx);
            state.set_selected_index(Some(IndexPath::new(system_index)), window, cx);
        });
    }
}

impl Shell {
    fn recording_host(&self, cx: &App) -> Option<HostHandle> {
        self.recorder(cx).map(|recorder| recorder.host())
    }

    /// Read the tab's data if it is stale (on first view and each visit).
    fn load_recording_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let tab = &self.settings.recording;
        if tab.frozen || tab.loading || !tab.stale {
            return;
        }
        let Some(host) = self.recording_host(cx) else {
            return;
        };
        let tab = &mut self.settings.recording;
        tab.stale = false;
        tab.loading = true;
        let task = self.model.read(cx).runtime.spawn(fetch(host));
        cx.spawn_in(window, async move |this, cx| {
            let fetched = task.await;
            let _ = this.update_in(cx, |shell, window, cx| {
                shell.apply_recording_fetch(fetched.ok(), window, cx);
            });
        })
        .detach();
    }

    fn apply_recording_fetch(
        &mut self,
        fetched: Option<Fetched>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let tab = &mut self.settings.recording;
        tab.loading = false;
        let Some(fetched) = fetched else {
            log::warn!("The recording settings could not be read");
            tab.preferences_unreadable = true;
            cx.notify();
            return;
        };
        // Unreadable preferences fall back to the defaults, as in Tauri.
        tab.preferences_unreadable = fetched.preferences.is_err();
        if let Err(error) = &fetched.preferences {
            log::warn!("Recording preferences could not be read: {error}");
        }
        let mut preferences = fetched.preferences.unwrap_or_default();
        let notice = fetched.notice.unwrap_or_else(|error| {
            log::warn!("The start reminder could not be read: {error}");
            true
        });
        match fetched.devices {
            Ok(devices) => {
                tab.devices_failed = false;
                tab.devices = Some(devices);
            }
            Err(error) => {
                log::warn!("The audio devices could not be listed: {error}");
                tab.devices_failed = true;
            }
        }
        let recording = self.home.is_recording();
        let tab = &mut self.settings.recording;
        // What is saved now; a device that is gone is only forgotten once
        // the reset has been saved.
        let before = preferences.clone();
        let reset = tab
            .devices
            .as_ref()
            .is_some_and(|devices| !recording && prune(&mut preferences, devices));
        tab.saved = Some(Saved {
            preferences: before,
            notice,
        });
        tab.show_choices(window, cx);
        if reset {
            self.save_recording_preferences(preferences, window, cx);
        }
        cx.notify();
    }

    /// "Hent listen igen".
    fn refresh_devices(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let tab = &mut self.settings.recording;
        if tab.frozen || tab.refreshing {
            return;
        }
        tab.refreshing = true;
        let task = self.model.read(cx).runtime.spawn(read_devices());
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await.unwrap_or_else(|error| Err(error.to_string()));
            let _ = this.update_in(cx, |shell, window, cx| {
                let recording = shell.home.is_recording();
                let tab = &mut shell.settings.recording;
                tab.refreshing = false;
                match result {
                    Ok(devices) => {
                        tab.devices_failed = false;
                        tab.devices = Some(devices);
                    }
                    Err(error) => {
                        log::warn!("The audio devices could not be listed: {error}");
                        tab.devices_failed = true;
                    }
                }
                let reset = match (&tab.saved, &tab.devices) {
                    (Some(saved), Some(devices)) if !recording => {
                        let mut preferences = saved.preferences.clone();
                        prune(&mut preferences, devices).then_some(preferences)
                    }
                    _ => None,
                };
                tab.show_choices(window, cx);
                if let Some(preferences) = reset {
                    shell.save_recording_preferences(preferences, window, cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// A select was confirmed: save the device it chose.
    fn device_chosen(
        &mut self,
        state: Entity<Choices>,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let tab = &self.settings.recording;
        if tab.frozen || tab.saving || self.home.is_recording() {
            return;
        }
        let Some(saved) = &tab.saved else {
            return;
        };
        let mut preferences = saved.preferences.clone();
        if state == tab.microphone {
            preferences.preferred_mic_device = tab.microphone_values.get(index).cloned().flatten();
        } else if state == tab.system {
            preferences.preferred_system_device = tab.system_values.get(index).cloned().flatten();
        } else {
            return;
        }
        if preferences.preferred_mic_device == saved.preferences.preferred_mic_device
            && preferences.preferred_system_device == saved.preferences.preferred_system_device
        {
            return;
        }
        self.save_recording_preferences(preferences, window, cx);
    }

    /// Save the preferences where the recording reads them. The selects
    /// are locked meanwhile; a failed save shows what is really saved.
    fn save_recording_preferences(
        &mut self,
        preferences: RecordingPreferences,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(host) = self.recording_host(cx) else {
            return;
        };
        let tab = &mut self.settings.recording;
        if tab.frozen {
            return;
        }
        tab.saving = true;
        let next = preferences.clone();
        let task = self.model.read(cx).runtime.spawn(async move {
            stored::save_recording_preferences(&host, &next)
                .await
                .map_err(|error| error.to_string())
        });
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await.unwrap_or_else(|error| Err(error.to_string()));
            let _ = this.update_in(cx, |shell, window, cx| {
                let tab = &mut shell.settings.recording;
                tab.saving = false;
                match result {
                    Ok(()) => {
                        window.push_notification(
                            Notification::success(text::recording_devices_saved(
                                preferences.preferred_mic_device.as_deref(),
                                preferences.preferred_system_device.as_deref(),
                            ))
                            .title(text::RECORDING_DEVICES_SAVED),
                            cx,
                        );
                        if let Some(saved) = tab.saved.as_mut() {
                            saved.preferences = preferences;
                        }
                    }
                    Err(error) => {
                        window.push_notification(
                            Notification::error(text::recording_devices_save_failed(&error))
                                .title(text::RECORDING_DEVICES_SAVE_FAILED),
                            cx,
                        );
                    }
                }
                tab.show_choices(window, cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// The reminder switch: shows the new value at once and saves it.
    fn set_recording_notice(&mut self, enabled: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(host) = self.recording_host(cx) else {
            return;
        };
        let tab = &mut self.settings.recording;
        if tab.frozen || tab.notice_pending {
            return;
        }
        let Some(saved) = tab.saved.as_mut() else {
            return;
        };
        let before = saved.notice;
        saved.notice = enabled;
        tab.notice_pending = true;
        let task = self
            .model
            .read(cx)
            .runtime
            .spawn_blocking(move || stored::set_recording_notice_enabled(&host, enabled));
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await.unwrap_or_else(|error| Err(error.to_string()));
            let _ = this.update_in(cx, |shell, window, cx| {
                let tab = &mut shell.settings.recording;
                tab.notice_pending = false;
                match result {
                    Ok(()) => window.push_notification(
                        Notification::success(text::RECORDING_SETTING_SAVED),
                        cx,
                    ),
                    Err(error) => {
                        log::warn!("The start reminder could not be saved: {error}");
                        if let Some(saved) = tab.saved.as_mut() {
                            saved.notice = before;
                        }
                        window.push_notification(
                            Notification::error(text::RECORDING_SETTING_SAVE_FAILED_HELP)
                                .title(text::RECORDING_SETTING_SAVE_FAILED),
                            cx,
                        );
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn recording_settings_tab(&self, cx: &mut Context<Self>) -> AnyElement {
        let tab = &self.settings.recording;
        if tab.stale && !tab.loading && !tab.frozen && self.recorder(cx).is_some() {
            // Drawing cannot start work that needs the window; do it right
            // after this frame.
            let shell = cx.entity().downgrade();
            let handle = tab.window;
            cx.defer(move |cx| {
                let _ = handle.update(cx, |_, window, cx| {
                    let _ = shell.update(cx, |shell, cx| shell.load_recording_settings(window, cx));
                });
            });
        }
        let model = self.model.read(cx);
        let runtime = model.runtime.clone();
        let palette = palette(cx);
        let recording = self.home.is_recording();
        // Without a host (no data folder) nothing can be read or saved.
        let editable = tab.frozen || (self.recorder(cx).is_some() && tab.saved.is_some());
        let folder = match &tab.saved {
            Some(saved) => Some(saved.preferences.save_folder.clone()),
            None => model.settings.recordings_dir.clone(),
        }
        .filter(|dir| !dir.as_os_str().is_empty());

        let title = v_flex()
            .child(heading(text::RECORDING_TITLE, Level::Section))
            .child(div().mt(space::XS).child(muted(text::RECORDING_HELP, cx)))
            .when(tab.preferences_unreadable, |this| {
                this.child(div().mt(space::MD).child(alert(
                    "recording-unreadable",
                    text::RECORDING_SETTINGS_UNREADABLE,
                    cx,
                )))
            });

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

        let notice_on = tab.saved.as_ref().is_none_or(|saved| saved.notice);
        let reminder = switch_row(
            "recording-start-notice",
            text::RECORDING_START_NOTICE,
            Some(text::RECORDING_START_NOTICE_HINT),
            controls::switch("recording-start-notice-switch")
                .checked(notice_on)
                .disabled(!editable || tab.notice_pending)
                .accessibility_label(text::RECORDING_START_NOTICE)
                .on_click(cx.listener(|shell, value: &bool, window, cx| {
                    shell.set_recording_notice(*value, window, cx)
                })),
            cx,
        );

        let locked = recording || tab.saving || !editable;
        let device = |id: &'static str,
                      icon: IconName,
                      label: &'static str,
                      select: &Entity<Choices>,
                      none_found: Option<&'static str>| {
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
                .child(
                    div().id(id).child(
                        Select::new(select)
                            .accessibility_label(SharedString::from(label))
                            .h(layout::CONTROL_HEIGHT)
                            .disabled(locked),
                    ),
                )
                .children(none_found.map(|message| {
                    div()
                        .id((id, 1usize))
                        .role(gpui::Role::Note)
                        .text_size(typography::XS)
                        .text_color(palette.muted_foreground)
                        .child(message)
                }))
        };
        let empty = |names: fn(&Devices) -> &Vec<String>| {
            tab.devices.as_ref().is_some_and(|d| names(d).is_empty())
        };
        let listing = h_flex()
            .justify_between()
            .child(
                div()
                    .text_size(typography::SM)
                    .child(field_label(text::RECORDING_DEVICES)),
            )
            .child(
                controls::small_icon_button(
                    "recording-refresh-devices",
                    IconName::RefreshCw,
                    text::RECORDING_DEVICES_REFRESH,
                )
                .tooltip(text::RECORDING_DEVICES_REFRESH_TOOLTIP)
                .disabled(tab.refreshing || recording || !editable)
                .on_click(cx.listener(|shell, _, window, cx| shell.refresh_devices(window, cx))),
            );
        let info = |label: &'static str, rest: &'static str| {
            h_flex()
                .gap(space::XS)
                .text_size(typography::XS)
                .text_color(palette.muted_foreground)
                .child("•")
                .child(div().font_weight(FontWeight::BOLD).child(label))
                .child(rest)
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
            .when(recording, |this| {
                this.child(div().mb(space::LG).child(tinted_box(
                    "recording-devices-locked",
                    palette.warning,
                    None,
                    muted(text::RECORDING_DEVICES_LOCKED, cx).into_any_element(),
                    cx,
                )))
            })
            .child(
                v_flex()
                    .gap(space::LG)
                    .child(listing)
                    .when(tab.devices_failed, |this| {
                        this.child(alert(
                            "recording-devices-error",
                            text::RECORDING_DEVICES_LOAD_FAILED,
                            cx,
                        ))
                    })
                    .when(
                        tab.devices.is_none() && !tab.devices_failed && editable,
                        |this| {
                            this.child(
                                div()
                                    .id("recording-devices-loading")
                                    .role(gpui::Role::Status)
                                    .child(muted(text::RECORDING_DEVICES_LOADING, cx)),
                            )
                        },
                    )
                    .child(
                        v_flex()
                            .gap(space::MD)
                            .child(device(
                                "recording-microphone",
                                IconName::Mic,
                                text::RECORDING_MICROPHONE,
                                &tab.microphone,
                                empty(|d| &d.inputs).then_some(text::RECORDING_NO_MICROPHONES),
                            ))
                            .child(device(
                                "recording-system",
                                IconName::Speaker,
                                text::RECORDING_SYSTEM,
                                &tab.system,
                                empty(|d| &d.outputs).then_some(text::RECORDING_NO_SYSTEM),
                            )),
                    )
                    .child(
                        v_flex()
                            .gap(space::XS)
                            .child(info(
                                text::RECORDING_INFO_MICROPHONE_LABEL,
                                text::RECORDING_INFO_MICROPHONE,
                            ))
                            .child(info(
                                text::RECORDING_INFO_SYSTEM_LABEL,
                                text::RECORDING_INFO_SYSTEM,
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

/// What the start and the end of a recording show, from the recorder's
/// state change: the reminder to tell the participants (Optagelser) and the
/// notifications about the start and the end (Generelt). Nothing happens
/// when the recording state did not change between recording and not.
pub(crate) fn recording_changed(
    was_recording: bool,
    is_recording: bool,
    runtime: tokio::runtime::Handle,
    host: HostHandle,
    cx: &mut AsyncApp,
) {
    if was_recording == is_recording {
        return;
    }
    let started = is_recording;
    let reads = runtime.spawn_blocking({
        let host = host.clone();
        move || {
            let reminder = stored::recording_notice_enabled(&host).unwrap_or_else(|error| {
                log::warn!("The start reminder could not be read: {error}");
                true
            });
            let notify = tingskriver_core::notification_settings::settings_path()
                .and_then(|path| {
                    tingskriver_core::notification_settings::recording_notifications_to_show(&path)
                        .map_err(|error| {
                            log::warn!("The notification settings could not be read: {error}")
                        })
                        .ok()
                })
                .unwrap_or(false);
            (reminder, notify)
        }
    });
    cx.spawn(async move |cx| {
        let Ok((reminder, notify)) = reads.await else {
            return;
        };
        cx.update(|cx| {
            let shown = lifecycle_shown(started, reminder, notify);
            if shown.lifecycle {
                crate::ui::notify_main_window(lifecycle_notification(started), cx);
            }
            if shown.reminder {
                crate::ui::notify_main_window(reminder_notification(runtime, host), cx);
            }
        });
    })
    .detach();
}

/// What a change of the recording state shows.
#[derive(Debug, PartialEq, Eq)]
struct LifecycleShown {
    /// "Optagelsen er startet/stoppet": the Generelt switch is on and
    /// "Forstyr ikke" is off (decided by `recording_notifications_to_show`).
    lifecycle: bool,
    /// The start reminder; only when a recording starts.
    reminder: bool,
}

fn lifecycle_shown(started: bool, reminder_on: bool, notify: bool) -> LifecycleShown {
    LifecycleShown {
        lifecycle: notify,
        reminder: started && reminder_on,
    }
}

/// "Optagelsen er startet…" or "…er stoppet og gemt" (Tauri's system
/// notifications). It goes to the operating system's notification center
/// and, so that it is never silently lost, to the window too.
fn lifecycle_notification(started: bool) -> Notification {
    Notification::info(if started {
        text::NOTIFY_STARTED
    } else {
        text::NOTIFY_STOPPED
    })
    .title(text::NOTIFY_APP)
    .in_app_and_system()
}

/// The reminder shown when a recording starts (`recordingNotification.tsx`):
/// the text, "Vis ikke dette igen" and "Deltagerne er informeret".
fn reminder_notification(runtime: tokio::runtime::Handle, host: HostHandle) -> Notification {
    let dont_show = Rc::new(Cell::new(false));
    let checked = dont_show.clone();
    Notification::info(text::RECORDING_REMINDER_TEXT)
        .title(text::RECORDING_REMINDER_TITLE)
        .content(move |_, _, _| {
            let dont_show = checked.clone();
            div()
                .mt(space::SM)
                .child(
                    Checkbox::new("recording-reminder-dont-show")
                        .label(text::RECORDING_REMINDER_DONT_SHOW)
                        .checked(dont_show.get())
                        .on_click(move |value, window, _| {
                            dont_show.set(*value);
                            window.refresh();
                        }),
                )
                .into_any_element()
        })
        .action(move |_, _, cx| {
            let dont_show = dont_show.clone();
            let runtime = runtime.clone();
            let host = host.clone();
            controls::button("recording-reminder-ack", Variant::Primary)
                .label(text::RECORDING_REMINDER_ACK)
                .on_click(cx.listener(move |notification, _, window, cx| {
                    if dont_show.get() {
                        save_reminder_off(&runtime, host.clone(), cx);
                    }
                    notification.dismiss(window, cx);
                }))
        })
        // Tauri's toast stays ten seconds; the button does not keep it open.
        .autohide(true)
}

/// "Vis ikke dette igen": the same switch as in Optagelser.
fn save_reminder_off<T: 'static>(
    runtime: &tokio::runtime::Handle,
    host: HostHandle,
    cx: &mut Context<T>,
) {
    let task = runtime.spawn_blocking(move || stored::set_recording_notice_enabled(&host, false));
    cx.spawn(async move |_, cx| {
        let result = task.await.unwrap_or_else(|error| Err(error.to_string()));
        if let Err(error) = result {
            log::warn!("The start reminder could not be switched off: {error}");
            cx.update(|cx| {
                crate::ui::notify_main_window(
                    Notification::error(text::RECORDING_SETTING_SAVE_FAILED_HELP)
                        .title(text::RECORDING_SETTING_SAVE_FAILED),
                    cx,
                )
            });
        }
    })
    .detach();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn devices() -> Devices {
        Devices {
            inputs: vec!["Mikrofon A".into(), "Headset".into()],
            outputs: vec!["Højttalere".into()],
        }
    }

    fn preferences(mic: Option<&str>, system: Option<&str>) -> RecordingPreferences {
        RecordingPreferences {
            preferred_mic_device: mic.map(str::to_string),
            preferred_system_device: system.map(str::to_string),
            ..RecordingPreferences::default()
        }
    }

    #[test]
    fn devices_are_saved_in_the_form_the_recording_parses() {
        // `start_recording` hands the saved value to `parse_audio_device`.
        for (name, input) in [("Headset (USB)", true), ("Højttalere", false)] {
            let parsed =
                tingskriver_core::audio::parse_audio_device(&device_value(name, input)).unwrap();
            assert_eq!(parsed.name, name);
            assert_eq!(parsed.device_type == DeviceType::Input, input);
        }
    }

    #[test]
    fn a_connected_device_is_kept() {
        let mut saved = preferences(Some("Headset (input)"), Some("Højttalere (output)"));
        assert!(!prune(&mut saved, &devices()));
        assert_eq!(
            saved.preferred_mic_device.as_deref(),
            Some("Headset (input)")
        );
    }

    #[test]
    fn a_device_that_is_gone_goes_back_to_the_default() {
        let mut saved = preferences(Some("Væk (input)"), Some("Væk (output)"));
        assert!(prune(&mut saved, &devices()));
        assert_eq!(saved.preferred_mic_device, None);
        assert_eq!(saved.preferred_system_device, None);
    }

    #[test]
    fn a_device_in_the_other_direction_does_not_count() {
        let mut saved = preferences(Some("Højttalere (input)"), None);
        assert!(prune(&mut saved, &devices()));
        assert_eq!(saved.preferred_mic_device, None);
    }

    #[test]
    fn an_empty_list_wipes_nothing() {
        let mut saved = preferences(Some("Headset (input)"), Some("Højttalere (output)"));
        assert!(!prune(&mut saved, &Devices::default()));
        assert!(saved.preferred_mic_device.is_some() && saved.preferred_system_device.is_some());
        // Each direction on its own.
        let only_outputs = Devices {
            inputs: Vec::new(),
            outputs: vec!["Andre".into()],
        };
        assert!(prune(&mut saved, &only_outputs));
        assert!(saved.preferred_mic_device.is_some());
        assert_eq!(saved.preferred_system_device, None);
    }

    #[test]
    fn the_default_is_the_first_choice_and_devices_follow() {
        let values = select_values(&devices().inputs, None, true);
        assert_eq!(
            values,
            vec![
                None,
                Some("Mikrofon A (input)".to_string()),
                Some("Headset (input)".to_string())
            ]
        );
        let labels = select_labels(&values, "Standardmikrofon");
        assert_eq!(labels, ["Standardmikrofon", "Mikrofon A", "Headset"]);
        assert_eq!(index_of(&values, Some("Headset (input)")), 2);
        assert_eq!(index_of(&values, None), 0);
    }

    #[test]
    fn a_saved_device_missing_from_the_list_is_still_shown() {
        let values = select_values(&[], Some("Ekstern (input)"), true);
        assert_eq!(values, vec![None, Some("Ekstern (input)".to_string())]);
        assert_eq!(index_of(&values, Some("Ekstern (input)")), 1);
        assert_eq!(select_labels(&values, "Standardmikrofon")[1], "Ekstern");
    }

    #[test]
    fn devices_are_told_apart_by_direction() {
        let list = [
            AudioDevice::new("Mic".into(), DeviceType::Input),
            AudioDevice::new("Speaker".into(), DeviceType::Output),
            AudioDevice::new("Headset".into(), DeviceType::Input),
        ];
        let devices = Devices::from_core(&list);
        assert_eq!(devices.inputs, ["Mic", "Headset"]);
        assert_eq!(devices.outputs, ["Speaker"]);
    }

    #[test]
    fn a_start_shows_the_notification_and_the_reminder_independently() {
        let shown = |started, reminder, notify| lifecycle_shown(started, reminder, notify);
        assert_eq!(
            shown(true, true, true),
            LifecycleShown {
                lifecycle: true,
                reminder: true
            }
        );
        assert_eq!(
            shown(true, true, false),
            LifecycleShown {
                lifecycle: false,
                reminder: true
            }
        );
        assert_eq!(
            shown(true, false, true),
            LifecycleShown {
                lifecycle: true,
                reminder: false
            }
        );
    }

    #[test]
    fn a_stop_never_shows_the_reminder() {
        assert_eq!(
            lifecycle_shown(false, true, true),
            LifecycleShown {
                lifecycle: true,
                reminder: false
            }
        );
        assert_eq!(
            lifecycle_shown(false, true, false),
            LifecycleShown {
                lifecycle: false,
                reminder: false
            }
        );
    }
}
