# The host: dictation hook, global shortcuts and tray

GPUI has no tray icon and no global shortcuts, and a hold-to-talk shortcut
of modifiers only (Ctrl+Win) cannot be registered with `RegisterHotKey`. The
`WH_KEYBOARD_LL` hook that serves it has to answer every key press in the
system within about 1 ms, whatever the UI is doing. So all of it runs in
`crates/tingskriver-host`, outside GPUI's event loop. The GPUI app starts it
with `Host::start` and only answers its requests (show the window, quit).

## Threads

| Thread | Owner | What |
|---|---|---|
| `dictation-keyboard-hook` | core, `dictation::win::KeyboardHook` | The one `WH_KEYBOARD_LL` hook, with its own message loop. Only tracks the configured dictation shortcut (`ChordTracker`), swallows the Win key-up so Start does not open, and times every callback. |
| `dictation-control` | core, `dictation::service` | Hold, double tap to lock, release: records, transcribes and inserts. |
| `tingskriver-host` | host, `win.rs` | A message-only window with a Win32 message loop: the tray icon and menu (`tray-icon`) and the `RegisterHotKey` shortcuts (`WM_HOTKEY`). |
| pill | overlay, `windows::CoreOverlay` | The dictation pill at the bottom of the screen. |
| core runtime | app | `watch_status` (tray follows the recording) and `watch_shortcuts` (re-registers changed shortcuts). |

Other threads talk to the host thread by queueing a command and posting a
message to its window. Messages posted to a window survive modal loops such
as the open tray menu; messages posted to a thread would not.

`Bridge` (`bridge.rs`) is the GPUI app's `AppHost`: events go to core's own
listeners and to UI subscribers (`Bridge::subscribe`), settings stores are
JSON files in the app data folder (written via a temporary file and rename),
the database is set once the app has opened it, and `refresh_status_ui`
wakes the tray at once.

## Nothing typed is seen or stored

- The hook passes every key on except the dictation shortcut's own keys. It
  keeps a bit set of held modifiers and whether the shortcut is held; it
  never stores, sends or logs which key was pressed.
- `WM_HOTKEY` only carries the id of the shortcut that fired.
- The log gets shortcut labels from the settings ("Shortcut for Ny optagelse
  registered: Ctrl+Alt+N") and timing summaries with numbers only.
- `crates/tingskriver-host/tests/hook_through_host.rs` checks the log after
  10,000 key presses for key names and codes.

## Hook timing

Every hook callback is timed (`dictation::logic::hook_stats`, 1 µs buckets,
lock-free). Every 10,000 calls the hook thread logs, after the callback has
returned:

```
INFO tingskriver_core::dictation::win: Keyboard hook timing: 10000 calls, p50 3 us, p99 41 us, max 310 us
```

A p99 of 1 ms or more is logged as a warning ("over the 1 ms target"). The
rest of a window is logged when the hook stops (shortcut changed, app quit).
`dictation::win::hook_times()` returns the current window for a debug page.

## Tray

The icon is the app icon with a dot: none when idle, red (`RECORDING`
token) while a meeting records, grey while it is paused, and a red ring
when something does not work (a shortcut is taken, dictation could not
start, or Tingskriver runs as administrator). The tooltip says what is going
on and names the problem. The state is read every 100 ms and at once when
core calls `refresh_status_ui`; the icon changes within 200 ms of a recording
starting or stopping (`watch::tests::recording_changes_reach_the_tray_within_200_ms`).

| Menu | Does |
|---|---|
| Ny optagelse / Stop optagelse | Stop stops the recording through core. Start asks the app; until the GPUI meeting page exists it opens the window. |
| Diktér | Starts hands-free dictation. Click into the field and press the dictation shortcut once to insert. Disabled while listening (the menu would insert into its own window) and when dictation is off. |
| Spørg (kommer senere) | Disabled until the assistant exists. |
| Åbn Tingskriver | Shows the main window. A left click on the icon does the same. |
| Afslut | Quits the app. |

With a tray icon, closing the main window hides the app to the tray.

## Shortcuts

| Action | Where it is set | Served by |
|---|---|---|
| Diktér (hold) | dictation settings (`shortcut`, default `Ctrl+Win`) | the keyboard hook |
| Ny optagelse, Spørg, Åbn Tingskriver | `%APPDATA%\dk.mahoje.tingskriver\shortcuts.json` | `RegisterHotKey` |

`shortcuts.json` is created on first start with no shortcuts:

```json
{
  "shortcuts": {
    "newRecording": "Ctrl+Alt+N",
    "ask": null,
    "openWindow": "Ctrl+Alt+T"
  }
}
```

The host reads the file every second and registers what changed, so an
edited shortcut works without a restart; unchanged ones stay registered. A
file that does not parse keeps the current shortcuts and logs a warning
once. A changed dictation shortcut (`Service::save_settings`) reinstalls the
hook at once.

The conflict check for the settings page is
`tingskriver_core::shortcuts::check(dictation, &settings)`. It returns every
problem with a Danish message (`Conflict::message`); `Conflict::blocks()`
says whether the shortcut cannot be saved.

| Problem | Blocks | Example |
|---|---|---|
| Not a shortcut | yes | `Ctrl+Hund` |
| Modifiers only, for anything but dictation | yes | `Ctrl+Alt` |
| Same as another action | yes | two actions on `Ctrl+Alt+N` |
| Windows uses it | yes | `Win+L`, `Win+E`, `Win+5`, `Ctrl+Win+D`, `Alt+F4` |
| Types text | yes | `A`, `Shift+A` |
| Another program has it (found when registering) | yes | shown in the tray tooltip |
| Contains the dictation modifiers | no | `Ctrl+Win+Space` starts dictation for a moment |
| Common in programs | no | `Ctrl+C`, `Ctrl+S` |
| A function key alone | no | `F9` |

`Host::save_shortcuts` saves and returns the check; the watcher registers
the new set within a second.

## Single instance

The shell's named pipe (`single_instance`) makes a second start of the exe
show the running app's window and exit, so there is only ever one hook and
one tray icon per user.

## Administrator and UIPI

Tingskriver must run as a normal user, never as administrator. The exe has
no manifest asking for elevation, and the installer must not add one.

Windows' User Interface Privilege Isolation (UIPI) keeps a normal process
from sending input to programs that run as administrator. So while an
elevated window (for example an administrator PowerShell, regedit or an
installer) has focus:

- the dictation shortcut is not seen by the hook,
- `RegisterHotKey` shortcuts are handled by Windows and normally still fire,
  but text cannot be pasted or typed into that window; dictation says so
  instead of failing silently.

Running Tingskriver itself as administrator would make the hook see keys in
elevated programs too, which is exactly what it should not do. If it is
started elevated anyway, the log warns and the tray shows the problem ring
with "Kører som administrator, så genveje og indsættelse virker ikke i alle
programmer".

## Manual test on Windows

Use the `tingskriver-app-windows-x64` artifact from the branch's Rust
workflow (or `cargo build --release -p tingskriver-app`). Quit the Tauri
Tingskriver first: both apps install a hook on Ctrl+Win.

1. **Hook timing (10,000 key presses).** In a terminal in the repository,
   with nothing else typing:
   `cargo test -p tingskriver-host --test hook_through_host -- --ignored --nocapture`.
   Do not touch the keyboard until it is done. It passes when both timing
   summaries have p99 under 1 ms and no log line names a key. In daily use,
   `tingskriver-app.log` gets a `Keyboard hook timing` line every 10,000
   key presses.
2. **Start menu and Windows shortcuts.** Start `tingskriver-app.exe`. Hold
   Ctrl+Win and release: the pill appears and goes, the Start menu does not
   open. Then press Win+E (Explorer opens) and Win+D (desktop shows), and
   press Win alone (Start opens). Repeat after a dictation.
3. **Tray colour.** Watch the tray icon while a recording starts and stops
   (when the GPUI meeting page exists; until then, the Tauri app's recording
   does not show here). The dot turns red at start and disappears at stop
   without a noticeable delay.
4. **Second start.** With the app running (window closed to the tray), start
   the exe again: no new process in Task Manager, the window appears.
5. **Shortcut changed while running.** Edit
   `%APPDATA%\dk.mahoje.tingskriver\shortcuts.json` and set
   `"openWindow": "Ctrl+Alt+T"`, save, close the window, press Ctrl+Alt+T
   within a few seconds: the window appears. Change it to `"Ctrl+Alt+Y"`
   and save: Ctrl+Alt+T does nothing now, Ctrl+Alt+Y opens the window. Set
   it to `"Win+E"`: the tray tooltip says Windows uses it, and Win+E still
   opens Explorer.
6. **Tray menu.** Right-click the icon: Ny optagelse, Diktér, Spørg (grey),
   Åbn Tingskriver, Afslut. Diktér, then click into Notepad, speak, press
   Ctrl+Win once: the text is inserted. Afslut ends the process and removes
   the icon.
7. **No keys in the log.** Type a sentence in Notepad with the app running,
   then search `tingskriver-app.log` for a word of it: not there.
