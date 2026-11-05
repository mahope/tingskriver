# MCP settings completion for PR #168

Refs #105. Refs #169. Scope: the existing GPUI Settings page, not a replacement
settings screen or a merge of the PR into main.

## Integration and conflicts

Merged `origin/main` with a normal merge commit, `e9012df`. Exactly one conflict
occurred: `.github/workflows/rust.yml`, where both branches added a different
step after the core tests. Kept both the MCP stdio tests and main's asynchronous
I/O guard. Core `lib.rs` merged automatically. Cargo.lock, installer and settings
had no conflicts in this merge. The branch was pushed without force-pushing.
GitHub subsequently returned `mergeable: true` for #168; final CI status is
recorded below after validation.

## Requirement audit

Paths below are relative to the repository root.

| Requirement | Before | Result and evidence |
| --- | --- | --- |
| Master switch and safe defaults | Access buttons existed in `crates/tingskriver-app/src/ui.rs::mcp_settings`; no independent activation | Added `Settings` and additive migration `20261105000000_add_mcp_controls.sql`. Master and all content switches default false, including upgrades; selected meeting policy stays marked. `tests/mcp_defaults.rs` proves defaults. |
| Status, path, port | Copy buttons resolved the installed sibling executable, but path/status were invisible | Existing settings show disabled/ready/missing/failed state, exact program path, and matching client-owned process count at status refresh. Stdio has no network port. Self-test status announces progress/result. `mcp_probe::running_servers` is exercised against the real subprocess. |
| Meetings and data policy | All/marked/metadata/off existed; no content switches | Kept existing controls and meeting sharing page. Added independent notes, summary/tasks, transcript and speaker controls; filtered `get_meeting`, FTS search and task list within policy snapshots. Link and shared-meeting count reuse `SharingMeeting`. Handoff requires all content switches and existing delivery policy. `tests/mcp_access.rs`, `tests/mcp_actions.rs`. |
| Client setup | Copy Claude command and Codex TOML already existed in GPUI | Retained installed-sibling path generation; disabled copy if binary missing; exposed path and missing-installation explanation. `strings::mcp_claude_command`, `strings::mcp_codex_config`, Windows manual test below. |
| Connection test | Missing | `mcp_probe::test_connection` launches the installed binary, initializes stdio, validates all ten tool names, then kills/reaps the child. Five-second timeout, no meeting request and no meeting content displayed. `tingskriver-mcp/tests/stdio.rs` probes while disabled and verifies no audit calls were created. |
| Tools and call log | No GPUI list/journal; existing app action notifications only | Shared `mcp_audit::TOOLS`; bounded separate `mcp-audit.sqlite`, latest 100 calls, UTC time, known tool, existing meeting ID, outcome. No arguments/content/errors. Native Settings loads, refreshes and clears it with error/empty states. `tests/mcp_audit.rs` checks capacity/order/clear; stdio test verifies rejected raw input never enters the journal. |
| Danish privacy explanation | Existing catalogue text covered external provider disclosure | Expanded `strings/mod.rs`: explicit opt-in, content boundaries, dictations/actions in All, delivery, prior disclosures and in-flight actions. Participant switches do not scrub names embedded in prose. |
| Keyboard, screen reader, themes and screenshots | Existing GPUI buttons and themes | Uses standard focusable GPUI buttons (Tab/Shift+Tab, Enter/Space), explicit toggle state for AccessKit, accessible labels, status nodes, shared theme tokens and wrapping controls. Native screenshot fixtures cover light/dark, disabled/empty, enabled with success/failure journal, invalid settings and missing executable. Actual Windows UIA/Narrator testing remains manual. |
| About/changelog | No About page or changelog content on this branch | No new screen created. `update.rs` has installer notifications rather than a changelog. Updated existing bilingual README and `docs/mcp.md`; future About/changelog work can reuse this description. |

The master switch applies to both hosts through core policy. The earlier Tauri
settings still expose only the meeting policy; master/content switches are edited
in GPUI, as requested here. No installation is automatically changed to All.

## Validation

- Before the fix, a regression using the original `e9012df` Reader against the
  migrated fixture failed: actual `Marked`, expected `Off`. The production
  `mcp_defaults` test now passes. The original migration's marked default was
  also reproduced directly with SQLite. No historical migration was changed.
- Targeted core controls, action, journal and Danish catalogue tests; real stdio
  process test including content-free probe and running-process detection.
- Native screenshot command: `python3 scripts/test-mcp-screenshots.py` after
  building the three binaries listed in the script docstring. Baselines live in
  `docs/screenshots/mcp-native/`. `--update` regenerates them for review. This is
  the real Linux GPUI renderer in Xvfb, not a browser recreation. SSIM threshold
  allows small rendering/path differences; visual review is still required.
- Build isolation: never run baseline Cargo checks against the same target
  directory while another test process is executing. Cargo's build lock does
  not protect running tests/rustdoc from another revision replacing artifacts.
  The first full run had missing rlibs for that reason; it is not a product or
  baseline defect. The final run is serialized after the baseline check.
- The same all-targets clippy command on untouched `e9012df` reports seven
  errors. Two were the MCP Unix setup's synchronous filesystem operations;
  these now use Tokio filesystem calls, with the existing real IPC test rerun.
  The remaining backend_config/openrouter/whisper findings are pre-existing.
- `cargo build --locked -p tingskriver-app -p tingskriver-mcp -p
  tingskriver-core --bin tingskriver-app --bin tingskriver-mcp --example
  mcp_preview` passes. The GPUI app links/runs locally; no linker workaround or
  additional package installation was needed in this environment.
- `cargo test --locked -p tingskriver-core -p tingskriver-mcp -p
  tingskriver-app -p tingskriver-host -p tingskriver-overlay --no-fail-fast`
  passes: **953 passed, 0 failed, 10 existing ignored**, including doctests.
- `python3 scripts/test-mcp-screenshots.py` passes all **16** comparisons.
  Reviewed the actual native controls, log, missing-binary and settings-error
  renders. Only synthetic IDs/data and generic `/var/tmp` paths are pictured.
- Full all-targets clippy retains the pre-existing core findings listed above;
  this is not reported as a green lint gate. The final command reports five pre-existing errors (seven on the
  untouched integration baseline). No lint was disabled and no unrelated audio/provider code changed.
- The final head is pushed for GitHub Actions and PR mergeability verification
  after this report is committed. The accompanying handoff records the exact
  final SHA, workflow result and GitHub `mergeable`/`mergeable_state` response.
  Review is intentionally left to Mads' PC as instructed; this job does not merge
  the PR or deploy.
- No dependencies added. Cargo used the existing locked graph; no new package
  installation, telemetry, production change, issue or PR creation occurred.

## Manual release test (Windows, Claude Code and Codex)

1. Install the Windows CI artifact with its MCP executable beside the app.
   Check a new profile and an upgraded profile: MCP is off, marked policy is
   selected, four content switches are off, and every meeting is unshared unless
   previously explicitly shared. A previously shared meeting must remain
   inaccessible until the master switch is enabled.
2. Use Tab/Shift+Tab through every MCP control, Enter/Space to activate, and
   Narrator/Inspect to check names, toggle states, disabled states and status
   announcements. Repeat at 125/200% scaling and Windows light/dark appearance.
   Check long installed paths and a narrow window. Confirm clipboard success.
3. Copy the Claude command and Codex TOML from the installed GPUI app. Confirm
   the real installed path, including spaces/non-ASCII and Velopack versioned
   folders. Start both real clients and list tools while access is disabled.
   Content reads/actions must fail, but the content-free self-test must pass.
4. Enable MCP with marked-only access. Share a synthetic meeting on Meetings;
   return to Settings and check the count. Unmarked meetings stay hidden. Enable
   each content switch separately and verify get/search/tasks only expose the
   permitted fields. Disabling speakers hides the structured speaker field;
   it does not anonymize names embedded in prose. Metadata never reveals meeting
   body content. All additionally permits dictation and recording actions.
5. Toggle master off with clients connected: subsequent reads/actions fail.
   Process count may remain nonzero because clients own their stdio subprocesses.
   Refresh status after starting/stopping clients. Inspect netstat for the server
   PID: no TCP/UDP listener. Already returned data cannot be recalled.
6. Generate successful/rejected synthetic calls. Refresh the log; verify tool,
   UTC time, technical meeting ID and result only. Clear it and refresh again.
   Check read-only data folder/log-open failures and missing-binary status/copy
   disabling. Restore the real executable and rerun the probe. No fixture data
   or strings should remain in a real user's profile.
7. With the app closed, permitted reads still work; actions report that the app
   must be opened. With the app running, verify action notifications, add-note,
   regenerate-summary and delivery safeguards. Handoff fails until all four
   content switches are selected, and still obeys delivery private/local-only.
8. Update through Velopack, restart both clients, then recopy/retest setup.
   Review About/changelog mention when those separate branches land.

## Suggested PR update (do not create another PR)

Title: **Add a local MCP server with native privacy controls and diagnostics**

Add the rmcp stdio server and app-mediated meeting actions with explicit native
privacy controls. Keep MCP disabled by default, restrict initial meeting access
to marked meetings, and require separate permission for notes, summaries/tasks,
transcripts and speaker fields. Apply those choices to reads and search, and
require all content permissions for delivery.

Extend the existing GPUI settings with installed-path client setup, connection
self-test, process/status information, shared-meeting navigation, tool list and
a bounded local journal containing call metadata only. Add accessible toggle
states, Danish privacy explanations and native light/dark screenshot fixtures.
Preserve both MCP and asynchronous I/O CI tests when integrating main.

Validation: fixture-based core policy/action/journal tests, real stdio handshake
and tool calls, native app build and screenshot regression checks. Final CI and
full-suite results are recorded in this report. Actual Claude Code/Codex use,
Windows installation/UIA and updater-path verification remain manual release
checks; no production deployment or real-user data test was performed.

Rollback: revert the follow-up implementation commits to restore the previous
UI/server behavior; retain applied additive columns. The migration does not
delete meeting data. Old binaries ignore the new master/content flags, so a
binary rollback also requires disabling MCP in clients (or setting access Off)
to retain the new privacy boundary. Do not delete shared databases as rollback.

Refs #105
Refs #169
