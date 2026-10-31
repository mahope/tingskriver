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
  (`tauri-plugin-dialog`), registering the opened database as `AppState`
  with `manage`, onboarding, the updater and the commands for the archived
  FastAPI backend.

## Startup, database and logs

- `tingskriver_core::logging::init(<file>)` installs the logger (stderr and
  a rotating file in `%LOCALAPPDATA%\dk.mahoje.tingskriver\logs`) and a
  panic hook that writes thread, message and backtrace to the log, from any
  thread. `logging::startup_phase("...")` logs `Startup: <phase> after N ms`
  from process start; the Tauri app marks `process start`, `tray ready`,
  `DB ready` and `window ready`, so `grep Startup:` gives the timeline.
- `DatabaseManager::open(app_data_dir)` never panics. It returns
  `DbInitError` (`Locked`, `Corrupt`, `DiskFull`, `Migration`, `Other`) with
  the database path, the log file and, for corrupt databases and failed
  migrations, a copy of the database and its `-wal`/`-shm` files in
  `backups/`. Nothing deletes `-wal` or `-shm` files: a WAL holds committed
  transactions. `summary_da()` and `advice_da()` give the error page text.
- Connections use WAL, `synchronous=NORMAL`, `busy_timeout=5s` and foreign
  keys (`database::manager::connect_options`).
- The Tauri app opens the database in a background task after the window is
  shown. The React `DatabaseGate` shows a loading state, and if opening fails
  an error page with the log and the backup, from the `database-status`
  event and the `get_database_status` command.
- `crash_dumps::apply_in_background()` sets the per-user Windows Error
  Reporting `LocalDumps` key for the running exe, so a native crash leaves a
  minidump in `%LOCALAPPDATA%\dk.mahoje.tingskriver\dumps`. The release
  profile keeps line tables in a separate PDB (`split-debuginfo`), which CI
  uploads as the `...-symbols-...` artifact. The GPUI app should call
  `apply_in_background()` at startup as well.

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
