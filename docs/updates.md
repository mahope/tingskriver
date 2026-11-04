# Updates in the GPUI app

The installed Windows app uses the existing Velopack 1.2.161 manager, installer
hooks, GitHub source and package verification. About shows the update status,
version, full-package size, release notes and the release date supplied in the
feed. No additional release API or telemetry service is used. The app remains
usable after update failures. Development and portable copies cannot update.

In **Indstillinger → Opdateringer**, choose automatic checks on/off and daily
(default) or weekly checks. Preferences and the last attempted check survive
restart in the existing local JSON settings store (`updates.json`). A failed
attempt also counts towards the interval, preventing offline retry storms.
The initial check waits 60 seconds; the app also checks when the interval expires
while it stays in the tray. Turning checks off prevents manual and automatic
checks and new downloads. An already started network operation may finish.
Downloaded packages remain staged for the next start.

**Om → Søg efter opdateringer** checks immediately when enabled. Nothing is
downloaded until **Hent og installér**. Progress is shown as a percentage.
Choose **Genstart nu** to apply the package, or **Genstart senere** to keep
working; the existing primary-instance startup hook applies it at next start.
Download and restart controls are disabled while the existing core recording or
dictation state is active. An action racing a recording is refused, with an
inline message and an explicit retry after recording ends. The final restart
hand-off blocks new recordings under the recording lifecycle lock and new
dictation starts under a shared gate. No recording is stopped by the updater.

The tray's About entry indicates an available or downloaded update. Settings
also show the current status. Controls use the existing GPUI keyboard buttons
and accessible names; status text has the AccessKit Status role. Windows
Narrator/UIA still needs the installed-build check below.

## Feed and release notes

The current installer publishes one `win` channel, not independent stable/beta
feeds, so the channel selector is hidden. Stable builds ignore GitHub
pre-releases; builds with a version suffix include them, as before. See
[installer/README.md](../installer/README.md) for the ten-release window and the
`TINGSKRIVER_UPDATE_FEED` folder/HTTP/off override. The HTTP adapter reads the
same Velopack feed without the default persistent staging identifier. Neither
source sends meeting data, settings, credentials or a user identifier.

Pass a Markdown file to `pack.ps1 -ReleaseNotes`. Optional `## da` and `## en`
sections select Danish first, otherwise English; unsectioned notes are shown
as supplied. `Release-Date: YYYY-MM-DD` supplies the release date; the packaging
script supplies its UTC date if absent. For delayed publication, set the actual
publication date explicitly. Older feeds with no date or notes show an honest
missing-data message. Notes are plain text, never executed HTML.

Velopack verifies package length and SHA256 (SHA1 fallback for old feeds).
An integrity failure is displayed as a verification error. This is **not**
proof of Authenticode publisher identity; signing/winget remain #62 work.
Network/feed failures and disk/access errors appear inline with retry guidance.
Raw errors, URLs, filesystem paths and response bodies are not shown in the UI.

## Verification and manual Windows plan

Core policy tests: `cargo test --locked -p tingskriver-core updates:: --lib`.
App unit tests compile the real Velopack adapter on Linux as well as Windows.
They exercise a loopback HTTP feed to verify that no persistent installation
identifier is sent, and check integrity, disk-space and access error messages.
When comparing a baseline checkout with this branch, use a separate
`CARGO_TARGET_DIR` for each checkout: sharing outputs between identical path
packages can leave stale core metadata in app tests.
The real GPUI renderer has seven offline fixtures in both themes:
`python3 scripts/screenshot-updates.py target/debug/tingskriver-app /tmp/update-shots`.
Fixtures require a debug build; release builds ignore the mock environment
variable. Images exercise rendering, not a real installer or UIA screen reader.
The [fixture gallery](screenshots/updates/README.md) contains all fourteen images.
The script also uses Tab/Enter to change settings, restarts the app and verifies
that on/off and frequency are persisted in both themes.

On a disposable Windows VM with two real Velopack builds and a local test feed:

1. Install the older build. Turn checks off, restart, wait over a minute and
   click manual check; observe zero feed requests. Toggle daily/weekly, restart
   and verify the choice. Repeated starts must not bypass the saved interval.
2. Enable checks, select the local folder/HTTP feed, and check manually. Verify
   version, size, Danish notes (then English fallback), date, and tray indicator.
   Use Tab/Enter and Narrator to exercise About and the settings controls.
3. During a meeting, a paused meeting and dictation, verify download/restart are
   disabled. Stop recording, download, observe progress, choose Later, restart
   and verify the newer version and retained meetings/settings. Repeat with Now.
4. Repeat offline, with a denied/missing feed, a truncated/tampered package and
   a full or unwritable package disk. Verify an inline error and a working app.
5. Run a portable and a development build; verify the unavailable message and
   no feed traffic. Check both themes and long notes at Windows text scaling.

Rollback: revert the feature commits. Settings are additive and can remain;
no database migration or user-data conversion is introduced. A revert restores
the previous background-download policy, so disable the feed during rollback
testing if no update traffic is wanted.
