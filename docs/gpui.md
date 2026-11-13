# GPUI: go/no-go, version lock and the app shell

Issue #25. The UI moves from Tauri to GPUI (owner decision 30/10-26). GPUI is
not a stable standalone framework: it is Zed's UI crate, published for other
apps by the gpui-component maintainers as `gpui-pre`, and mixed versions
break the build with E0277. This page records what the shell proves, how the
version is locked, and the manual Windows tests that decide go or no-go.

Automated PNG tests and reference updates: [GPUI screenshots](gpui-screenshots.md).
The meeting-list port and its verification limits are described in
[meeting-list.md](meeting-list.md).

## Status

| Area | Status |
|---|---|
| Version lock (one GPUI, same as gpui-component's lock) | Done, enforced in CI |
| Shell: window, sidebar, tokens, fonts, light/dark, window position, `--hidden`, single instance | Built, tested on Linux (Xvfb) and type-checked for Windows |
| Database opens in the background, error page instead of exit | Built and tested (unit tests and Xvfb screenshots) |
| GPU missing or lost | Handled (software rendering via WARP, else error dialog with log path) |
| Danish text input, dead keys, AltGr, IME | **Manual on Windows, pending** |
| UI Automation tree (Accessibility Insights) | **Manual on Windows, pending** |
| Two monitors with different DPI | **Manual on Windows, pending** |
| First frame within 500 ms on the reference machine | **Manual on Windows, pending** |
| Machine without usable GPU (VM) | **Manual on Windows, pending** |
| Meeting recording: start, pause, stop, save, transcribe, recovery | Built; recorder and screen states tested on Linux with a fake core; **manual on Windows, pending** (tests 10 to 13) |

Go/no-go is decided when the pending rows are filled in below.

## Version lock

- `crates/tingskriver-app/Cargo.toml` pins `gpui-pre =0.3.10`,
  `gpui-pre-platform =0.3.10`, `gpui-component =0.8.1` and
  `gpui-kit-assets =0.8.1`. Both GPUI and gpui-component were published on
  2026-10-26 by the gpui-component maintainer (crates.io user `huacnlee`).
- `scripts/check-gpui-lock.py` fails when `Cargo.lock` holds more than one
  version of any `gpui*` package, when `cargo tree -d --depth 0 --target all`
  lists a `gpui*` duplicate, or when a GPUI package's version or checksum
  differs from gpui-component's own `Cargo.lock` (it ships one in the crate).
  The Linux CI job runs it on every push. It was tested by tampering with a
  copy of the lock file: a wrong `gpui-pre` version and a second
  `gpui-pre-util` version both fail with exit code 1.
- **Upgrade policy:** upgrade GPUI only together with gpui-component, to the
  exact versions gpui-component's new release locks. Never bump `gpui-pre`
  alone, and never add a second GPUI source (git dependency on Zed, another
  fork). Run `python3 scripts/check-gpui-lock.py` after `cargo update`.

## What the Windows backend does (read from gpui-pre-windows 0.3.10)

- **Text input:** characters arrive through `WM_CHAR` after `ToUnicode`
  translation of the keyboard layout, so dead keys and AltGr go through the
  Danish layout. IME composition is handled through `WM_IME_COMPOSITION`
  (`GCS_COMPSTR` and `GCS_RESULTSTR`). The crate is a 2026 Zed snapshot, so
  it should contain the Zed fix for IME and dead keys from November 2025
  (Zed #41259); the manual test below confirms it.
- **Accessibility:** windows answer `WM_GETOBJECT` through
  `accesskit_windows`, so a UI Automation tree exists. gpui-component sets
  roles and names on its controls (sidebar items, buttons, inputs).
- **DPI:** `WM_DPICHANGED` is handled per window; bounds are in logical
  pixels.
- **No GPU:** GPUI uses Direct3D 11 and takes the first DXGI adapter that
  works. On a machine without a usable GPU that is "Microsoft Basic Render
  Driver" (WARP, software rendering); the log then says
  `Using GPU: Microsoft Basic Render Driver`. If even that fails, GPUI panics
  with "failed to initialize DirectX".
- **GPU lost:** GPUI rebuilds its renderer up to five times and then panics
  with "Device lost".

The app's panic hook (`crates/tingskriver-app/src/crash.rs`) turns both
panics into a log entry and a native Windows error dialog ("Tingskriver kan
ikke vise sit vindue" or "Tingskriver mistede forbindelsen til
grafikkortet") with the log path and a "Prøv igen" button that starts the
app again. It never leaves a silent, frozen window. Other panics are only
logged, because tokio catches panics in core tasks.

## The shell

`crates/tingskriver-app` (library plus a thin `main.rs`):

| Module | What |
|---|---|
| `theme` | Design tokens from 0.5.0-dev.2 (`frontend/src/app/globals.css`): palette for light and dark, spacing, type sizes, radii, layout sizes. `apply` maps them onto gpui-component's theme. The window follows the system appearance live (`observe_window_appearance`, Windows sends `ImmersiveColorSet`). Tests check WCAG AA contrast for every text pair in both modes. |
| `assets` | Hanken Grotesk (400, 500, 600, 700) and Literata (400, 600, italic 400) as static TTF, embedded and registered at start. They are instances of the woff2 variable fonts the Tauri UI uses, made with fontTools (licence in `assets/fonts/OFL.txt`). Tests parse each font and require glyphs for æøåÆØÅ, the vowels dead keys produce, and AltGr symbols. Icons: gpui-component's defaults plus the shell's Lucide icons. |
| `ui` | Main window: the Tauri sidebar (`sidebar.rs`: Ny optagelse, search, the meeting list from `meetings.rs`, Diktering, imports, Indstillinger, ⓘ for Om; collapses to 64 px), the home screen, the open meeting, the placeholder pages, the loading state and the database error page. Måling is not in the sidebar; open it with `--page måling`. |
| `startup` | Startup clock (on Windows from process creation via `GetProcessTimes`), the first-frame log line against the 500 ms budget, and the database open with Danish error texts. |
| `window_state` | Window position and size in `%APPDATA%\dk.mahoje.tingskriver\gpui-window.json`, saved 0.5 s after the last move and on close, restored only when at least 120×40 px land on a connected display. |
| `single_instance` | Per-user named pipe `\\.\pipe\dk.mahoje.tingskriver.gpui-<user>` (Unix socket elsewhere). A second launch asks the running app to show its window and exits. |
| `args` | `--hidden` (start without a window; a later launch or the tray shows it), `--page home|dictation|settings|about|måling` (`meetings`/`møder` opens home). With a tray icon the app stays running when the window closes. |
| `crash` | Panic hook for graphics failures, see above. |
| `probe` | The test field on the Diktering page lists every special character with its code point and warns when a dead key was not composed. |

Core is only called on the tokio runtime the app starts (`runtime.spawn`),
never on the UI thread. The database opens there while the window draws;
the sidebar list shows "Indlæser møder …" until it is done. No database file
means first launch: the shell does not create one (onboarding will). A
database that fails to open shows the error page with summary, advice, the
technical error, the database path, the log path and the buttons "Åbn
logmappen" and "Prøv igen".

Logs go to `%LOCALAPPDATA%\dk.mahoje.tingskriver\logs\tingskriver-app.log`
(rotated at 5 MB, three files), separate from the Tauri app's
`tingskriver.log`. Without `RUST_LOG`, Tingskriver and GPUI log from info,
other crates from warn. `RUST_LOG` takes a plain level (`RUST_LOG=debug`)
for all crates; per-module filters such as
`RUST_LOG=tingskriver_core::audio=debug` are not supported by this logger,
which logs a warning and keeps the defaults.

`TINGSKRIVER_APPEARANCE=dark` (or `light`) pins the appearance instead of
following Windows, and `TINGSKRIVER_DATA_DIR=<folder>` points the app at
another data folder; both are for tests and screenshots.

### Measured on Linux

Xvfb with Mesa llvmpipe (software OpenGL), agent-worker, release build
without LTO: first frame 216, 328 and 565 ms after process start (the first
run had a cold disk cache). The database (migrations on an empty file)
finished about 20 ms after the first frame and never delayed it. The UI
thread then spends roughly a second compiling llvmpipe shaders for new
primitives, which delays when the database result appears on screen; that
is a software-GL effect and says nothing about Windows.

## Manual test on Windows (Mads)

Download `tingskriver-app-windows-x64` from the "GPUI app artifact (Windows
release)" job of the newest CI run on the branch (Actions, run, Artifacts),
unpack all files into the same folder and start `tingskriver-app.exe`. Keep
`tingskriver-stt.exe`, `onnxruntime.dll`, `onnxruntime_providers_shared.dll`
and `onnxruntime-LICENSE.txt` beside the app for local transcription.
The Tauri app may run at
the same time; both use the same database with the same migrations.

1. **Start time.** Close the app, wait 10 s, start it five times. After each
   start open the log (Indstillinger, "Åbn logmappen", `tingskriver-app.log`)
   and note `startup: first frame N ms after process start (M ms before
   main, …)` and `startup: database ready after …`. The first frame line must
   come before the database line. Compare with the 0.5.0-dev.2 baseline in
   `docs/ydeevne.md` on main (issue #3).
2. **Danish input.** Diktering page: unfold "Test tastatur" at the bottom. Type `æøå ÆØÅ`. Dead keys:
   `´` then `e` gives `é`, `¨` then `u` gives `ü`, `^` then `o` gives `ô`,
   `~` then `n` gives `ñ`, `` ` `` then `a` gives `à`; a dead key followed
   by space gives the accent itself. AltGr: `@ £ $ € { [ ] } | \`. The line
   under the fields must list the letters (é as U+00E9) and show no warning
   about an uncomposed dead key.
3. **IME.** In the multi-line field: Win+. (emoji panel) and pick an emoji;
   Win+H (voice typing) if available; with a Japanese or Chinese IME added,
   type a word and confirm the candidate. The composed text must appear once
   at the caret.
4. **UI Automation.** Start Accessibility Insights for Windows (Live
   Inspect), hover the sidebar items, a button and the text fields. Each
   must show a name ("Ny optagelse", "Søg i møder og transskripter", "Om Tingskriver", "Åbn logmappen", …) and a role. Tab from the window start: Ny optagelse, search, the meeting rows (with Omdøb and Slet), the footer rows, ⓘ, the collapse handle.
5. **Light and dark.** Settings, Personalisering, Farver: switch between
   light and dark while the app is open. The app must follow without
   restart.
6. **Two monitors, different DPI** (for example 100 % and 150 %). Move the
   window between them: text stays sharp and the layout keeps its size.
   Close the app on the second monitor and start it again: it opens there.
   Unplug that monitor and start again: it opens centred on the main one.
7. **Single instance and `--hidden`.** Start the exe twice: the second start
   brings the first window to the front and exits. Start with `--hidden`:
   no window; start the exe normally: the window appears. Close it: the app
   keeps running in the tray; the tray menu's "Afslut" ends it. The tray and
   shortcut tests are in [host.md](host.md#manual-test-on-windows).
8. **Database error** (on a test folder, never on the real meetings). In
   PowerShell:
   ```powershell
   mkdir C:\temp\ts-test
   Set-Content C:\temp\ts-test\meeting_minutes.sqlite "ikke en database"
   $env:TINGSKRIVER_DATA_DIR = "C:\temp\ts-test"; .\tingskriver-app.exe
   ```
   The error page ("Tingskriver kunne ikke åbne dine møder", "Databasefilen
   er beskadiget.") with the log path appears instead of an exit; "Åbn
   logmappen" opens the log folder. With an empty folder instead, the sidebar
   says "Ingen møder endnu" and no database file is created.
9. **No usable GPU.** In a Hyper-V or VirtualBox VM without GPU passthrough,
   start the app. Expected: it runs in software rendering and the log says
   `Using GPU: Microsoft Basic Render Driver`. If it cannot render at all,
   the error dialog with log path appears. Note which happened.
10. **Record a meeting.** Indstillinger: choose the microphone and the
    system audio device (or keep the defaults). Play a video with speech in
    the browser and talk into the microphone. Press "Start optagelse": the
    pill shows "Gør klar…", then "Pause", "Stop og gem" and three bars that
    move with your voice and with the video; the status bar shows "Optager",
    a running clock and the device names; the sidebar says "Optagelse i
    gang", and the tray icon shows the recording. After about 2 minutes press
    "Pause": the clock stops, the bars become dots, the tray shows paused;
    "Genoptag" continues. Press "Stop og gem": "Gemmer optagelsen…", "Gemmer
    mødet…", "Gør transskriptionen færdig…", then "Optagelsen er gemt", and
    the meeting opens with its transcript of both voices. It is listed in the
    sidebar under "I dag". The recording folder holds `audio.mp4`.
11. **Other ways in.** With the window closed to the tray, start a
    recording from the tray menu ("Ny optagelse") or its shortcut: the
    window opens and shows the running recording. Stop it from the tray: the
    window follows through saving to the finished meeting. Record again and
    stop; while "Gør transskriptionen færdig…" shows, choose "Ny optagelse"
    in the tray: the new recording takes over the screen, and the first
    meeting is transcribed in the background and stays in the sidebar. The
    card "En optagelse blev afbrudt" must not appear after any of these stops.
12. **Errors.** Disable the microphone in Windows (Indstillinger, Lyd,
    Inputenheder) and start: either the recording runs on system audio with
    "Ingen mikrofon tilgængelig. Der optages kun systemlyd.", or the red
    alert "Mikrofonen er ikke tilgængelig" appears under the pill. Unplug a
    USB headset during a recording: a notice in the status bar says what
    happened, and the recording continues.
13. **Gendan afbrudte møder.** Start a recording, talk for 30 s and end the
    app in Task Manager (Afslut opgave). Start it again: the dialog
    "Gendan afbrudte møder" opens with the recording (length, "Lyd findes").
    "Gendan" saves the meeting, transcribes it and opens it. Repeat and use
    "Slet" (it asks first): the dialog closes and the card on Home is gone.

14. Move from the Tauri app (#282; needs a clean Windows VM, steps in
    `installer/README.md`, "Testing the move"). Install the Tauri build
    with the move, start it, make two meetings with a transcript, change a
    setting and download one model. Wait a minute. Expected: the GPUI app
    starts, `%LOCALAPPDATA%\Tingskriver` is gone, Settings, Apps lists only
    the GPUI app, and the same two meetings, the setting and the model are
    there. In `%LOCALAPPDATA%\dk.mahoje.tingskriver\logs\tingskriver.log`:
    `move to GPUI: data folder has N files`, `Tauri app uninstalled` and
    `the data folder is intact`; `%TEMP%\tingskriver-move-
    database-backup\database-<id>` holds the database from before the move. Repeat with a
    recording running: nothing happens until it is stopped and encoded.
14. **Sleep and logoff (#179).** Record 1 minute, close the lid (or Sleep)
    and wake the machine. No recording runs and nothing starts by itself;
    Home shows the card "Et møde blev afsluttet af systemet" with cause
    "dvale", the time and "Lyden er gemt", and the meeting opens with audio up
    to the moment of sleep. Repeat with sign-out and sign-in: cause "logget
    af". The log has one `power:` line per event ([host.md](host.md), steps
    8 to 14).
15. **Meeting program on another device (#199).** Set Windows' default output
    to the speakers, play Teams or Zoom on a headset, and record. The pill
    keeps recording; the system line shows the headset once the follower has
    switched (about 4 s), the remote voices are in the saved system track,
    and the log has `[DEVICE_FOLLOW]` with a gap of at most 1000 ms
    ([enheder.md](enheder.md), test A).

Write the results here:

| Test | Result | Date, machine |
|---|---|---|
| 1. First frame (median of five) | | |
| 2. æøå, dead keys, AltGr | | |
| 3. IME | | |
| 4. UIA tree | | |
| 5. Live light/dark | | |
| 6. Two monitors, different DPI | | |
| 7. Single instance, `--hidden` | | |
| 8. Database error page | | |
| 9. No GPU (VM) | | |
| 10. Record 2 min, mic and system audio, stop, meeting and transcript | | |
| 11. Tray start and stop | | |
| 12. Microphone errors | | |
| 13. Gendan afbrudte møder | | |
| 14. Move from the Tauri app | | |
| 14. Sleep and logoff | | |
| 15. Meeting program on another device | | |

**No-go** if text input (2, 3) or the UIA tree (4) fails without a fix in
gpui-component's next release; the rest are fixable in the app.

## Known limitations

- The database error texts classify the sqlx error message. When issue #5
  adds `DbInitError` to core, the error page should map its variants and add
  "Åbn backup".
- Settings and dictation are still placeholders. The meeting document shows the
  summary read-only; editing, making a summary, delivery and retranscription
  are disabled with an explanation until they are ported.
- `AppHost` is implemented by `tingskriver-host`'s bridge ([host.md](host.md));
  the pages do not subscribe to its events yet.

About diagnostics report the compiled Whisper GPU backend (or CPU), never
CUDA/Vulkan SDK environment variables. On Windows, the platform line includes
the Windows product version and build number from sysinfo. Opening About from
the tray or settings does not change the startup page.

Light/dark About screenshots currently use `scripts/screenshot-about.py`
manually. Automated screenshot coverage follows in #49 / PR #172; #171 stays
open until that acceptance criterion is covered.
