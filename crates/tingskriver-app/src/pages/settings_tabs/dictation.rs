//! The Diktering tab of Indstillinger, a copy of the Tauri tab
//! (`frontend/src/components/dictation/DictationSettings.tsx`): Genvej,
//! Mikrofon og lyd, Transskription, Renskrivning, Tekst-hook, Stil pr.
//! program, Ordbog, Snippets and Historik og privatliv.
//!
//! Everything reads and writes core's dictation service, as the Tauri
//! commands do (`frontend/src-tauri/src/dictation/commands.rs`). The Syv.ai
//! key goes straight to Windows Credential Manager: the field is masked, is
//! emptied after saving, and the key is never shown, logged or kept here.

use std::future::Future;
use std::sync::Arc;

use gpui::{
    div, prelude::*, relative, AnyElement, AnyWindowHandle, App, Context, Entity, FontWeight,
    SharedString, Subscription, Window,
};
use gpui_component::{
    h_flex,
    input::{Input, InputEvent, InputState, Textarea, TextareaState},
    notification::Notification,
    select::{Select, SelectEvent, SelectState},
    slider::{Slider, SliderEvent, SliderState},
    v_flex, Disableable, IndexPath, WindowExt,
};
use gpui_kit_assets::IconName;
use tingskriver_core::dictation::logic::cleanup::Style;
use tingskriver_core::dictation::logic::gesture::ShortcutMode;
use tingskriver_core::dictation::service::{self, Service};
use tingskriver_core::dictation::settings::{AppStyleSetting, DictationSettings};
use tingskriver_core::dictation::store::{DictionaryWord, SnippetRow};
use tingskriver_core::dictation::{capture, secrets, syv};
use tingskriver_core::strings::{self as all_strings, dictation_settings as strings};

use super::subheading_regular;
use crate::controls::{self, Variant};
use crate::pages::dictation::{read_status, Status};
use crate::pages::settings::{alert, field_label, switch_row, tab_column};
use crate::theme::{self, dictation as size, layout, space, typography};
use crate::ui::{heading, muted, palette, Level, Shell};

type Choices = SelectState<Vec<SharedString>>;

/// Tauri's shortcut modes, in the order of `strings.modes`.
const MODES: [(ShortcutMode, &str); 4] = [
    (ShortcutMode::Hold, strings::MODE_HOLD),
    (ShortcutMode::Toggle, strings::MODE_TOGGLE),
    (ShortcutMode::Hybrid, strings::MODE_HYBRID),
    (ShortcutMode::DoubleTap, strings::MODE_DOUBLE_TAP),
];
const ENGINES: [(&str, &str); 2] = [
    ("auto", strings::ENGINE_AUTO),
    ("local", strings::ENGINE_LOCAL),
];
/// "Brug lokal model, hvis Syv.ai er længere end" (Tauri's list).
const FALLBACKS: [u64; 5] = [1000, 1500, 2000, 3000, 5000];
const PROVIDERS: [(&str, &str); 2] = [
    ("builtin-ai", strings::PROVIDER_BUILTIN),
    ("ollama", strings::PROVIDER_OLLAMA),
];
const CHANNELS: [(&str, &str); 3] = [
    ("mix", strings::CHANNEL_MIX),
    ("left", strings::CHANNEL_LEFT),
    ("right", strings::CHANNEL_RIGHT),
];
const STYLES: [(&str, &str); 2] = [
    ("raw", strings::STYLE_RAW),
    ("normal", strings::STYLE_NORMAL),
];
/// Tauri's hybrid threshold field: 100-1000 ms, 300 when empty.
const HYBRID_MIN: u64 = 100;
const HYBRID_MAX: u64 = 1000;
const HYBRID_DEFAULT: u64 = 300;

/// What the tab shows, read together as Tauri's `loadAll`.
#[derive(Debug, Clone)]
struct Loaded {
    settings: DictationSettings,
    status: Status,
    words: Vec<DictionaryWord>,
    snippets: Vec<SnippetRow>,
}

/// One row of Stil pr. program: the program file and its style. The rows
/// are a draft until "Gem programmer", as in Tauri.
struct StyleRow {
    process: Entity<InputState>,
    style: Entity<Choices>,
    _subscriptions: Vec<Subscription>,
}

/// The controls whose text or choice lives in the window.
struct Fields {
    shortcut: Entity<InputState>,
    mode: Entity<Choices>,
    hybrid: Entity<InputState>,
    microphone: Entity<Choices>,
    channel: Entity<Choices>,
    volume: Entity<SliderState>,
    engine: Entity<Choices>,
    syv_key: Entity<InputState>,
    fallback: Entity<Choices>,
    preview: Entity<InputState>,
    provider: Entity<Choices>,
    polish_model: Entity<InputState>,
    polish_endpoint: Entity<InputState>,
    hook: Entity<Choices>,
    word: Entity<InputState>,
    aliases: Entity<InputState>,
    phrase: Entity<InputState>,
    snippet_text: Entity<TextareaState>,
}

/// Per-window state of the Diktering tab (part of `SettingsView`).
pub(crate) struct DictationTab {
    /// The window the fields live in, for results that arrive later.
    window: AnyWindowHandle,
    data: Option<Loaded>,
    load_error: Option<String>,
    loading: bool,
    /// Read again the next time the tab is drawn (Tauri reloads on mount).
    stale: bool,
    /// Fixture data: never read from or written to core.
    frozen: bool,
    /// Counts settings saves, so only the newest one is applied.
    saves: u64,
    /// Serializes the saves in core and holds the number of the newest one
    /// written, so an older save never lands after a newer one.
    written: Arc<tokio::sync::Mutex<u64>>,
    testing: bool,
    preview: Option<String>,
    /// The microphone choices after "Windows' standardmikrofon"; a saved
    /// microphone that is not connected comes first.
    microphones: Vec<String>,
    hook_folder: String,
    hook_scripts: Vec<String>,
    /// The values behind the microphone and script choices, by index.
    microphone_values: Vec<String>,
    hook_values: Vec<String>,
    styles: Vec<StyleRow>,
    word_id: Option<i64>,
    snippet_id: Option<i64>,
    fields: Fields,
    _subscriptions: Vec<Subscription>,
}

fn labels(items: &[(impl Copy, &'static str)]) -> Vec<SharedString> {
    items.iter().map(|(_, l)| SharedString::from(*l)).collect()
}

impl DictationTab {
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
        let shortcut = input(None, false);
        let hybrid = input(None, false);
        // The key is never shown: masked, and emptied after saving.
        let syv_key = input(Some(strings::KEY_NEW_PLACEHOLDER), true);
        let preview = input(None, false);
        let polish_model = input(Some(strings::POLISH_MODEL_PLACEHOLDER), false);
        let polish_endpoint = input(Some(strings::OLLAMA_ENDPOINT_PLACEHOLDER), false);
        let word = input(Some(strings::WORD_PLACEHOLDER), false);
        let aliases = input(Some(strings::ALIASES_PLACEHOLDER), false);
        let phrase = input(Some(strings::PHRASE_PLACEHOLDER), false);
        let snippet_text = cx.new(|cx| TextareaState::new(window, cx).rows(3));
        preview.update(cx, |input, cx| {
            input.set_value(strings::PREVIEW_SAMPLE, window, cx)
        });
        let mut choices =
            |items: Vec<SharedString>| cx.new(|cx| SelectState::new(items, None, window, cx));
        let fields = Fields {
            mode: choices(labels(&MODES)),
            microphone: choices(vec![strings::MIC_DEFAULT.into()]),
            channel: choices(labels(&CHANNELS)),
            engine: choices(labels(&ENGINES)),
            fallback: choices(
                FALLBACKS
                    .iter()
                    .map(|ms| strings::duration_ms(*ms).into())
                    .collect(),
            ),
            provider: choices(labels(&PROVIDERS)),
            hook: choices(vec![strings::HOOK_NONE.into()]),
            volume: cx.new(|_| SliderState::new().min(0.0).max(100.0).step(5.0)),
            shortcut,
            hybrid,
            syv_key,
            preview,
            polish_model,
            polish_endpoint,
            word,
            aliases,
            phrase,
            snippet_text,
        };
        let mut subscriptions = Vec::new();
        // Buttons that follow a field's text ("Gem genvej", "Gem nøgle",
        // "Tilføj") redraw as it changes; Enter submits the forms.
        for input in [
            &fields.shortcut,
            &fields.syv_key,
            &fields.preview,
            &fields.word,
            &fields.aliases,
            &fields.phrase,
        ] {
            subscriptions.push(cx.subscribe_in(
                input,
                window,
                |shell: &mut Shell, state, event: &InputEvent, window, cx| match event {
                    InputEvent::Change => cx.notify(),
                    InputEvent::PressEnter { .. } => {
                        let fields = &shell.settings.dictation.fields;
                        if *state == fields.word || *state == fields.aliases {
                            shell.save_word(window, cx);
                        } else if *state == fields.phrase {
                            shell.save_snippet(window, cx);
                        } else if *state == fields.preview {
                            shell.run_preview(window, cx);
                        }
                    }
                    _ => {}
                },
            ));
        }
        subscriptions.push(cx.subscribe_in(
            &fields.snippet_text,
            window,
            |_: &mut Shell, _, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify()
                }
            },
        ));
        // Uncontrolled fields saved when they lose focus, as Tauri's onBlur.
        for input in [
            &fields.hybrid,
            &fields.polish_model,
            &fields.polish_endpoint,
        ] {
            subscriptions.push(cx.subscribe_in(
                input,
                window,
                |shell: &mut Shell, state, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::Blur) {
                        shell.blur_field(state.clone(), window, cx);
                    }
                },
            ));
        }
        for select in [
            &fields.mode,
            &fields.microphone,
            &fields.channel,
            &fields.engine,
            &fields.fallback,
            &fields.provider,
            &fields.hook,
        ] {
            subscriptions.push(cx.subscribe_in(
                select,
                window,
                |shell: &mut Shell, state, event: &SelectEvent<Vec<SharedString>>, window, cx| {
                    let SelectEvent::Confirm(Some(_)) = event else {
                        return;
                    };
                    if let Some(index) = state.read(cx).selected_index(cx) {
                        shell.choice_made(state.clone(), index.row, window, cx);
                    }
                },
            ));
        }
        subscriptions.push(cx.subscribe_in(
            &fields.volume,
            window,
            |shell: &mut Shell, _, event: &SliderEvent, window, cx| match event {
                SliderEvent::Change(_) => cx.notify(),
                SliderEvent::Release(value) => {
                    let volume = value.start().round().clamp(0.0, 100.0) as u32;
                    shell.save_dictation(move |s| s.sound_volume = volume, window, cx);
                }
            },
        ));
        DictationTab {
            window: window.window_handle(),
            data: None,
            load_error: None,
            loading: false,
            stale: true,
            frozen: false,
            saves: 0,
            written: Arc::new(tokio::sync::Mutex::new(0)),
            testing: false,
            preview: None,
            microphones: Vec::new(),
            hook_folder: String::new(),
            hook_scripts: Vec::new(),
            microphone_values: vec![String::new()],
            hook_values: vec![String::new()],
            styles: Vec::new(),
            word_id: None,
            snippet_id: None,
            fields,
            _subscriptions: subscriptions,
        }
    }

    /// The tab was opened: read again, as Tauri reloads on mount.
    pub(crate) fn shown(&mut self) {
        self.stale = true;
    }

    /// Synthetic state for the screenshot fixtures (the values of
    /// `docs/screenshots/indstillinger-diktering-lys.png`).
    #[cfg(feature = "screenshot-tests")]
    pub(crate) fn freeze_fixture(shell: &mut Shell, window: &mut Window, cx: &mut Context<Shell>) {
        let at = |id: i64, word: &str, aliases: &str| DictionaryWord {
            id,
            word: word.to_string(),
            aliases: aliases.to_string(),
            english: false,
        };
        let snippet = |id: i64, phrase: &str, text: &str| SnippetRow {
            id,
            phrase: phrase.to_string(),
            text: text.to_string(),
        };
        let tab = &mut shell.settings.dictation;
        tab.frozen = true;
        tab.stale = false;
        tab.hook_folder = "fixtures/dictation_hooks".to_string();
        let loaded = Loaded {
            settings: DictationSettings::default(),
            status: Status {
                platform_supported: true,
                has_syv_key: true,
                local_model_available: true,
                last_error: None,
            },
            words: vec![
                at(1, "Fixturefirma", "fikstur firma"),
                at(2, "Syv.ai", "syv ai"),
                at(3, "Tingskriver", ""),
            ],
            snippets: vec![
                snippet(1, "mit cvr", "CVR 12345678"),
                snippet(
                    2,
                    "booking-link",
                    "Book et møde her: https://example.com/book",
                ),
            ],
        };
        tab.apply(loaded, window, cx);
    }

    /// Take a fresh read: the data, then every field that shows it.
    fn apply(&mut self, loaded: Loaded, window: &mut Window, cx: &mut Context<Shell>) {
        self.load_error = None;
        let missing =
            |saved: &str, list: &[String]| !saved.is_empty() && !list.iter().any(|m| m == saved);
        let settings = &loaded.settings;
        // Microphones: Windows' default, a saved one that is not connected,
        // then the connected ones (Tauri's options).
        self.microphone_values = std::iter::once(String::new())
            .chain(
                missing(&settings.microphone, &self.microphones)
                    .then(|| settings.microphone.clone()),
            )
            .chain(self.microphones.iter().cloned())
            .collect();
        let microphone_labels = self
            .microphone_values
            .iter()
            .enumerate()
            .map(|(i, name)| match i {
                0 => SharedString::from(strings::MIC_DEFAULT),
                1 if missing(&settings.microphone, &self.microphones) => {
                    strings::mic_missing(name).into()
                }
                _ => name.clone().into(),
            })
            .collect();
        self.hook_values = std::iter::once(String::new())
            .chain(
                missing(&settings.text_hook_script, &self.hook_scripts)
                    .then(|| settings.text_hook_script.clone()),
            )
            .chain(self.hook_scripts.iter().cloned())
            .collect();
        let hook_labels = self
            .hook_values
            .iter()
            .enumerate()
            .map(|(i, name)| match i {
                0 => SharedString::from(strings::HOOK_NONE),
                1 if missing(&settings.text_hook_script, &self.hook_scripts) => {
                    strings::hook_missing(name).into()
                }
                _ => name.clone().into(),
            })
            .collect();
        let f = &self.fields;
        f.microphone
            .update(cx, |s, cx| s.set_items(microphone_labels, window, cx));
        f.hook
            .update(cx, |s, cx| s.set_items(hook_labels, window, cx));
        self.data = Some(loaded);
        self.show_settings(window, cx);
    }

    /// Put the saved settings into the fields (after a load or a save).
    fn show_settings(&mut self, window: &mut Window, cx: &mut Context<Shell>) {
        let Some(data) = &self.data else {
            return;
        };
        let s = data.settings.clone();
        let has_key = data.status.has_syv_key;
        let f = &self.fields;
        let set = |input: &Entity<InputState>,
                   value: String,
                   window: &mut Window,
                   cx: &mut Context<Shell>| {
            input.update(cx, |input, cx| {
                if input.value().as_ref() != value {
                    input.set_value(value, window, cx)
                }
            })
        };
        set(&f.shortcut, s.shortcut.clone(), window, cx);
        set(&f.hybrid, s.hybrid_threshold_ms.to_string(), window, cx);
        set(&f.polish_model, s.polish_model.clone(), window, cx);
        set(&f.polish_endpoint, s.polish_endpoint.clone(), window, cx);
        let ollama = s.polish_provider == "ollama";
        f.polish_model.update(cx, |input, cx| {
            input.set_placeholder(
                if ollama {
                    strings::POLISH_MODEL_OLLAMA_PLACEHOLDER
                } else {
                    strings::POLISH_MODEL_PLACEHOLDER
                },
                window,
                cx,
            )
        });
        f.syv_key.update(cx, |input, cx| {
            input.set_placeholder(
                if has_key {
                    strings::KEY_REPLACE_PLACEHOLDER
                } else {
                    strings::KEY_NEW_PLACEHOLDER
                },
                window,
                cx,
            )
        });
        let select = |state: &Entity<Choices>,
                      index: Option<usize>,
                      window: &mut Window,
                      cx: &mut Context<Shell>| {
            state.update(cx, |state, cx| {
                state.set_selected_index(index.map(IndexPath::new), window, cx)
            })
        };
        select(
            &f.mode,
            MODES.iter().position(|(m, _)| *m == s.shortcut_mode),
            window,
            cx,
        );
        select(
            &f.engine,
            ENGINES.iter().position(|(v, _)| *v == s.engine),
            window,
            cx,
        );
        select(
            &f.fallback,
            FALLBACKS.iter().position(|ms| *ms == s.fallback_after_ms),
            window,
            cx,
        );
        select(
            &f.provider,
            PROVIDERS.iter().position(|(v, _)| *v == s.polish_provider),
            window,
            cx,
        );
        select(
            &f.channel,
            CHANNELS.iter().position(|(v, _)| *v == s.channel),
            window,
            cx,
        );
        select(
            &f.microphone,
            self.microphone_values
                .iter()
                .position(|v| *v == s.microphone),
            window,
            cx,
        );
        select(
            &f.hook,
            self.hook_values
                .iter()
                .position(|v| *v == s.text_hook_script),
            window,
            cx,
        );
        f.volume.update(cx, |slider, cx| {
            slider.set_value(s.sound_volume as f32, window, cx)
        });
        self.styles = s
            .app_styles
            .iter()
            .map(|row| style_row(row, window, cx))
            .collect();
    }

    /// Whether the program rows differ from the saved ones (Tauri's
    /// `stylesChanged`).
    fn styles_changed(&self, cx: &App) -> bool {
        let Some(data) = &self.data else {
            return false;
        };
        self.style_settings(cx) != data.settings.app_styles
    }

    fn style_settings(&self, cx: &App) -> Vec<AppStyleSetting> {
        self.styles
            .iter()
            .map(|row| AppStyleSetting {
                process: row.process.read(cx).value().to_string(),
                style: row
                    .style
                    .read(cx)
                    .selected_index(cx)
                    .map_or(STYLES[0].0, |i| STYLES[i.row.min(1)].0)
                    .to_string(),
            })
            .collect()
    }
}

/// A Stil pr. program row with its own controls.
fn style_row(row: &AppStyleSetting, window: &mut Window, cx: &mut Context<Shell>) -> StyleRow {
    let process = cx.new(|cx| InputState::new(window, cx));
    process.update(cx, |input, cx| {
        input.set_value(row.process.clone(), window, cx)
    });
    let at = STYLES
        .iter()
        .position(|(v, _)| *v == row.style)
        .unwrap_or(0);
    let style =
        cx.new(|cx| SelectState::new(labels(&STYLES), Some(IndexPath::new(at)), window, cx));
    let subscriptions = vec![
        cx.subscribe_in(
            &process,
            window,
            |_: &mut Shell, _, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify()
                }
            },
        ),
        cx.subscribe_in(
            &style,
            window,
            |_: &mut Shell, _, _: &SelectEvent<Vec<SharedString>>, _, cx| cx.notify(),
        ),
    ];
    StyleRow {
        process,
        style,
        _subscriptions: subscriptions,
    }
}

/// Settings, status, dictionary, snippets, microphones and hook scripts, as
/// Tauri's `loadAll` reads them.
async fn fetch() -> Result<(Loaded, Vec<String>, (String, Vec<String>)), String> {
    let service = service::get()?;
    let words = service.store.dictionary().await?;
    let snippets = service.store.snippets().await?;
    let status = read_status(&service).await;
    let loaded = Loaded {
        settings: service.settings(),
        status,
        words,
        snippets,
    };
    // The page works without these two lists, as in Tauri.
    let microphones = tokio::task::spawn_blocking(capture::input_device_names)
        .await
        .unwrap_or_default();
    let hooks = service.hook_scripts().await;
    Ok((loaded, microphones, hooks))
}

/// The hybrid threshold a typed value means, as Tauri's onBlur: a number
/// rounded and kept within 100-1000 ms; empty or not a number gives 300.
fn hybrid_threshold(text: &str) -> u64 {
    text.trim()
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
        .map_or(HYBRID_DEFAULT, |v| {
            (v.round() as i64).clamp(HYBRID_MIN as i64, HYBRID_MAX as i64) as u64
        })
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// The result of a save a newer save made redundant; never shown, as only
/// the newest save's result is applied.
const SUPERSEDED: &str = "superseded by a newer save";

/// A failure toast with Tauri's title and the reason below it.
fn failed(title: &'static str, error: &str, window: &mut Window, cx: &mut App) {
    window.push_notification(Notification::error(error.to_string()).title(title), cx);
}

impl Shell {
    /// Read the tab's data if it is stale (on first view and each visit).
    fn load_dictation_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let tab = &mut self.settings.dictation;
        if tab.frozen || tab.loading || !tab.stale {
            return;
        }
        tab.stale = false;
        tab.loading = true;
        let task = self.model.read(cx).runtime.spawn(fetch());
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await.unwrap_or_else(|error| Err(error.to_string()));
            let _ = this.update_in(cx, |shell, window, cx| {
                let tab = &mut shell.settings.dictation;
                tab.loading = false;
                match result {
                    Ok((loaded, microphones, (folder, scripts))) => {
                        tab.microphones = microphones;
                        tab.hook_folder = folder;
                        tab.hook_scripts = scripts;
                        tab.apply(loaded, window, cx);
                    }
                    Err(error) => {
                        log::warn!("Dictation settings could not be read: {error}");
                        tab.load_error = Some(error);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Run `work` on core's dictation service, then `done` in the window.
    fn dictation_act<T, W, F>(
        &mut self,
        work: W,
        done: impl FnOnce(&mut Shell, Result<T, String>, &mut Window, &mut Context<Shell>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) where
        T: Send + 'static,
        W: FnOnce(Arc<Service>) -> F + Send + 'static,
        F: Future<Output = Result<T, String>> + Send + 'static,
    {
        if self.settings.dictation.frozen {
            return;
        }
        let task = self.model.read(cx).runtime.spawn(async move {
            let service = service::get()?;
            work(service).await
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await.unwrap_or_else(|error| Err(error.to_string()));
            let _ = this.update_in(cx, |shell, window, cx| {
                done(shell, result, window, cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Change one setting and save all of them (Tauri's `save`). The change
    /// shows at once; a failed save reads the saved settings back.
    fn save_dictation(
        &mut self,
        patch: impl FnOnce(&mut DictationSettings),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let tab = &mut self.settings.dictation;
        let Some(data) = tab.data.as_mut() else {
            return;
        };
        patch(&mut data.settings);
        let next = data.settings.clone();
        tab.saves += 1;
        let save = tab.saves;
        let written = tab.written.clone();
        cx.notify();
        self.dictation_act(
            move |service| async move {
                // Each save carries all settings, so a save that waited
                // behind a newer one has nothing left to write.
                let mut newest = written.lock().await;
                if *newest > save {
                    return Err(SUPERSEDED.to_string());
                }
                let saved = service.save_settings(next).await?;
                *newest = save;
                drop(newest);
                Ok((saved, read_status(&service).await))
            },
            move |shell, result, window, cx| {
                let tab = &mut shell.settings.dictation;
                if save != tab.saves {
                    return;
                }
                match result {
                    Ok((saved, status)) => {
                        if let Some(data) = tab.data.as_mut() {
                            data.settings = saved;
                            data.status = status;
                        }
                        tab.show_settings(window, cx);
                        window.push_notification(Notification::success(strings::SAVED), cx);
                    }
                    Err(error) => {
                        failed(strings::SAVE_FAILED, &error, window, cx);
                        tab.stale = true;
                        shell.load_dictation_settings(window, cx);
                    }
                }
                // The Diktering page's status line follows the settings.
                shell.dictation_changed(cx);
            },
            window,
            cx,
        );
    }

    /// A select was confirmed: save what it chose.
    fn choice_made(
        &mut self,
        state: Entity<Choices>,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let tab = &self.settings.dictation;
        let f = &tab.fields;
        let patch: Box<dyn FnOnce(&mut DictationSettings)> = if state == f.mode {
            let mode = MODES[index.min(MODES.len() - 1)].0;
            Box::new(move |s| s.shortcut_mode = mode)
        } else if state == f.engine {
            let engine = ENGINES[index.min(1)].0.to_string();
            Box::new(move |s| s.engine = engine)
        } else if state == f.fallback {
            let ms = FALLBACKS[index.min(FALLBACKS.len() - 1)];
            Box::new(move |s| s.fallback_after_ms = ms)
        } else if state == f.provider {
            let provider = PROVIDERS[index.min(1)].0.to_string();
            Box::new(move |s| s.polish_provider = provider)
        } else if state == f.channel {
            let channel = CHANNELS[index.min(CHANNELS.len() - 1)].0.to_string();
            Box::new(move |s| s.channel = channel)
        } else if state == f.microphone {
            let Some(name) = tab.microphone_values.get(index).cloned() else {
                return;
            };
            Box::new(move |s| s.microphone = name)
        } else if state == f.hook {
            let Some(name) = tab.hook_values.get(index).cloned() else {
                return;
            };
            Box::new(move |s| s.text_hook_script = name)
        } else {
            return;
        };
        // Tauri's select only reports a real change: re-picking the chosen
        // value saves nothing (and keeps an unsaved Stil pr. program draft).
        let Some(data) = tab.data.as_ref() else {
            return;
        };
        let mut next = data.settings.clone();
        patch(&mut next);
        if next == data.settings {
            return;
        }
        self.save_dictation(move |s| *s = next, window, cx);
    }

    /// A field saved on blur lost focus (hybrid threshold, polish model and
    /// Ollama address).
    fn blur_field(
        &mut self,
        state: Entity<InputState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let tab = &self.settings.dictation;
        let Some(data) = &tab.data else {
            return;
        };
        let value = state.read(cx).value().to_string();
        let f = &tab.fields;
        if state == f.hybrid {
            let ms = hybrid_threshold(&value);
            state.update(cx, |input, cx| input.set_value(ms.to_string(), window, cx));
            if ms != data.settings.hybrid_threshold_ms {
                self.save_dictation(move |s| s.hybrid_threshold_ms = ms, window, cx);
            }
        } else if state == f.polish_model {
            if value != data.settings.polish_model {
                self.save_dictation(move |s| s.polish_model = value, window, cx);
            }
        } else if state == f.polish_endpoint && value != data.settings.polish_endpoint {
            self.save_dictation(move |s| s.polish_endpoint = value, window, cx);
        }
    }

    fn save_shortcut(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let shortcut = self
            .settings
            .dictation
            .fields
            .shortcut
            .read(cx)
            .value()
            .to_string();
        self.save_dictation(move |s| s.shortcut = shortcut, window, cx);
    }

    fn save_syv_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let input = self.settings.dictation.fields.syv_key.clone();
        let key = input.read(cx).value().trim().to_string();
        if key.is_empty() {
            return;
        }
        self.dictation_act(
            move |service| async move {
                secrets::write(secrets::SYV_TARGET, &key)?;
                Ok(read_status(&service).await)
            },
            move |shell, result, window, cx| match result {
                Ok(status) => {
                    input.update(cx, |input, cx| input.set_value("", window, cx));
                    shell.dictation_status(status, window, cx);
                    window.push_notification(Notification::success(strings::KEY_SAVED), cx);
                }
                Err(error) => failed(strings::KEY_SAVE_FAILED, &error, window, cx),
            },
            window,
            cx,
        );
    }

    fn remove_syv_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.dictation_act(
            |service| async move {
                secrets::delete(secrets::SYV_TARGET)?;
                Ok(read_status(&service).await)
            },
            |shell, result, window, cx| match result {
                Ok(status) => {
                    shell.dictation_status(status, window, cx);
                    window.push_notification(Notification::success(strings::KEY_REMOVED), cx);
                }
                Err(error) => failed(strings::KEY_REMOVE_FAILED, &error, window, cx),
            },
            window,
            cx,
        );
    }

    fn test_syv_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings.dictation.frozen {
            return;
        }
        self.settings.dictation.testing = true;
        self.dictation_act(
            |service| async move {
                let key = secrets::syv_key().ok_or_else(|| strings::KEY_NONE.to_string())?;
                let endpoint = service.settings().syv_endpoint;
                syv::test_key(&service.http, &endpoint, &key).await
            },
            |shell, result, window, cx| {
                shell.settings.dictation.testing = false;
                match result {
                    Ok(ms) => window.push_notification(
                        Notification::success(strings::syv_latency(&strings::duration_ms(ms)))
                            .title(strings::SYV_ANSWERS),
                        cx,
                    ),
                    Err(error) => failed(strings::SYV_TEST_FAILED, &error, window, cx),
                }
            },
            window,
            cx,
        );
    }

    /// A new status after a key change; the Diktering page follows.
    fn dictation_status(&mut self, status: Status, window: &mut Window, cx: &mut Context<Self>) {
        let tab = &mut self.settings.dictation;
        if let Some(data) = tab.data.as_mut() {
            data.status = status;
        }
        tab.show_settings(window, cx);
        self.dictation_changed(cx);
    }

    fn run_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self
            .settings
            .dictation
            .fields
            .preview
            .read(cx)
            .value()
            .to_string();
        self.dictation_act(
            move |service| async move { Ok(service.preview(&text, Style::Normal)) },
            |shell, result, window, cx| match result {
                Ok(text) => shell.settings.dictation.preview = Some(text),
                Err(error) => failed(strings::PREVIEW_FAILED, &error, window, cx),
            },
            window,
            cx,
        );
    }

    fn add_style_row(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let row = style_row(
            &AppStyleSetting {
                process: String::new(),
                style: STYLES[0].0.to_string(),
            },
            window,
            cx,
        );
        self.settings.dictation.styles.push(row);
        cx.notify();
    }

    fn save_styles(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let styles = self.settings.dictation.style_settings(cx);
        self.save_dictation(move |s| s.app_styles = styles, window, cx);
    }

    fn save_word(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let tab = &self.settings.dictation;
        let word = tab.fields.word.read(cx).value().to_string();
        let aliases = tab.fields.aliases.read(cx).value().to_string();
        if word.trim().is_empty() {
            return;
        }
        let id = tab.word_id;
        self.dictation_act(
            move |service| async move {
                service
                    .store
                    .upsert_word(id, &word, &aliases, now_ms())
                    .await?;
                service.reload_rules().await?;
                service.store.dictionary().await
            },
            |shell, result, window, cx| match result {
                Ok(words) => {
                    let tab = &mut shell.settings.dictation;
                    tab.word_id = None;
                    for input in [&tab.fields.word, &tab.fields.aliases] {
                        input.update(cx, |input, cx| input.set_value("", window, cx));
                    }
                    if let Some(data) = tab.data.as_mut() {
                        data.words = words;
                    }
                }
                Err(error) => failed(strings::WORD_SAVE_FAILED, &error, window, cx),
            },
            window,
            cx,
        );
    }

    fn edit_word(&mut self, word: DictionaryWord, window: &mut Window, cx: &mut Context<Self>) {
        let tab = &mut self.settings.dictation;
        tab.word_id = Some(word.id);
        tab.fields
            .word
            .update(cx, |input, cx| input.set_value(word.word, window, cx));
        tab.fields
            .aliases
            .update(cx, |input, cx| input.set_value(word.aliases, window, cx));
        cx.notify();
    }

    fn delete_word(&mut self, id: i64, window: &mut Window, cx: &mut Context<Self>) {
        self.dictation_act(
            move |service| async move {
                service.store.delete_word(id).await?;
                service.reload_rules().await?;
                service.store.dictionary().await
            },
            |shell, result, window, cx| match result {
                Ok(words) => {
                    if let Some(data) = shell.settings.dictation.data.as_mut() {
                        data.words = words;
                    }
                }
                Err(error) => failed(strings::WORD_DELETE_FAILED, &error, window, cx),
            },
            window,
            cx,
        );
    }

    fn save_snippet(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let tab = &self.settings.dictation;
        let phrase = tab.fields.phrase.read(cx).value().to_string();
        let text = tab.fields.snippet_text.read(cx).value().to_string();
        if phrase.trim().is_empty() || text.trim().is_empty() {
            return;
        }
        let id = tab.snippet_id;
        self.dictation_act(
            move |service| async move {
                service
                    .store
                    .upsert_snippet(id, &phrase, &text, now_ms())
                    .await?;
                service.reload_rules().await?;
                service.store.snippets().await
            },
            |shell, result, window, cx| match result {
                Ok(snippets) => {
                    let tab = &mut shell.settings.dictation;
                    tab.snippet_id = None;
                    tab.fields
                        .phrase
                        .update(cx, |input, cx| input.set_value("", window, cx));
                    tab.fields
                        .snippet_text
                        .update(cx, |input, cx| input.set_value("", window, cx));
                    if let Some(data) = tab.data.as_mut() {
                        data.snippets = snippets;
                    }
                }
                Err(error) => failed(strings::SNIPPET_SAVE_FAILED, &error, window, cx),
            },
            window,
            cx,
        );
    }

    fn edit_snippet(&mut self, snippet: SnippetRow, window: &mut Window, cx: &mut Context<Self>) {
        let tab = &mut self.settings.dictation;
        tab.snippet_id = Some(snippet.id);
        tab.fields
            .phrase
            .update(cx, |input, cx| input.set_value(snippet.phrase, window, cx));
        tab.fields
            .snippet_text
            .update(cx, |input, cx| input.set_value(snippet.text, window, cx));
        cx.notify();
    }

    fn delete_snippet(&mut self, id: i64, window: &mut Window, cx: &mut Context<Self>) {
        self.dictation_act(
            move |service| async move {
                service.store.delete_snippet(id).await?;
                service.reload_rules().await?;
                service.store.snippets().await
            },
            |shell, result, window, cx| match result {
                Ok(snippets) => {
                    if let Some(data) = shell.settings.dictation.data.as_mut() {
                        data.snippets = snippets;
                    }
                }
                Err(error) => failed(strings::SNIPPET_DELETE_FAILED, &error, window, cx),
            },
            window,
            cx,
        );
    }

    /// The Diktering tab: the error box, the loading lines or the sections.
    pub(crate) fn dictation_settings_tab(&self, cx: &mut Context<Self>) -> AnyElement {
        let tab = &self.settings.dictation;
        if tab.stale && !tab.loading && !tab.frozen {
            // Drawing cannot start work that needs the window; do it right
            // after this frame.
            let shell = cx.entity().downgrade();
            let handle = tab.window;
            cx.defer(move |cx| {
                let _ = handle.update(cx, |_, window, cx| {
                    let _ = shell.update(cx, |shell, cx| shell.load_dictation_settings(window, cx));
                });
            });
        }
        let palette = palette(cx);
        // Tauri shows the error instead of the settings whenever a read
        // failed, also a reload after a failed save.
        if let Some(error) = &tab.load_error {
            return tab_column(vec![div()
                .p(space::LG)
                .rounded(theme::RADIUS_LG)
                .border_1()
                .border_color(
                    palette
                        .destructive
                        .opacity(theme::settings_tabs::TINT_BORDER),
                )
                .bg(theme::over(
                    palette.destructive,
                    palette.background,
                    theme::settings_tabs::TINT,
                ))
                .child(alert(
                    "dictation-settings-error",
                    &strings::load_failed(error),
                    cx,
                ))
                .into_any_element()]);
        }
        let Some(data) = &tab.data else {
            // Tauri's pulsing placeholder: a short and a full-width bar.
            return tab_column(vec![v_flex()
                .gap(space::LG)
                .child(
                    div()
                        .h(space::LG)
                        .w(relative(0.25))
                        .rounded(theme::RADIUS)
                        .bg(palette.muted),
                )
                .child(
                    div()
                        .h(space::XXL)
                        .w_full()
                        .rounded(theme::RADIUS)
                        .bg(palette.muted),
                )
                .into_any_element()]);
        };
        let s = &data.settings;
        let status = &data.status;

        let header = v_flex()
            .child(
                div()
                    .mb(space::XS)
                    .child(heading(all_strings::DICTATION, Level::Section)),
            )
            .child(muted(strings::INTRO, cx))
            .when(!status.platform_supported, |this| {
                this.child(
                    div()
                        .mt(space::MD)
                        .text_size(typography::SM)
                        .text_color(palette.warning)
                        .child(strings::ONLY_WINDOWS),
                )
            })
            .when_some(status.last_error.as_deref(), |this, error| {
                this.child(
                    div()
                        .mt(space::MD)
                        .child(alert("dictation-last-error", error, cx)),
                )
            });

        let mut column = vec![header.into_any_element()];
        column.push(self.shortcut_section(s, cx));
        column.push(self.microphone_section(s, cx));
        column.push(self.transcription_section(s, status, cx));
        column.push(self.cleanup_section(s, cx));
        column.push(self.hook_section(s, cx));
        column.push(self.styles_section(cx));
        column.push(self.words_section(&data.words, cx));
        column.push(self.snippets_section(&data.snippets, cx));
        column.push(self.history_section(s, cx));
        tab_column(column)
    }

    fn shortcut_section(&self, s: &DictationSettings, cx: &mut Context<Self>) -> AnyElement {
        let f = &self.settings.dictation.fields;
        let changed = f.shortcut.read(cx).value().as_ref() != s.shortcut;
        let mut body = vec![
            switch_row(
                "dictation-enabled",
                strings::ENABLED_LABEL,
                Some(strings::ENABLED_HINT),
                controls::switch("dictation-enabled-switch")
                    .checked(s.enabled)
                    .accessibility_label(strings::ENABLED_LABEL)
                    .on_click(cx.listener(|shell, value: &bool, window, cx| {
                        let enabled = *value;
                        shell.save_dictation(move |s| s.enabled = enabled, window, cx)
                    })),
                cx,
            )
            .into_any_element(),
            v_flex()
                .gap(space::SM)
                .child(field_label(strings::SHORTCUT_LABEL))
                .child(
                    h_flex()
                        .gap(space::SM)
                        .child(
                            div()
                                .w_full()
                                .max_w(size::SHORTCUT_INPUT_MAX)
                                .child(text_input(&f.shortcut, strings::SHORTCUT_LABEL)),
                        )
                        .child(
                            controls::button("dictation-shortcut-save", Variant::Outline)
                                .label(strings::SHORTCUT_SAVE)
                                .disabled(!changed)
                                .on_click(cx.listener(|shell, _, window, cx| {
                                    shell.save_shortcut(window, cx)
                                })),
                        ),
                )
                .child(muted(strings::SHORTCUT_HINT, cx))
                .into_any_element(),
            labelled(
                strings::MODE_LABEL,
                select(&f.mode, strings::MODE_LABEL).w_full(),
            ),
        ];
        if s.shortcut_mode == ShortcutMode::Hybrid {
            body.push(
                v_flex()
                    .gap(space::SM)
                    .child(field_label(strings::HYBRID_LABEL))
                    .child(
                        div()
                            .max_w(size::SHORTCUT_INPUT_MAX)
                            .child(text_input(&f.hybrid, strings::HYBRID_LABEL)),
                    )
                    .child(muted(strings::HYBRID_HELP, cx))
                    .into_any_element(),
            );
        }
        section(
            "dictation-shortcut",
            strings::SHORTCUT_TITLE,
            None,
            body,
            cx,
        )
    }

    fn microphone_section(&self, s: &DictationSettings, cx: &mut Context<Self>) -> AnyElement {
        let f = &self.settings.dictation.fields;
        let volume = f.volume.read(cx).value().start().round() as u32;
        let mut body = vec![
            h_flex()
                .gap(space::MD)
                .child(div().flex_1().min_w_0().child(small_labelled(
                    strings::MIC_LABEL,
                    select(&f.microphone, strings::MIC_LABEL).w_full(),
                )))
                .child(div().flex_1().min_w_0().child(small_labelled(
                    strings::CHANNEL_LABEL,
                    select(&f.channel, strings::CHANNEL_LABEL).w_full(),
                )))
                .into_any_element(),
            muted(strings::MIC_NOTE, cx).into_any_element(),
            switch_row(
                "dictation-keep-open",
                strings::KEEP_OPEN_LABEL,
                Some(strings::KEEP_OPEN_HINT),
                controls::switch("dictation-keep-open-switch")
                    .checked(s.keep_mic_open)
                    .accessibility_label(strings::KEEP_OPEN_LABEL)
                    .on_click(cx.listener(|shell, value: &bool, window, cx| {
                        let on = *value;
                        shell.save_dictation(move |s| s.keep_mic_open = on, window, cx)
                    })),
                cx,
            )
            .into_any_element(),
            switch_row(
                "dictation-sound",
                strings::SOUND_LABEL,
                Some(strings::SOUND_HINT),
                controls::switch("dictation-sound-switch")
                    .checked(s.sound_enabled)
                    .accessibility_label(strings::SOUND_LABEL)
                    .on_click(cx.listener(|shell, value: &bool, window, cx| {
                        let on = *value;
                        shell.save_dictation(move |s| s.sound_enabled = on, window, cx)
                    })),
                cx,
            )
            .into_any_element(),
        ];
        if s.sound_enabled {
            body.push(
                h_flex()
                    .gap(space::MD)
                    .child(div().text_size(typography::SM).child(strings::VOLUME_LABEL))
                    .child(
                        div().w(size::VOLUME_SLIDER).child(
                            Slider::new(&f.volume)
                                .accessibility_label(strings::volume_text(&volume.to_string())),
                        ),
                    )
                    .child(muted(&strings::volume_value(&volume.to_string()), cx))
                    .into_any_element(),
            );
        }
        section(
            "dictation-microphone",
            strings::MIC_TITLE,
            Some(strings::MIC_HELP),
            body,
            cx,
        )
    }

    fn transcription_section(
        &self,
        s: &DictationSettings,
        status: &Status,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let tab = &self.settings.dictation;
        let f = &tab.fields;
        let palette = palette(cx);
        let mut body = vec![v_flex()
            .gap(space::SM)
            .child(field_label(strings::ENGINE_LABEL))
            .child(select(&f.engine, strings::ENGINE_LABEL).w_full())
            .when(!status.local_model_available, |this| {
                this.child(
                    div()
                        .text_size(typography::SM)
                        .text_color(palette.warning)
                        .child(strings::NO_LOCAL_MODEL),
                )
            })
            .into_any_element()];
        if s.engine == "auto" {
            let typed = !f.syv_key.read(cx).value().trim().is_empty();
            let buttons =
                h_flex()
                    .flex_wrap()
                    .gap(space::SM)
                    .child(
                        div()
                            .w_full()
                            .max_w(size::SHORTCUT_INPUT_MAX)
                            .child(text_input(&f.syv_key, strings::KEY_LABEL)),
                    )
                    .child(
                        controls::button("dictation-key-save", Variant::Outline)
                            .label(strings::KEY_SAVE)
                            .disabled(!typed)
                            .on_click(
                                cx.listener(|shell, _, window, cx| shell.save_syv_key(window, cx)),
                            ),
                    )
                    .when(status.has_syv_key, |this| {
                        this.child(
                            controls::button("dictation-key-test", Variant::Outline)
                                .label(if tab.testing {
                                    strings::KEY_TESTING
                                } else {
                                    strings::KEY_TEST
                                })
                                .disabled(tab.testing)
                                .on_click(cx.listener(|shell, _, window, cx| {
                                    shell.test_syv_key(window, cx)
                                })),
                        )
                        .child(
                            controls::button("dictation-key-remove", Variant::Ghost)
                                .label(strings::KEY_REMOVE)
                                .on_click(cx.listener(|shell, _, window, cx| {
                                    shell.remove_syv_key(window, cx)
                                })),
                        )
                    });
            body.push(
                v_flex()
                    .gap(space::SM)
                    .child(field_label(strings::KEY_LABEL))
                    .child(muted(
                        if status.has_syv_key {
                            strings::KEY_STORED
                        } else {
                            strings::KEY_NONE
                        },
                        cx,
                    ))
                    .child(buttons)
                    .child(
                        h_flex()
                            .pt(space::SM)
                            .gap(space::SM)
                            .child(
                                div()
                                    .text_size(typography::SM)
                                    .child(strings::FALLBACK_LABEL),
                            )
                            .child(
                                div()
                                    .w(size::FALLBACK_SELECT)
                                    .child(select(&f.fallback, strings::FALLBACK_LABEL).w_full()),
                            ),
                    )
                    .into_any_element(),
            );
        }
        section(
            "dictation-transcription",
            strings::TRANSCRIPTION_TITLE,
            Some(strings::TRANSCRIPTION_HELP),
            body,
            cx,
        )
    }

    fn cleanup_section(&self, s: &DictationSettings, cx: &mut Context<Self>) -> AnyElement {
        let tab = &self.settings.dictation;
        let f = &tab.fields;
        let palette = palette(cx);
        let mut body = vec![
            v_flex()
                .gap(space::SM)
                .child(field_label(strings::PREVIEW_LABEL))
                .child(
                    h_flex()
                        .gap(space::SM)
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .child(text_input(&f.preview, strings::PREVIEW_LABEL)),
                        )
                        .child(
                            controls::button("dictation-preview", Variant::Outline)
                                .label(strings::PREVIEW_SHOW)
                                .on_click(cx.listener(|shell, _, window, cx| {
                                    shell.run_preview(window, cx)
                                })),
                        ),
                )
                .when_some(
                    tab.preview.as_ref().filter(|p| !p.is_empty()),
                    |this, preview| {
                        this.child(
                            div()
                                .id("dictation-preview-result")
                                .role(gpui::Role::Status)
                                .px(space::MD)
                                .py(space::SM)
                                .rounded(theme::RADIUS)
                                .border_1()
                                .border_color(palette.border)
                                .bg(palette.muted)
                                .text_size(typography::SM)
                                .child(preview.clone()),
                        )
                    },
                )
                .into_any_element(),
            self.toggle(
                "dictation-stutters",
                strings::STUTTERS_LABEL,
                strings::STUTTERS_HINT,
                s.collapse_stutters,
                |s, on| s.collapse_stutters = on,
                cx,
            ),
            self.toggle(
                "dictation-polish",
                strings::POLISH_LABEL,
                strings::POLISH_HINT,
                s.polish_enabled,
                |s, on| s.polish_enabled = on,
                cx,
            ),
        ];
        if s.polish_enabled {
            let mut grid = v_flex().gap(space::MD).child(
                h_flex()
                    .gap(space::MD)
                    .items_start()
                    .child(div().flex_1().min_w_0().child(small_labelled(
                        strings::POLISH_PROVIDER,
                        select(&f.provider, strings::POLISH_PROVIDER).w_full(),
                    )))
                    .child(div().flex_1().min_w_0().child(small_labelled(
                        strings::POLISH_MODEL,
                        text_input(&f.polish_model, strings::POLISH_MODEL),
                    ))),
            );
            if s.polish_provider == "ollama" {
                grid = grid.child(small_labelled(
                    strings::OLLAMA_ENDPOINT,
                    text_input(&f.polish_endpoint, strings::OLLAMA_ENDPOINT),
                ));
            }
            body.push(grid.into_any_element());
        }
        section(
            "dictation-cleanup",
            strings::CLEANUP_TITLE,
            Some(strings::CLEANUP_HELP),
            body,
            cx,
        )
    }

    fn hook_section(&self, s: &DictationSettings, cx: &mut Context<Self>) -> AnyElement {
        let tab = &self.settings.dictation;
        let folder = if tab.hook_folder.is_empty() {
            strings::HOOK_FOLDER_UNKNOWN
        } else {
            tab.hook_folder.as_str()
        };
        let where_line = if tab.hook_scripts.is_empty() {
            strings::hook_no_scripts(folder)
        } else {
            strings::hook_scripts_in(folder)
        };
        let body = vec![
            self.toggle(
                "dictation-hook",
                strings::HOOK_LABEL,
                strings::HOOK_HINT,
                s.text_hook_enabled,
                |s, on| s.text_hook_enabled = on,
                cx,
            ),
            v_flex()
                .gap(space::XS)
                .child(
                    div()
                        .text_size(typography::SM)
                        .font_weight(FontWeight::MEDIUM)
                        .child(strings::HOOK_SCRIPT),
                )
                .child(
                    div()
                        .w_full()
                        .max_w(size::SHORTCUT_INPUT_MAX)
                        .child(select(&tab.fields.hook, strings::HOOK_SCRIPT).w_full()),
                )
                .child(muted(&where_line, cx))
                .into_any_element(),
        ];
        section(
            "dictation-hook",
            strings::HOOK_TITLE,
            Some(strings::HOOK_HELP),
            body,
            cx,
        )
    }

    fn styles_section(&self, cx: &mut Context<Self>) -> AnyElement {
        let tab = &self.settings.dictation;
        let changed = tab.styles_changed(cx);
        let rows = v_flex()
            .gap(space::SM)
            .children(tab.styles.iter().enumerate().map(|(index, row)| {
                let name = row.process.read(cx).value().to_string();
                let program = if name.is_empty() {
                    strings::STYLE_THE_PROGRAM.to_string()
                } else {
                    name.clone()
                };
                let remove = if name.is_empty() {
                    strings::STYLE_THE_ROW.to_string()
                } else {
                    name
                };
                h_flex()
                    .id(("dictation-style", index))
                    .gap(space::SM)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(text_input(&row.process, strings::STYLE_PROGRAM)),
                    )
                    .child(
                        div()
                            .w(size::STYLE_SELECT)
                            .child(select(&row.style, &strings::style_for(&program)).w_full()),
                    )
                    .child(
                        controls::icon_button(
                            ("dictation-style-remove", index),
                            IconName::Trash,
                            strings::style_remove(&remove),
                            Variant::Ghost,
                        )
                        .self_center()
                        .on_click(cx.listener(move |shell, _, _, cx| {
                            let styles = &mut shell.settings.dictation.styles;
                            if index < styles.len() {
                                styles.remove(index);
                            }
                            cx.notify();
                        })),
                    )
                    .into_any_element()
            }));
        let buttons = h_flex()
            .gap(space::SM)
            .child(
                controls::button("dictation-style-add", Variant::Outline)
                    .icon(IconName::Plus)
                    .label(strings::STYLE_ADD)
                    .on_click(cx.listener(|shell, _, window, cx| shell.add_style_row(window, cx))),
            )
            .child(
                controls::button("dictation-styles-save", Variant::Primary)
                    .label(strings::STYLES_SAVE)
                    .disabled(!changed)
                    .on_click(cx.listener(|shell, _, window, cx| shell.save_styles(window, cx))),
            );
        section(
            "dictation-styles",
            strings::STYLES_TITLE,
            Some(strings::STYLES_HELP),
            vec![rows.into_any_element(), buttons.into_any_element()],
            cx,
        )
    }

    fn words_section(&self, words: &[DictionaryWord], cx: &mut Context<Self>) -> AnyElement {
        let tab = &self.settings.dictation;
        let f = &tab.fields;
        let typed = !f.word.read(cx).value().trim().is_empty();
        let form = h_flex()
            .id("dictation-word-form")
            .role(gpui::Role::Form)
            .items_end()
            .gap(space::SM)
            .child(div().flex_1().min_w_0().child(small_labelled(
                strings::WORD_LABEL,
                text_input(&f.word, strings::WORD_LABEL),
            )))
            .child(div().flex_1().min_w_0().child(small_labelled(
                strings::ALIASES_LABEL,
                text_input(&f.aliases, strings::ALIASES_LABEL),
            )))
            .child(
                controls::button("dictation-word-save", Variant::Primary)
                    .label(if tab.word_id.is_none() {
                        strings::WORD_ADD
                    } else {
                        strings::WORD_SAVE
                    })
                    .disabled(!typed)
                    .self_end()
                    .on_click(cx.listener(|shell, _, window, cx| shell.save_word(window, cx))),
            );
        let list = if words.is_empty() {
            muted(strings::WORDS_EMPTY, cx).into_any_element()
        } else {
            list_box(
                "dictation-words",
                words
                    .iter()
                    .map(|w| {
                        let palette = palette(cx);
                        let edit = w.clone();
                        let id = w.id;
                        list_row(
                            ("dictation-word", w.id as usize).into(),
                            // Word and "· høres som" on one line, as Tauri's spans.
                            h_flex()
                                .flex_wrap()
                                .items_baseline()
                                .child(div().font_weight(FontWeight::MEDIUM).child(w.word.clone()))
                                .when(!w.aliases.is_empty(), |this| {
                                    this.child(
                                        div()
                                            .text_size(typography::SM)
                                            .text_color(palette.muted_foreground)
                                            .child(strings::word_heard_as(&w.aliases)),
                                    )
                                })
                                .into_any_element(),
                            &w.word,
                            ("dictation-word-edit", w.id as usize).into(),
                            ("dictation-word-delete", w.id as usize).into(),
                            cx.listener(move |shell, _, window, cx| {
                                shell.edit_word(edit.clone(), window, cx)
                            }),
                            cx.listener(move |shell, _, window, cx| {
                                shell.delete_word(id, window, cx)
                            }),
                            false,
                            cx,
                        )
                    })
                    .collect(),
                cx,
            )
        };
        section(
            "dictation-words",
            strings::WORDS_TITLE,
            Some(strings::WORDS_HELP),
            vec![form.into_any_element(), list],
            cx,
        )
    }

    fn snippets_section(&self, snippets: &[SnippetRow], cx: &mut Context<Self>) -> AnyElement {
        let tab = &self.settings.dictation;
        let f = &tab.fields;
        let ready = !f.phrase.read(cx).value().trim().is_empty()
            && !f.snippet_text.read(cx).value().trim().is_empty();
        let form = v_flex()
            .id("dictation-snippet-form")
            .role(gpui::Role::Form)
            .gap(space::SM)
            .child(small_labelled(
                strings::PHRASE_LABEL,
                text_input(&f.phrase, strings::PHRASE_LABEL),
            ))
            .child(small_labelled(
                strings::SNIPPET_TEXT,
                Textarea::new(&f.snippet_text).aria_label(strings::SNIPPET_TEXT),
            ))
            .child(
                controls::button("dictation-snippet-save", Variant::Primary)
                    .label(if tab.snippet_id.is_none() {
                        strings::SNIPPET_ADD
                    } else {
                        strings::SNIPPET_SAVE
                    })
                    .disabled(!ready)
                    .on_click(cx.listener(|shell, _, window, cx| shell.save_snippet(window, cx))),
            );
        let list = if snippets.is_empty() {
            muted(strings::SNIPPETS_EMPTY, cx).into_any_element()
        } else {
            list_box(
                "dictation-snippets",
                snippets
                    .iter()
                    .map(|s| {
                        let edit = s.clone();
                        let id = s.id;
                        list_row(
                            ("dictation-snippet", s.id as usize).into(),
                            v_flex()
                                .child(
                                    div()
                                        .font_weight(FontWeight::MEDIUM)
                                        .child(s.phrase.clone()),
                                )
                                .child(div().line_clamp(3).child(muted(&s.text, cx)))
                                .into_any_element(),
                            &s.phrase,
                            ("dictation-snippet-edit", s.id as usize).into(),
                            ("dictation-snippet-delete", s.id as usize).into(),
                            cx.listener(move |shell, _, window, cx| {
                                shell.edit_snippet(edit.clone(), window, cx)
                            }),
                            cx.listener(move |shell, _, window, cx| {
                                shell.delete_snippet(id, window, cx)
                            }),
                            true,
                            cx,
                        )
                    })
                    .collect(),
                cx,
            )
        };
        section(
            "dictation-snippets",
            strings::SNIPPETS_TITLE,
            Some(strings::SNIPPETS_HELP),
            vec![form.into_any_element(), list],
            cx,
        )
    }

    fn history_section(&self, s: &DictationSettings, cx: &mut Context<Self>) -> AnyElement {
        let keep_audio = switch_row(
            "dictation-keep-audio",
            strings::AUDIO_LABEL,
            Some(strings::AUDIO_HINT),
            controls::switch("dictation-keep-audio-switch")
                .checked(s.keep_audio)
                .disabled(!s.history_enabled)
                .accessibility_label(strings::AUDIO_LABEL)
                .on_click(cx.listener(|shell, value: &bool, window, cx| {
                    let on = *value;
                    shell.save_dictation(move |s| s.keep_audio = on, window, cx)
                })),
            cx,
        )
        .into_any_element();
        let body = vec![
            self.toggle(
                "dictation-history",
                strings::HISTORY_LABEL,
                strings::HISTORY_HINT,
                s.history_enabled,
                |s, on| s.history_enabled = on,
                cx,
            ),
            keep_audio,
            muted(strings::PRIVACY_NOTE, cx).into_any_element(),
        ];
        section("dictation-history", strings::HISTORY_TITLE, None, body, cx)
    }

    /// Tauri's `ToggleRow` saving one flag.
    fn toggle(
        &self,
        id: &'static str,
        label: &'static str,
        hint: &'static str,
        checked: bool,
        set: fn(&mut DictationSettings, bool),
        cx: &mut Context<Self>,
    ) -> AnyElement {
        switch_row(
            id,
            label,
            Some(hint),
            controls::switch((id, 1usize))
                .checked(checked)
                .accessibility_label(label)
                .on_click(cx.listener(move |shell, value: &bool, window, cx| {
                    let on = *value;
                    shell.save_dictation(move |s| set(s, on), window, cx)
                })),
            cx,
        )
        .into_any_element()
    }
}

/// A Tauri `Section` of this tab: a 20 px serif title, the description and
/// the controls 16 px apart. Unlike Generelt, the sections have no rule
/// between them (`space-y-10` only), as in `indstillinger-diktering-*.png`.
fn section(
    id: &'static str,
    title: &'static str,
    help: Option<&'static str>,
    body: Vec<AnyElement>,
    cx: &App,
) -> AnyElement {
    v_flex()
        .id(id)
        .role(gpui::Role::Group)
        .aria_label(SharedString::from(title))
        .gap(space::LG)
        .child(
            v_flex()
                .child(subheading_regular(title))
                .when_some(help, |this, help| {
                    this.child(div().mt(space::XS).child(muted(help, cx)))
                }),
        )
        .children(body)
        .into_any_element()
}

/// A field with its name above it (`font-medium`, `space-y-2`).
fn labelled(label: &'static str, control: impl IntoElement) -> AnyElement {
    v_flex()
        .gap(space::SM)
        .child(field_label(label))
        .child(control)
        .into_any_element()
}

/// A field with a 14 px medium name above it (`text-sm font-medium`).
fn small_labelled(label: &'static str, control: impl IntoElement) -> AnyElement {
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
}

/// Tauri's `Input` (36 px) named after its label, for screen readers.
fn text_input(state: &Entity<InputState>, label: &'static str) -> Input {
    controls::input(state).aria_label(label)
}

/// Tauri's native `select` (`h-11`).
fn select(state: &Entity<Choices>, label: &str) -> Select<Vec<SharedString>> {
    Select::new(state)
        .accessibility_label(SharedString::from(label.to_string()))
        .h(layout::CONTROL_HEIGHT)
}

/// The bordered list of Ordbog and Snippets (`divide-y rounded-lg border`).
fn list_box(id: &'static str, rows: Vec<AnyElement>, cx: &App) -> AnyElement {
    let palette = palette(cx);
    let last = rows.len().saturating_sub(1);
    v_flex()
        .id(id)
        .role(gpui::Role::List)
        .rounded(theme::RADIUS_LG)
        .border_1()
        .border_color(palette.border)
        .children(rows.into_iter().enumerate().map(|(index, row)| {
            div()
                .when(index < last, |this| {
                    this.border_b_1().border_color(palette.border)
                })
                .child(row)
        }))
        .into_any_element()
}

/// One list row: the content and Ret / Slet icon buttons.
#[allow(clippy::too_many_arguments)]
fn list_row(
    id: gpui::ElementId,
    content: AnyElement,
    name: &str,
    edit_id: gpui::ElementId,
    delete_id: gpui::ElementId,
    on_edit: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
    on_delete: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
    top: bool,
    _cx: &App,
) -> AnyElement {
    h_flex()
        .id(id)
        .role(gpui::Role::ListItem)
        .justify_between()
        .when(top, |this| this.items_start())
        .gap(space::MD)
        .px(space::LG)
        .py(space::SM)
        .child(div().min_w_0().flex_1().child(content))
        .child(
            h_flex()
                .flex_none()
                .child(
                    controls::icon_button(
                        edit_id,
                        IconName::Pencil,
                        strings::edit(name),
                        Variant::Ghost,
                    )
                    .on_click(on_edit),
                )
                .child(
                    controls::icon_button(
                        delete_id,
                        IconName::Trash,
                        strings::delete(name),
                        Variant::Ghost,
                    )
                    .on_click(on_delete),
                ),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hybrid_threshold_is_clamped_like_tauri() {
        assert_eq!(hybrid_threshold("250"), 250);
        assert_eq!(hybrid_threshold(" 249.6 "), 250);
        assert_eq!(hybrid_threshold("20"), HYBRID_MIN);
        assert_eq!(hybrid_threshold("5000"), HYBRID_MAX);
        assert_eq!(hybrid_threshold(""), HYBRID_DEFAULT);
        assert_eq!(hybrid_threshold("abc"), HYBRID_DEFAULT);
        assert_eq!(hybrid_threshold("NaN"), HYBRID_DEFAULT);
    }

    #[test]
    fn choices_follow_the_tauri_order() {
        assert_eq!(
            MODES.map(|(m, _)| m),
            [
                ShortcutMode::Hold,
                ShortcutMode::Toggle,
                ShortcutMode::Hybrid,
                ShortcutMode::DoubleTap
            ]
        );
        assert_eq!(ENGINES.map(|(v, _)| v), ["auto", "local"]);
        assert_eq!(PROVIDERS.map(|(v, _)| v), ["builtin-ai", "ollama"]);
        assert_eq!(CHANNELS.map(|(v, _)| v), ["mix", "left", "right"]);
        assert_eq!(STYLES.map(|(v, _)| v), ["raw", "normal"]);
        // Core's defaults are among the choices, so the selects show them.
        let defaults = DictationSettings::default();
        assert!(FALLBACKS.contains(&defaults.fallback_after_ms));
        assert!(ENGINES.iter().any(|(v, _)| *v == defaults.engine));
        assert!(PROVIDERS
            .iter()
            .any(|(v, _)| *v == defaults.polish_provider));
        assert!(CHANNELS.iter().any(|(v, _)| *v == defaults.channel));
        assert!(MODES.iter().any(|(m, _)| *m == defaults.shortcut_mode));
        assert!((HYBRID_MIN..=HYBRID_MAX).contains(&defaults.hybrid_threshold_ms));
    }
}
