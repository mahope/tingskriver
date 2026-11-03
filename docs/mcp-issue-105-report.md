# Issue #105: blocked by the delivery layer

Status on 2026-11-03: **blocked; no MCP implementation shipped**.

## Evidence

The task branch and remote `main` both started at
`d480672648239fc7307d8071042a3e69231d6763` ("Merge pull request #163 from
mahope/release-0.5.0-dev.3-flyt-fra-tauri"). `git ls-remote origin
refs/heads/main` confirmed the remote commit. This issue is not already solved:
`Cargo.toml:3` lists the workspace crates without `tingskriver-mcp`; the
installer's extra files in `.github/workflows/app-release.yml:150` do not
include an MCP executable.

The public GitHub issue API reports:

| Dependency | State | Evidence in this checkout |
| --- | --- | --- |
| [#1: core extraction](https://github.com/mahope/tingskriver/issues/1) | Closed | `crates/tingskriver-core` exists. |
| [#31: FTS5 search](https://github.com/mahope/tingskriver/issues/31) | Closed | `database/repositories/search.rs` and `migrations/20261101120000_add_search_fts5.sql` exist in core. |
| [#70: delivery targets](https://github.com/mahope/tingskriver/issues/70) | **Open** | No implementation of configured folder/webhook/SSH delivery targets or `handoff` exists in core, host or app. |

Issue #70 is explicitly a prerequisite of #105. Its contract includes a
versioned sidecar, retries, configured targets and enforcement of private/local
mode. Implementing an independent delivery path in MCP would duplicate that
unfinished feature and risk bypassing its privacy rules. Returning success or
shipping a permanently unavailable `handoff` would not meet #105.

The supported app is now GPUI, despite older Tauri instructions in `CLAUDE.md`.
Its meeting page is a placeholder (`crates/tingskriver-app/src/ui.rs:363`),
so the per-meeting sharing toggle also needs integration with the forthcoming
meeting UI. The host's new-recording request currently only opens the window
(`crates/tingskriver-app/src/lib.rs:172`). These are integration constraints,
not claims that core recording or persistence is absent.

## Work performed

Read the repository README, CLAUDE.md, workspace, current app and host,
database schema, FTS5 repository, note storage and installer configuration.
Checked remote main and dependency issue states. No application code, schema,
settings or owner data was changed. In particular, the owner's installation
was not accessed and its access level was not changed.

No new package was installed. Preliminary dependency research found `rmcp`
is the official SDK at <https://github.com/modelcontextprotocol/rust-sdk>.
The registry index dates cached version 2.2.0 to 2026-07-08 and latest 3.8.0
to 2026-10-26. This is not a completed dependency approval: inspect the chosen
version's build script and resolved dependency delta before installation.
Conclusion for this blocked run: **do not install**.

## Validation

Baseline checks on the unchanged core:

- `cargo build --locked --offline -p tingskriver-core`: passed, with three
  existing library warnings (`unused_mut` and two dead-code warnings).
- `cargo test --locked --offline -p tingskriver-core`: passed, including
  701 unit tests and the calendar, database startup, notes, FTS5 and STT
  integration suites. The existing ignored tests were not enabled.
- `cargo clippy --locked --offline -p tingskriver-core -- -D warnings`:
  failed on existing code, including `config.rs:4` and `summary/mod.rs:8`
  (`empty_line_after_doc_comments`) and `audio/recording_preferences.rs:110`
  (`unused_mut`). The Rust workflow explicitly defers strict Clippy and fmt
  until the inherited code is cleaned up; this report does not fix that code.
- `python3 scripts/check-gpui-lock.py`: passed (16 GPUI packages match).

These are Linux core checks, not the Windows workspace build. No MCP fixture,
client handshake, named-pipe action, notification, Windows installer or
`netstat` test is claimed; the MCP feature does not exist yet.

## Unblocking and remaining scope

Merge #70 and expose its app-side handoff entry point first. Then implement
the rmcp stdio crate, read-only access with fixture-first privacy tests,
app-owned named-pipe actions with notifications and audit logging, settings
and per-meeting consent, installer packaging, and Danish/English setup and
privacy documentation. Resolve the executable location from the current
Velopack installation rather than assuming the old installer path.

No PR was created. This branch contains a blocker report only and must not
be presented as implementing or closing #105.

## Review follow-up (2026-11-03)

**P1 accepted and unresolved.** The reviewer correctly identifies that the
implementation is absent. Reproduced with
`cargo test --locked --offline -p tingskriver-mcp`, which fails with
`package ID specification 'tingskriver-mcp' did not match any packages`.
The entire branch diff against `origin/main` contains only this report.
There are no P2 or P3 findings in the supplied review.

Rechecked #70 using the public GitHub issue API: it remains open. A search
of the core, host and app Rust sources finds no `handoff`, `DeliveryTarget`
or `delivery_target` implementation. The issue explicitly requires configured
targets, retries, and private/local-mode enforcement; #105 explicitly lists
it as a blocking dependency. This follow-up does not implement #70 or invent
an alternative delivery contract. That leaves #105 blocked and the P1 open,
rather than treating a documentation change as a feature fix. Independent
read-only MCP work remains possible, but would not resolve this finding about
the missing complete implementation.

No application code or dependencies were changed, and no regression test for
implemented behavior is claimed. Do not merge this branch as a solution to
#105 or close the issue on the strength of the baseline checks below.

Fresh validation for this follow-up (all Cargo commands used `--locked
--offline`):

- Build of `tingskriver-core`, `tingskriver-overlay`, and `tingskriver-host`:
  passed, with the existing core warnings.
- Full Linux CI crate suite (`cargo test -p tingskriver-core
  -p tingskriver-overlay -p tingskriver-host`): passed, exit 0. Existing
  ignored tests were not enabled.
- Full workspace suite (`cargo test --workspace`): failed before tests,
  exit 101. The archived Tauri app's build script requires the missing
  `binaries/llama-helper-x86_64-unknown-linux-gnu` resource. This is not a
  passing full workspace run or Windows validation.
- Strict Clippy for the three Linux crates: failed, exit 101, with 127
  existing core errors including `unused_mut`, doc-comment formatting, and
  `incompatible_msrv`. No Rust source was changed in this branch.
- `python3 scripts/check-gpui-lock.py`: passed, 16 matching packages.
- `git diff --check`: passed.

No package was installed or added. The follow-up changes documentation only.
