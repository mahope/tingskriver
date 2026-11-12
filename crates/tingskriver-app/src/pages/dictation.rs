//! The Diktering page, a copy of the Tauri page
//! (`frontend/src/components/dictation/DictationHistory.tsx`): the shortcut
//! explanation, a status line, "Seneste dikteringer" with Kopiér and Slet
//! per entry, "Ryd historik" and the empty and loading states. New in GPUI:
//! the keyboard test field (`probe`) folds away under "Test tastatur" at the
//! bottom, for the manual Windows input tests (docs/gpui.md).
//!
//! History, settings and status come from core's dictation service, as the
//! Tauri commands read them (`frontend/src-tauri/src/dictation/commands.rs`).

use std::time::Duration;

use chrono::{DateTime, FixedOffset, Local};
use gpui::{
    div, prelude::*, relative, AnyElement, ClipboardItem, Context, FontWeight, HighlightStyle,
    SharedString, StyledText, Window,
};
use gpui_component::{
    h_flex, input::Textarea, notification::Notification, tooltip::Tooltip, v_flex, Icon, WindowExt,
};
use gpui_kit_assets::IconName;
use tingskriver_core::dictation::settings::DictationSettings;
use tingskriver_core::dictation::store::{HistoryEntry, HISTORY_LIMIT};
use tingskriver_core::{format_da, strings};

use crate::controls::{self, Variant};
use crate::probe;
use crate::theme::{self, dictation as size, layout, space, typography};
use crate::ui::{heading, muted, palette, Level, Page, Shell};

/// How long the first failed load waits before the page reads again; each
/// further failure doubles the wait.
const LOAD_RETRY: Duration = Duration::from_secs(3);
/// Retries after a failed load before the page stops by itself (3 + 6 + 12
/// + 24 + 48 s, enough for the service to start). Some failures last the
/// whole session (the Tauri app owns dictation), so the page must not poll
/// forever; opening the page again starts a new round.
const LOAD_RETRIES: u32 = 5;

/// What the status line knows about dictation (Tauri `DictationStatus`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Status {
    /// The shortcut and text insertion only work on Windows.
    pub platform_supported: bool,
    pub has_syv_key: bool,
    pub local_model_available: bool,
    pub last_error: Option<String>,
}

/// The color of the status line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tone {
    Muted,
    Warning,
    Destructive,
}

/// The page's state, kept in the shell while the app runs.
#[derive(Debug, Default)]
pub(crate) struct DictationPage {
    /// `None` until the first load has answered (the loading cards).
    history: Option<Vec<HistoryEntry>>,
    settings: Option<DictationSettings>,
    status: Option<Status>,
    /// Why the history could not be read (Tauri shows a toast).
    load_error: Option<String>,
    /// Failed loads in a row since the last success or page visit.
    load_failures: u32,
    /// "Ryd historik" was clicked once and asks for a second click.
    confirm_clear: bool,
    /// "Test tastatur" is unfolded.
    probe_open: bool,
    loading: bool,
    /// Read again the next time the page is drawn.
    stale: bool,
    /// Fixture data: never read from core.
    frozen: bool,
    /// Fixture time zone, so the dates do not depend on the machine.
    offset: Option<FixedOffset>,
}

impl DictationPage {
    pub(crate) fn new() -> Self {
        DictationPage {
            stale: true,
            ..Default::default()
        }
    }

    /// The shortcut the page explains: the saved one, or core's default
    /// until the settings are read.
    fn shortcut(&self) -> String {
        self.settings
            .as_ref()
            .map(|s| s.shortcut.clone())
            .unwrap_or_else(|| DictationSettings::default().shortcut)
    }

    /// A load failed: keep what was shown (no false "Ingen dikteringer
    /// endnu"). Returns how long to wait before reading again, or `None`
    /// when the page should stop trying (fixtures never read from core).
    fn load_failed(&mut self, error: String) -> Option<Duration> {
        self.load_error = Some(error);
        self.load_failures = self.load_failures.saturating_add(1);
        if self.frozen || !self.retrying() {
            return None;
        }
        Some(LOAD_RETRY * 2u32.pow(self.load_failures - 1))
    }

    /// Whether a failed load will be read again without the user's help.
    fn retrying(&self) -> bool {
        !self.frozen && self.load_failures <= LOAD_RETRIES
    }

    /// The page left the screen: read again on return (Tauri reloads on
    /// mount) and forget a half-done "Ryd historik".
    pub(crate) fn hide(&mut self) {
        self.stale = true;
        self.load_failures = 0;
        self.confirm_clear = false;
    }

    /// Fixed state for the screenshot fixtures (synthetic entries).
    #[cfg(feature = "screenshot-tests")]
    pub(crate) fn fixture(variant: crate::ui::FixtureVariant) -> Self {
        use crate::ui::FixtureVariant;
        let empty = matches!(
            variant,
            FixtureVariant::DictationEmpty | FixtureVariant::DictationProbe
        );
        DictationPage {
            history: Some(if empty { Vec::new() } else { fixture_history() }),
            settings: Some(DictationSettings::default()),
            status: Some(Status {
                platform_supported: true,
                has_syv_key: true,
                local_model_available: true,
                last_error: None,
            }),
            probe_open: variant == FixtureVariant::DictationProbe,
            frozen: true,
            offset: FixedOffset::east_opt(0),
            ..Default::default()
        }
    }
}

/// The four synthetic dictations of `docs/screenshots/diktering-lys.png`.
#[cfg(feature = "screenshot-tests")]
fn fixture_history() -> Vec<HistoryEntry> {
    let at = |day: u32, hour: u32, minute: u32| {
        chrono::NaiveDate::from_ymd_opt(2026, 10, day)
            .and_then(|d| d.and_hms_opt(hour, minute, 0))
            .map(|t| t.and_utc().timestamp_millis())
            .unwrap_or_default()
    };
    let entry =
        |id, created_at, text: &str, app: &str, engine: &str, latency_ms, fallback| HistoryEntry {
            id,
            created_at,
            text: text.to_string(),
            app: Some(app.to_string()),
            style: "normal".into(),
            engine: engine.into(),
            provider: None,
            syv_model: None,
            language: Some("da".into()),
            fallback_reason: fallback,
            latency_ms,
            audio_ms: 2000,
            insert_method: None,
            has_audio: false,
        };
    vec![
        entry(
            4,
            at(30, 14, 2),
            "Hej Anna, tak for i dag. Jeg sender tilbuddet i morgen formiddag.",
            "Spark.exe",
            "syv",
            940,
            None,
        ),
        entry(
            3,
            at(30, 13, 48),
            "git commit -m \"ret hotswap\"",
            "WindowsTerminal.exe",
            "syv",
            710,
            None,
        ),
        entry(
            2,
            at(30, 11, 15),
            "Kan vi rykke mødet til torsdag kl. 10? Jeg har en kunde om onsdagen.",
            "ms-teams.exe",
            "local",
            2400,
            Some("Syv.ai svarede ikke inden for 2 s".into()),
        ),
        entry(
            1,
            at(29, 16, 30),
            "Husk at opdatere prislisten.\nSend den til Mahoje bagefter.",
            "chrome.exe",
            "syv",
            1100,
            None,
        ),
    ]
}

/// The status line under the explanation (Tauri `StatusLine`).
pub(crate) fn status_line(status: &Status, settings: &DictationSettings) -> (String, Tone) {
    if !status.platform_supported {
        (strings::DICTATION_ONLY_WINDOWS.into(), Tone::Muted)
    } else if !settings.enabled {
        (strings::DICTATION_DISABLED.into(), Tone::Muted)
    } else if let Some(error) = &status.last_error {
        (error.clone(), Tone::Destructive)
    } else if !status.has_syv_key && !status.local_model_available {
        (strings::DICTATION_NO_ENGINE.into(), Tone::Warning)
    } else {
        let engine = if settings.engine == "local" || !status.has_syv_key {
            strings::DICTATION_ENGINE_LOCAL
        } else {
            strings::DICTATION_ENGINE_SYV
        };
        (strings::dictation_ready(engine), Tone::Muted)
    }
}

/// "30. okt., 14.02" in local time (or the fixture's zone).
fn created(entry: &HistoryEntry, offset: Option<FixedOffset>) -> String {
    let Some(utc) = DateTime::from_timestamp_millis(entry.created_at) else {
        return String::new();
    };
    let local = match offset {
        Some(offset) => utc.with_timezone(&offset).naive_local(),
        None => utc.with_timezone(&Local).naive_local(),
    };
    format_da::day_month_clock(local)
}

fn engine_label(entry: &HistoryEntry) -> &'static str {
    if entry.engine == "syv" {
        strings::DICTATION_ENGINE_SYV_SHORT
    } else {
        strings::DICTATION_ENGINE_LOCAL_SHORT
    }
}

/// History, settings and status, as the Tauri commands read them.
async fn fetch() -> Result<(Vec<HistoryEntry>, DictationSettings, Status), String> {
    use tingskriver_core::dictation::{secrets, service};
    let service = service::get()?;
    let history = service.store.history(HISTORY_LIMIT).await?;
    // Ok(false) also means "the Parakeet engine has not started". Tauri starts
    // it at launch; GPUI only starts it with a meeting transcription, and
    // until then local dictation cannot run either (warm::ensure_parakeet
    // fails), so the warning matches what a dictation would do. Only a failed
    // model scan is unknown, and the line does not claim there is no model.
    let local_model_available =
        tingskriver_core::parakeet_engine::commands::parakeet_has_available_models()
            .await
            .unwrap_or(true);
    let status = Status {
        platform_supported: cfg!(windows),
        has_syv_key: secrets::syv_key().is_some(),
        local_model_available,
        last_error: service.last_error(),
    };
    Ok((history, service.settings(), status))
}

impl Shell {
    /// Core's history changed (a new dictation, or a deletion elsewhere).
    pub(crate) fn dictation_changed(&mut self, cx: &mut Context<Self>) {
        self.dictation.stale = true;
        if self.page == Page::Dictation {
            cx.notify();
        }
    }

    fn load_dictation(&mut self, cx: &mut Context<Self>) {
        let state = &mut self.dictation;
        if state.frozen || state.loading || !state.stale {
            return;
        }
        state.stale = false;
        state.loading = true;
        let task = self.model.read(cx).runtime.spawn(fetch());
        cx.spawn(async move |this, cx| {
            let result = task.await.unwrap_or_else(|error| Err(error.to_string()));
            let _ = this.update(cx, |shell, cx| {
                let state = &mut shell.dictation;
                state.loading = false;
                match result {
                    Ok((history, settings, status)) => {
                        state.history = Some(history);
                        state.settings = Some(settings);
                        state.status = Some(status);
                        state.load_error = None;
                        state.load_failures = 0;
                    }
                    Err(error) => {
                        // Warn once per round; the retries say the same.
                        if state.load_failures == 0 {
                            log::warn!("Dictation history could not be read: {error}");
                        } else {
                            log::debug!("Dictation history could not be read: {error}");
                        }
                        // The service may not be ready yet (startup): try
                        // again a few times with a growing wait.
                        if let Some(wait) = state.load_failed(error) {
                            cx.spawn(async move |this, cx| {
                                cx.background_executor().timer(wait).await;
                                let _ = this.update(cx, |shell, cx| shell.dictation_changed(cx));
                            })
                            .detach();
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn copy_dictation(&mut self, text: String, window: &mut Window, cx: &mut Context<Self>) {
        self.dictation.confirm_clear = false;
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        window.push_notification(Notification::success(strings::DICTATION_COPIED), cx);
        cx.notify();
    }

    fn delete_dictation(&mut self, id: i64, window: &mut Window, cx: &mut Context<Self>) {
        self.dictation.confirm_clear = false;
        let task = self.model.read(cx).runtime.spawn(async move {
            let service = tingskriver_core::dictation::service::get()?;
            if let Some(path) = service.store.delete_history(id).await? {
                service.delete_files(vec![path]);
            }
            service.notify_history_changed();
            Ok::<_, String>(())
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await.unwrap_or_else(|error| Err(error.to_string()));
            let _ = this.update_in(cx, |shell, window, cx| {
                match result {
                    Ok(()) => {
                        if let Some(history) = &mut shell.dictation.history {
                            history.retain(|entry| entry.id != id);
                        }
                    }
                    Err(error) => window.push_notification(
                        Notification::error(error).title(strings::DICTATION_DELETE_FAILED),
                        cx,
                    ),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// The first click asks for a second one (Tauri `clearAll`).
    fn clear_dictations(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.dictation.confirm_clear {
            self.dictation.confirm_clear = true;
            cx.notify();
            return;
        }
        self.dictation.confirm_clear = false;
        let task = self.model.read(cx).runtime.spawn(async move {
            let service = tingskriver_core::dictation::service::get()?;
            let paths = service.store.clear_history().await?;
            service.delete_files(paths);
            service.notify_history_changed();
            Ok::<_, String>(())
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await.unwrap_or_else(|error| Err(error.to_string()));
            let _ = this.update_in(cx, |shell, window, cx| {
                match result {
                    Ok(()) => {
                        shell.dictation.history = Some(Vec::new());
                        window.push_notification(
                            Notification::success(strings::DICTATION_CLEARED),
                            cx,
                        );
                    }
                    Err(error) => window.push_notification(
                        Notification::error(error).title(strings::DICTATION_CLEAR_FAILED),
                        cx,
                    ),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn dictation_page(&mut self, cx: &mut Context<Self>) -> AnyElement {
        self.load_dictation(cx);
        let body = v_flex()
            .w_full()
            .max_w(layout::DICTATION_MAX)
            .mx_auto()
            .gap(space::XL)
            .child(self.dictation_header(cx))
            .child(self.dictation_history(cx))
            .child(self.dictation_probe(cx));
        v_flex()
            .id("page-dictation")
            .size_full()
            .overflow_y_scroll()
            .px(space::XXL)
            .py(space::XXL)
            .child(body)
            .into_any_element()
    }

    /// Title with "Indstillinger", the shortcut explanation and the status.
    fn dictation_header(&self, cx: &mut Context<Self>) -> AnyElement {
        let palette = palette(cx);
        let state = &self.dictation;
        let shortcut = state.shortcut();
        // Tauri's <kbd>: the shortcut on a muted chip inside the running
        // text. A text run has no border or padding; no-break spaces pad it.
        let mut chip = String::from('\u{a0}');
        chip.push_str(&shortcut);
        chip.push('\u{a0}');
        let hold = strings::dictation_hold(&chip);
        let highlight = hold
            .find(&chip)
            .map(|start| (start..start + chip.len(), palette.muted));
        let explanation =
            StyledText::new(hold.clone()).with_highlights(highlight.map(|(range, background)| {
                (
                    range,
                    HighlightStyle {
                        background_color: Some(background),
                        ..Default::default()
                    },
                )
            }));
        let status = match (&state.load_error, &state.status, &state.settings) {
            (Some(error), _, _) => {
                Some((strings::dictation_load_failed(&error), Tone::Destructive))
            }
            (None, Some(status), Some(settings)) => Some(status_line(status, settings)),
            _ => None,
        };
        v_flex()
            .gap(space::MD)
            .child(
                h_flex()
                    .items_start()
                    .justify_between()
                    .gap(space::LG)
                    .child(heading(strings::DICTATION, Level::Page))
                    .child(
                        controls::button("dictation-settings", Variant::Outline)
                            .icon(IconName::Settings)
                            .label(strings::DICTATION_SETTINGS)
                            // Tauri opens Indstillinger on the Diktering tab.
                            .on_click(cx.listener(|shell, _, _, cx| {
                                shell
                                    .settings
                                    .select_tab(crate::pages::settings::SettingsTab::Dictation);
                                shell.page = Page::Settings;
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .text_color(palette.foreground)
                    .line_height(relative(typography::LINE_HEIGHT))
                    .child(explanation),
            )
            .when_some(status, |this, (text, tone)| {
                this.child(
                    div()
                        .id("dictation-status")
                        .role(gpui::Role::Status)
                        .text_size(typography::SM)
                        .text_color(match tone {
                            Tone::Muted => palette.muted_foreground,
                            Tone::Warning => palette.warning,
                            Tone::Destructive => palette.destructive,
                        })
                        .child(text),
                )
            })
            .into_any_element()
    }

    /// "Seneste dikteringer": loading cards, the empty card or the list.
    fn dictation_history(&self, cx: &mut Context<Self>) -> AnyElement {
        let palette = palette(cx);
        let state = &self.dictation;
        let has_entries = state.history.as_ref().is_some_and(|h| !h.is_empty());
        let history_off = state.settings.as_ref().is_some_and(|s| !s.history_enabled);
        let shortcut = state.shortcut();
        let content = match &state.history {
            // Not read yet and the read failed: the status line says why,
            // and the page tries again by itself or asks to be reopened.
            None if state.load_error.is_some() => muted(
                if state.retrying() {
                    strings::DICTATION_LOAD_RETRYING
                } else {
                    strings::DICTATION_LOAD_GAVE_UP
                },
                cx,
            )
            .into_any_element(),
            None => v_flex()
                .gap(space::MD)
                .children((0..3).map(|_| {
                    div()
                        .h(size::SKELETON_HEIGHT)
                        .rounded(theme::RADIUS_LG)
                        .bg(palette.muted)
                }))
                .into_any_element(),
            Some(history) if history.is_empty() => v_flex()
                .items_center()
                .px(space::XL)
                .py(space::XXXL)
                .rounded(theme::RADIUS_LG)
                .border_1()
                .border_dashed()
                .border_color(palette.border)
                .child(
                    Icon::new(IconName::Keyboard)
                        .size(size::EMPTY_ICON)
                        .text_color(palette.muted_foreground),
                )
                .child(
                    div()
                        .mt(space::MD)
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette.foreground)
                        .child(strings::DICTATION_EMPTY),
                )
                .child(
                    div()
                        .mt(space::XS)
                        .text_size(typography::SM)
                        .text_center()
                        .text_color(palette.muted_foreground)
                        .child(strings::dictation_empty_hint(&shortcut)),
                )
                .into_any_element(),
            Some(history) => v_flex()
                .gap(space::MD)
                .children(history.iter().map(|entry| self.dictation_entry(entry, cx)))
                .into_any_element(),
        };
        let confirm = state.confirm_clear;
        v_flex()
            .gap(space::MD)
            .child(
                h_flex()
                    .items_center()
                    .justify_between()
                    .gap(space::LG)
                    .child(
                        div()
                            .font_family(typography::SERIF)
                            .font_weight(FontWeight::NORMAL)
                            .text_size(typography::XL)
                            .line_height(relative(typography::LINE_HEIGHT_TIGHT))
                            .child(strings::DICTATION_HISTORY),
                    )
                    .when(has_entries, |this| {
                        // Tauri resets the confirmation on blur; a click
                        // anywhere else does the same here.
                        this.child(
                            div()
                                .on_mouse_down_out(cx.listener(|shell, _, _, cx| {
                                    if shell.dictation.confirm_clear {
                                        shell.dictation.confirm_clear = false;
                                        cx.notify();
                                    }
                                }))
                                .child(
                                    controls::small_button(
                                        "dictation-clear",
                                        if confirm {
                                            Variant::Danger
                                        } else {
                                            Variant::Ghost
                                        },
                                    )
                                    .label(if confirm {
                                        strings::DICTATION_CLEAR_CONFIRM
                                    } else {
                                        strings::DICTATION_CLEAR
                                    })
                                    .on_click(cx.listener(
                                        |shell, _, window, cx| shell.clear_dictations(window, cx),
                                    )),
                                ),
                        )
                    }),
            )
            .when(history_off, |this| {
                this.child(muted(strings::DICTATION_HISTORY_OFF, cx))
            })
            .child(content)
            .into_any_element()
    }

    /// One history card: text, "dato · program · udbyder · varighed" and
    /// the Kopiér and Slet buttons.
    fn dictation_entry(&self, entry: &HistoryEntry, cx: &mut Context<Self>) -> AnyElement {
        let palette = palette(cx);
        let id = entry.id;
        let key = id as u64;
        let separator = || div().child(" · ");
        let text = entry.text.clone();
        let meta = h_flex()
            .flex_wrap()
            .text_size(typography::SM)
            .text_color(palette.muted_foreground)
            .child(created(entry, self.dictation.offset))
            .when_some(entry.app.clone(), |this, app| {
                this.child(separator()).child(app)
            })
            .child(separator())
            .child(engine_label(entry))
            .child(separator())
            .child(
                div()
                    .id(("dictation-latency", key))
                    .tooltip(|window, cx| {
                        Tooltip::new(strings::DICTATION_LATENCY).build(window, cx)
                    })
                    .child(format_da::milliseconds(entry.latency_ms)),
            )
            .when_some(entry.fallback_reason.clone(), |this, reason| {
                let reason = SharedString::from(reason);
                this.child(
                    h_flex()
                        .id(("dictation-fallback", key))
                        .text_color(palette.warning)
                        .tooltip(move |window, cx| Tooltip::new(reason.clone()).build(window, cx))
                        .child(separator())
                        .child(strings::DICTATION_FALLBACK),
                )
            });
        v_flex()
            .p(space::LG)
            .rounded(theme::RADIUS_LG)
            .border_1()
            .border_color(palette.border)
            .bg(palette.card)
            .text_color(palette.card_foreground)
            .child(div().child(entry.text.clone()))
            .child(
                h_flex()
                    .mt(space::MD)
                    .flex_wrap()
                    .items_center()
                    .justify_between()
                    .gap(space::SM)
                    .child(meta)
                    .child(
                        h_flex()
                            .gap(space::XS)
                            .child(
                                controls::small_button(("dictation-copy", key), Variant::Ghost)
                                    .icon(IconName::Copy)
                                    .label(strings::DICTATION_COPY)
                                    .accessibility_label(strings::DICTATION_COPY_LABEL)
                                    .on_click(cx.listener(move |shell, _, window, cx| {
                                        shell.copy_dictation(text.clone(), window, cx)
                                    })),
                            )
                            .child(
                                controls::small_icon_button(
                                    ("dictation-delete", key),
                                    IconName::Trash,
                                    strings::DICTATION_DELETE_LABEL,
                                )
                                .on_click(cx.listener(
                                    move |shell, _, window, cx| {
                                        shell.delete_dictation(id, window, cx)
                                    },
                                )),
                            ),
                    ),
            )
            .into_any_element()
    }

    /// "Test tastatur" (GPUI only): the probe fields, folded by default.
    fn dictation_probe(&self, cx: &mut Context<Self>) -> AnyElement {
        let palette = palette(cx);
        let open = self.dictation.probe_open;
        let toggle = controls::button("dictation-probe-toggle", Variant::Ghost)
            .icon(if open {
                IconName::ChevronDown
            } else {
                IconName::ChevronRight
            })
            .label(strings::PROBE)
            .on_click(cx.listener(|shell, _, _, cx| {
                shell.dictation.probe_open = !shell.dictation.probe_open;
                cx.notify();
            }));
        let section = v_flex()
            .gap(space::SM)
            .pt(space::LG)
            .border_t_1()
            .border_color(palette.border)
            .child(toggle);
        if !open {
            return section.into_any_element();
        }
        let typed = format!(
            "{}\n{}",
            self.probe_line.read(cx).value(),
            self.probe_text.read(cx).value()
        );
        let described = probe::describe(&typed);
        let warning = probe::composition_warning(&typed);
        section
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
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready() -> Status {
        Status {
            platform_supported: true,
            has_syv_key: true,
            local_model_available: true,
            last_error: None,
        }
    }

    #[test]
    fn status_line_follows_the_tauri_order() {
        let settings = DictationSettings::default();
        assert_eq!(
            status_line(&ready(), &settings),
            (
                "Genvejen er klar. Transskriberes med Syv.ai (lokal model som reserve).".into(),
                Tone::Muted
            )
        );
        let no_key = Status {
            has_syv_key: false,
            ..ready()
        };
        assert_eq!(
            status_line(&no_key, &settings).0,
            "Genvejen er klar. Transskriberes med den lokale model."
        );
        let local = DictationSettings {
            engine: "local".into(),
            ..DictationSettings::default()
        };
        assert!(status_line(&ready(), &local)
            .0
            .ends_with("den lokale model."));
        let nothing = Status {
            has_syv_key: false,
            local_model_available: false,
            ..ready()
        };
        assert_eq!(status_line(&nothing, &settings).1, Tone::Warning);
        let failed = Status {
            last_error: Some("Genvejen kunne ikke registreres".into()),
            ..nothing.clone()
        };
        assert_eq!(
            status_line(&failed, &settings),
            ("Genvejen kunne ikke registreres".into(), Tone::Destructive)
        );
        let off = DictationSettings {
            enabled: false,
            ..DictationSettings::default()
        };
        assert_eq!(
            status_line(&failed, &off).0,
            strings::DICTATION_DISABLED,
            "a disabled dictation hides the last error"
        );
        let linux = Status {
            platform_supported: false,
            ..failed
        };
        assert_eq!(status_line(&linux, &off).0, strings::DICTATION_ONLY_WINDOWS);
    }

    #[test]
    fn failed_load_keeps_the_history_unknown_and_retries() {
        let mut page = DictationPage::new();
        assert_eq!(
            page.load_failed("Diktering er ikke klar endnu".into()),
            Some(LOAD_RETRY)
        );
        // No false empty history: the empty card needs Some(vec![]).
        assert!(page.history.is_none());
        assert!(page.load_error.is_some());

        let mut fixture = DictationPage {
            frozen: true,
            ..Default::default()
        };
        assert_eq!(fixture.load_failed("x".into()), None);
    }

    #[test]
    fn failed_loads_back_off_and_stop_until_the_page_is_reopened() {
        let mut page = DictationPage::new();
        let waits: Vec<_> = (0..LOAD_RETRIES)
            .map(|_| page.load_failed("x".into()).expect("retries"))
            .collect();
        assert_eq!(
            waits,
            [3, 6, 12, 24, 48].map(Duration::from_secs),
            "the wait doubles"
        );
        assert!(page.retrying());
        // A lasting failure (the Tauri app owns dictation) stops polling.
        assert_eq!(page.load_failed("x".into()), None);
        assert!(!page.retrying());
        assert_eq!(page.load_failed("x".into()), None);
        // Opening the page again starts a new round.
        page.hide();
        assert_eq!(page.load_failed("x".into()), Some(LOAD_RETRY));
    }

    #[test]
    fn default_shortcut_is_the_one_the_page_explains() {
        assert_eq!(DictationSettings::default().shortcut, "Ctrl+Win");
    }

    #[test]
    fn dates_use_the_given_zone() {
        let entry = HistoryEntry {
            id: 1,
            created_at: chrono::NaiveDate::from_ymd_opt(2026, 10, 30)
                .unwrap()
                .and_hms_opt(14, 2, 0)
                .unwrap()
                .and_utc()
                .timestamp_millis(),
            text: String::new(),
            app: None,
            style: "normal".into(),
            engine: "syv".into(),
            provider: None,
            syv_model: None,
            language: None,
            fallback_reason: None,
            latency_ms: 0,
            audio_ms: 0,
            insert_method: None,
            has_audio: false,
        };
        assert_eq!(created(&entry, FixedOffset::east_opt(0)), "30. okt., 14.02");
        assert_eq!(
            created(&entry, FixedOffset::east_opt(3600)),
            "30. okt., 15.02"
        );
        assert_eq!(engine_label(&entry), "Syv.ai");
    }
}
