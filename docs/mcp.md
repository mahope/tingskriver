# Local MCP integration

`tingskriver-mcp` uses the official Rust SDK (`rmcp`) over newline-delimited
JSON-RPC stdio. Stdout contains only protocol traffic. No HTTP server, network
port, cloud service or credentials are used by the MCP process.

Build with `cargo build --locked -p tingskriver-mcp`. For a development data
directory, start `tingskriver-mcp --data-dir /absolute/path/to/fixture`.
The default is core's existing app-data directory, including
`%APPDATA%\dk.mahoje.tingskriver` on Windows. It opens the meeting database
and `dictation.sqlite` read-only, without migrations, recovery writes or
creation of missing files. Open the updated app once to apply the migration.

The installer places the server beside the app. Tauri's NSIS installation
normally uses `%LOCALAPPDATA%\Tingskriver`; Velopack uses a versioned/current
directory. Always prefer the setup copied from the running app: it resolves
the actual executable location. Claude Code and Codex launch the same server.
Restart the MCP client after installing an update if it holds an older process.

## Native settings and privacy controls

The GPUI app has a separate **Aktivér MCP-serveren** switch. It defaults off,
including when upgrading an existing MCP installation. The selected meeting
policy remains `marked`; notes, summary/tasks, transcript and participant/speaker
fields all start disabled. Enable only the fields you intend to share. These
choices also filter search excerpts and tasks. Speaker suppression removes the
structured speaker field, not names embedded in prose. Metadata policy still
limits content even when content switches are selected. `all` separately permits
dictations and recording actions; use marked-only access to exclude those.

Settings show the installed executable path, whether it is available, the number
of matching client-owned server processes at the last refresh, shared-meeting
count and a link to the existing sharing page. There is no listening port or
app-owned daemon to start: each client launches its own stdio process. Turning
off access blocks subsequent reads/actions without killing the client's process;
it cannot retract content already supplied or cancel an action already running.

**Test forbindelsen** starts that executable, initializes MCP and lists tools,
then kills/reaps the test process. It never calls a meeting tool and works while
sharing is disabled. Missing binary, timeout and protocol failures are displayed.

The native call journal contains the latest 100 known tool calls, UTC timestamps,
validated existing meeting IDs and success/failure. It stores no tool arguments,
results, titles, search terms, notes or error strings. `mcp-audit.sqlite` is a
separate writable local file; the meeting database stays read-only in the server.
Refresh and clear are explicit actions in Settings. If journaling fails after an
action, stderr reports the failure without changing the completed action's result.
Local users with access to the data directory can also access this metadata.

Both clients' configuration buttons resolve the binary beside the running app,
including Velopack's current/versioned path; they are disabled if it is missing.
The earlier Tauri settings expose the meeting policy only; use the GPUI app to
set the master/content switches. No owner-specific automatic permission exists.

## Access

The following table applies **after** enabling the master switch; meeting
content is further restricted by the four independent content switches.

| Level | Meetings | Content | Dictation | Actions |
| --- | --- | --- | --- | --- |
| `marked` (default) | Explicitly shared only | Shared meetings | Denied | Shared meetings only |
| `metadata` | All | Titles, dates, duration; tasks only with summary permission | Denied | Denied |
| `all` | All | All | Allowed | Allowed |
| `off` | None | None | Denied | Denied |

Recording actions require `all`: stop has no meeting ID, so it must not bypass
the sharing decision on a hidden recording. New recordings remain unmarked.
There is no owner detection or automatic `all` setting. The owner can select
Alt explicitly. The installed GPUI app's Meetings page has an "AI-værktøjer (MCP)"
section below the meeting list that lists both shared and private meetings
and lets the owner share or revoke each meeting. Refresh the
list to pick up meetings created while the app is open. Tauri also has a
per-meeting sharing checkbox. Both apps persist the same sharing flag.

Each read snapshots policy and data together. Invalid/missing policy or an old
schema fails closed. Unknown meeting IDs and hidden meetings have the same
error. Search applies sharing and metadata restrictions before LIMIT; snippets
come from the existing FTS5 index, never a separate copy. Metadata search only
matches titles. Tasks are Markdown checkboxes or list items under recognized
task headings in the saved summary; arbitrary surrounding summary text is
never returned as a task. There is no structured task/assignee model yet.

## Tools

| Tool | Arguments |
| --- | --- |
| `list_meetings` | `fra?`, `til?` (SQLite date/ISO time, lower inclusive/upper exclusive), `søg?` (title substring) |
| `get_meeting` | `id`, `dele?` (`referat`, `noter`, `transskript`; default all allowed parts) |
| `search` | `tekst` (literal FTS5 terms, AND) |
| `list_tasks` | `åbne?` (default true) |
| `recent_dictations` | `n?` (default 20, maximum 500) |
| `start_recording` | `titel?` |
| `stop_recording` | none |
| `regenerate_summary` | `id`, `skabelon` (existing template ID) |
| `handoff` | `id`, `mål` (existing delivery target ID) |
| `add_note` | `id`, `tekst` (Markdown to append) |

Actions use `\\.\pipe\tingskriver` on Windows; remote pipe clients are refused.
Unix development uses `mcp-ipc/actions.sock` inside a mode-0700 directory.
Requests and replies are bounded to 64 KiB. Calls time out without automatic
retry: check the app before repeating a timed-out action. The app rechecks
access independently; the MCP process cannot write its own policy, sharing
flags, notes, summaries, targets or queue entries. App logs record operation
name and success, never action arguments or note text. Both app hosts forward
action notifications. Notifications may be limited by OS permissions.

`handoff` requires all four content switches and stores a snapshot in
`awaiting_approval` for an already configured target. It never approves delivery.
Open the meeting in the app, select the target and give the same explicit consent
as any manual delivery through **Godkend og aflever referat**. Approval replaces
the request snapshot with the current saved content and the visible summary.
Unapproved requests cannot be retried or sent by the worker, including after restart.
They do not block editing the target or rotating its webhook credential. Approval
revalidates the current target; once approved, the job blocks target edits until
delivery completes.
Local-only and private-mode switches still block both requests and approval.
There is no persisted per-meeting private flag in #70; unshare sensitive meetings
and use marked-only access, or enable the delivery private-mode switch.

The approval dialog currently lives in the Tauri app (`DeliveryApproval`); GPUI
does not yet have a delivery approval screen. Requests made through the native
host remain unapproved until the user switches to the Tauri app and approves
there. Close the native app before switching queue owners.

The native host runs a delivery worker because approved jobs must survive app
restarts and retry without the Tauri host. It shares one service with its MCP
channel. Both are disabled when `dictation_blocked` detects the legacy Tauri app:
that app owns the action endpoint and delivery queue, avoiding two queue owners.

`add_note` requires the notes switch; `regenerate_summary` requires the summary
switch. Regeneration uses the existing `result_backup` field to copy the current summary
before starting and retains it after successful completion, so user edits are not lost. Metadata mode exposes
extracted tasks independently of the full-summary switch and includes duration.
Windows pipe names include a hash of the data directory and username, shared
by the app and MCP client. Connection failures recreate the pipe after a backoff.

**Data passed to an external AI tool leaves this computer via that tool's
provider.** The local server cannot enforce a third party's retention policy.

## Verification

`cargo test --locked -p tingskriver-core --test mcp_access --test mcp_actions`
checks opt-in defaults, all access modes, policy refresh, FTS privacy, task
projection, dictation fields and app-side action authorization/notifications.
`cargo test --locked -p tingskriver-mcp` starts the real subprocess, initializes
MCP, discovers tools and calls them with the app closed. UI tests live in
`frontend/tests/lib/mcp.test.tsx`.

Windows release verification still needs an installed app with real audio
devices: add it to both clients, list/get a marked meeting, exercise recording
and regeneration, check action notifications/logs, and run `netstat -ano`
while the server is active. No port should belong to the MCP process. These
hardware/client checks cannot be replaced by Linux fixture tests.

Web control previews use only synthetic data (native GPUI rendering is separate):

![MCP controls at 375 px, light](screenshots/mcp-375-light.png)
![MCP controls at 1440 px, dark](screenshots/mcp-1440-dark.png)
