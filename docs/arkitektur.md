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
  `DbInitError` (`Locked`, `Corrupt`, `DiskFull`, `Migration`, `NewerDatabase`, `Other`) with
  the database path, the log file and, for corrupt databases and failed
  migrations, a copy of the database and its `-wal`/`-shm` files in
  `backups/`. Nothing deletes `-wal` or `-shm` files: a WAL holds committed
  transactions. A database from a newer Tingskriver is refused before it is
  opened for writing and is never copied or changed, see
  [database-migrationer.md](database-migrationer.md). `summary_da()` and `advice_da()` give the error page text.
  The safety-copy space check resolves the nearest existing ancestor and
  canonicalizes mount points too, including Windows verbatim drive paths,
  so it works before the backup directory exists.
- Write attempts are announced as sets of table names by SQLite hooks
  (`database::changes`, `db.changes()`): update hook collects, commit hook
  sends, rollback hook clears. A list subscribes with
  `keep_fresh(db.changes().subscribe(), &["meetings"], quiet, max_wait, reload)`,
  which merges bursts into one reload. Both durations must be positive;
  use a quiet period such as 60 ms because sqlx exposes no post-commit hook.
  These are change hints, not commit acknowledgements: a delayed or failed
  commit can still cause a stale or unnecessary reload. Listen on base tables,
  not on `search_fts`; see the module comment for the limits. The separate
  `delivery.sqlite` pool has no hooks and is not announced by this feed.
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

External CLI and URL commands use the existing single-instance endpoint, now
implemented in core behind a transport trait. Commands run on the core runtime
without showing a window. See [kommandolinje.md](kommandolinje.md) for the
command vocabulary, queue acknowledgement, account boundary and current
meeting workflow limitation.

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

The same crate has the prompt window for questions with buttons (meeting
question, reminder, "Er mødet slut?"): a non-activating native popup with real
controls, global shortcuts instead of focus, and a Focus Assist decision. See
[promptvindue.md](promptvindue.md).

## Speech models: loaded once, shared, unloaded when idle

Meetings and dictation share the Parakeet and Whisper engines in core.

Dictation polishing treats text inside `<transcript>` tags with escaped closing delimiters as content,
never as questions to answer or instructions to follow. `dictation::logic::polish_guard`
removes invisible controls and leading `<think>` blocks from model output.
Empty or late output falls back to layer 1, with a maximum budget of two seconds.
The dictation polish call rejects `finish_reason=length` as an error
without changing summary handling. The built-in sidecar currently returns text without a finish
reason; truncation detection through this field applies to compatible HTTP providers.

- Native inference and ONNX startup run in `tingskriver-stt`, reached over
  private stdin/stdout pipes with request IDs and raw f32 PCM. A worker crash
  fails the current call in Danish; the next call restarts and reloads it.
  Silero also runs there; failed startup selects energy VAD in core.
  Calls have deadlines and a GPU worker is retried once on the CPU, see
  [stt-tidsfrister.md](stt-tidsfrister.md).
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
- A separate `audio::disk_guard` polls the recording drive every 5 s on a
  blocking worker, starting immediately. Below 256 MiB it emits
  `recording-disk-low` (`kind: DiskLow`); at 320 MiB it emits
  `recording-disk-ok` (`kind: DiskOk`).
  Each transition is emitted once. Unknown space leaves the warning unchanged.
  When a low-space guard stops, it also emits `DiskOk` to clear its warning.
  The recording context replaces the warning on recovery and dismisses it
  immediately on recording stop, independently of write errors. A low-space
  event arriving before `recording-started` is held until that event; recovery,
  stop and the next startup clear any pending warning. The dictation
  overlay has no meeting-recording status connection yet; its disk warning is
  deferred to #169.
  Testing on an almost-full Windows VHD remains a manual validation step.
  The guard only advises through `AppHost`; it never stops or blocks capture.
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
verified restore, database and encoder failures, retries, empty folders, and
crash snapshots of both tracks with at most two seconds of loss. The loss
budget test drives the production track writers and real WAV storage with a
controlled clock, checking every 10 ms of audio without a final commit. It
also freezes the clock to exercise sample-count commits after a burst.
Snapshots are taken between writes, without repairing a live writer's files;
threaded queue draining and slow storage are covered by the recording-writer
tests. This isolates the commit budget from CI thread scheduling and queued
audio, rather than assuming a successful channel send has reached disk.

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
# Settings transfer (core API, Refs #85)

`settings_transfer::SettingsSnapshot` is a versioned JSON document containing
meeting preferences, dictation settings (including modes and their shortcuts),
dictionary words, snippets, global shortcuts and the shared provider config.
`MeetingSettings::from_legacy` explicitly selects the portable columns from
legacy database rows; legacy endpoints and custom options must first be migrated
to the shared provider config. No credential store, ICS address, meeting data,
dictation history or machine-local row ids are part of the snapshot.
Endpoint URLs with login credentials, a query or a fragment are rejected.

`export_json` and `import_json` validate the document and reject unsupported
transfer/provider/mode versions. Dictation settings must already be normalized
as by `DictationSettings::normalized`; values that would change on save are
rejected by export, import and preview so the diff matches persisted values.
`preview` reads Credential Manager but does not write live settings. It returns
a keyed diff (object arrays by id and dictionary entries by word) with before/after
values and the resulting snapshot. Keyed arrays also report their ordered keys
when membership or order changes, including dictation mode priority changes.
Replace uses the complete incoming portable snapshot. Merge uses
incoming meeting/dictation preferences, merges dictionary words and snippet
phrases case-insensitively, and merges providers by id and shortcuts by action.
Merge also retains the local microphone and channel, merges dictation modes by id
and app styles by process. Incoming function chains replace the complete ordered
chain for that function.
An unset incoming global shortcut retains the current shortcut in merge mode;
replace mode can clear it. Duplicate words, phrases and provider ids are rejected.

The caller gathers snapshots from the existing stores and, after user
confirmation, persists the returned result while retaining local credentials
and the ICS address. When a provider or Syv dictation endpoint changes origin
(scheme, host or port) and has a saved key, the preview retains the local URL.
Its proposed change is marked `kræver ny nøgle`. Explicitly accepting it through
`ImportPreview::accept_endpoint_changes` deletes the corresponding credentials
before exposing the new URLs; a deletion error keeps the safe result unchanged.
If an imported provider id has a saved key but no local provider configuration,
preview rejects the import until that key is removed: there is no known local
origin to preserve, so even a built-in provider id cannot inherit the key.
`preview_with_keys` supports injected credential stores for tests and hosts.
This store wiring, file dialogs and the import preview UI
are not implemented yet. No Tauri or GPUI dependency is needed by this API.

# Meeting export

`tingskriver_core::export` renders `MeetingDetails` (loaded with
`MeetingsRepository::get_meeting`) as Markdown with YAML frontmatter or as a
self-contained HTML document. Callers supply summary text, serialized Markdown
notes (`notes::markdown::serialize`), optional duration in seconds,
participant names, speaker labels, action items from the summary, an optional
audio file path and language. Participant names are supplied separately from
labels such as `Taler 1`; labels never populate `deltagere`. Frontmatter includes
`dato` (the creation timestamp), `deltagere` and `opgaver` (string arrays), and
`kilde` (the audio path, or `import` when absent). Missing duration and language
remain null; the exporter does not infer metadata from transcript text.
Transcript segments are rendered in recording order. HTML preserves source
text in escaped, wrapping blocks, including Markdown notation in notes and
summaries; it does not interpret raw HTML or fetch external resources.

`filename` expands `{dato}` and `{titel}` once and sanitizes the result for
Windows and Unix. `export_meeting` combines rendering, filename expansion and
writing for a future UI wrapper, using the creation timestamp converted to
local time for `{dato}`. Invalid timestamps return an input error before
writing. It returns the actual saved path. `write_document` takes a basename
and a typed format, adds the extension and runs disk work on the blocking
pool. It syncs a temporary file in the destination directory and publishes
with `persist_noclobber`
(no-replace rename or atomic hard-link fallback). Existing files, including
concurrent exports, receive ` (2)`, ` (3)`, etc. The caller must create/select
the directory and use the returned path. This core API adds no UI command,
dialog, DOCX support or export preference storage.

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

Core HTTP uses asynchronous reqwest with a ten-second connection deadline.
Ordinary requests also have a ten-second total deadline, including the body.
Model downloads go through the shared downloader in `model_download`: a
ten-second connection deadline, no total deadline (the files are gigabytes)
and an abort after 60 seconds without data, which keeps the `.part` file so
the next attempt resumes. Summary generation allows 300 seconds;
provider inference uses the function default or the explicit per-link limit,
including limits longer than ten seconds. Local inference does not use HTTP
and retains its existing cancellation and timing policy.

`crates/tingskriver-core/clippy.toml` bans blocking reqwest and synchronous
filesystem calls. Core denies both `disallowed_methods` and `disallowed_types`.
Exceptions are scoped to synchronous disk workers and test fixtures; their
async callers must offload them. `scripts/check-async-io-guard.sh` proves that
new blocking HTTP and file calls fail Clippy, and runs in Linux CI.

OpenRouter's model list is async in core and in the Tauri wrapper. Its
single-worker fake-server test covers stalled headers, stalled bodies and
concurrent core calls. `read_audio_file` has already been removed; playback
uses the core playback API instead of JSON arrays of audio bytes.

## Speaker identity and suggestions

`tingskriver_core::speakers` contains pure logic, with no UI, network or database
calls. `identity::from_email` derives a tentative personal name and retains the
company domain as evidence; common Danish and international webmail domains
never count as a company. The major webmail providers are matched across TLDs.
Email names ignore plus-address tags and digits, and split on dots, underscores
and hyphens. Whole local-part placeholders (including separated “No Reply”
variants) and ambiguous local parts are refused. Personal names accept both
straight and typographic apostrophes.

`suggest::SpeakerStore` supplies a meeting's transcript, speaker IDs, participants
and dictionary through a read-only boundary. `build_prompt` encodes that context
as JSON data; output validation in `parse_response` provides the protection.
The parser prefers Markdown-fenced JSON and uses JSON parsing to separate arrays
from surrounding prose, even when that prose contains brackets. It skips invalid
entries and duplicate IDs, and rejects responses containing unknown speaker IDs.
Suggestions require user confirmation;
they are evidence for review, not verified identities. Provider invocation, UI
integration and saving accepted names are outside this change (Refs #87; saving
is deferred to #229).
## Dictionary suggestions from corrections (#81)

`dictation::logic::suggest` compares word snapshots of the inserted span within
30 seconds (inclusive). It skips password fields and bounds comparisons to 256
words per snapshot. Case-only changes, punctuation, insertions and deletions
yield no new words. Only equal-sized replacement gaps between LCS anchors
(including snapshot boundaries) produce candidates; unequal gaps fail closed.
Leading/trailing apostrophes and hyphens are trimmed, while internal ones remain.
`Store::suggest_corrections` persists only candidate words in the additive
`dictation_suggestions` table, never snapshots or keystrokes. Unicode lowercase
keys deduplicate suggestions and retain ignored/added decisions across restarts.
Only the 200 newest pending candidates are retained, including when an existing
database is opened. `clear_suggestions` deletes pending candidates and all
remembered decisions without changing dictionary entries.
Only `resolve_suggestion` with an explicit add decision writes to the dictionary;
ignoring is permanent, including when the same word later has different casing.
Addition rechecks dictionary words with the same Unicode lowercase normalization
inside the decision transaction, preserving words added manually in the meantime.

Windows UIA snapshot reading, identifying the same field and inserted span,
checking password status before reading, and UI controls for add/ignore remain
follow-up work. The caller must measure elapsed time from insertion with a
monotonic clock and supply only that span, never unrelated field contents.

## Summary quality nets

`summary/quality.rs` and `summary/names.rs` hold pure functions around the
report model; `processor.rs` and `map_reduce.rs` use them.

- **Minimum:** a transcript under 5 words or 160 characters of speech gets no
  model call. The process fails with "For kort til referat: …"
  (`quality::TOO_SHORT_MESSAGE`), which the UI shows as the reason.
- **Length:** the report prompt gets a cap (a quarter of the transcript, floor
  320 characters or 150 per template section, ceiling 10,000) and a paragraph
  interval that grows with the transcript (`quality::length_policy`).
- **Template start:** the answer must start with `# ` and its first section
  must match the template's first within 30 % Levenshtein distance, after a
  Unicode-aware normalisation (æøå are kept). If not, it is generated once more
  with "IMPORTANT: Previous attempt failed. <feedback>"; the better attempt is
  kept, so a usable report is never dropped.
- **Preferred names:** dictionary words (and calendar attendee names, via
  `names::attendee_names`) are added to the report and title prompts with the
  rule "write them exactly like this".
- **Title:** made from the finished report in a separate short call (one line,
  at most 160 characters, `<EMPTY>` means none). `meetings.title_source` is
  `user` for a title the user saved; a generated title is only written (in one
  atomic UPDATE) while the title is empty, the default `Meeting …` name or an
  earlier generated one.

## Dictation shortcut gestures

`dictation::logic::gesture` supports `hold`, `toggle`, `hybrid` and
`double-tap` through the persisted `shortcutMode` setting. Existing settings
keep `double-tap`. Hybrid switches to hands-free recording for a press shorter
than `hybridThresholdMs` (default 300 ms, clamped to 100–1000 ms) and stops
on release for a longer press. All four modes are selectable in dictation settings. Release bounce is filtered
for 30 ms, with a 50 ms release grace period. Double-tap keeps its 400 ms window.
Tracker interruptions cancel and clear the gesture immediately: the tracker
deactivates without a later release signal, so the next shortcut can start again.

One unmodified Escape cancels all pending dictations, including earlier turns
still processing or waiting for insertion. Ctrl+Esc, Ctrl+Shift+Esc and other
modified Escape shortcuts pass through to Windows. Cancellation wakes waiting
turns through `tokio::sync::Notify`, without polling.

The Windows hook consumes Escape only during an active dictation, including
transcription, polishing and waiting for an insertion turn. It shows “Afbrudt”.
`dictation::logic::cancellation` carries the cancellation generation from capture
to processing and atomically checks it at the insertion boundary. Cancelled
attempts cannot enter insertion, voice editing or history. Once insertion has
begun, Escape belongs to the foreground application again. This boundary also
prevents a later dictation's Escape from interrupting clipboard restoration.

## Notes editing

`notes::edit::Editor` edits `notes::model::Document` without UI dependencies.
`Position` uses a block index and an extended grapheme index in the entire
block, including graphemes that cross formatting spans. Invalid selections
return an error without changing the editor. The editor keeps at least one
paragraph, even for a new empty document.

`type_text` recognizes `# `, `## `, `### `, `- `, `[] ` and `1. ` at the start of an otherwise
empty paragraph. `insert_plain_text` pastes literal text with normalized line
endings and no markdown interpretation. Enter splits a block, continues lists
(with a fresh unchecked checkbox), or leaves an empty list. Headings continue
as paragraphs. Backspace removes one grapheme, unwraps a list/heading at its
start, or joins paragraphs with the previous block. Splits retain the original
block's timestamp on its left half; new blocks are untimed. Enter at the start
of a nonempty block inserts an empty paragraph above it and preserves the
existing block's metadata. Joining an empty, untimed predecessor preserves
the current block's metadata; other joins retain the predecessor's metadata.

Selections can span blocks in either direction. Bold and italic toggle the
selected text, or the insertion style at an empty selection. A range containing
only empty blocks enables the insertion style. Consecutive typing is grouped
by Unicode word boundaries in the combined existing and inserted text, with a
new step after a one-second pause or another operation. History
is limited to 500 steps and restores document, selection and insertion style;
editing after undo clears redo. Selection movement itself is not an undo step.
`copy_markdown` serializes the selected fragments with `notes::markdown`,
retaining block kinds and formatting but omitting meeting timestamps.
Selection style toggles also set the style for replacement text. When copying
up to the start of another block, the selected newline is retained without
including that block's unselected kind. A GPUI widget, keyboard/clipboard
integration and persistence wiring remain for the UI half of #47.
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
