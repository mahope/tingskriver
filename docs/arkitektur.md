# Architecture: crates and boundaries

Tingskriver is one Cargo workspace. The UI is moving from Tauri to GPUI, so
all logic lives in a UI-free crate that both UIs share.

```
Cargo.toml                    workspace root, shared [workspace.package] and release profile
crates/
  tingskriver-core/           audio, transcription, database, AI providers, dictation, summaries
  tingskriver-app/            GPUI app (gpui + gpui-component): shell with navigation, see gpui.md
  tingskriver-overlay/        pill and recording indicator: native Win32 layered window (Windows)
frontend/
  src-tauri/                  the current Tauri app: commands, windows, tray, notifications
  src/                        the React UI of the Tauri app
llama-helper/                 local LLM sidecar used by the summary engine
site/                         the project website, not built yet
installer/                    the Velopack installer and updater, not built yet
```

## Dependency direction

```
tingskriver-app ─────┐
tingskriver-overlay ─┼──▶ tingskriver-core ──▶ (whisper-rs, ort, cpal, sqlx, reqwest, windows, ...)
frontend/src-tauri ──┘
```

- `tingskriver-core` depends on no UI crate: no `tauri`, `tauri-*`, `gpui`,
  `gpui-*`, `wry` or `webview2`. The Linux job in `.github/workflows/rust.yml`
  fails if `cargo tree -p tingskriver-core` contains one of them.
- The UI crates depend on core, never the other way round, and not on each
  other.

## How core talks to the UI

Core code that used to take a Tauri `AppHandle` takes a
`tingskriver_core::host::HostHandle` (`Arc<dyn AppHost>`). `AppHost` is what
core needs from the application around it:

| Method | Used for |
|---|---|
| `emit_json` (and `HostExt::emit`) | events to the UI (`transcript-update`, download progress, ...), serialized exactly as before |
| `listen` / `unlisten` | the recording's transcript history listens to its own `transcript-update` events |
| `database` | the open database, `None` before onboarding has created it |
| `app_data_dir` | models, dictation store, summary models |
| `read_setting` / `write_setting` | recording preferences (`recording_preferences.json`) |
| `refresh_status_ui` | recording or model availability changed (tray menu) |

The Tauri implementation is `frontend/src-tauri/src/core_bridge.rs`; event
names and payloads are unchanged, so the React UI did not change. The
dictation pill is drawn by the UI through `tingskriver_core::dictation::overlay::Overlay`.
Async work started from audio and keyboard-hook threads uses
`tingskriver_core::runtime::spawn`, which runs on the runtime the host
registers at startup.

## What stays in the Tauri crate

- `#[tauri::command]` wrappers with the original names and arguments. They
  convert `AppHandle` to a host handle and `State<T>` to `&T` and call core.
- Code that needs Tauri itself: windows and the webview dictation overlay,
  the tray, notifications (`tauri-plugin-notification`), the file dialogs
  (`tauri-plugin-dialog`), database initialisation that registers `AppState`
  with `manage`, onboarding, the updater, logging setup and the commands for
  the archived FastAPI backend.

The GPUI app will implement `AppHost` on its own event channel and call the
same core functions. Today it calls core only on its own tokio runtime (the
database open at startup), never on the UI thread; see [gpui.md](gpui.md)
for the shell and the GPUI version lock.

## The pill overlay

`crates/tingskriver-overlay` draws the dictation pill (and later the
recording indicator) as a native Win32 layered window instead of a GPUI or
webview window, so it can stay on top, be click-through and never take focus.

- `model`, `layout`, `render`, `text`, `theme`: the state machine (auto-hide,
  fade-out, level bars), geometry per DPI scale, tiny-skia drawing and swash
  text. Platform independent and tested on Linux.
- `windows`: one hidden `WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST |
  WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW` window on its own thread, shown with
  `SW_SHOWNOACTIVATE` at the bottom centre of the monitor with the active
  window. A 33 ms timer draws frames only while the pill is visible.
  `windows::Overlay` has `set_state`, `set_level`, `show` and `hide`;
  `windows::CoreOverlay` implements core's dictation `Overlay` trait.
- `cargo run -p tingskriver-overlay --example demo` shows every state on
  Windows; `-- --png <folder>` writes them as PNG files on any OS.

## Speech models: loaded once, shared, unloaded when idle

Meetings and dictation share the Parakeet and Whisper engines in core.

- Inference never runs on a tokio worker or under an engine lock. Whisper
  clones its context (`Arc`) and runs in `spawn_blocking` with its own state.
  An ONNX session needs `&mut`, so Parakeet keeps a pool of up to two loaded
  instances (`parakeet_engine::pool`): a short dictation during a long meeting
  segment borrows the second instance instead of waiting. The second instance
  is loaded the first time two calls overlap and then stays.
- Stopping a meeting no longer unloads the model. The model keeper
  (`dictation::warm`, decisions in `dictation::logic::residency`) unloads a
  model after the idle time in the dictation settings (`modelIdleMinutes`,
  default 10, 0 = never), keeps models listed in `keepWarmModels`, and
  unloads after 30 s idle when less than 10 % of memory is free. It never
  unloads during a call or a meeting recording, and logs process memory before
  and after (`Model keeper: unloaded ...`).
- Pressing the dictation shortcut prewarms the local model while the user
  speaks. Dictation logs `model_load=none` when the model was already loaded.

## AI providers: one model, a chain per function

`tingskriver_core::providers` is the one place that knows which AI services
exist (SPEC D1). Meetings and dictation ask it for a provider per function
instead of keeping their own endpoints and keys.

- **Providers** (`providers::model`): name, base URL, model ids, kind
  (`syv-ai`, `openai`, `odincore`, `open-ai-compatible`, `ollama`, `local`)
  and whether the service labels speakers. Presets: Syv.ai
  (`https://platform.syv.ai/v1`, with diarization), OpenAI, OdinCore
  (`https://api.odincore.ai/v1`, `ordbogen/whisper` and `odin-2`, no
  diarization) and the built-in local model. Everything else, also Groq and
  OpenRouter, is a custom OpenAI-compatible provider.
- **Chains**: each function (`transcription`, `dictation`, `summary`,
  `assistant`, `dictation-polish`) has an ordered list of provider and model,
  with an optional time limit per link. Defaults: Syv.ai then local for
  transcription and dictation (Syv.ai gives up after 2 s for dictation), the
  local model for summaries and the assistant, polishing off. A link that
  needs a key it does not have is skipped without a call.
- **Keys** live only in Windows Credential Manager, under `Tingskriver/syv.ai`
  (unchanged from dictation) and `Tingskriver/udbyder/<id>`. `providers.json`
  in the app data folder holds no key; `ApiKey` has no `Serialize` and a
  `Debug` that prints `***`.
- **Calls** (`providers::client`): `/audio/transcriptions`,
  `/chat/completions` and `/models` with a time limit around the whole call,
  so a server that hangs mid-answer counts as a timeout. Errors are
  classified (`ProviderError`: missing key, 401/403, 404, 429 with
  `Retry-After`, timeout, unreachable, ...) and explained in Danish without
  the key.
- **Fallback** (`providers::chain`): the next link is used on an error or a
  time limit. The result says who delivered (`Delivered::delivered_by()`,
  "leveret af Syv.ai (syv-transcribe)") and lists the failed links; both are
  logged. A transcript from a link without diarization has
  `speakers_missing = true` when speakers were asked for.
- **Migration** (`providers::migrate`): the first lookup after the database
  exists turns the old `settings`/`transcript_settings` rows and the
  dictation endpoint into `providers.json`, moves every key to Credential
  Manager and then clears the old columns. Claude, Deepgram and ElevenLabs
  are no longer providers, but their keys are kept. If a key cannot be
  stored, nothing is cleared and the migration runs again next time. An old
  value the settings UI would refuse (an Ollama endpoint without `http://`,
  an empty custom endpoint) drops only that provider and its links, with a
  note; it never replaces the user's choices with the defaults.
- **Old settings code**: the `SettingsRepository` key functions now save
  keys to Credential Manager on Windows and NULL the column (elsewhere the
  column is kept), read the column first and Credential Manager second, and
  delete from Credential Manager too. Targets are those of the new model,
  except that the transcription keys of OpenAI and Groq go to
  `Tingskriver/udbyder/openai-transcription` and `groq-transcription` (read
  before the shared `openai`/`groq` key). The migration also picks up keys
  already saved there, and stores a transcription key under that target even
  when it equals the summary key, so deleting the summary key in the old
  screen leaves meeting transcription working.

Using it from dictation or meetings:

```rust
use tingskriver_core::providers::{self, Function, ProviderError, TranscriptionRequest};

let request = TranscriptionRequest { wav, file_name: "diktering.wav".into(), language: Some("da".into()), prompt, diarize: false };
let delivered = providers::transcribe(&host, Function::Dictation, request, |link, req| async move {
    // Only links on the built-in model end up here; link.model names it.
    local_parakeet(&link.model, req.wav).await.map_err(ProviderError::local)
})
.await?; // LookupError explains every failed link in Danish
log::info!("dictation {}", delivered.delivered_by());
```

`providers::chat` works the same way for summaries, the assistant and
polishing. `providers::chain_for` returns the resolved links for callers that
need their own loop. The settings UI uses `config`, `save_config`,
`statuses`, `set_key`, `delete_key` and `test_provider` (a real `GET /models`
with the measured round trip).
