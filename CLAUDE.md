# CLAUDE.md

Guidance for Claude Code in this repository. The rules for everyone (the table from changed component to CI check, skipped checks, porting) are in [AGENTS.md](AGENTS.md); each crate has its own `AGENTS.md`. This file only adds what is useful to know before you start.

## Project Overview

**Tingskriver** is a privacy-first meeting assistant for Windows that records, transcribes and summarizes meetings entirely on the user's machine. The supported application is the **GPUI desktop app** with a Rust core. The older Tauri app in `frontend/` is being phased out; keep it running, but put new logic in `crates/tingskriver-core`. The Python/FastAPI backend in `backend/` is archived and unsupported: do not add endpoints to it.

| Where | What |
|---|---|
| `crates/tingskriver-core` | all logic, no UI crate ([AGENTS.md](crates/tingskriver-core/AGENTS.md)) |
| `crates/tingskriver-app` | the GPUI app ([AGENTS.md](crates/tingskriver-app/AGENTS.md), [docs/gpui.md](docs/gpui.md)) |
| `crates/tingskriver-host` | keyboard hook, shortcuts, tray ([AGENTS.md](crates/tingskriver-host/AGENTS.md), [docs/host.md](docs/host.md)) |
| `crates/tingskriver-overlay` | the dictation pill ([AGENTS.md](crates/tingskriver-overlay/AGENTS.md)) |
| `installer/` | Velopack installer ([AGENTS.md](installer/AGENTS.md)) |
| `frontend/` | the older Tauri app (being phased out) |
| `llama-helper/` | local LLM sidecar for summaries |

Crate boundaries and how core talks to the UI (`AppHost`): [docs/arkitektur.md](docs/arkitektur.md).

## Commands

Agents on Linux cannot build or run the Windows app. Use the commands in [docs/agenter.md](docs/agenter.md) and [docs/testing.md](docs/testing.md), for example:

```bash
cargo test --locked -p tingskriver-core
cargo xwin check --locked --target x86_64-pc-windows-msvc -p tingskriver-core -p tingskriver-overlay -p tingskriver-host -p tingskriver-app
cargo deny --locked check licenses bans
rustfmt --edition 2021 <file>      # only files you touch
```

Build instructions for people: [docs/BUILDING.md](docs/BUILDING.md).

## Audio recording path (critical understanding)

The recording only captures and saves. There is no VAD, transcription model or
ffmpeg during a recording, so a crash in any of them cannot cost audio:

```
Mic callback ──┐                                  ┌─ microphone_000.wav ...
               ├─ AudioCapture (pipeline.rs) ─────┤   (.checkpoints/, committed every 1 s)
System callback┘  bounded channel, never blocks   └─ system_000.wav ...
                  RecordingWriter thread                     ↓ after stop, in the background
                  (recording_writer.rs, track_writer.rs)     finalize.rs: microphone.m4a,
                                                             system.m4a, audio.mp4, verified
```

**Key Insight**: stop means "the audio is on disk" (the writer drains its queue
and returns within seconds). Encoding, verification and deletion of the raw
tracks (only on status `ok`) happen afterwards; transcription runs from the
saved files. See [docs/arkitektur.md](docs/arkitektur.md#recording-the-audio-is-saved-first).

Related: Mute never changes the microphone device ([docs/enheder.md](docs/enheder.md)); the capture callback never allocates, logs or blocks ([docs/audio-callback.md](docs/audio-callback.md)).

Where to look in `crates/tingskriver-core/src/audio/`:
- Device detection: `devices/discovery.rs`, `devices/platform/{windows,macos,linux}.rs`
- Capture streams: `capture/microphone.rs`, `capture/system.rs`
- Capture processing: `pipeline.rs`; saving: `recording_writer.rs` and `track_writer.rs`; final files and crash recovery: `finalize.rs`
- Recording workflow: `recording_manager.rs`, `recording_commands.rs`

## Adding a Tauri command (older Tauri app)

Put the logic in `crates/tingskriver-core` as a plain function (take a `HostHandle` instead of an `AppHandle` if it emits events). Add a thin `#[tauri::command]` wrapper in `frontend/src-tauri/src` with the same name and arguments, and register it in the `tauri::Builder` `invoke_handler`. See [docs/arkitektur.md](docs/arkitektur.md).

## Conventions

- Rust uses `anyhow::Result`. Use `perf_debug!()`/`perf_trace!()` for hot-path logging (zero cost in release).
- Share state across async tasks with `Arc<RwLock<T>>`, simple flags with `Arc<AtomicBool>`.
- Audio devices are called "microphone" and "system" (not "input"/"output").
- Use Tauri/GPUI path APIs, never hard-coded paths. Models live in `%APPDATA%\dk.mahoje.tingskriver\models` in production (`app_data_dir().join("models")`, see `tingskriver_core::paths`) and `frontend/models/` in development.
- Branches: `main` is stable; work on `fix/*`, `enhance/*` or `issue/<n>-*` and open a PR to `main` ([CONTRIBUTING.md](CONTRIBUTING.md)).
- The repository is public: no secrets, personal data or private paths. Test with synthetic fixtures ([docs/testing.md](docs/testing.md)).
