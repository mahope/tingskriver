# GPUI meeting list (issue #56)

The meeting page now renders core's grouped list with GPUI's virtual `list`.
Only visible items and a small overdraw become elements. Date headers have their
own height, and meeting actions sit below the title, duration and Danish status
explanation so they can fit a narrow window.

`meeting_list` in core owns grouping (with an explicit timezone), status mapping,
chunk-status precedence, title validation, deletion guards and recovery candidate
selection. Its viewport API borrows rows from the original allocation. Database
mutations reuse `MeetingsRepository`; the UI never implements persistence rules.
All disk/database work runs on Tokio or a blocking worker. Empty, loading and
load/scan failure states reuse the existing shell blocks and theme.

Rename trims the title and rejects empty input. Delete requires an explicit
confirmation showing the meeting title. Recording, queued, transcribing,
summarizing and unknown states cannot be deleted. Deletion removes the database
meeting and its text; audio files remain on disk, as the confirmation explains.

Recovery scans checkpoints independently of the database query, including on a
first launch without a database. A user-triggered restore creates the database
if needed, saves the recovered meeting, and reports partial audio explicitly.
Failed recovery leaves the candidate available for another attempt.

## Boundaries still awaiting other GPUI work

- The recording screen and meeting document are separate ports. **New recording**
  currently explains that recording must start in the existing app. **Open**
  selects the meeting and offers its saved audio folder; it explains where to
  read the transcript until the document port is available. These are handoffs,
  not an implementation of those screens.
- Recovery restores audio and the meeting; transcription must currently be
  started in the existing app.
- Queue events on `transcription-queue-status` update the list. The host worker
  and persisted startup queue snapshot are not integrated yet; see
  [transcription-queue.md](transcription-queue.md). `load_with_jobs` is ready to
  consume `Queue::statuses()` when the host owns that queue. Metadata and saved
  summary states are read on refresh in the meantime.
- No snapshots were added: #49 / PR #172 has not merged. Required light/dark
  fixtures are listed in [design.md](design.md#meeting-list-verification-pending-49).

## Verification

Commands use `CARGO_TARGET_DIR=$HOME/dev/cargo-target/tingskriver` and build only
the changed app/core crates and their necessary dependencies, never the workspace.

| Command | Result / final output line |
|---|---|
| `cargo check -p tingskriver-app --locked` | Passed. `Finished dev profile [unoptimized + debuginfo] target(s)` (Cargo quotes `dev` with backticks). |
| `cargo test -p tingskriver-core meeting_list --locked` | Nine new tests passed, including 1,000 meetings and pointer identity at every scroll position, timezone boundaries, checkpoint scanning and persisted rename/delete. Unit summary: `test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 764 filtered out; finished in 0.03s`. Later filtered integration binaries finish with `test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 4 filtered out; finished in 0.00s`. |
| `cargo test -p tingskriver-core --locked` | 870 passed, 0 failed, 10 ignored across unit/integration/doc tests. Final line: `test result: ok. 2 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 5.20s`. |
| `.github/scripts/check-fmt-changed.sh origin/main` | `All changed files are rustfmt-clean.` |
| `python3 scripts/check-design-doc.py` | `design doc ok: 28 components in code, 30 rows` |
| `git diff --check` | Passed with no output. |
| `cargo clippy -p tingskriver-core -p tingskriver-app --locked` | Failed: `error: could not compile tingskriver-core (lib) due to 1 previous error; 130 warnings emitted` (Cargo quotes the crate name with backticks). The unchanged `audio/capture/backend_config.rs:55` triggers `clippy::inherent_to_string_shadow_display`. Reproduced by running core Clippy on a clean worktree of the base commit. No new module warnings were reported before that failure. App Clippy could not complete behind the core failure. |

No new Cargo dependencies or lockfile changes. Missing Linux build headers were
installed from the system package repository (ALSA, udev, XKB, Wayland, Vulkan and
Fontconfig); no third-party project packages were added.

Not verified: runtime GPUI layout/input, installed Windows build, Windows-only
paths, actual scroll frame times, end-to-end crash/restore/encode in the GPUI app,
and screenshot fixtures. The core recovery suite ran, but that is not a manual
crash-recovery test of the desktop app. Review and CI remain for the orchestrator;
this job does not create a PR.

## Proposed pull request

Title: **Add the virtual GPUI meeting list and recovery banner**

Body:

```markdown
Refs #56

Replace the meeting placeholder with a virtual date-grouped list, durations,
Danish status explanations, rename, confirmed deletion and startup recovery.
Keep the list model and mutation/recovery rules in UI-free core with nine tests.

Reused components: page, heading/section_title, paragraph/muted, loading,
empty_state, Button and Input, using the existing theme. Added MeetingsView
because no meeting-list view existed; updated docs/design.md.

Validation: app cargo check; full core suite (870 passed, 0 failed, 10 ignored);
changed-file rustfmt; design-document check. Clippy is blocked by the same
pre-existing core error reproduced on the base commit.

Screenshots remain pending #49 / PR #172; required light/dark fixtures are in
docs/design.md. Windows/runtime visual checks are not performed. Recording and
meeting-document screens remain explicit handoffs; the queue worker and startup
status snapshot are still pending host integration. Recovery restores the audio
and meeting; transcription starts in the existing app.
```

Use `Refs #56` until the missing screenshot acceptance criterion is verified;
do not auto-close the issue with a `Fixes #56` keyword at this stage.
