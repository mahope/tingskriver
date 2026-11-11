# Architecture: crates and boundaries

Tingskriver is one Cargo workspace. The UI is moving from Tauri to GPUI, so
all logic lives in a UI-free crate that both UIs share.

```
Cargo.toml                    workspace root, shared [workspace.package] and release profile
crates/
  tingskriver-core/           audio, transcription, database, AI providers, dictation, summaries
  tingskriver-app/            GPUI app (gpui + gpui-component): shell with navigation, see gpui.md
  tingskriver-overlay/        pill and recording indicator: native Win32 layered window (Windows)
  tingskriver-host/           host outside GPUI: dictation hook, global shortcuts, tray, AppHost bridge
frontend/
  src-tauri/                  the current Tauri app: commands, windows, tray, notifications
  src/                        the React UI of the Tauri app
llama-helper/                 local LLM sidecar used by the summary engine
site/                         the project website, not built yet
installer/                    the Velopack installer and updater (installer/README.md)
```

## Dependency direction

```
tingskriver-app ──▶ tingskriver-host ──┬──▶ tingskriver-overlay ──┐
                                       └──────────────────────────┼──▶ tingskriver-core ──▶ (whisper-rs, ort, cpal, sqlx, reqwest, windows, ...)
frontend/src-tauri ───────────────────────────────────────────────┘
```

- `tingskriver-core` depends on no UI crate: no `tauri`, `tauri-*`, `gpui`,
  `gpui-*`, `wry` or `webview2`. The Linux job in `.github/workflows/rust.yml`
  fails if `cargo tree -p tingskriver-core` contains one of them.
- The UI crates depend on core, never the other way round. The host joins
  core, the pill and the tray for the GPUI app, so it depends on the overlay;
  it has no GPUI or Tauri dependency (the tray is the `tray-icon` crate), and
  the overlay does not depend on the app.

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

- Recording state, the recording manager, audio buffer pools and transcription
  command locks recover the inner value from `PoisonError`, so a worker panic
  does not make later recording attempts panic on the same lock. Release builds
  retain `panic = "unwind"`. The active `audio` module denies Clippy's
  `unwrap_used` and `expect_used` outside tests; fallible import and conversion
  paths return errors through the existing command/error-event handling.
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
  The safety-copy space check resolves the nearest existing ancestor and
  canonicalizes mount points too, including Windows verbatim drive paths,
  so it works before the backup directory exists.
- Connections use WAL, `synchronous=NORMAL`, `busy_timeout=5s` and foreign
  keys (`database::manager::connect_options`).
- The Tauri app opens the database in a background task after the window is
  shown. The React `DatabaseGate` shows a loading state, and if opening fails
  an error page with the log and the backup, from the `database-status`
  event and the `get_database_status` command.
- `crash_dumps::apply_in_background()` sets the per-exe Windows Error
  Reporting `LocalDumps` key under HKLM for the running exe. This requires
  administrator privileges; setup errors are logged at startup and returned
  when changing the setting. Once configured, a native crash leaves a
  minidump in `%LOCALAPPDATA%\dk.mahoje.tingskriver\dumps`. The release
  profile keeps line tables in a separate PDB (`split-debuginfo`), which CI
  uploads as the `...-symbols-...` artifact. The GPUI app should call
  `apply_in_background()` at startup as well.

The GPUI app will implement `AppHost` on its own event channel and call the
same core functions. Today it calls core only on its own tokio runtime (the
database open at startup), never on the UI thread; see [gpui.md](gpui.md)
for the shell and the GPUI version lock.

## The host: keyboard hook, shortcuts and tray

`crates/tingskriver-host` runs what must not wait for GPUI's event loop: the
dictation hook (core's `WH_KEYBOARD_LL` thread), a host thread with its own
Win32 message loop for the tray icon and the `RegisterHotKey` shortcuts, and
the pill. Its `Bridge` is the GPUI app's `AppHost`. Details, the shortcut
settings file, the hook timing log and the Windows test are in
[host.md](host.md).

## Repeat a saved dictation

`dictation::rerun` reads the newest history entry (timestamp, then id).
`last_text` rejects empty text. `rerun` reads that entry's `audio_path`
asynchronously, rejects missing or empty files and recordings at least 24 hours
old, and returns a `Comparison` with the history id, original text, new text
and a `changed` flag. It never overwrites history or inserts the new result.
The injected async provider makes this path testable without credentials.

The optional `InsertLast` and `RerunLast` shortcuts are stored as `insertLast`
and `rerunLast` in `shortcuts.json`. The host dispatches them to the core
`dictation::rerun` module. Reinsertion uses the existing Windows target checks.
Rerun switches between local Parakeet and Syv. History stores the resolved provider,
Syv model and language. Local-only settings and local-only or unknown original
providers prevent cloud reruns, including legacy entries with no saved provider.
Syv reruns reuse the saved model and language.
The local path ensures Parakeet is ready before inference, reloading an idle-unloaded model.
Missing audio produces a Danish pill message and no insertion.

A successful rerun emits `dictation-rerun-comparison` through `AppHost`, with
`historyId`, `before`, `after` and `changed`. Only the GPUI host emits and handles this event to
show the original and new text with an explicit changed/unchanged status,
bringing the window forward even when it was hidden. Closing the comparison
discards it without inserting or overwriting anything. Shortcut settings controls
remain open before issue #76 can be closed.

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

- Native inference and ONNX startup run in `tingskriver-stt`, reached over
  private stdin/stdout pipes with request IDs and raw f32 PCM. A worker crash
  fails the current call in Danish; the next call restarts and reloads it.
  Silero also runs there; failed startup selects energy VAD in core.
- Inference never runs on an app tokio worker or under an engine lock.
  Whisper keeps one warm process and lazily starts a second for overlapping
  calls. An ONNX session needs `&mut`, so Parakeet keeps a pool of up to two loaded
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

On Windows, device changes are handled by `IMMNotificationClient` and the
stream-health checks in `windows_device_follower`, without the additional
two-second `AudioDeviceMonitor` polling loop. Other platforms retain that
monitor. Device enumeration runs on Tokio's blocking pool; stopping the
monitor waits at most two seconds before aborting its async task. An already
running driver call cannot be cancelled, but no longer holds up that stop.

A recording captures and saves, nothing else. Transcription, VAD and the
summary run after the meeting from the saved files, so a crash in a model
(a native abort in ggml or ONNX Runtime) cannot stop the audio.

The desktop stop flow saves the meeting first, waits for the matching
`recording-audio-finalized` event, then runs file transcription with the
configured local model and the saved database meeting id. Completion is
retained if encoding finishes before the database save. Failed encoding or
transcription leaves the saved meeting available and reports the error.

- Capture (`audio::pipeline::AudioCapture`) resamples to 48 kHz mono and
  applies the microphone filters, then hands the chunk to the disk writer
  through `RecordingState::send_audio_chunk`. That uses a bounded channel
  with `try_send`: capture never blocks; a full queue drops the chunk and the
  writer reports it.
- The CPAL callback queue carries a cumulative loss count with each accepted
  buffer. The DSP worker inserts the missing duration as silence before the
  next buffer (or at stop for trailing loss), through the same resampler.
  Loss is converted from interleaved device samples to 48 kHz mono and added
  to the writer's persistent dropped-sample count, so even gaps below the
  clock-gap threshold keep their position and finalize reports partial audio.
- The writer (`audio::recording_writer`) is one thread. It writes the
  microphone and system tracks separately (`audio::track_writer`) as 16-bit
  WAV segments of 10 minutes in `<meeting>/.checkpoints/`
  (`microphone_000.wav`, `system_000.wav`, ...). Every second the new samples
  and the header sizes go to the OS (a killed process loses at most about a
  second); every 1 s and at stop they are synced to the disk. The sync interval
  leaves room for the next commit within the 2 s power-loss budget on a healthy
  disk; storage stalls are reported through the errors and backlog below.
  `tracks.json` holds each track's offset on the shared recording clock (the
  active duration, pauses excluded). A track that lags that clock for more
  than a second gets silence for the missing time, and at stop both tracks
  are padded to the same end. Audio received while confirming a gap stays
  buffered so silence can be inserted before it, at the start of the outage.
- Storage errors are a state, not a log line: the first failure emits
  `recording-storage-error` (`{ message, kind }`, e.g. "Lyden gemmes ikke:
  disken er fuld"), recovery emits `recording-storage-recovered`, and more
  than 5 s of queued audio emits `recording-storage-backlog`. While the disk
  fails, the writer retries at most once per 10 s and keeps at most 30 s per
  track in memory (older audio becomes silence of the same length, so the
  time axis survives). `recording_commands::current_storage_error()` and
  `get_recording_state().storage_error` give the current message to UIs and
  the recording indicator.
  Replaced audio and samples dropped from a full queue are persisted in
  `tracks.json`; a later finalize still reports `delvis` after storage recovers.
- Internal error stops wait for startup to publish the session and emit
  `recording-started` before tearing it down and emitting `recording-stopped`
  with a reason, including when a worker fails while streams are starting.
- Stop stops the streams (their last callbacks and the resampler tail still
  reach the writer), closes the channel and waits for the writer to drain it.
  The log line `Stop-to-saved: N ms` measures this. `recording-saved` and
  `recording-stopped` and the `stop_recording` return value carry the save
  status (`ok`, `delvis`, `fejlet`). Writer failures and timeouts report
  `fejlet`; they are never returned as a successful save.
- In the background, `audio::finalize` encodes `microphone.m4a`,
  `system.m4a` and the mixed `audio.mp4` (all on the shared clock), decodes
  each and requires at most 1 s difference from the recorded length. The
  two-track mix is `weight * first + (1 - weight) * second`, so it can never
  clip. A track counts as active when a 10 ms block has an RMS above about
  -60 dBFS (DC offset removed), so a noise floor counts as silent, and it stays
  active for 4 s after its last speech. While both are active each track gets
  half gain; while only one is, that track keeps unity gain and the quiet
  one is dropped. A track that starts to speak gets its share at once (one
  block of lookahead); the move back to unity after the hold takes 1 s, so
  the level does not change at pauses shorter than the hold. A single-track
  mix keeps unity gain; the separate track files keep their original levels.
  More than two tracks cannot be mixed and fail the finalize.
  The status is `ok`, `delvis` or `fejlet` (`recording-audio-finalized`,
  `metadata.json` field `audio_status`). The raw tracks are deleted only on
  `ok`; otherwise the message is "Lyden kunne ikke samles. Originalen er
  gemt."
- After a crash, `recover_audio_from_checkpoints` runs the same finalize:
  headers that lag their data are repaired, whole PCM samples in truncated
  segments are salvaged, and unreadable segments are skipped (silence keeps
  later segments in place). Truncated headers stay unchanged so retries still
  report `delvis` and keep the originals. Recovery
  keeps the raw tracks until the frontend saves the meeting to the database
  and calls `cleanup_checkpoints`, allowing retries after a failed save.
  Cleanup decodes all final files again and checks their durations before
  deleting originals, even when an earlier verification reported `ok`.
  Legacy MP4 recovery also skips damaged chunks, verifies the merged duration
  against the decoded input durations, and permits cleanup only on `ok`.
  Every recovery attempt invalidates earlier verification, even with no
  chunks left. Recovery and cleanup share a lock on the canonical folder
  path; verification and deletion both run on a blocking worker.

### Disk recovery regression (#54)

PR #146 tested recovery against a scanner that retained readable samples from
truncated segments. Main also received `f910177` through #26, which skipped a
truncated final segment entirely. After merging #146, the truncated fixture
(three seconds declared, two seconds plus one byte present) therefore disappeared
from `scan`, and `restore` reported no audio. This was a merge interaction, not
a dependency or test-order difference.

Recovery retains the complete 16-bit PCM samples, ignores an incomplete trailing
byte, and keeps the truncated header and original file unchanged. Verification
still reports partial audio, so repeated recovery cannot authorize deletion of
the damaged originals. The recovery fixtures cover scanning without UI storage,
verified restore, database and encoder failures, retries, empty folders, and a
terminated writer with both tracks and at most two seconds of loss.

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

Connection tests use `providers::verify`, with an eight-second total deadline,
the same URL policy as the provider settings, and no URL credentials or redirects.
OpenAI-compatible and Ollama providers allow HTTP to any host; public HTTP
endpoints produce a warning in `TestReport.warning` rather than an error.
A redirect returns a Danish explanation without forwarding the key. A custom OpenAI-compatible
gateway with a configured key also receives a random negative-control key.
Accepting that key produces a warning in `TestReport.warning`, with a stronger
warning for public gateways, while connectivity still succeeds. Keyless endpoints
only test connectivity. Existing HTTP, network and timeout error classes retain
their Danish explanations. Latency measures only the real request and its body,
excluding the control. Every Test invocation makes a fresh request; results are
not cached, so repeated tests measure the current connection and latency.
GPUI presentation of the verification states remains separate work for #53.

Database safety copies use a unique folder on every attempt, including the
current WAL and rollback journal. Error advice only confirms a copy when its
path is available; failed copies are reported explicitly.

To verify WER with an actual native exception, run from an elevated Windows
terminal: `cargo test --locked -p tingskriver-core
crash_dumps::tests::native_crash_produces_a_minidump -- --ignored --exact`.
The test registers only its uniquely named child executable, checks the dump
signature and removes its HKLM key afterward. It is excluded from ordinary
unit tests because it requires administrator access and the WER service.
# Network and file I/O

The GPUI shell calls core only through its background executor. Core async
functions run on the host's Tokio runtime; synchronous disk/CPU APIs must run
on a `spawn_blocking` worker, never on the UI thread or a Tokio worker.
Tokio file APIs delegate disk work to the blocking pool. Recording callbacks
continue to hand audio to their dedicated writer thread.

Import and retranscription serialize and atomically write transcripts and
metadata on the blocking pool. Dictation also loads and parses spoken rules
there during initialization and reload. Tests hold the blocking pool busy
to verify that these operations yield until a disk worker is available.

Core HTTP uses asynchronous reqwest with a total deadline of at most ten
seconds, including the response body. Provider-specific shorter deadlines
are preserved. This also bounds model downloads, pulls and remote inference;
a slow operation returns an error and can be retried. Local inference does
not use HTTP and retains its existing cancellation and timing policy.

`crates/tingskriver-core/clippy.toml` bans blocking reqwest and synchronous
filesystem calls. Core denies both `disallowed_methods` and `disallowed_types`.
Exceptions are scoped to synchronous disk workers and test fixtures; their
async callers must offload them. `scripts/check-async-io-guard.sh` proves that
new blocking HTTP and file calls fail Clippy, and runs in Linux CI.

OpenRouter's model list is async in core and in the Tauri wrapper. Its
single-worker fake-server test covers stalled headers, stalled bodies and
concurrent core calls. `read_audio_file` has already been removed; playback
uses the core playback API instead of JSON arrays of audio bytes.

## Dictation without the main window

On main before #46, `tingskriver-host::Host::start` already initialized
`dictation::init` independently of the GPUI window. Core owned the keyboard
hook control thread, capture, Syv.ai deadline and local Parakeet fallback,
cleanup, insertion ordering and Windows clipboard/SendInput insertion.

`dictation::service::Service::subscribe_states` now exposes content-free
lifecycle events through a broadcast channel: inactive → recording →
processing → inserting → inactive. Error and cancellation return to inactive
as well. Each overlapping dictation has its own lifecycle; the service stream
can interleave those lifecycles. `cancel_processing` cancels pending processing;
an insertion already handed to the operating system cannot be recalled. Its
processing future and insertion turn remain alive until insertion completes,
so cancellation cannot release the next insertion early. Cancelling processing
hides its pill synchronously, before a newer recording can show its own pill.
Interrupted keyboard chords cancel only the current capture. Released turns
continue processing when a chord is interrupted or settings reinstall the hook.

`Service::open_for_test` opens an isolated service with a fake `AppHost`,
overlay and `DictationBackend`, without registering the singleton or keyboard
hook. Its `test_trigger` accepts 16 kHz mono samples and runs the ordinary
`Service::process` on a dedicated thread/runtime, returning a per-run state
channel, cancellation handle and join handle. It disables history for that
run and requires an injected backend, so the trigger cannot accidentally use
real providers. Engine planning, fallback, cleanup, ordering and timing remain
the production implementations. Dropping a cancelled transcription also aborts
its outstanding Syv request task. Already spawned blocking local inference
continues after its awaiting future is dropped. Text hooks use
`kill_on_drop(true)`, so dropping their future kills
the child process (without guaranteeing termination of its descendants).
No new dependencies are required.

`cargo test --locked -p tingskriver-core --test dictation_flow` exercises a
real Syv HTTP request rejected with 401 by a localhost fake server, mock
Parakeet fallback, fake insertion, cancellation during local inference,
engine/insertion failures and
captured logs without dictated text, keys or keyboard events. It does not
exercise real audio capture, model inference or Windows insertion.
The manual acceptance checks in #46 (Notepad, measured latency and Start menu
launch) remain for a Windows session.

## Code dictation identifiers

The pure `dictation::logic::identifiers` token pass runs only for `Style::Raw`,
using the style already selected by `app_style` and dictation modes. It runs
before spoken punctuation, after hesitation removal, and leaves literal words,
snippets and dictionary replacements protected. A trailing `camel case`,
`snake case`, `kebab case` or `pascal case` formats at most the three preceding
plain words (also accepted as `camelCase`, `snake_case`, `kebab-case`,
`PascalCase`). The scan stops at symbols, protected tokens and any word in the
active spoken-command table, including words in multiword commands. Parts are
lowercased before the requested casing is applied. This is a bounded heuristic:
earlier words remain unchanged, even when they belong to a longer identifier.

For example, `lav en variabel get user id camel case` becomes
`lav en variabel getUserId`. `punktum` joins a filename only when the next word
has at most four ASCII letters or digits and is not a spoken-command word:
`index punktum ts` becomes `index.ts`, while `fix login punktum tilføj test`
becomes `fix login. tilføj test`. Short prose words can still look like file
extensions under this heuristic. `bindestreg` before an alphanumeric flag
produces `-`, and two consecutive occurrences produce `--`, with a space
before the flag: `ls bindestreg la` becomes `ls -la` and
`cargo build bindestreg bindestreg release` becomes `cargo build --release`.
Flag recognition is limited to a command with at most one subcommand, followed
by zero or more flags. After a positional argument or path starts, hyphens
retain their normal joining: `git checkout bindestreg b feature bindestreg login`
becomes `git checkout -b feature-login`, and
`src skråstreg my bindestreg module punktum rs` becomes `src/my-module.rs`.
This conservative heuristic does not recognize flags after positional arguments
or command prefixes longer than two words. Flag words are protected from
subsequent casing commands.
No application detection or I/O lives in this pass, so it can move to a text
crate independently of desktop hosts.

The `cleanup::tests::code_identifiers` and regression tests cover the four
casing styles, bounded phrases, spoken-command boundaries, hesitations,
filenames, prose punctuation, command flags, paths and Danish words. Separate
tests cover normal-style isolation, application style overrides and protected
literal, dictionary and snippet text.
