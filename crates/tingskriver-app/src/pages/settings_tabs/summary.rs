//! Indstillinger > Opsummering, as in the Tauri app
//! (`components/SummaryModelSettings.tsx:125-148`,
//! `SummaryLanguageSettings.tsx`, the provider part of
//! `ModelSettingsModal.tsx`): three cards.
//!
//! * "Automatisk opsummering": make the summary when a recording has been
//!   transcribed.
//! * "Opsummeringssprog": up to five quick-pick languages, one of them
//!   pinned as the default for new meetings.
//! * "Model til opsummering": provider, model (the list comes from core), the
//!   provider's key, the Ollama address and the own server. "Gem" writes the
//!   same database rows Tauri writes, so existing choices are kept.
//!
//! The auto switch and the quick picks were Tauri `localStorage`, which only
//! its webview can read, so they live in `summary_preferences.json` in the
//! app data folder (`meeting_summary::Preferences`). Existing Tauri users
//! start from the defaults for those two.

use gpui::{
    div, prelude::*, AnyElement, AnyWindowHandle, App, Context, Entity, FontWeight, SharedString,
    Subscription, Window,
};
use gpui_component::{
    button::Button,
    h_flex,
    input::{InputEvent, InputState},
    menu::{DropdownMenu, PopupMenuItem},
    notification::Notification,
    select::{Select, SelectEvent, SelectState},
    v_flex, Disableable, IndexPath, WindowExt,
};
use gpui_kit_assets::IconName;
use tingskriver_core::meeting_summary::{
    self as core, CustomServer, ModelChoice, ModelList, ModelOption, Preferences,
};
use tingskriver_core::strings::meeting_summary as mtext;
use tingskriver_core::strings::settings_tabs as text;

use super::{card_title, settings_card};
use crate::controls::{self, Variant};
use crate::pages::settings::{alert, field_label, tab_column};
use crate::startup::DbState;
use crate::theme::{space, typography};
use crate::ui::{muted, Shell};

type Choices = SelectState<Vec<SharedString>>;

/// The model list of the chosen provider.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
enum Models {
    #[default]
    Idle,
    Loading,
    Ready(Vec<ModelOption>),
    NeedsKey,
    Failed(String),
}

/// What the tab shows, read together.
struct Loaded {
    choice: Option<ModelChoice>,
    preferences: Preferences,
    /// The preferences file could not be read; the switch is shown off and
    /// a change is not saved over the unreadable file by accident.
    preferences_unreadable: bool,
    has_key: bool,
    custom: CustomServer,
}

struct Fields {
    provider: Entity<Choices>,
    model: Entity<Choices>,
    endpoint: Entity<InputState>,
    key: Entity<InputState>,
    custom_endpoint: Entity<InputState>,
    custom_model: Entity<InputState>,
    custom_key: Entity<InputState>,
}

/// Per-window state of the Opsummering tab (part of `SettingsView`).
pub(crate) struct SummaryTab {
    window: AnyWindowHandle,
    stale: bool,
    loading: bool,
    frozen: bool,
    data: Option<Loaded>,
    load_error: Option<String>,
    /// The provider shown in the first select (not yet saved).
    provider: Option<String>,
    /// The model shown in the second select (not yet saved).
    model: Option<String>,
    models: Models,
    /// Counts list reads, so only the newest answer is used.
    list_reads: u64,
    busy: bool,
    fields: Fields,
    _subscriptions: Vec<Subscription>,
}

fn provider_labels() -> Vec<SharedString> {
    core::PROVIDERS
        .iter()
        .map(|id| SharedString::from(text::summary_provider(id)))
        .collect()
}

impl SummaryTab {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Shell>) -> Self {
        let mut input = |placeholder: Option<&'static str>, masked: bool| {
            cx.new(|cx| {
                let state = InputState::new(window, cx).masked(masked);
                match placeholder {
                    Some(text) => state.placeholder(text),
                    None => state,
                }
            })
        };
        let endpoint = input(Some("http://localhost:11434"), false);
        let key = input(Some(mtext::SUMMARY_KEY_PLACEHOLDER), true);
        let custom_endpoint = input(Some(mtext::SUMMARY_CUSTOM_ENDPOINT_PLACEHOLDER), false);
        let custom_model = input(None, false);
        let custom_key = input(Some(mtext::SUMMARY_KEY_PLACEHOLDER), true);
        let provider = cx.new(|cx| SelectState::new(provider_labels(), None, window, cx));
        let model = cx.new(|cx| SelectState::new(Vec::<SharedString>::new(), None, window, cx).searchable(true));
        let fields = Fields {
            provider,
            model,
            endpoint,
            key,
            custom_endpoint,
            custom_model,
            custom_key,
        };
        let mut subscriptions = Vec::new();
        for input in [
            &fields.endpoint,
            &fields.key,
            &fields.custom_endpoint,
            &fields.custom_model,
            &fields.custom_key,
        ] {
            subscriptions.push(cx.subscribe_in(
                input,
                window,
                |shell: &mut Shell, state, event: &InputEvent, _, cx| match event {
                    InputEvent::Change => cx.notify(),
                    // The Ollama address is applied when the field loses focus,
                    // as Tauri reads the list again for the new address.
                    InputEvent::Blur if *state == shell.settings.summary.fields.endpoint => {
                        shell.reload_models(cx)
                    }
                    _ => {}
                },
            ));
        }
        subscriptions.push(cx.subscribe_in(
            &fields.provider,
            window,
            |shell: &mut Shell, _, event: &SelectEvent<Vec<SharedString>>, window, cx| {
                if let SelectEvent::Confirm(Some(label)) = event {
                    let id = core::PROVIDERS
                        .iter()
                        .find(|id| text::summary_provider(id) == label.as_ref());
                    if let Some(id) = id {
                        shell.provider_chosen((*id).to_string(), window, cx);
                    }
                }
            },
        ));
        subscriptions.push(cx.subscribe_in(
            &fields.model,
            window,
            |shell: &mut Shell, _, event: &SelectEvent<Vec<SharedString>>, _, cx| {
                if let SelectEvent::Confirm(Some(label)) = event {
                    shell.model_chosen(label.to_string(), cx);
                }
            },
        ));
        SummaryTab {
            window: window.window_handle(),
            stale: true,
            loading: false,
            frozen: false,
            data: None,
            load_error: None,
            provider: None,
            model: None,
            models: Models::Idle,
            list_reads: 0,
            busy: false,
            fields,
            _subscriptions: subscriptions,
        }
    }

    /// The tab was opened: read again, as Tauri reads on mount.
    pub(crate) fn shown(&mut self) {
        self.stale = true;
    }

    /// The label of the chosen model: the list's label when it has one.
    fn model_label(&self) -> Option<String> {
        let model = self.model.as_deref()?;
        match &self.models {
            Models::Ready(list) => list
                .iter()
                .find(|option| option.id == model)
                .map(|option| option.label.clone()),
            _ => None,
        }
        .or_else(|| Some(model.to_string()))
    }

    /// The choice the selects show differs from the saved one.
    fn is_dirty(&self) -> bool {
        let Some(data) = &self.data else {
            return false;
        };
        let shown = (self.provider.as_deref(), self.model.as_deref());
        let saved = data
            .choice
            .as_ref()
            .map(|c| (Some(c.provider.as_str()), Some(c.model.as_str())))
            .unwrap_or((None, None));
        shown != saved
    }

    /// "Gem" can be pressed: a provider, and a model (or the own server's
    /// address and model).
    fn can_save(&self, cx: &App) -> bool {
        let Some(provider) = self.provider.as_deref() else {
            return false;
        };
        if self.busy {
            return false;
        }
        if provider == "custom-openai" {
            let f = &self.fields;
            return !f.custom_endpoint.read(cx).value().trim().is_empty()
                && !f.custom_model.read(cx).value().trim().is_empty();
        }
        self.model.as_deref().is_some_and(|m| !m.is_empty()) && self.is_dirty()
    }
}

fn notify_error(title: &str, error: &str, window: &mut Window, cx: &mut App) {
    window.push_notification(
        Notification::error(error.to_string()).title(title.to_string()),
        cx,
    );
}

impl Shell {
    /// Read the saved choices if the tab is stale.
    fn load_summary_settings(&mut self, window: &mut Window, cx: &mut Context<Shell>) {
        let tab = &mut self.settings.summary;
        if tab.frozen || tab.loading || !tab.stale {
            return;
        }
        let state = self.model.read(cx);
        let DbState::Ready(db) = &state.db else {
            return;
        };
        let (pool, dir) = (db.pool().clone(), state.app_data_dir.clone());
        tab.stale = false;
        tab.loading = true;
        let task = state.runtime.spawn(async move {
            let choice = core::read_model(&pool).await?;
            let preferences = match dir {
                Some(dir) => Preferences::load_async(dir).await,
                None => Ok(Preferences::default()),
            };
            let provider = choice.as_ref().map(|c| c.provider.clone());
            let has_key = match provider.as_deref() {
                Some(provider) if core::needs_key(provider) => {
                    core::has_key(&pool, provider).await?
                }
                _ => false,
            };
            let custom = core::read_custom_server(&pool).await?;
            Ok::<_, String>((choice, preferences, has_key, custom))
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await.unwrap_or_else(|error| Err(error.to_string()));
            let _ = this.update_in(cx, |shell, window, cx| {
                let tab = &mut shell.settings.summary;
                tab.loading = false;
                match result {
                    Ok((choice, preferences, has_key, custom)) => {
                        let unreadable = preferences.is_err();
                        if let Err(error) = &preferences {
                            log::warn!("The summary preferences could not be read: {error}");
                        }
                        tab.load_error = None;
                        tab.provider = choice.as_ref().map(|c| c.provider.clone());
                        tab.model = choice
                            .as_ref()
                            .map(|c| c.model.clone())
                            .filter(|m| !m.is_empty());
                        let endpoint = choice
                            .as_ref()
                            .and_then(|c| c.ollama_endpoint.clone())
                            .unwrap_or_default();
                        let provider_row = tab
                            .provider
                            .as_deref()
                            .and_then(|p| core::PROVIDERS.iter().position(|id| *id == p));
                        tab.fields.provider.update(cx, |select, cx| {
                            select.set_selected_index(provider_row.map(IndexPath::new), window, cx)
                        });
                        tab.fields
                            .endpoint
                            .update(cx, |input, cx| input.set_value(endpoint, window, cx));
                        tab.fields.custom_endpoint.update(cx, |input, cx| {
                            input.set_value(custom.endpoint.clone(), window, cx)
                        });
                        tab.fields.custom_model.update(cx, |input, cx| {
                            input.set_value(custom.model.clone(), window, cx)
                        });
                        tab.data = Some(Loaded {
                            choice,
                            preferences: preferences.unwrap_or_default(),
                            preferences_unreadable: unreadable,
                            has_key,
                            custom,
                        });
                        shell.reload_models(cx);
                    }
                    Err(error) => {
                        log::warn!("The summary settings could not be read: {error}");
                        tab.load_error = Some(error);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Read the model list of the shown provider.
    fn reload_models(&mut self, cx: &mut Context<Shell>) {
        let state = self.model.read(cx);
        let (DbState::Ready(db), Some(host)) = (&state.db, state.host.clone()) else {
            return;
        };
        let tab = &mut self.settings.summary;
        let Some(provider) = tab.provider.clone() else {
            tab.models = Models::Idle;
            return;
        };
        if provider == "custom-openai" {
            tab.models = Models::Idle;
            return;
        }
        let endpoint = Some(tab.fields.endpoint.read(cx).value().trim().to_string())
            .filter(|endpoint| !endpoint.is_empty());
        tab.models = Models::Loading;
        tab.list_reads += 1;
        let read = tab.list_reads;
        let pool = db.pool().clone();
        let task = state
            .runtime
            .spawn(async move { core::model_list(host, &pool, &provider, endpoint).await });
        let window = tab.window;
        cx.spawn(async move |this, cx| {
            let list = task.await;
            let _ = window.update(cx, |_, window, cx| {
                let _ = this.update(cx, |shell, cx| {
                    let tab = &mut shell.settings.summary;
                    if tab.list_reads != read {
                        return;
                    }
                    tab.models = match list {
                        Ok(ModelList::Models(list)) => Models::Ready(list),
                        Ok(ModelList::NeedsKey) => Models::NeedsKey,
                        Ok(ModelList::Failed(error)) => {
                            log::warn!("The model list could not be read: {error}");
                            Models::Failed(error)
                        }
                        Err(error) => Models::Failed(error.to_string()),
                    };
                    shell.apply_models(window, cx);
                    cx.notify();
                });
            });
        })
        .detach();
    }

    /// Put the list in the model select and select the chosen model.
    fn apply_models(&mut self, window: &mut Window, cx: &mut Context<Shell>) {
        let tab = &self.settings.summary;
        let (labels, row) = match &tab.models {
            Models::Ready(list) => {
                let labels: Vec<SharedString> = list
                    .iter()
                    .map(|option| SharedString::from(option.label.clone()))
                    .collect();
                let row = tab
                    .model
                    .as_deref()
                    .and_then(|m| list.iter().position(|option| option.id == m));
                (labels, row)
            }
            _ => (Vec::new(), None),
        };
        tab.fields.model.update(cx, |select, cx| {
            select.set_items(labels, window, cx);
            select.set_selected_index(row.map(IndexPath::new), window, cx);
        });
    }

    fn provider_chosen(&mut self, provider: String, window: &mut Window, cx: &mut Context<Shell>) {
        let tab = &mut self.settings.summary;
        if tab.provider.as_deref() == Some(provider.as_str()) {
            return;
        }
        // Tauri keeps the model of the provider the choice came back to; a
        // new provider starts without one.
        let saved = tab
            .data
            .as_ref()
            .and_then(|d| d.choice.as_ref())
            .filter(|c| c.provider == provider)
            .map(|c| c.model.clone())
            .filter(|m| !m.is_empty());
        tab.provider = Some(provider.clone());
        tab.model = saved;
        let needs_key = core::needs_key(&provider);
        // Does the new provider have a key stored?
        if needs_key {
            self.refresh_key_state(cx);
        }
        self.reload_models(cx);
        self.apply_models(window, cx);
        cx.notify();
    }

    fn model_chosen(&mut self, label: String, cx: &mut Context<Shell>) {
        let tab = &mut self.settings.summary;
        if let Models::Ready(list) = &tab.models {
            if let Some(option) = list.iter().find(|option| option.label == label) {
                tab.model = Some(option.id.clone());
            }
        }
        cx.notify();
    }

    /// Whether a key is stored for the shown provider.
    fn refresh_key_state(&mut self, cx: &mut Context<Shell>) {
        let state = self.model.read(cx);
        let DbState::Ready(db) = &state.db else {
            return;
        };
        let Some(provider) = self.settings.summary.provider.clone() else {
            return;
        };
        let pool = db.pool().clone();
        let task = state
            .runtime
            .spawn(async move { core::has_key(&pool, &provider).await });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |shell, cx| {
                if let (Some(data), Ok(Ok(has_key))) = (shell.settings.summary.data.as_mut(), result)
                {
                    data.has_key = has_key;
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// The switch "Automatisk opsummering".
    fn set_auto_summary(&mut self, on: bool, window: &mut Window, cx: &mut Context<Shell>) {
        let tab = &mut self.settings.summary;
        let Some(data) = tab.data.as_mut() else {
            return;
        };
        if data.preferences_unreadable {
            // Do not write over a file that could not be read.
            return;
        }
        let Some(dir) = self.model.read(cx).app_data_dir.clone() else {
            return;
        };
        let mut next = data.preferences.clone();
        next.auto_summary = on;
        self.save_preferences(next, dir, window, cx);
    }

    /// Write the preferences; the screen shows them when the write is done.
    fn save_preferences(
        &mut self,
        next: Preferences,
        dir: std::path::PathBuf,
        window: &mut Window,
        cx: &mut Context<Shell>,
    ) {
        let _ = window;
        let task = self
            .model
            .read(cx)
            .runtime
            .spawn({
                let next = next.clone();
                async move { next.save_async(dir).await }
            });
        let handle = self.settings.summary.window;
        cx.spawn(async move |this, cx| {
            let result = task.await.unwrap_or_else(|error| Err(error.to_string()));
            let _ = handle.update(cx, |_, window, cx| {
                let _ = this.update(cx, |shell, cx| {
                    match result {
                        Ok(()) => {
                            if let Some(data) = shell.settings.summary.data.as_mut() {
                                data.preferences = next.clone();
                            }
                        }
                        Err(error) => {
                            log::warn!("The summary preferences could not be saved: {error}");
                            notify_error(mtext::SUMMARY_AUTO_SAVE_FAILED, &error, window, cx);
                        }
                    }
                    cx.notify();
                });
            });
        })
        .detach();
    }

    /// "Gem": the provider and the model.
    fn save_summary_model(&mut self, window: &mut Window, cx: &mut Context<Shell>) {
        let state = self.model.read(cx);
        let DbState::Ready(db) = &state.db else {
            return;
        };
        let tab = &mut self.settings.summary;
        let Some(provider) = tab.provider.clone() else {
            return;
        };
        if tab.busy {
            return;
        }
        tab.busy = true;
        let f = &tab.fields;
        let custom = (
            f.custom_endpoint.read(cx).value().to_string(),
            f.custom_model.read(cx).value().to_string(),
            f.custom_key.read(cx).value().to_string(),
        );
        let endpoint = Some(f.endpoint.read(cx).value().trim().to_string())
            .filter(|endpoint| !endpoint.is_empty());
        let model = tab.model.clone().unwrap_or_default();
        let pool = db.pool().clone();
        let task = state.runtime.spawn(async move {
            let model = if provider == "custom-openai" {
                core::save_custom_server(&pool, &custom.0, &custom.1, &custom.2).await?;
                custom.1.trim().to_string()
            } else {
                model
            };
            let choice = ModelChoice {
                provider,
                model,
                ollama_endpoint: endpoint,
            };
            core::save_model(&pool, &choice).await?;
            let server = core::read_custom_server(&pool).await?;
            Ok::<_, String>((choice, server))
        });
        let handle = tab.window;
        cx.spawn(async move |this, cx| {
            let result = task.await.unwrap_or_else(|error| Err(error.to_string()));
            let _ = handle.update(cx, |_, window, cx| {
                let _ = this.update(cx, |shell, cx| {
                    let tab = &mut shell.settings.summary;
                    tab.busy = false;
                    match result {
                        Ok((choice, server)) => {
                            tab.model = Some(choice.model.clone()).filter(|m| !m.is_empty());
                            if let Some(data) = tab.data.as_mut() {
                                data.choice = Some(choice);
                                data.custom = server;
                            }
                            tab.fields
                                .custom_key
                                .update(cx, |input, cx| input.set_value("", window, cx));
                            window.push_notification(
                                Notification::success(mtext::SUMMARY_SETTINGS_SAVED),
                                cx,
                            );
                        }
                        Err(error) => {
                            log::warn!("The summary model could not be saved: {error}");
                            notify_error(mtext::SUMMARY_SETTINGS_SAVE_FAILED, &error, window, cx);
                        }
                    }
                    cx.notify();
                });
            });
        })
        .detach();
        cx.notify();
        let _ = window;
    }

    /// "Gem nøgle": the key of the shown provider.
    fn save_summary_key(&mut self, window: &mut Window, cx: &mut Context<Shell>) {
        let state = self.model.read(cx);
        let DbState::Ready(db) = &state.db else {
            return;
        };
        let tab = &mut self.settings.summary;
        let Some(provider) = tab.provider.clone() else {
            return;
        };
        let key = tab.fields.key.read(cx).value().to_string();
        if key.trim().is_empty() || tab.busy {
            return;
        }
        tab.busy = true;
        let pool = db.pool().clone();
        let task = state
            .runtime
            .spawn(async move { core::save_key(&pool, &provider, &key).await });
        let handle = tab.window;
        cx.spawn(async move |this, cx| {
            let result = task.await.unwrap_or_else(|error| Err(error.to_string()));
            let _ = handle.update(cx, |_, window, cx| {
                let _ = this.update(cx, |shell, cx| {
                    shell.settings.summary.busy = false;
                    match result {
                        Ok(()) => {
                            // The key is not kept in the window.
                            shell
                                .settings
                                .summary
                                .fields
                                .key
                                .update(cx, |input, cx| input.set_value("", window, cx));
                            if let Some(data) = shell.settings.summary.data.as_mut() {
                                data.has_key = true;
                            }
                            window.push_notification(
                                Notification::success(mtext::SUMMARY_KEY_SAVED_TOAST),
                                cx,
                            );
                            shell.reload_models(cx);
                        }
                        Err(error) => {
                            log::warn!("The key could not be saved: {error}");
                            notify_error(mtext::SUMMARY_KEY_SAVE_FAILED, &error, window, cx);
                        }
                    }
                    cx.notify();
                });
            });
        })
        .detach();
        cx.notify();
        let _ = window;
    }

    /// Pin, unpin, add or remove a quick-pick language.
    fn change_languages(
        &mut self,
        change: impl FnOnce(&mut Preferences),
        window: &mut Window,
        cx: &mut Context<Shell>,
    ) {
        let Some(data) = self.settings.summary.data.as_ref() else {
            return;
        };
        if data.preferences_unreadable {
            return;
        }
        let Some(dir) = self.model.read(cx).app_data_dir.clone() else {
            return;
        };
        let mut next = data.preferences.clone();
        change(&mut next);
        self.save_preferences(next, dir, window, cx);
    }

    pub(crate) fn summary_settings_tab(&self, cx: &mut Context<Self>) -> AnyElement {
        let tab = &self.settings.summary;
        if tab.stale && !tab.loading && !tab.frozen {
            // Drawing cannot start work that needs the window.
            let shell = cx.entity().downgrade();
            let handle = tab.window;
            cx.defer(move |cx| {
                let _ = handle.update(cx, |_, window, cx| {
                    let _ = shell.update(cx, |shell, cx| shell.load_summary_settings(window, cx));
                });
            });
        }
        if let Some(error) = &tab.load_error {
            return tab_column(vec![alert(
                "summary-settings-error",
                &format!("{} {error}", mtext::SUMMARY_SETTINGS_UNREADABLE),
                cx,
            )]);
        }
        let loaded = tab.data.as_ref();
        let unreadable = loaded.is_some_and(|d| d.preferences_unreadable);
        let preferences = loaded.map(|d| d.preferences.clone()).unwrap_or_default();

        let note = |id: &'static str, why: &str| {
            div()
                .id(id)
                .role(gpui::Role::Alert)
                .mt(space::MD)
                .child(alert_text(why, cx))
                .into_any_element()
        };

        // ---- automatic summary ----
        let auto_switch = {
            let shell = cx.entity().downgrade();
            controls::switch("summary-auto-switch")
                .checked(preferences.auto_summary)
                .disabled(loaded.is_none() || unreadable)
                .accessibility_label(text::SUMMARY_AUTO)
                .on_click(move |on, window, cx| {
                    let on = *on;
                    let _ = shell.update(cx, |shell, cx| shell.set_auto_summary(on, window, cx));
                })
        };
        let mut auto_children = vec![h_flex()
            .justify_between()
            .gap(space::XL)
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(space::SM)
                    .child(card_title(text::SUMMARY_AUTO, None, cx))
                    .child(muted(text::SUMMARY_AUTO_HELP, cx)),
            )
            .child(div().flex_none().child(auto_switch))
            .into_any_element()];
        if unreadable {
            auto_children.push(note("summary-auto-unreadable", mtext::SUMMARY_AUTO_UNREADABLE));
        }
        let auto = settings_card("summary-auto", auto_children, cx);

        // ---- languages ----
        let shell = cx.entity().downgrade();
        let chips = preferences
            .language_recents
            .iter()
            .map(|code| language_chip(code, preferences.language_default.as_deref(), &shell, cx))
            .collect::<Vec<_>>();
        let add_shell = shell.clone();
        let recents = preferences.language_recents.clone();
        let add = controls::small_button("summary-language-add", Variant::Outline)
            .label(format!("＋ {}", mtext::SUMMARY_ADD_LANGUAGE))
            .disabled(preferences.language_recents.len() >= core::MAX_RECENT_LANGUAGES || unreadable)
            .dropdown_menu(move |menu, _, _| {
                let mut menu = menu.max_h(crate::theme::document::MENU_MAX).scrollable(true);
                for (code, label) in core::LANGUAGES {
                    if recents.iter().any(|recent| recent == code) {
                        continue;
                    }
                    let (shell, code) = (add_shell.clone(), (*code).to_string());
                    menu = menu.item(PopupMenuItem::new(*label).on_click(move |_, window, cx| {
                        let code = code.clone();
                        let _ = shell.update(cx, |shell, cx| {
                            shell.change_languages(|p| p.add_recent(&code), window, cx)
                        });
                    }));
                }
                menu
            });
        let line = match preferences.language_default.as_deref() {
            Some(code) => mtext::pinned_standard(&core::language_label(code)),
            None => mtext::SUMMARY_PINNED_NONE.to_string(),
        };
        let mut language_children = vec![
            card_title(text::SUMMARY_LANGUAGE, Some(IconName::Globe), cx),
            div()
                .mt(space::SM)
                .child(muted(text::SUMMARY_LANGUAGE_HELP, cx))
                .into_any_element(),
            h_flex()
                .mt(space::MD)
                .flex_wrap()
                .items_center()
                .gap(space::SM)
                .children(chips)
                .child(add)
                .into_any_element(),
            div()
                .mt(space::MD)
                .child(muted(&line, cx))
                .into_any_element(),
        ];
        if unreadable {
            language_children.push(note(
                "summary-language-unreadable",
                mtext::SUMMARY_LANGUAGES_UNREADABLE,
            ));
        }
        let language = settings_card("summary-language", language_children, cx);

        // ---- model ----
        let model_card = settings_card("summary-model", self.model_card_children(cx), cx);

        tab_column(vec![v_flex()
            .id("settings-summary")
            .role(gpui::Role::Group)
            .gap(space::LG)
            .child(auto)
            .child(language)
            .child(model_card)
            .into_any_element()])
    }

    fn model_card_children(&self, cx: &mut Context<Shell>) -> Vec<AnyElement> {
        let tab = &self.settings.summary;
        let f = &tab.fields;
        let provider = tab.provider.clone();
        let own = provider.as_deref() == Some("custom-openai");
        let builtin = provider.as_deref() == Some("builtin-ai");
        let ollama = provider.as_deref() == Some("ollama");
        let keyed = provider.as_deref().is_some_and(core::needs_key);
        let has_key = tab.data.as_ref().is_some_and(|d| d.has_key);
        let shell = cx.entity().downgrade();

        let select = |state: &Entity<Choices>, label: &str, placeholder: &str, disabled: bool| {
            Select::new(state)
                .accessibility_label(SharedString::from(label.to_string()))
                .placeholder(SharedString::from(placeholder.to_string()))
                .disabled(disabled)
                .h(crate::theme::layout::CONTROL_HEIGHT)
        };
        let mut body = v_flex()
            .gap(space::SM)
            .child(
                div()
                    .text_size(typography::SM)
                    .child(field_label(text::SUMMARY_MODEL)),
            )
            .child(muted(text::SUMMARY_MODEL_PICK, cx))
            .child(select(
                &f.provider,
                text::SUMMARY_PROVIDER,
                text::SUMMARY_PROVIDER_PLACEHOLDER,
                tab.data.is_none() || tab.busy,
            ));

        if own {
            let labelled = |label: &'static str, control: AnyElement| {
                v_flex()
                    .gap(space::XS)
                    .child(
                        div()
                            .text_size(typography::SM)
                            .font_weight(FontWeight::MEDIUM)
                            .child(label),
                    )
                    .child(control)
                    .into_any_element()
            };
            let server_has_key = tab.data.as_ref().is_some_and(|d| d.custom.has_key);
            body = body
                .child(labelled(
                    mtext::SUMMARY_CUSTOM_ENDPOINT,
                    controls::input(&f.custom_endpoint)
                        .aria_label(mtext::SUMMARY_CUSTOM_ENDPOINT)
                        .into_any_element(),
                ))
                .child(labelled(
                    mtext::SUMMARY_CUSTOM_MODEL,
                    controls::input(&f.custom_model)
                        .aria_label(mtext::SUMMARY_CUSTOM_MODEL)
                        .into_any_element(),
                ))
                .child(labelled(
                    mtext::SUMMARY_KEY,
                    controls::input(&f.custom_key)
                        .aria_label(mtext::SUMMARY_KEY)
                        .into_any_element(),
                ))
                .child(muted(
                    if server_has_key {
                        mtext::SUMMARY_KEY_SAVED
                    } else {
                        mtext::SUMMARY_KEY_MISSING
                    },
                    cx,
                ));
        } else if provider.is_some() {
            body = body.child(select(
                &f.model,
                text::SUMMARY_MODEL,
                text::SUMMARY_MODEL_PLACEHOLDER,
                matches!(tab.models, Models::Loading | Models::NeedsKey),
            ));
            match &tab.models {
                Models::Loading => body = body.child(muted(mtext::SUMMARY_MODELS_LOADING, cx)),
                Models::NeedsKey => body = body.child(muted(mtext::SUMMARY_MODELS_KEY_NEEDED, cx)),
                Models::Ready(list) if list.is_empty() => {
                    body = body.child(muted(mtext::SUMMARY_MODELS_NONE, cx))
                }
                Models::Failed(error) => {
                    body = body.child(alert(
                        "summary-models-error",
                        &format!("{} {error}", mtext::SUMMARY_MODELS_FAILED),
                        cx,
                    ))
                }
                _ => {}
            }
            if builtin {
                body = body.child(muted(mtext::SUMMARY_BUILTIN_HELP, cx));
            }
            if let (Some(label), Models::Failed(_) | Models::Idle) = (tab.model_label(), &tab.models)
            {
                body = body.child(muted(&text::transcription_model_current(&label), cx));
            }
        }
        if ollama {
            body = body
                .child(
                    div()
                        .mt(space::SM)
                        .text_size(typography::SM)
                        .font_weight(FontWeight::MEDIUM)
                        .child(mtext::SUMMARY_OLLAMA_ENDPOINT),
                )
                .child(
                    controls::input(&f.endpoint)
                        .aria_label(mtext::SUMMARY_OLLAMA_ENDPOINT),
                )
                .child(muted(mtext::SUMMARY_OLLAMA_ENDPOINT_HELP, cx));
        }
        if keyed {
            let save_shell = shell.clone();
            let ready = !f.key.read(cx).value().trim().is_empty() && !tab.busy;
            body = body
                .child(
                    div()
                        .mt(space::SM)
                        .text_size(typography::SM)
                        .font_weight(FontWeight::MEDIUM)
                        .child(mtext::SUMMARY_KEY),
                )
                .child(controls::input(&f.key).aria_label(mtext::SUMMARY_KEY))
                .child(muted(
                    if has_key {
                        mtext::SUMMARY_KEY_SAVED
                    } else {
                        mtext::SUMMARY_KEY_MISSING
                    },
                    cx,
                ))
                .child(
                    save_button("summary-key-save", mtext::SUMMARY_KEY_SAVE, Variant::Outline, ready)
                        .on_click(move |_, window, cx| {
                            let _ = save_shell
                                .update(cx, |shell, cx| shell.save_summary_key(window, cx));
                        }),
                );
        }
        let can_save = tab.can_save(cx);
        let save_shell = shell;
        vec![
            card_title(text::SUMMARY_MODEL, None, cx),
            div()
                .mt(space::SM)
                .mb(space::XL)
                .child(muted(text::SUMMARY_MODEL_HELP, cx))
                .into_any_element(),
            body.into_any_element(),
            div()
                .mt(space::LG)
                .flex()
                .justify_end()
                .child(
                    save_button("summary-model-save", "Gem", Variant::Primary, can_save).on_click(
                        move |_, window, cx| {
                            let _ = save_shell
                                .update(cx, |shell, cx| shell.save_summary_model(window, cx));
                        },
                    ),
                )
                .into_any_element(),
        ]
    }
}

fn save_button(id: &'static str, label: &'static str, variant: Variant, enabled: bool) -> Button {
    controls::button(id, variant).label(label).disabled(!enabled)
}

fn alert_text(text: &str, cx: &App) -> impl IntoElement {
    div()
        .text_size(typography::SM)
        .text_color(crate::ui::palette(cx).destructive)
        .child(text.to_string())
}

/// One quick-pick language: the pin button and the remove button
/// (`SummaryLanguageSettings.tsx`).
fn language_chip(
    code: &str,
    pinned: Option<&str>,
    shell: &gpui::WeakEntity<Shell>,
    cx: &App,
) -> AnyElement {
    let palette = crate::ui::palette(cx);
    let is_pinned = pinned == Some(code);
    let label = core::language_label(code);
    let (pin_shell, remove_shell) = (shell.clone(), shell.clone());
    let (pin_code, remove_code) = (code.to_string(), code.to_string());
    h_flex()
        .rounded_full()
        .border_1()
        .border_color(if is_pinned {
            palette.primary.opacity(0.3)
        } else {
            palette.border
        })
        .bg(if is_pinned {
            palette.primary.opacity(0.1)
        } else {
            palette.secondary
        })
        .child(
            controls::small_button(SharedString::from(format!("summary-pin-{code}")), Variant::Ghost)
                .icon(if is_pinned { IconName::Check } else { IconName::Plus })
                .label(label.clone())
                .accessibility_label(if is_pinned {
                    mtext::language_unuse(&label)
                } else {
                    mtext::language_use(&label)
                })
                .tooltip(if is_pinned {
                    mtext::language_unuse(&label)
                } else {
                    mtext::language_use(&label)
                })
                .on_click(move |_, window, cx| {
                    let code = pin_code.clone();
                    let _ = pin_shell.update(cx, |shell, cx| {
                        shell.change_languages(|p| p.toggle_pinned(&code), window, cx)
                    });
                }),
        )
        .child(
            controls::small_icon_button(
                SharedString::from(format!("summary-remove-{code}")),
                IconName::X,
                mtext::language_remove(&label),
            )
            .on_click(move |_, window, cx| {
                let code = remove_code.clone();
                let _ = remove_shell.update(cx, |shell, cx| {
                    shell.change_languages(|p| p.remove_recent(&code), window, cx)
                });
            }),
        )
        .into_any_element()
}

#[cfg(feature = "screenshot-tests")]
impl SummaryTab {
    /// Fixed contents for the screenshot fixtures: the chosen model is
    /// Ollama's, with a fixed list.
    pub(crate) fn freeze_fixture(shell: &mut Shell, window: &mut Window, cx: &mut Context<Shell>) {
        let tab = &mut shell.settings.summary;
        tab.frozen = true;
        tab.stale = false;
        tab.provider = Some("ollama".into());
        tab.model = Some("gemma3:1b".into());
        tab.models = Models::Ready(vec![
            ModelOption {
                id: "gemma3:1b".into(),
                label: "gemma3:1b".into(),
            },
            ModelOption {
                id: "llama3.2:latest".into(),
                label: "llama3.2:latest".into(),
            },
        ]);
        tab.data = Some(Loaded {
            choice: Some(ModelChoice {
                provider: "ollama".into(),
                model: "gemma3:1b".into(),
                ollama_endpoint: None,
            }),
            preferences: Preferences {
                auto_summary: true,
                language_recents: vec!["da".into(), "en".into()],
                language_default: Some("da".into()),
            },
            preferences_unreadable: false,
            has_key: false,
            custom: CustomServer::default(),
        });
        let row = core::PROVIDERS.iter().position(|id| *id == "ollama");
        tab.fields.provider.update(cx, |select, cx| {
            select.set_selected_index(row.map(IndexPath::new), window, cx)
        });
        shell.apply_models(window, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn providers_are_listed_in_tauris_order_with_its_names() {
        let labels: Vec<_> = provider_labels().iter().map(|l| l.to_string()).collect();
        assert_eq!(labels[0], "Indbygget AI (offline, ingen API nødvendig)");
        assert_eq!(labels[1], "Claude");
        assert_eq!(labels[2], "Egen server (OpenAI-kompatibel)");
        assert_eq!(labels.len(), 7);
    }

    #[test]
    fn the_models_state_defaults_to_not_read() {
        assert_eq!(Models::default(), Models::Idle);
    }
}
