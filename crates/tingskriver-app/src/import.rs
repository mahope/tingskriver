//! Import of audio files and transcripts, as in the Tauri app
//! (`frontend/src/components/ImportAudio/`, `contexts/ImportDialogContext.tsx`,
//! `hooks/useImportAudio.ts`, `src-tauri/src/audio/import.rs`).
//!
//! * "Importér lyd" opens a dialog: pick a file (or drop one), check it, name
//!   the meeting, optionally choose language and model, import with progress,
//!   cancel, or see the error and try again. Core does the work
//!   (`audio::import`) and reports through the `import-progress`,
//!   `import-complete` and `import-error` events.
//! * "Importér transskript" picks a `.vtt` or `.srt` file and core
//!   (`subtitle::import`) saves the meeting and starts the summary.
//! * The audio import is a Beta feature (`importAndRetranscribe`, on by
//!   default). Tauri keeps the switch in the web view's local storage, which
//!   this app cannot read, so the GPUI app keeps its own in the settings
//!   store [`BETA_STORE`].

use std::path::{Path, PathBuf};

use gpui::{
    div, prelude::*, px, AnyElement, App, Context, Entity, ExternalPaths, PathPromptOptions,
    Pixels, SharedString, Subscription, Window,
};
use gpui_component::{
    h_flex,
    input::InputState,
    notification::Notification,
    select::{Select, SelectState},
    v_flex, Disableable, Icon, IndexPath, WindowExt,
};
use gpui_kit_assets::IconName;
use tingskriver_core::audio::constants::AUDIO_EXTENSIONS;
use tingskriver_core::audio::import::{AudioFileInfo, ImportProgress, ImportResult};
use tingskriver_core::host::HostHandle;
use tingskriver_core::strings::{self, import as text};

use crate::controls::{self, Variant};
use crate::startup::DbState;
use crate::theme::{self, icon_size, layout, space, typography};
use crate::ui::{palette, AppModel, Shell};

/// The settings file and key of the Beta switch.
pub(crate) const BETA_STORE: &str = "beta_features.json";
pub(crate) const BETA_KEY: &str = "importAndRetranscribe";

/// `sm:max-w-[500px]`.
const DIALOG_WIDTH: Pixels = px(500.0);

/// Whisper's languages as Tauri lists them (`constants/languages.ts`).
const LANGUAGES: &[(&str, &str)] = &[
    ("auto", "Find sprog automatisk (originalsprog)"),
    (
        "auto-translate",
        "Find sprog automatisk (oversæt til engelsk)",
    ),
    ("en", "Engelsk"),
    ("zh", "Kinesisk"),
    ("de", "Tysk"),
    ("es", "Spansk"),
    ("ru", "Russisk"),
    ("ko", "Koreansk"),
    ("fr", "Fransk"),
    ("ja", "Japansk"),
    ("pt", "Portugisisk"),
    ("tr", "Tyrkisk"),
    ("pl", "Polsk"),
    ("ca", "Catalansk"),
    ("nl", "Hollandsk"),
    ("ar", "Arabisk"),
    ("sv", "Svensk"),
    ("it", "Italiensk"),
    ("id", "Indonesisk"),
    ("hi", "Hindi"),
    ("fi", "Finsk"),
    ("vi", "Vietnamesisk"),
    ("he", "Hebraisk"),
    ("uk", "Ukrainsk"),
    ("el", "Græsk"),
    ("ms", "Malajisk"),
    ("cs", "Tjekkisk"),
    ("ro", "Rumænsk"),
    ("da", "Dansk"),
    ("hu", "Ungarsk"),
    ("ta", "Tamil"),
    ("no", "Norsk"),
    ("th", "Thailandsk"),
    ("ur", "Urdu"),
    ("hr", "Kroatisk"),
    ("bg", "Bulgarsk"),
    ("lt", "Litauisk"),
];

type Choices = SelectState<Vec<SharedString>>;

/// Whether the Beta switch allows the audio import. Not set (or unreadable)
/// is Tauri's default: on.
pub(crate) fn beta_enabled(host: &HostHandle) -> bool {
    match host.read_setting(BETA_STORE, BETA_KEY) {
        Ok(Some(value)) => value.as_bool().unwrap_or(true),
        _ => true,
    }
}

pub(crate) fn set_beta_enabled(host: &HostHandle, on: bool) -> Result<(), String> {
    host.write_setting(BETA_STORE, BETA_KEY, serde_json::Value::Bool(on))
}

/// What a file is, by its extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FileKind {
    Audio,
    Subtitle,
    Other,
}

impl FileKind {
    pub(crate) fn of(path: &Path) -> FileKind {
        let extension = path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .unwrap_or_default();
        if matches!(extension.as_str(), "vtt" | "srt") {
            FileKind::Subtitle
        } else if AUDIO_EXTENSIONS.contains(&extension.as_str()) {
            FileKind::Audio
        } else {
            FileKind::Other
        }
    }
}

/// Where the dialog is (`useImportAudio`'s `status`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Phase {
    /// No file, or a checked file waiting for "Importér".
    Idle,
    /// The picked file is being checked.
    Validating,
    /// Core is importing.
    Running,
    /// The import or the check failed.
    Failed(String),
}

/// What core's `import-*` events say.
#[derive(Debug, Clone)]
pub(crate) enum ImportEvent {
    Progress(ImportProgress),
    Complete(ImportResult),
    Failed(String),
}

impl ImportEvent {
    /// `None` for other events and for payloads that cannot be read.
    pub(crate) fn parse(name: &str, payload: &str) -> Option<ImportEvent> {
        match name {
            "import-progress" => serde_json::from_str(payload)
                .ok()
                .map(ImportEvent::Progress),
            "import-complete" => match serde_json::from_str(payload) {
                Ok(result) => Some(ImportEvent::Complete(result)),
                Err(error) => {
                    log::warn!("import-complete: unreadable payload: {error}");
                    None
                }
            },
            "import-error" => Some(ImportEvent::Failed(
                serde_json::from_str::<serde_json::Value>(payload)
                    .ok()
                    .and_then(|v| v["error"].as_str().map(str::to_string))
                    .unwrap_or_else(|| text::IMPORT_FAILED_TOAST.to_string()),
            )),
            _ => None,
        }
    }
}

/// A transcription model the user can pick (`ModelOption`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ModelOption {
    /// `whisper` or `parakeet`.
    pub provider: &'static str,
    pub name: String,
    pub size_mb: u32,
}

impl ModelOption {
    pub(crate) fn label(&self) -> String {
        let icon_name = match self.provider {
            "parakeet" => "⚡ Parakeet",
            _ => "🏠 Whisper",
        };
        text::model_label(icon_name, &self.name, self.size_mb)
    }
}

/// The language code core gets: Parakeet detects the language itself and
/// "auto" means none (`handleStartImport`).
pub(crate) fn language_argument(code: &str, parakeet: bool) -> Option<String> {
    if parakeet || code == "auto" {
        None
    } else {
        Some(code.to_string())
    }
}

/// The title the meeting gets: the edited one, or the file name.
pub(crate) fn meeting_title(edited: &str, filename: &str) -> String {
    let edited = edited.trim();
    if edited.is_empty() {
        filename.to_string()
    } else {
        edited.to_string()
    }
}

/// The model to start on: the saved one if it is installed, else the first.
pub(crate) fn default_model(options: &[ModelOption], saved: Option<&(String, String)>) -> usize {
    saved
        .and_then(|(engine, model)| {
            let provider = if engine == "parakeet" {
                "parakeet"
            } else {
                "whisper"
            };
            options
                .iter()
                .position(|o| o.provider == provider && &o.name == model)
        })
        .unwrap_or(0)
}

/// The import dialog and the transcript import. Lives as long as the window,
/// so core's events always have somewhere to go.
pub(crate) struct ImportView {
    model: Entity<AppModel>,
    phase: Phase,
    file: Option<AudioFileInfo>,
    progress: Option<ImportProgress>,
    title: Entity<InputState>,
    advanced: bool,
    language: Entity<Choices>,
    models: Entity<Choices>,
    model_options: Vec<ModelOption>,
    models_loading: bool,
    /// The user cancelled: late events are ignored (`isCancelledRef`). Kept
    /// until core reports the cancelled import finished (its `import-error`
    /// or `import-complete`), so a dialog opened straight after "Annullér"
    /// does not show the old import's late events.
    cancelled: bool,
    /// Bumped by `reset`; a file check that answers for an older dialog is
    /// dropped.
    generation: u64,
    /// A transcript import is running.
    transcript_busy: bool,
    /// The Beta switch (`betaFeatures.importAndRetranscribe`).
    beta: bool,
    _subscriptions: Vec<Subscription>,
}

impl ImportView {
    pub(crate) fn new(
        model: Entity<AppModel>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let beta = model
            .read(cx)
            .host
            .as_ref()
            .map(beta_enabled)
            .unwrap_or(true);
        let title =
            cx.new(|cx| InputState::new(window, cx).placeholder(text::IMPORT_TITLE_PLACEHOLDER));
        let labels: Vec<SharedString> = LANGUAGES.iter().map(|(_, n)| (*n).into()).collect();
        let language = cx.new(|cx| SelectState::new(labels, Some(IndexPath::new(0)), window, cx));
        let models = cx.new(|cx| SelectState::new(Vec::<SharedString>::new(), None, window, cx));
        ImportView {
            model,
            phase: Phase::Idle,
            file: None,
            progress: None,
            title,
            advanced: false,
            language,
            models,
            model_options: Vec::new(),
            models_loading: false,
            cancelled: false,
            generation: 0,
            transcript_busy: false,
            beta,
            _subscriptions: Vec::new(),
        }
    }

    /// Whether "Importér lyd" is shown (the Beta switch).
    pub(crate) fn audio_enabled(&self) -> bool {
        self.beta
    }

    /// The Beta switch was changed: save it, and only then show it.
    pub(crate) fn save_beta(&mut self, on: bool, cx: &mut Context<Self>) {
        let saved = match self.model.read(cx).host.clone() {
            Some(host) => set_beta_enabled(&host, on),
            None => Err(text::IMPORT_NO_HOST.to_string()),
        };
        match saved {
            Ok(()) => self.beta = on,
            Err(error) => {
                log::warn!("Could not save the Beta switch: {error}");
                crate::ui::notify_main_window(
                    Notification::error(text::IMPORT_BETA_SAVE_FAILED),
                    cx,
                );
            }
        }
        cx.notify();
    }

    pub(crate) fn is_running(&self) -> bool {
        self.phase == Phase::Running
    }

    /// A fresh dialog (`reset()` when it opens).
    fn reset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.phase = Phase::Idle;
        self.file = None;
        self.progress = None;
        self.advanced = false;
        self.generation += 1;
        self.model_options.clear();
        self.title
            .update(cx, |input, cx| input.set_value("", window, cx));
        let primary = self.model.read(cx).languages.primary.clone();
        let at = LANGUAGES
            .iter()
            .position(|(c, _)| *c == primary)
            .unwrap_or(0);
        self.language.update(cx, |select, cx| {
            select.set_selected_index(Some(IndexPath::new(at)), window, cx)
        });
        self.load_models(window, cx);
        cx.notify();
    }

    /// Fetch the installed Whisper and Parakeet models (`fetchModels`). A
    /// failure leaves the list empty, as in Tauri, and core's default model
    /// is used.
    fn load_models(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(host) = self.model.read(cx).host.clone() else {
            return;
        };
        self.models_loading = true;
        let task = self.model.read(cx).runtime.spawn(async move {
            use tingskriver_core::{parakeet_engine, whisper_engine};
            let mut options = Vec::new();
            let host_for_dirs = host.clone();
            let _ = tokio::task::spawn_blocking(move || {
                whisper_engine::commands::set_models_directory(&host_for_dirs);
                parakeet_engine::commands::set_models_directory(&host_for_dirs);
            })
            .await;
            match whisper_engine::whisper_get_available_models().await {
                Ok(models) => options.extend(
                    models
                        .into_iter()
                        .filter(|m| {
                            matches!(
                                m.status,
                                whisper_engine::whisper_engine::ModelStatus::Available
                            )
                        })
                        .map(|m| ModelOption {
                            provider: "whisper",
                            name: m.name,
                            size_mb: m.size_mb,
                        }),
                ),
                Err(error) => log::warn!("Import: could not list Whisper models: {error}"),
            }
            if let Err(error) = parakeet_engine::parakeet_init().await {
                log::warn!("Import: could not start the Parakeet engine: {error}");
            }
            match parakeet_engine::parakeet_get_available_models().await {
                Ok(models) => options.extend(
                    models
                        .into_iter()
                        .filter(|m| matches!(m.status, parakeet_engine::ModelStatus::Available))
                        .map(|m| ModelOption {
                            provider: "parakeet",
                            name: m.name,
                            size_mb: m.size_mb,
                        }),
                ),
                Err(error) => log::warn!("Import: could not list Parakeet models: {error}"),
            }
            options
        });
        cx.spawn_in(window, async move |this, cx| {
            let options = task.await.unwrap_or_default();
            let _ = this.update_in(cx, |this, window, cx| {
                let saved = this
                    .model
                    .read(cx)
                    .settings
                    .saved_models
                    .transcription
                    .clone();
                let at = default_model(&options, saved.as_ref());
                let labels: Vec<SharedString> = options.iter().map(|o| o.label().into()).collect();
                let selected = (!labels.is_empty()).then(|| IndexPath::new(at));
                this.models.update(cx, |select, cx| {
                    select.set_items(labels, window, cx);
                    select.set_selected_index(selected, window, cx);
                });
                this.model_options = options;
                this.models_loading = false;
                cx.notify();
            });
        })
        .detach();
    }

    /// The model the select shows.
    fn selected_model(&self, cx: &App) -> Option<&ModelOption> {
        let at = self.models.read(cx).selected_index(cx)?.row;
        self.model_options.get(at)
    }

    fn selected_language(&self, cx: &App) -> &'static str {
        self.language
            .read(cx)
            .selected_index(cx)
            .and_then(|at| LANGUAGES.get(at.row))
            .map(|(code, _)| *code)
            .unwrap_or("auto")
    }

    /// "Vælg lydfil": the native file picker, then the check.
    fn pick_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: None,
        });
        self.phase = Phase::Validating;
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let path = match picked.await {
                Ok(Ok(Some(mut paths))) if !paths.is_empty() => Some(paths.remove(0)),
                Ok(Err(error)) => {
                    let message = error.to_string();
                    let _ = this.update(cx, |this, cx| this.fail(message, cx));
                    return;
                }
                _ => None,
            };
            let _ = this.update_in(cx, |this, window, cx| match path {
                Some(path) => this.validate(path, window, cx),
                None => {
                    // Cancelled: back to where it was.
                    this.phase = Phase::Idle;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Check a file (`validateFile`) and show it.
    fn validate(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.phase = Phase::Validating;
        self.generation += 1;
        let generation = self.generation;
        cx.notify();
        let task = self.model.read(cx).runtime.spawn_blocking(move || {
            tingskriver_core::audio::import::validate_audio_file(&path).map_err(|e| e.to_string())
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = task
                .await
                .unwrap_or_else(|_| Err(strings::import::IMPORT_FILE_CHECK_FAILED.to_string()));
            let _ = this.update_in(cx, |this, window, cx| match result {
                _ if this.generation != generation => {}
                Ok(info) => {
                    this.title.update(cx, |input, cx| {
                        input.set_value(info.filename.clone(), window, cx)
                    });
                    this.file = Some(info);
                    this.phase = Phase::Idle;
                    cx.notify();
                }
                Err(error) => this.fail(error, cx),
            });
        })
        .detach();
    }

    fn fail(&mut self, message: String, cx: &mut Context<Self>) {
        log::warn!("Import failed: {message}");
        self.phase = Phase::Failed(message.clone());
        self.progress = None;
        crate::ui::notify_main_window(
            Notification::error(message).title(text::IMPORT_FAILED_TOAST),
            cx,
        );
        cx.notify();
    }

    /// "Importér" (`handleStartImport`).
    fn start(&mut self, cx: &mut Context<Self>) {
        let Some(file) = self.file.clone() else {
            return;
        };
        let Some(host) = self.model.read(cx).host.clone() else {
            self.phase = Phase::Failed(text::IMPORT_NO_HOST.to_string());
            cx.notify();
            return;
        };
        let (model_name, provider) = match self.selected_model(cx) {
            Some(option) => (Some(option.name.clone()), Some(option.provider.to_string())),
            None => (None, None),
        };
        let parakeet = provider.as_deref() == Some("parakeet");
        let language = language_argument(self.selected_language(cx), parakeet);
        let title = meeting_title(&self.title.read(cx).value(), &file.filename);
        if self.cancelled {
            if tingskriver_core::audio::import::is_import_in_progress() {
                // Core is still stopping the cancelled import.
                self.phase = Phase::Failed(text::IMPORT_BUSY.to_string());
                cx.notify();
                return;
            }
            self.cancelled = false;
        }
        self.phase = Phase::Running;
        self.progress = None;
        cx.notify();
        let task = self.model.read(cx).runtime.spawn(async move {
            tingskriver_core::audio::import::start_import_audio_command(
                host, file.path, title, language, model_name, provider,
            )
            .await
        });
        cx.spawn(async move |this, cx| {
            // Core answers at once and imports in the background; only a
            // refusal ("already in progress") arrives here.
            if let Ok(Err(error)) = task.await {
                let error = if error == "Import already in progress" {
                    text::IMPORT_BUSY.to_string()
                } else {
                    error
                };
                let _ = this.update(cx, |this, cx| this.fail(error, cx));
            }
        })
        .detach();
    }

    /// "Annullér" while importing.
    fn cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_running() {
            self.cancelled = true;
            self.model
                .read(cx)
                .runtime
                .spawn(tingskriver_core::audio::import::cancel_import_command());
            self.phase = Phase::Idle;
            self.progress = None;
            crate::ui::notify_main_window(Notification::info(text::IMPORT_CANCELLED), cx);
        }
        window.close_dialog(cx);
        cx.notify();
    }

    /// One of core's `import-*` events (`useImportAudio`'s listeners).
    pub(crate) fn on_event(&mut self, name: &str, payload: &str, cx: &mut Context<Self>) {
        let event = ImportEvent::parse(name, payload);
        if self.cancelled {
            // The cancelled import has finished: later events are new ones.
            if matches!(
                event,
                Some(ImportEvent::Complete(_)) | Some(ImportEvent::Failed(_))
            ) {
                self.cancelled = false;
            }
            return;
        }
        match event {
            Some(ImportEvent::Progress(progress)) => {
                self.progress = Some(progress);
                self.phase = Phase::Running;
                cx.notify();
            }
            Some(ImportEvent::Complete(result)) => {
                self.phase = Phase::Idle;
                self.progress = None;
                self.file = None;
                crate::ui::notify_main_window(
                    Notification::success(text::import_done(result.segments_count)),
                    cx,
                );
                crate::ui::close_main_dialog(cx);
                crate::ui::meetings_changed(cx);
                crate::ui::open_meeting(result.meeting_id, cx);
                cx.notify();
            }
            Some(ImportEvent::Failed(message)) => self.fail(message, cx),
            None => {}
        }
    }

    /// "Importér transskript": pick a `.vtt`/`.srt` file, or use the one
    /// that was dropped (`importTranscript`).
    fn run_transcript_import(
        &mut self,
        path: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.transcript_busy || self.is_running() {
            crate::ui::notify_main_window(Notification::info(text::IMPORT_BUSY), cx);
            return;
        }
        self.transcript_busy = true;
        let picked = path.is_none().then(|| {
            cx.prompt_for_paths(PathPromptOptions {
                files: true,
                directories: false,
                multiple: false,
                prompt: None,
            })
        });
        cx.spawn_in(window, async move |this, cx| {
            let path = match (path, picked) {
                (Some(path), _) => Some(path),
                (None, Some(picked)) => match picked.await {
                    Ok(Ok(Some(mut paths))) if !paths.is_empty() => Some(paths.remove(0)),
                    _ => None,
                },
                (None, None) => None,
            };
            let Some(path) = path else {
                // Cancelled.
                let _ = this.update(cx, |this, _| this.transcript_busy = false);
                return;
            };
            let started = this.update(cx, |this, cx| this.start_transcript(path, cx));
            let task = match started {
                Ok(Some(task)) => task,
                _ => {
                    let _ = this.update(cx, |this, _| this.transcript_busy = false);
                    return;
                }
            };
            let result = task
                .await
                .unwrap_or_else(|_| Err(text::IMPORT_TRANSCRIPT_FILE_FAILED.to_string()));
            let _ = this.update(cx, |this, cx| {
                this.transcript_busy = false;
                this.finish_transcript(result, cx);
            });
        })
        .detach();
    }

    /// Check the file type and start core's import on the runtime.
    fn start_transcript(
        &mut self,
        path: PathBuf,
        cx: &mut Context<Self>,
    ) -> Option<
        tokio::task::JoinHandle<
            Result<tingskriver_core::subtitle::import::SubtitleImportOutcome, String>,
        >,
    > {
        if FileKind::of(&path) != FileKind::Subtitle {
            crate::ui::notify_main_window(Notification::error(text::IMPORT_SUBTITLES_ONLY), cx);
            return None;
        }
        let state = self.model.read(cx);
        let (Some(host), DbState::Ready(db)) = (state.host.clone(), &state.db) else {
            let message = if state.host.is_none() {
                text::IMPORT_NO_HOST
            } else {
                text::IMPORT_NO_DATABASE
            };
            crate::ui::notify_main_window(
                Notification::error(message).title(text::IMPORT_TRANSCRIPT_FAILED),
                cx,
            );
            return None;
        };
        let app_state = tingskriver_core::state::AppState {
            db_manager: db.clone(),
        };
        Some(state.runtime.spawn(async move {
            tingskriver_core::subtitle::import::import_subtitle_file(
                host,
                &app_state,
                &path.to_string_lossy(),
            )
            .await
        }))
    }

    fn finish_transcript(
        &mut self,
        result: Result<tingskriver_core::subtitle::import::SubtitleImportOutcome, String>,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok(outcome) => {
                if outcome.already_imported {
                    crate::ui::notify_main_window(
                        Notification::info(text::IMPORT_TRANSCRIPT_EXISTS_TEXT)
                            .title(text::IMPORT_TRANSCRIPT_EXISTS),
                        cx,
                    );
                } else {
                    crate::ui::notify_main_window(
                        Notification::success(text::transcript_done_text(outcome.segments_count))
                            .title(text::IMPORT_TRANSCRIPT_DONE),
                        cx,
                    );
                }
                if let Some(error) = &outcome.summary_error {
                    log::warn!("Imported transcript: the summary did not start: {error}");
                    crate::ui::notify_main_window(
                        Notification::error(text::IMPORT_SUMMARY_FAILED_TEXT)
                            .title(text::IMPORT_SUMMARY_FAILED),
                        cx,
                    );
                }
                crate::ui::meetings_changed(cx);
                crate::ui::open_meeting(outcome.meeting_id, cx);
            }
            Err(error) => crate::ui::notify_main_window(
                Notification::error(error).title(text::IMPORT_TRANSCRIPT_FAILED),
                cx,
            ),
        }
    }
}

/// The import dialog over the current screen. It cannot be closed while the
/// import runs (`handleOpenChange`, `onEscapeKeyDown`).
fn show_dialog(view: Entity<ImportView>, window: &mut Window, cx: &mut App) {
    window.open_dialog(cx, move |dialog, _, cx| {
        let locked = view.read(cx).is_running();
        dialog
            .w(DIALOG_WIDTH)
            .p(space::XL)
            .centered()
            .close_button(!locked)
            .overlay_closable(!locked)
            .keyboard(!locked)
            .child(view.clone())
    });
}

/// The dialog states of the screenshot tests.
#[cfg(feature = "screenshot-tests")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FixtureImport {
    /// A checked file with "Flere valg" open.
    File,
    /// 42 % into the transcription.
    Running,
    /// The import failed.
    Failed,
}

#[cfg(feature = "screenshot-tests")]
impl ImportView {
    pub(crate) fn fixture(
        &mut self,
        step: FixtureImport,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.title.update(cx, |input, cx| {
            input.set_value("Statusmøde.m4a", window, cx)
        });
        self.file = Some(AudioFileInfo {
            path: "fixtures/Statusmøde.m4a".into(),
            filename: "Statusmøde.m4a".into(),
            duration_seconds: 3725.0,
            size_bytes: 58_720_256,
            format: "M4A".into(),
        });
        let primary = "da";
        let at = LANGUAGES
            .iter()
            .position(|(c, _)| *c == primary)
            .unwrap_or(0);
        self.language.update(cx, |select, cx| {
            select.set_selected_index(Some(IndexPath::new(at)), window, cx)
        });
        match step {
            FixtureImport::File => self.advanced = true,
            FixtureImport::Running => {
                self.phase = Phase::Running;
                self.progress = Some(ImportProgress {
                    stage: "transcribing".into(),
                    progress_percentage: 42,
                    message: "Transcribing audio 21/50".into(),
                });
            }
            FixtureImport::Failed => {
                self.phase = Phase::Failed("Filen kunne ikke dekodes.".into());
                self.file = None;
            }
        }
        show_dialog(cx.entity(), window, cx);
        cx.notify();
    }
}

impl Shell {
    /// "Importér lyd" (and a dropped audio file): the dialog, unless the
    /// Beta switch is off or an import is running.
    pub(crate) fn open_import_audio(
        &mut self,
        file: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let view = self.import.clone();
        if !view.read(cx).audio_enabled() {
            window.push_notification(
                Notification::error(text::IMPORT_BETA_OFF_TEXT).title(text::IMPORT_BETA_OFF_TITLE),
                cx,
            );
            return;
        }
        if self.model.read(cx).host.is_none() {
            window.push_notification(Notification::error(text::IMPORT_NO_HOST), cx);
            return;
        }
        if view.read(cx).is_running()
            || view.read(cx).transcript_busy
            || window.has_active_dialog(cx)
        {
            window.push_notification(Notification::info(text::IMPORT_BUSY), cx);
            return;
        }
        view.update(cx, |view, cx| {
            view.reset(window, cx);
            if let Some(file) = file {
                view.validate(file, window, cx);
            }
        });
        show_dialog(view, window, cx);
    }

    /// "Importér transskript": a file picker, or the dropped file.
    pub(crate) fn import_transcript(
        &mut self,
        file: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.import
            .update(cx, |view, cx| view.run_transcript_import(file, window, cx));
    }

    /// Files dropped on the window (`ImportDropOverlay`, `onDragDropEvent`):
    /// a subtitle file is imported, an audio file opens the dialog.
    pub(crate) fn drop_files(
        &mut self,
        paths: &ExternalPaths,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(path) = paths.paths().first().cloned() else {
            return;
        };
        match FileKind::of(&path) {
            FileKind::Subtitle => self.import_transcript(Some(path), window, cx),
            FileKind::Audio => self.open_import_audio(Some(path), window, cx),
            FileKind::Other => {
                window.push_notification(Notification::error(text::IMPORT_DROP_UNSUPPORTED), cx)
            }
        }
    }
}

impl Render for ImportView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = palette(cx);
        let running = self.is_running();
        let failed = match &self.phase {
            Phase::Failed(message) => Some(message.clone()),
            _ => None,
        };
        let validating = self.phase == Phase::Validating;
        let (icon, icon_color, title, lead) = if running {
            (
                IconName::LoaderCircle,
                palette.primary,
                text::IMPORT_DIALOG_BUSY_TITLE,
                self.progress
                    .as_ref()
                    .map(|p| p.message.clone())
                    .filter(|m| !m.is_empty())
                    .unwrap_or_else(|| text::IMPORT_DIALOG_BUSY_LEAD.to_string()),
            )
        } else if failed.is_some() {
            (
                IconName::CircleAlert,
                palette.destructive,
                text::IMPORT_DIALOG_FAILED_TITLE,
                text::IMPORT_DIALOG_FAILED_LEAD.to_string(),
            )
        } else {
            (
                IconName::Upload,
                palette.primary,
                text::IMPORT_DIALOG_TITLE,
                text::IMPORT_DIALOG_LEAD.to_string(),
            )
        };

        let header = v_flex()
            .gap(space::SM)
            .child(
                h_flex()
                    .gap(space::SM)
                    .child(Icon::new(icon).size(icon_size::MD).text_color(icon_color))
                    .child(crate::ui::section_title(title)),
            )
            .child(crate::ui::muted(&lead, cx));

        let mut body = v_flex().gap(space::LG).py(space::LG);
        if !running && failed.is_none() {
            body = body.child(match self.file.clone() {
                Some(file) => self.file_card(file, cx),
                None => self.empty_card(validating, cx),
            });
            if self.file.is_some() {
                body = body.child(self.more_options(cx));
            }
        }
        if running {
            let percentage = self
                .progress
                .as_ref()
                .map(|p| p.progress_percentage.min(100))
                .unwrap_or(0);
            let stage = self
                .progress
                .as_ref()
                .map(|p| p.stage.clone())
                .unwrap_or_default();
            let message = self
                .progress
                .as_ref()
                .map(|p| p.message.clone())
                .unwrap_or_default();
            body = body.child(
                v_flex()
                    .id("import-progress")
                    .role(gpui::Role::ProgressIndicator)
                    .aria_label(SharedString::from(text::percent(percentage)))
                    .gap(space::SM)
                    .child(
                        // `h-3 rounded-full bg-muted` with a primary fill.
                        div()
                            .w_full()
                            .h(px(12.0))
                            .rounded(theme::RADIUS_FULL)
                            .bg(palette.muted)
                            .child(
                                div()
                                    .h_full()
                                    .w(gpui::relative(percentage as f32 / 100.0))
                                    .rounded(theme::RADIUS_FULL)
                                    .bg(palette.primary),
                            ),
                    )
                    .child(
                        h_flex()
                            .justify_between()
                            .text_size(typography::XS)
                            .text_color(palette.muted_foreground)
                            .child(stage)
                            .child(text::percent(percentage)),
                    )
                    .child(
                        div()
                            .text_size(typography::SM)
                            .text_color(palette.muted_foreground)
                            .text_center()
                            .child(message),
                    ),
            );
        }
        if let Some(message) = failed.clone() {
            body = body.child(
                div()
                    .id("import-error")
                    .role(gpui::Role::Alert)
                    .p(space::MD)
                    .rounded(theme::RADIUS_LG)
                    .border_1()
                    .border_color(theme::over(palette.destructive, palette.card, 0.3))
                    .bg(theme::over(palette.destructive, palette.card, 0.1))
                    .text_size(typography::SM)
                    .text_color(palette.destructive)
                    .child(message),
            );
        }

        let footer = h_flex().justify_end().gap(space::SM);
        let footer = if running {
            footer.child(
                controls::button("import-cancel", Variant::Outline)
                    .label(text::IMPORT_CANCEL)
                    .on_click(cx.listener(|this, _, window, cx| this.cancel(window, cx))),
            )
        } else if failed.is_some() {
            footer
                .child(
                    controls::button("import-close", Variant::Outline)
                        .label(text::IMPORT_CLOSE)
                        .on_click(|_, window, cx| window.close_dialog(cx)),
                )
                .child(
                    controls::button("import-retry", Variant::Outline)
                        .label(text::IMPORT_RETRY)
                        .on_click(cx.listener(|this, _, window, cx| this.reset(window, cx))),
                )
        } else {
            footer
                .child(
                    controls::button("import-dismiss", Variant::Outline)
                        .label(text::IMPORT_CANCEL)
                        .on_click(|_, window, cx| window.close_dialog(cx)),
                )
                .child(
                    controls::button("import-start", Variant::Primary)
                        .label(text::IMPORT_START)
                        .icon(IconName::Upload)
                        .disabled(self.file.is_none() || validating)
                        .on_click(cx.listener(|this, _, _, cx| this.start(cx))),
                )
        };

        v_flex()
            .id("import-dialog")
            .role(gpui::Role::Dialog)
            .aria_label(SharedString::from(title))
            .font_family(typography::SANS)
            .text_size(typography::SM)
            .text_color(palette.foreground)
            .child(header)
            .child(body)
            .child(footer)
    }
}

impl ImportView {
    /// No file yet: the dashed box with "Vælg lydfil".
    fn empty_card(&self, validating: bool, cx: &mut Context<Self>) -> AnyElement {
        let palette = palette(cx);
        v_flex()
            .items_center()
            .gap(space::SM)
            .p(space::XXL)
            .rounded(theme::RADIUS_LG)
            .border_2()
            .border_dashed()
            .border_color(palette.input)
            .child(
                Icon::new(IconName::FileMusic)
                    .size(px(48.0))
                    .text_color(palette.muted_foreground),
            )
            .child(
                controls::button("import-pick", Variant::Primary)
                    .label(if validating {
                        text::IMPORT_VALIDATING
                    } else {
                        text::IMPORT_PICK_FILE
                    })
                    .icon(IconName::Upload)
                    .disabled(validating)
                    .on_click(cx.listener(|this, _, window, cx| this.pick_file(window, cx))),
            )
            .child(crate::ui::muted(text::IMPORT_FORMATS, cx))
            .into_any_element()
    }

    /// The checked file: name, length, size, format, the title field.
    fn file_card(&self, file: AudioFileInfo, cx: &mut Context<Self>) -> AnyElement {
        let palette = palette(cx);
        let detail = |icon: IconName, value: String| {
            h_flex()
                .gap(space::XS)
                .child(Icon::new(icon).size(px(14.0)))
                .child(value)
        };
        v_flex()
            .gap(space::MD)
            .p(space::LG)
            .rounded(theme::RADIUS_LG)
            .bg(palette.background)
            .child(
                h_flex()
                    .items_start()
                    .gap(space::MD)
                    .child(
                        Icon::new(IconName::FileMusic)
                            .size(px(32.0))
                            .text_color(palette.primary),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .truncate()
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .child(file.filename.clone()),
                            )
                            .child(
                                h_flex()
                                    .gap(space::LG)
                                    .mt(space::XS)
                                    .text_color(palette.muted_foreground)
                                    .child(detail(
                                        IconName::Clock,
                                        text::file_duration(file.duration_seconds),
                                    ))
                                    .child(detail(
                                        IconName::HardDrive,
                                        text::file_size(file.size_bytes),
                                    ))
                                    .child(
                                        div()
                                            .text_color(palette.primary)
                                            .font_weight(gpui::FontWeight::MEDIUM)
                                            .child(file.format.clone()),
                                    ),
                            ),
                    ),
            )
            .child(
                v_flex()
                    .gap(space::XS)
                    .child(
                        div()
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .child(text::IMPORT_MEETING_TITLE),
                    )
                    .child(
                        controls::input(&self.title)
                            .aria_label(SharedString::from(text::IMPORT_MEETING_TITLE)),
                    ),
            )
            .child(
                controls::button("import-pick-other", Variant::Outline)
                    .label(text::IMPORT_PICK_OTHER)
                    .w_full()
                    .on_click(cx.listener(|this, _, window, cx| this.pick_file(window, cx))),
            )
            .into_any_element()
    }

    /// "Flere valg": language and model.
    fn more_options(&self, cx: &mut Context<Self>) -> AnyElement {
        let palette = palette(cx);
        let parakeet = self
            .selected_model(cx)
            .is_some_and(|m| m.provider == "parakeet");
        let open = self.advanced;
        let labelled = |icon: IconName, label: &'static str, control: AnyElement| {
            v_flex()
                .gap(space::SM)
                .child(
                    h_flex()
                        .gap(space::SM)
                        .child(
                            Icon::new(icon)
                                .size(px(16.0))
                                .text_color(palette.muted_foreground),
                        )
                        .child(div().font_weight(gpui::FontWeight::MEDIUM).child(label)),
                )
                .child(control)
        };
        let language: AnyElement = if parakeet {
            crate::ui::muted(text::IMPORT_PARAKEET_LANGUAGE, cx).into_any_element()
        } else {
            Select::new(&self.language)
                .accessibility_label(text::IMPORT_LANGUAGE)
                .w_full()
                .h(layout::CONTROL_HEIGHT)
                .into_any_element()
        };
        v_flex()
            .rounded(theme::RADIUS_LG)
            .border_1()
            .border_color(palette.border)
            .child(
                controls::button("import-more", Variant::Ghost)
                    .label(text::IMPORT_MORE_OPTIONS)
                    .icon(if open {
                        IconName::ChevronUp
                    } else {
                        IconName::ChevronDown
                    })
                    .w_full()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.advanced = !this.advanced;
                        cx.notify();
                    })),
            )
            .when(open, |this| {
                this.child(
                    v_flex()
                        .gap(space::LG)
                        .p(space::MD)
                        .border_t_1()
                        .border_color(palette.border)
                        .child(labelled(IconName::Globe, text::IMPORT_LANGUAGE, language))
                        .when(
                            !self.model_options.is_empty() || self.models_loading,
                            |this| {
                                this.child(labelled(
                                    IconName::Cpu,
                                    text::IMPORT_MODEL,
                                    Select::new(&self.models)
                                        .accessibility_label(text::IMPORT_MODEL)
                                        .placeholder(if self.models_loading {
                                            text::IMPORT_LOADING_MODELS
                                        } else {
                                            text::IMPORT_MODEL
                                        })
                                        .disabled(self.models_loading)
                                        .w_full()
                                        .h(layout::CONTROL_HEIGHT)
                                        .into_any_element(),
                                ))
                            },
                        ),
                )
            })
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_kinds_follow_the_extension() {
        assert_eq!(FileKind::of(Path::new("a/möde.MP3")), FileKind::Audio);
        assert_eq!(FileKind::of(Path::new("møde.mp4")), FileKind::Audio);
        assert_eq!(FileKind::of(Path::new("m.m4a")), FileKind::Audio);
        assert_eq!(FileKind::of(Path::new("m.wav")), FileKind::Audio);
        assert_eq!(FileKind::of(Path::new("teams.VTT")), FileKind::Subtitle);
        assert_eq!(FileKind::of(Path::new("zoom.srt")), FileKind::Subtitle);
        assert_eq!(FileKind::of(Path::new("notes.txt")), FileKind::Other);
        assert_eq!(FileKind::of(Path::new("no-extension")), FileKind::Other);
    }

    #[test]
    fn language_is_left_to_the_engine_for_auto_and_parakeet() {
        assert_eq!(language_argument("auto", false), None);
        assert_eq!(language_argument("da", true), None);
        assert_eq!(language_argument("da", false), Some("da".to_string()));
        assert_eq!(
            language_argument("auto-translate", false),
            Some("auto-translate".to_string())
        );
    }

    #[test]
    fn an_empty_title_falls_back_to_the_file_name() {
        assert_eq!(meeting_title("  ", "optagelse.mp3"), "optagelse.mp3");
        assert_eq!(meeting_title(" Statusmøde ", "optagelse.mp3"), "Statusmøde");
    }

    #[test]
    fn the_saved_model_is_preselected_when_installed() {
        let options = vec![
            ModelOption {
                provider: "whisper",
                name: "small".into(),
                size_mb: 466,
            },
            ModelOption {
                provider: "parakeet",
                name: "parakeet-tdt".into(),
                size_mb: 670,
            },
        ];
        let saved = ("parakeet".to_string(), "parakeet-tdt".to_string());
        assert_eq!(default_model(&options, Some(&saved)), 1);
        let missing = ("localWhisper".to_string(), "large".to_string());
        assert_eq!(default_model(&options, Some(&missing)), 0);
        assert_eq!(default_model(&options, None), 0);
    }

    #[test]
    fn the_language_list_starts_with_the_automatic_choices() {
        assert_eq!(LANGUAGES[0].0, "auto");
        assert_eq!(LANGUAGES[1].0, "auto-translate");
        assert!(LANGUAGES.iter().any(|(c, _)| *c == "da"));
        let mut codes: Vec<_> = LANGUAGES.iter().map(|(c, _)| *c).collect();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), LANGUAGES.len());
    }

    #[test]
    fn model_labels_name_the_engine_and_size() {
        let option = ModelOption {
            provider: "whisper",
            name: "small".into(),
            size_mb: 466,
        };
        assert_eq!(option.label(), "🏠 Whisper: small (466 MB)");
    }

    use std::collections::HashMap;
    use std::sync::Mutex;
    use tingskriver_core::host::{AppHost, ListenerId};

    /// A host that keeps settings in memory.
    #[derive(Default)]
    struct FakeHost {
        settings: Mutex<HashMap<(String, String), serde_json::Value>>,
        fail_writes: bool,
    }

    impl AppHost for FakeHost {
        fn emit_json(&self, _: &str, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn database(&self) -> Option<tingskriver_core::database::manager::DatabaseManager> {
            None
        }
        fn app_data_dir(&self) -> Result<PathBuf, String> {
            Err("none".into())
        }
        fn listen(&self, _: &str, _: Box<dyn Fn(&str) + Send + Sync>) -> ListenerId {
            0
        }
        fn unlisten(&self, _: ListenerId) {}
        fn read_setting(
            &self,
            store: &str,
            key: &str,
        ) -> Result<Option<serde_json::Value>, String> {
            Ok(self
                .settings
                .lock()
                .unwrap()
                .get(&(store.to_string(), key.to_string()))
                .cloned())
        }
        fn write_setting(
            &self,
            store: &str,
            key: &str,
            value: serde_json::Value,
        ) -> Result<(), String> {
            if self.fail_writes {
                return Err("disk full".into());
            }
            self.settings
                .lock()
                .unwrap()
                .insert((store.to_string(), key.to_string()), value);
            Ok(())
        }
    }

    #[test]
    fn the_beta_switch_is_on_until_the_user_turns_it_off() {
        let host: HostHandle = std::sync::Arc::new(FakeHost::default());
        assert!(beta_enabled(&host), "Tauri's default is on");
        set_beta_enabled(&host, false).unwrap();
        assert!(!beta_enabled(&host));
        set_beta_enabled(&host, true).unwrap();
        assert!(beta_enabled(&host));
    }

    #[test]
    fn a_failed_save_is_reported_and_keeps_the_old_value() {
        let host: HostHandle = std::sync::Arc::new(FakeHost {
            fail_writes: true,
            ..Default::default()
        });
        assert_eq!(set_beta_enabled(&host, false), Err("disk full".to_string()));
        assert!(beta_enabled(&host));
    }

    #[test]
    fn core_events_become_progress_completion_and_failure() {
        let progress = ImportEvent::parse(
            "import-progress",
            r#"{"stage":"transcribing","progress_percentage":42,"message":"Transcribing 3/10"}"#,
        );
        assert!(matches!(
            progress,
            Some(ImportEvent::Progress(ref p))
                if p.stage == "transcribing" && p.progress_percentage == 42
        ));
        let done = ImportEvent::parse(
            "import-complete",
            r#"{"meeting_id":"m-1","title":"Møde","segments_count":12,"duration_seconds":3600.0}"#,
        );
        assert!(matches!(
            done,
            Some(ImportEvent::Complete(ref r)) if r.meeting_id == "m-1" && r.segments_count == 12
        ));
        assert!(matches!(
            ImportEvent::parse("import-error", r#"{"error":"Import cancelled"}"#),
            Some(ImportEvent::Failed(ref m)) if m == "Import cancelled"
        ));
        // No readable message: the generic one, never a blank error box.
        assert!(matches!(
            ImportEvent::parse("import-error", "{}"),
            Some(ImportEvent::Failed(ref m)) if m == text::IMPORT_FAILED_TOAST
        ));
        assert!(ImportEvent::parse("import-progress", "not json").is_none());
        assert!(ImportEvent::parse("recording-started", "{}").is_none());
    }
}
