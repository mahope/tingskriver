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

## Recording: the audio is saved first

A recording captures and saves, nothing else. Transcription, VAD and the
summary run after the meeting from the saved files, so a crash in a model
(a native abort in ggml or ONNX Runtime) cannot stop the audio.

- Capture (`audio::pipeline::AudioCapture`) resamples to 48 kHz mono and
  applies the microphone filters, then hands the chunk to the disk writer
  through `RecordingState::send_audio_chunk`. That uses a bounded channel
  with `try_send`: capture never blocks; a full queue drops the chunk and the
  writer reports it.
- The writer (`audio::recording_writer`) is one thread. It writes the
  microphone and system tracks separately (`audio::track_writer`) as 16-bit
  WAV segments of 10 minutes in `<meeting>/.checkpoints/`
  (`microphone_000.wav`, `system_000.wav`, ...). Every second the new samples
  and the header sizes go to the OS (a killed process loses at most about a
  second); every 5 s and at stop they are synced to the disk.
  `tracks.json` holds each track's offset on the shared recording clock (the
  active duration, pauses excluded). A track that lags that clock for more
  than a second gets silence for the missing time, and at stop both tracks
  are padded to the same end.
- Storage errors are a state, not a log line: the first failure emits
  `recording-storage-error` (`{ message, kind }`, e.g. "Lyden gemmes ikke:
  disken er fuld"), recovery emits `recording-storage-recovered`, and more
  than 5 s of queued audio emits `recording-storage-backlog`. While the disk
  fails, the writer retries at most once per 10 s and keeps at most 30 s per
  track in memory (older audio becomes silence of the same length, so the
  time axis survives). `recording_commands::current_storage_error()` and
  `get_recording_state().storage_error` give the current message to UIs and
  the recording indicator.
- Stop stops the streams (their last callbacks and the resampler tail still
  reach the writer), closes the channel and waits for the writer to drain it.
  The log line `Stop-to-saved: N ms` measures this. `recording-saved` and
  `recording-stopped` carry the save status.
- In the background, `audio::finalize` encodes `microphone.m4a`,
  `system.m4a` and the mixed `audio.mp4` (all on the shared clock), decodes
  each and requires at most 1 s difference from the recorded length. The
  status is `ok`, `delvis` or `fejlet` (`recording-audio-finalized`,
  `metadata.json` field `audio_status`). The raw tracks are deleted only on
  `ok`; otherwise the message is "Lyden kunne ikke samles. Originalen er
  gemt."
- After a crash, `recover_audio_from_checkpoints` runs the same finalize:
  headers that lag their data are repaired, a truncated segment keeps its
  readable part, an unreadable one is skipped (silence keeps later segments
  in place), and any of these gives `delvis` with the other pieces.
