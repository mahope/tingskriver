# GPUI: go/no-go, version lock and the app shell

Issue #25. The UI moves from Tauri to GPUI (owner decision 30/10-26). GPUI is
not a stable standalone framework: it is Zed's UI crate, published for other
apps by the gpui-component maintainers as `gpui-pre`, and mixed versions
break the build with E0277. This page records what the shell proves, how the
version is locked, and the manual Windows tests that decide go or no-go.

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
| `ui` | Main window with sidebar (Møder, Diktering, Indstillinger; Assistent comes later), the placeholder pages, the loading state and the database error page. |
| `startup` | Startup clock (on Windows from process creation via `GetProcessTimes`), the first-frame log line against the 500 ms budget, and the database open with Danish error texts. |
| `window_state` | Window position and size in `%APPDATA%\dk.mahoje.tingskriver\gpui-window.json`, saved 0.5 s after the last move and on close, restored only when at least 120×40 px land on a connected display. |
| `single_instance` | Per-user named pipe `\\.\pipe\dk.mahoje.tingskriver.gpui-<user>` (Unix socket elsewhere). A second launch asks the running app to show its window and exits. |
| `args` | `--hidden` (start without a window; a later launch or the tray shows it), `--page meetings|dictation|settings`. With a tray icon the app stays running when the window closes. |
| `crash` | Panic hook for graphics failures, see above. |
| `probe` | The test field on the Diktering page lists every special character with its code point and warns when a dead key was not composed. |

Core is only called on the tokio runtime the app starts (`runtime.spawn`),
never on the UI thread. The database opens there while the window draws;
the Møder page shows "Indlæser møder …" until it is done. No database file
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
unpack `tingskriver-app.exe` anywhere and start it. The Tauri app may run at
the same time; both use the same database with the same migrations.

1. **Start time.** Close the app, wait 10 s, start it five times. After each
   start open the log (Indstillinger, "Åbn logmappen", `tingskriver-app.log`)
   and note `startup: first frame N ms after process start (M ms before
   main, …)` and `startup: database ready after …`. The first frame line must
   come before the database line. Compare with the 0.5.0-dev.2 baseline in
   `docs/ydeevne.md` on main (issue #3).
2. **Danish input.** Diktering page, test field. Type `æøå ÆØÅ`. Dead keys:
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
   must show a name ("Møder", "Åbn logmappen", …) and a role.
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
   logmappen" opens the log folder. With an empty folder instead, the Møder
   page says "Ingen møder endnu" and no database file is created.
9. **No usable GPU.** In a Hyper-V or VirtualBox VM without GPU passthrough,
   start the app. Expected: it runs in software rendering and the log says
   `Using GPU: Microsoft Basic Render Driver`. If it cannot render at all,
   the error dialog with log path appears. Note which happened.

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

**No-go** if text input (2, 3) or the UIA tree (4) fails without a fix in
gpui-component's next release; the rest are fixable in the app.

## Known limitations

- The database error texts classify the sqlx error message. When issue #5
  adds `DbInitError` to core, the error page should map its variants and add
  "Åbn backup".
- The pages are placeholders: no meeting list, settings or dictation yet.
- `AppHost` is implemented by `tingskriver-host`'s bridge ([host.md](host.md));
  the pages do not subscribe to its events yet.
