# Command line and `tingskriver://` links

The GPUI executable is `tingskriver-app.exe`. Commands are processed by core
on its runtime, independently of the main window. A command launch starts
hidden when no instance exists; subsequent launches send to the existing
instance and exit. A launch without arguments shows the existing window.

| Argument | Action | URL equivalent |
| --- | --- | --- |
| `--toggle-dictation` | Start hands-free dictation, or stop and insert it | `tingskriver://toggle-dictation` |
| `--cancel` | Discard the active dictation capture | `tingskriver://cancel` |
| `--start-meeting` | Disabled until recording status is visible; exits with code 4 | None |
| `--stop-meeting` | Stop recording and save the captured audio | None |
| `--hidden` | Start without a window; a second hidden launch does nothing | — |
| `--minimized` | Alias for `--hidden` | — |
| `tingskriver://mode/Chat` | Select a named dictation mode for subsequent captures | Same |

Quote URLs in a shell. Mode names match exactly and must exist in the saved
dictation settings. For example, `"tingskriver://mode/M%C3%B8de%20noter"`
selects `Møde noter`. Names are UTF-8, percent-decoded once; `+` stays `+`.
Empty names, control characters, invalid UTF-8/escapes, query strings,
fragments and extra URL path segments are rejected. Decoded names have a
256-byte limit. An explicit mode overrides automatic foreground matching
until another mode is selected or the application restarts. Each capture uses
the selected mode's current saved settings, including edits and renames. If
the mode is deleted, the selection is cleared and automatic matching resumes.

Only one action is allowed per invocation, optionally with `--hidden`.
Unknown arguments and URLs are errors. The existing `--diagnose [--json]`
and `--page meetings|dictation|settings` options remain available; diagnosis
cannot be combined with an external action. Installer hooks run before
normal argument validation.

## Exit codes

| Code | Meaning |
| --- | --- |
| 0 | Startup succeeded, or the existing instance acknowledged queuing the command |
| 2 | Invalid or unknown command, URL, or argument combination |
| 3 | Hand-over to the running instance failed: the pipe stayed busy or the instance did not answer within about five seconds, a command was not acknowledged, or the pipe server is another account's (see below) |
| 4 | Meeting recording is disabled because recording status is not visible |
| 5 | An instance of this account already runs as administrator; a normal process can neither verify nor reach it (or is refused at the pipe). Close it and start the app again without administrator rights |

Acknowledgement confirms delivery to the queue, **not completion** of audio
capture, transcription or other asynchronous work. Execution failures (for
example unavailable microphone, dictation still initializing, missing mode,
or a meeting already running) go to the app log. `--cancel` cancels active
capture; it does not revoke text already sent for transcription or insertion.
The diagnostic command retains its own 0/1 healthy/error exit status.

## Single instance and security

The existing single-instance transport now lives in UI-free core. Its
`Transport` trait allows a fake claim/forward transport on Linux; production
uses Tokio named pipes on Windows. The pipe name contains the account SID
read from the process token, never `USERNAME` or another environment value.
A protected DACL grants access only to that SID, and a Medium integrity label
(`S:(ML;;NW;;;ME)`, no write up) lets any normal process of the account reach
the pipe of an instance running as administrator, which would otherwise be
refused at open and could not tell that instance from a stranger. The label is
Medium and not Low (`LW`) on purpose: a sandboxed Low-integrity process of the
same account (a browser tab, say) must not be able to send commands. Remote
clients are rejected. Every replacement pipe instance gets the same DACL and
label. First-instance creation is atomic. Before writing anything, the client
obtains the server PID with `GetNamedPipeServerProcessId` and compares its
process-token SID with the client account SID. A mismatch or failed
verification rejects the hand-over and nothing is sent; a failed
acknowledgement is a failure too. What the launch
does then depends on the cause (below).

### When claiming or handing over fails

A second full instance means a second keyboard hook, doubled inserted text and
a shared data folder, so a launch only runs without forwarding where no
instance of this account can be running. Every failure is sorted by cause:

| Cause | Launch without a command | Launch with a command |
| --- | --- | --- |
| The pipe belongs to another account: it opened, but its server process runs as another SID | Starts without forwarding; logs a warning with no content | Exit 3, nothing sent |
| No pipe can be named or created (for example the SID lookup fails), so no other instance can hold one either | Starts without forwarding; same warning | Exit 3 |
| Opening the pipe is refused (access denied). The name holds this account's own SID, so this is almost always its own instance running as administrator, started before the integrity label above existed or otherwise out of reach. The owner of the pipe object is read without opening the pipe: Administrators or this account is an elevated instance | Exit 5 | Exit 5 |
| Opening the pipe is refused and its owner cannot be read, or is another account | Exit 3 | Exit 3 |
| The pipe is busy, or the instance went away between the check and the connection | Tries again after 100, 200, 400, 800, 1500 and 2000 ms (about five seconds in all), then exits with 3 | Same |
| The message was sent but not acknowledged within two seconds | Tried again the same way | Exit 3 at once: the command may have arrived, and sending it twice could toggle dictation on and off |
| This account's own instance runs as administrator: its process token cannot be read, and the pipe object is owned by Administrators or by this account | Exit 5 with a message | Exit 5 |
| Anything else, for example the server process id is unavailable | Exit 3 | Exit 3 |

A refused open never starts a second instance: when in doubt the launch does
not run on its own, because two instances mean two keyboard hooks and doubled
text. An elevated instance is told from another account's by the owner of the
pipe object, because both refuse the process-token query. Another elevated
administrator also reads as elevated; nothing is sent to it either way. After
"Prøv igen" in the crash dialog the five-second wait for the old instance to
release the pipe is the retry window, and nothing is retried afterwards. The
messages for codes 3 and 5 are logged, and shown when the app was started from
a terminal. A command is never executed locally as a fallback.

Each connection accepts one newline-terminated command, capped at 4096 bytes
and two seconds, then closes. The sender waits at most two seconds for an
acknowledgement. Only the fixed command vocabulary above and the window-show
signal are accepted. There is no shell execution, arbitrary file path,
script, export, database query or data-return command. The reply is one
constant status byte. This is a local account boundary, not a protection
against another program already running as the same account.

Unix builds retain the existing development socket transport. The Windows
SID, DACL and registry paths require Windows CI and runtime verification;
Linux tests exercise the parser, queue framing and hand-over policy.

## Installation

The after-install and after-update hooks create
`HKCU\Software\Classes\tingskriver`, set `URL Protocol` to an empty string,
and set `shell\open\command` to `"<installed exe>" "%1"`. Registry writes
use the existing `win::set_string` / `create()` helper, including for new
profiles. Before uninstall, only that per-user protocol tree is removed.
No machine-wide association or administrator rights are required.

## Current meeting limitation

The GPUI app cannot yet show or stop CLI meeting recordings or list them.
Therefore `--start-meeting` is rejected before startup with exit code 4 and a
Danish/English explanation. The dispatcher also refuses it as defense in depth.
`--stop-meeting` remains a CLI flag only. Both meeting URLs are invalid (code 2);
the URL scheme accepts only `mode/<name>`, `toggle-dictation` and `cancel`.

The Windows release attaches to the invoking console before validating arguments,
preserving redirected stdin, stdout and stderr independently. Launching from
Explorer still opens no console window. It keeps the console only while startup
can still print: invalid arguments, a refused hand-over, and `--diagnose`, which
holds it until it exits. A launch that runs on as the application then releases
it (`FreeConsole`), so log lines stop landing in the shell and neither Ctrl+C nor
closing the terminal kills the app or a dictation in progress. Redirected
handles stay valid. `FreeConsole` closes the console's handles but leaves their
numbers in the process, where the next file the app opens could be given the
same number and receive every log line (the logger writes to stderr). So the
standard handles the attach filled in, and only those, are set to null
just before `FreeConsole` (`SetStdHandle`), the state of a program that never had a
console. Rust's `print!`, `eprintln!` and the logger then write nothing and do
not panic: a null standard handle reads as an invalid handle, which `std`
treats as a silent success.

To verify the actual Windows release startup (a console test executable does not
exercise this path), run `tingskriver-app.exe --start-meeting` from PowerShell
without redirection and confirm the Danish/English refusal is visible. Use
`Start-Process -FilePath .\tingskriver-app.exe -ArgumentList '--start-meeting'
-NoNewWindow -Wait -PassThru` to inspect `ExitCode` (4), and repeat with
`--unknown-command` (2). Repeat with only stdout redirected, only stderr
redirected, and both redirected; the refusal must reach stderr's chosen target
in every case. Check `--diagnose --json` with stdout redirected separately to
confirm diagnostics still reach that file. Start `tingskriver-app.exe --hidden`
from a terminal, then press Ctrl+C and close that terminal: the app must keep
running and print no further log lines there. With an instance started as
administrator, a normal `tingskriver-app.exe --toggle-dictation` must print the
administrator message and exit with 5, and no second instance may appear; a plain
launch without a command must do the same, not start a second instance. Open a file
in the app after starting it from a terminal (a recording, say) and check that no
log line ends up in it. These checks require Windows.
