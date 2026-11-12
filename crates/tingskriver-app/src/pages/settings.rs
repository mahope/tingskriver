//! The Indstillinger page: MCP access and data controls, appearance,
//! meeting and report languages, voiceprint, data folder and log.

use gpui::{div, prelude::*, AnyElement, App, Context, Entity};
use gpui_component::{h_flex, v_flex, ActiveTheme, Disableable, Selectable, WindowExt};
use gpui_kit_assets::IconName;
use tingskriver_core::strings;

use crate::controls::{self, Variant};
use crate::startup::{self, DbState};
use crate::theme::{self, layout, space};
use crate::ui::{
    muted, page, palette, paragraph, section_title, setting_group, AppModel, Page, Shell,
};

impl Shell {
    pub(crate) fn settings_page(&self, cx: &mut Context<Self>) -> AnyElement {
        let model = self.model.read(cx);
        let mode = if cx.theme().mode.is_dark() {
            strings::DARK
        } else {
            strings::LIGHT
        };
        let data_dir = model
            .app_data_dir
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| strings::UNKNOWN.to_string());
        let database = match &model.db {
            DbState::Loading => strings::DB_OPENING,
            DbState::NotCreated => strings::DB_NOT_CREATED,
            DbState::Ready(_) => strings::DB_READY,
            DbState::Failed(_) => strings::DB_FAILED,
        };
        let log_path = model
            .log_path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| strings::NO_LOG.to_string());
        let log_dir = model.log_dir();

        let body = v_flex()
            .gap(space::XL)
            .child(
                controls::button("settings-about", Variant::Outline)
                    .label(strings::ABOUT)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.page = Page::About;
                        this.about_copy_focus.focus(window, cx);
                        cx.notify();
                    })),
            )
            .child(paragraph(strings::SETTINGS_MCP_HELP, cx))
            .child(self.mcp_settings(cx))
            .child(setting_group(
                strings::APPEARANCE,
                vec![strings::appearance(mode)],
                cx,
            ))
            .child(self.language_group(cx))
            .child(setting_group(
                strings::VOICEPRINT_TITLE,
                voiceprint_lines(),
                cx,
            ))
            .child(setting_group(
                strings::DATA,
                vec![strings::data_dir(&data_dir), strings::database(database)],
                cx,
            ))
            .child(
                v_flex()
                    .gap(space::SM)
                    .child(setting_group(
                        strings::LOG,
                        vec![strings::log_file(&log_path)],
                        cx,
                    ))
                    .child(
                        h_flex()
                            .gap(space::SM)
                            .child(
                                controls::button("open-log-dir", Variant::Outline)
                                    .label(strings::OPEN_LOG)
                                    .icon(IconName::FolderOpen)
                                    .on_click(move |_, _, _| {
                                        if let Some(dir) = &log_dir {
                                            startup::open_folder(dir);
                                        }
                                    }),
                            )
                            .child(
                                controls::button("quit", Variant::Outline)
                                    .label(strings::QUIT)
                                    .on_click(|_, _, cx| cx.quit()),
                            ),
                    ),
            );
        page(
            strings::SETTINGS,
            None,
            false,
            layout::SETTINGS_SECTION_MAX,
            body.into_any_element(),
            cx,
        )
    }

    fn mcp_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        use tingskriver_core::mcp::Access;
        let state = self.model.read(cx);
        let current = state.mcp_access;
        let mut controls = h_flex().gap(space::SM).flex_wrap();
        for (index, (access, label)) in [
            (Access::Marked, strings::MCP_MARKED),
            (Access::Metadata, strings::MCP_METADATA),
            (Access::All, strings::MCP_ALL),
            (Access::Off, strings::MCP_OFF),
        ]
        .into_iter()
        .enumerate()
        {
            let model = self.model.clone();
            controls = controls.child(
                controls::button(("mcp-access", index), Variant::Outline)
                    .label(label)
                    .selected(current == Some(access))
                    .toggled(current == Some(access))
                    .accessibility_label(label)
                    .disabled(current.is_none() || state.mcp_pending)
                    .on_click(move |_, _, cx| {
                        let model = model.clone();
                        let state = model.read(cx);
                        let DbState::Ready(db) = &state.db else {
                            return;
                        };
                        let pool = db.pool().clone();
                        let task = state.runtime.spawn(async move {
                            tingskriver_core::mcp::set_access(&pool, access).await
                        });
                        model.update(cx, |state, cx| {
                            state.mcp_pending = true;
                            cx.notify();
                        });
                        cx.spawn(async move |cx| {
                            let result = task.await;
                            model.update(cx, |state, cx| {
                                state.mcp_pending = false;
                                if matches!(result, Ok(Ok(()))) {
                                    state.mcp_access = Some(access);
                                    state.mcp_message = strings::MCP_SAVED.into();
                                } else {
                                    state.mcp_message = strings::MCP_SAVE_FAILED.into();
                                }
                                cx.notify();
                            });
                        })
                        .detach();
                    }),
            );
        }
        let mut body = v_flex()
            .gap(space::SM)
            .child(section_title(strings::MCP_TITLE))
            .child(paragraph(strings::MCP_PRIVACY, cx))
            .child(controls)
            .child(
                div()
                    .id("mcp-result")
                    .role(gpui::Role::Status)
                    .aria_label(state.mcp_message.clone())
                    .child(paragraph(&state.mcp_message, cx)),
            );
        if let Ok(exe) = std::env::current_exe() {
            if let Some(parent) = exe.parent() {
                let path = parent.join(if cfg!(windows) {
                    "tingskriver-mcp.exe"
                } else {
                    "tingskriver-mcp"
                });
                if let Ok(quoted) = serde_json::to_string(&path.to_string_lossy()) {
                    for (index, (label, text)) in [
                        (
                            strings::MCP_COPY_CLAUDE,
                            strings::mcp_claude_command(&path.to_string_lossy()),
                        ),
                        (strings::MCP_COPY_CODEX, strings::mcp_codex_config(&quoted)),
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        body = body.child(
                            controls::button(("mcp-copy", index), Variant::Outline)
                                .label(label)
                                .disabled(!path.is_file())
                                .on_click(move |_, window, cx| {
                                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                                        text.clone(),
                                    ));
                                    window.push_notification(strings::MCP_COPIED, cx);
                                }),
                        );
                    }
                }
            }
        }
        self.mcp_details(body, cx).into_any_element()
    }

    fn mcp_details(&self, mut body: gpui::Div, cx: &mut Context<Self>) -> gpui::Div {
        let state = self.model.read(cx);
        let exe = state.mcp_server.clone();
        let exists = exe.as_ref().is_some_and(|p| p.is_file());
        let status = match state.mcp_controls {
            None if matches!(state.db, DbState::Loading) => strings::MCP_LOADING,
            None => strings::MCP_LOAD_FAILED,
            Some(_) if !exists => strings::MCP_MISSING,
            Some(value)
                if !value.enabled
                    || state.mcp_access == Some(tingskriver_core::mcp::Access::Off) =>
            {
                strings::MCP_DISABLED
            }
            Some(_) if state.mcp_running > 0 => strings::MCP_ACTIVE,
            Some(_) => strings::MCP_AVAILABLE,
        };
        body = body.child(
            div()
                .id("mcp-status")
                .role(gpui::Role::Status)
                .aria_label(status)
                .child(paragraph(status, cx)),
        );
        if state.mcp_running > 0 {
            body = body.child(paragraph(
                &strings::MCP_RUNNING.replace("{value}", &state.mcp_running.to_string()),
                cx,
            ));
        }
        if let Some(path) = &exe {
            body = body.child(paragraph(
                &strings::MCP_PATH.replace("{value}", &path.display().to_string()),
                cx,
            ));
        }
        if let Some(value) = state.mcp_controls {
            let mut toggles = h_flex().gap(space::SM).flex_wrap();
            for (index, (label, active)) in [
                (strings::MCP_ENABLE, value.enabled),
                (strings::MCP_NOTES, value.notes),
                (strings::MCP_SUMMARY, value.summary),
                (strings::MCP_TRANSCRIPT, value.transcript),
                (strings::MCP_PARTICIPANTS, value.participants),
            ]
            .into_iter()
            .enumerate()
            {
                let model = self.model.clone();
                toggles = toggles.child(
                    controls::button(("mcp-data", index), Variant::Outline)
                        .label(label)
                        .accessibility_label(label)
                        .selected(active)
                        .toggled(active)
                        .disabled(state.mcp_pending)
                        .on_click(move |_, _, cx| {
                            let mut updated = value;
                            match index {
                                0 => updated.enabled = !active,
                                1 => updated.notes = !active,
                                2 => updated.summary = !active,
                                3 => updated.transcript = !active,
                                _ => updated.participants = !active,
                            }
                            save_mcp_controls(model.clone(), updated, cx);
                        }),
                );
            }
            body = body
                .child(paragraph(strings::MCP_DATA_HELP, cx))
                .child(toggles);
        }
        if let Some(Ok(meetings)) = &state.sharing_meetings {
            body = body.child(paragraph(
                &strings::MCP_SHARED_COUNT.replace(
                    "{value}",
                    &meetings.iter().filter(|m| m.shared).count().to_string(),
                ),
                cx,
            ));
        }
        body = body.child(
            controls::button("mcp-open-meetings", Variant::Outline)
                .label(strings::MCP_OPEN_MEETINGS)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.page = Page::Meetings;
                    cx.notify();
                })),
        );
        let model = self.model.clone();
        let dir = state.app_data_dir.clone();
        let test_exe = exe.clone();
        body = body.child(
            controls::button("mcp-test", Variant::Outline)
                .label(strings::MCP_TEST)
                .disabled(state.mcp_pending || !exists || dir.is_none())
                .on_click(move |_, _, cx| {
                    let (Some(exe), Some(dir)) = (test_exe.clone(), dir.clone()) else {
                        return;
                    };
                    let task = model.update(cx, |state, cx| {
                        state.mcp_pending = true;
                        state.mcp_message = strings::MCP_TESTING.into();
                        cx.notify();
                        state.runtime.spawn(async move {
                            tingskriver_core::mcp_probe::test_connection(&exe, &dir).await
                        })
                    });
                    let model = model.clone();
                    cx.spawn(async move |cx| {
                        let result = task.await;
                        model.update(cx, |state, cx| {
                            state.mcp_pending = false;
                            state.mcp_message = match result {
                                Ok(Ok(())) => strings::MCP_TEST_OK.into(),
                                Ok(Err(error)) => error,
                                Err(_) => strings::MCP_FAILED.into(),
                            };
                            cx.notify();
                        });
                    })
                    .detach();
                }),
        );
        body = body.child(section_title(strings::MCP_TOOLS));
        for tool in tingskriver_core::mcp_audit::TOOLS {
            body = body.child(paragraph(tool, cx));
        }
        body = body.child(paragraph(strings::MCP_LOG, cx));
        let mut buttons = h_flex().gap(space::SM);
        for (index, (label, clear)) in [
            (strings::MCP_LOG_REFRESH, false),
            (strings::MCP_LOG_CLEAR, true),
        ]
        .into_iter()
        .enumerate()
        {
            let model = self.model.clone();
            let exe = exe.clone();
            buttons = buttons.child(
                controls::button(("mcp-log", index), Variant::Outline)
                    .label(label)
                    .disabled(state.mcp_pending || state.app_data_dir.is_none())
                    .on_click(move |_, _, cx| {
                        let task = model.update(cx, |state, cx| {
                            let dir = state
                                .app_data_dir
                                .clone()
                                .expect("button requires data directory");
                            state.mcp_pending = true;
                            cx.notify();
                            let exe = exe.clone();
                            state.runtime.spawn(async move {
                                if clear {
                                    tingskriver_core::mcp_audit::clear(&dir).await?;
                                }
                                let entries = tingskriver_core::mcp_audit::recent(&dir).await?;
                                let running = tokio::task::spawn_blocking(move || {
                                    exe.map(|exe| {
                                        tingskriver_core::mcp_probe::running_servers(&exe)
                                    })
                                    .unwrap_or(0)
                                })
                                .await
                                .unwrap_or(0);
                                Ok::<_, String>((entries, running))
                            })
                        });
                        let model = model.clone();
                        cx.spawn(async move |cx| {
                            let result = task
                                .await
                                .unwrap_or_else(|_| Err(strings::MCP_FAILED.into()));
                            model.update(cx, |state, cx| {
                                state.mcp_pending = false;
                                if clear && result.is_ok() {
                                    state.mcp_message = strings::MCP_LOG_CLEARED.into();
                                }
                                state.mcp_log = Some(result.map(|(entries, running)| {
                                    state.mcp_running = running;
                                    entries
                                }));
                                cx.notify();
                            });
                        })
                        .detach();
                    }),
            );
        }
        body = body.child(buttons);
        match &state.mcp_log {
            Some(Err(error)) => body = body.child(paragraph(error, cx)),
            Some(Ok(entries)) if !entries.is_empty() => {
                for entry in entries {
                    let text = strings::MCP_LOG_ENTRY
                        .replace("{time}", &entry.time)
                        .replace("{tool}", &entry.tool)
                        .replace(
                            "{id}",
                            entry.meeting_id.as_deref().unwrap_or(strings::MCP_NO_ID),
                        )
                        .replace(
                            "{result}",
                            if entry.success {
                                strings::MCP_OK
                            } else {
                                strings::MCP_FAILED
                            },
                        );
                    body = body.child(paragraph(&text, cx));
                }
            }
            _ => body = body.child(paragraph(strings::MCP_LOG_EMPTY, cx)),
        }
        body
    }

    fn language_group(&self, cx: &mut Context<Self>) -> impl IntoElement {
        use tingskriver_core::meeting_language::{label, CHOICES};
        let model = self.model.read(cx);
        let languages = model.languages.clone();
        let error = model.language_error.clone();
        let primary_buttons = CHOICES.iter().enumerate().map(|(at, (code, _))| {
            let code = *code;
            let model = self.model.clone();
            controls::button(("language-primary", at), Variant::Outline)
                .label(label(code))
                .selected(languages.primary == code)
                .on_click(move |_, _, cx| {
                    model.update(cx, |m, cx| {
                        m.change_languages(cx, |l| l.with_primary(code));
                        cx.notify();
                    })
                })
        });
        let extra_buttons = CHOICES
            .iter()
            .enumerate()
            .filter(|(_, (code, _))| *code != languages.primary)
            .map(|(at, (code, _))| {
                let code = *code;
                let model = self.model.clone();
                controls::button(("language-extra", at), Variant::Outline)
                    .label(label(code))
                    .selected(languages.extra.iter().any(|c| c == code))
                    .on_click(move |_, _, cx| {
                        model.update(cx, |m, cx| {
                            m.change_languages(cx, |l| l.with_extra_toggled(code));
                            cx.notify();
                        })
                    })
            });
        let summary_model = self.model.clone();
        let summary_button = controls::button("language-summary-primary", Variant::Outline)
            .label(if languages.summary_in_primary {
                strings::LANGUAGE_SUMMARY_ON
            } else {
                strings::LANGUAGE_SUMMARY_OFF
            })
            .selected(languages.summary_in_primary)
            .on_click(move |_, _, cx| {
                summary_model.update(cx, |m, cx| {
                    m.change_languages(cx, |mut l| {
                        l.summary_in_primary = !l.summary_in_primary;
                        Ok(l)
                    });
                    cx.notify();
                })
            });
        // The selection is also written out, so it does not rest on the
        // button highlight alone (screen readers, high contrast).
        let current = {
            let extra: Vec<String> = languages.extra.iter().map(|c| label(c)).collect();
            let extra = if extra.is_empty() {
                strings::LANGUAGE_NO_EXTRA.to_string()
            } else {
                extra.join(", ")
            };
            strings::language_current(&format!(
                "{}, {}",
                label(&languages.primary),
                strings::language_extra_list(&extra)
            ))
        };
        v_flex()
            .gap(space::SM)
            .p(space::LG)
            .rounded(theme::RADIUS_LG)
            .border_1()
            .border_color(palette(cx).border)
            .bg(palette(cx).card)
            .child(section_title(strings::LANGUAGES))
            .child(paragraph(&current, cx))
            .child(muted(strings::LANGUAGE_PRIMARY, cx))
            .child(muted(strings::LANGUAGE_PRIMARY_HELP, cx))
            .child(
                h_flex()
                    .gap(space::SM)
                    .flex_wrap()
                    .children(primary_buttons),
            )
            .child(muted(strings::LANGUAGE_EXTRA, cx))
            .child(muted(strings::LANGUAGE_EXTRA_HELP, cx))
            .child(h_flex().gap(space::SM).flex_wrap().children(extra_buttons))
            .child(summary_button)
            .child(muted(strings::LANGUAGE_PROVIDERS, cx))
            .child(muted(strings::LANGUAGE_NEW_MEETINGS, cx))
            .children(error.map(|e| paragraph(&e, cx)))
    }
}

/// The voiceprint explanation, Danish first and English below on the same
/// page. Voiceprints are off by default and there is no switch yet, so the
/// status line says off.
fn voiceprint_lines() -> Vec<String> {
    use tingskriver_core::voiceprint::texts;
    let paragraphs = texts::explanation();
    let mut lines = vec![strings::VOICEPRINT_STATUS_OFF.to_string()];
    lines.extend(paragraphs.iter().map(|p| p.da.to_string()));
    lines.push(texts::EN_TITLE.to_string());
    lines.push(texts::EN_STATUS_OFF.to_string());
    lines.extend(paragraphs.iter().map(|p| p.en.to_string()));
    lines
}

fn save_mcp_controls(
    model: Entity<AppModel>,
    value: tingskriver_core::mcp::Settings,
    cx: &mut App,
) {
    let state = model.read(cx);
    let DbState::Ready(db) = &state.db else {
        return;
    };
    let pool = db.pool().clone();
    let task = state
        .runtime
        .spawn(async move { tingskriver_core::mcp::save_settings(&pool, value).await });
    model.update(cx, |state, cx| {
        state.mcp_pending = true;
        cx.notify();
    });
    cx.spawn(async move |cx| {
        let result = task.await;
        model.update(cx, |state, cx| {
            state.mcp_pending = false;
            if matches!(result, Ok(Ok(()))) {
                state.mcp_controls = Some(value);
                state.mcp_message = strings::MCP_SAVED.into();
            } else {
                state.mcp_message = strings::MCP_SAVE_FAILED.into();
            }
            cx.notify();
        });
    })
    .detach();
}
