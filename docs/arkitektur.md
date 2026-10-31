# Architecture: crates and boundaries

Tingskriver is one Cargo workspace. The UI is moving from Tauri to GPUI, so
all logic lives in a UI-free crate that both UIs share.

```
Cargo.toml                    workspace root, shared [workspace.package] and release profile
crates/
  tingskriver-core/           audio, transcription, database, AI providers, dictation, summaries
  tingskriver-app/            GPUI app (gpui + gpui-component), an empty window for now
  tingskriver-overlay/        pill and recording indicator (Windows), a stub for now
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
same core functions.

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
