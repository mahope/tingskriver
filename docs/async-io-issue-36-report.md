# Async core I/O (#36)

## Latest main integration

Merged main at `eb5b584a7bc1989cf3804078909f4bdb612c6a8d` with a normal
merge (no rebase). Git resolved all files automatically; there were no
unmerged paths or manual conflict choices in this integration. The earlier
conflict-resolution notes below describe the preceding integration.

Reviewed main's changes for saved dictation reruns (#261), parallel audio
finalization (#239), the summary engine (#257), crate-root formatting (#260),
and CI runner/dependency/cancellation rules (#247, #263). The merged service
retains the saved provider, Syv model and language fields. The parallel
encoders, summary modules and documentation are retained alongside the PR's
async HTTP, blocking-worker boundaries and I/O guard.

The Rust workflow and Clippy configuration are identical to main, including
`LINUX_RUNNER`, Windows `needs: [scope, lint]` and conditional cancellation.
README content is unchanged by this merge; the architecture document gains
main's saved-dictation section while retaining the async executor contract.
Formatting every Rust file changed against main produced no changes, and
the changed-file CI formatting gate reports:
`All changed files are rustfmt-clean.`

Verification of this integration (Linux, locked dependencies, shared Cargo
target directory; no workspace build):

- Targeted `cargo test --locked -p tingskriver-core dictation`: unit result
  `test result: ok. 211 passed; 0 failed; 1 ignored; 0 measured; 586 filtered out; finished in 1.00s`.
- Full `cargo test --locked -p tingskriver-core`: 895 passed, 0 failed,
  10 ignored across the test programs and doc-tests. The isolated ORT child
  prints an additional successful result already covered by its parent test;
  it is not counted twice. Last result:
  `test result: ok. 2 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 1.04s`.
- `cargo check --locked -p tingskriver-app`: last line
  `Finished dev profile [unoptimized + debuginfo] target(s) in 3m 57s`.
- `bash scripts/check-async-io-guard.sh`: async positive control and all nine
  negative controls pass. Last line:
  `Guard rejected: std::fs::canonicalize("unused")`.
- `bash .github/scripts/check-fmt-changed.sh origin/main`: all 53 changed
  Rust files pass. `git diff --check` passes; a repository-wide search finds
  no conflict markers.

The missing distribution package `libasound2-dev` was installed as requested;
`pkg-config` was already installed. No dependency or lockfile was changed.
Existing unused-mut, unused-parentheses, unused-import, unused-variable and
dead-code warnings remain. Windows execution and the fresh-context whole-PR
review remain with CI and the maintainer's machine respectively.

## Change

OpenRouter model discovery and its Tauri wrapper now await asynchronous HTTP.
Core HTTP requests have a total deadline of at most 10 seconds, including the
response body. Clients share a connection pool where no specialized download
configuration is needed. Provider overrides, downloads, Ollama pulls, summary
requests, dictation and analytics obey the same upper bound; shorter existing
limits remain in effect.

Model initialization, standalone model scanning, summary-language metadata
reads/writes and dictation WAV encoding/writes run on `spawn_blocking` instead
of an async worker. Existing synchronous recording/playback disk workers remain
synchronous. Console-window commands remain in the Tauri shell, outside core.

The architecture document requires GPUI to enter core through its background
executor. Core's Clippy configuration rejects blocking reqwest types; async
HTTP modules additionally deny blocking HTTP methods and direct `std::fs`
operations. The Linux CI job runs positive controls for blocking HTTP and file
reads. The guard's probe compilation allows unrelated legacy Clippy lints;
the normal workspace Clippy command remains unchanged.

`read_audio_file` was already removed on the starting main revision. Existing
`playback::tests::typed_errors_and_no_json_audio_command` verifies its absence
and the absence of frontend JSON number-array playback.

## Verification

Tests were added before implementation. The first targeted run failed because
the async endpoint helper did not exist. The implemented tests use a local
fake server that hangs before headers and during the body. Both requests time
out at the configured 10-second deadline; a separate core call completes while
the request is pending on a single-thread Tokio runtime. The elapsed-time
assertion allows 500 ms of scheduling tolerance. Success decoding, HTTP error
handling and provider timeout caps also have regression coverage.

- `cargo build --workspace --locked`: passes. The initial run required the
  missing Linux llama-helper sidecar; an ignored local placeholder was created
  using the same approach as CI. This validates compilation, not sidecar launch.
- `cargo test --workspace --locked`: 803 passed, 0 failed, 8 ignored.
- Final separate core suite: 683 passed, 0 failed, 8 ignored.
- `scripts/check-async-io-guard.sh`: passes; both deliberately inserted calls
  are rejected with `clippy::disallowed_methods`.
- `cargo clippy --workspace --locked -- -D warnings`: fails on existing Tauri
  build-script lints (`std::io::Read`, `double_ended_iterator_last`).
- Separate strict core Clippy: 135 errors on unchanged code. Comparing primary
  diagnostic spans with the diff finds no diagnostic on changed or added lines.
- `cargo fmt --check`: fails on pre-existing workspace formatting differences,
  beginning in `crates/tingskriver-app/src/update.rs`. No bulk reformat applied.
- `cargo tree -p tingskriver-core --locked -d`: inspected existing duplicates.
  No dependency version added or upgraded; Cargo.lock is unchanged. Existing
  lockfile builds need no new-package installation decision.

## Compatibility

Superseded by main's #172: the 10-second total deadline applies only to
ordinary requests (including the delivery webhook). Model downloads have a
four-hour total deadline with ten-second connection and chunk-inactivity
limits, summary generation allows 300 seconds, and provider inference keeps
its function default or explicit per-link limit. The provider test that
asserted the old 10-second cap was dropped when main was merged.

No UI runtime, live provider account or production deployment was used.
Fresh-context PR review is left to the orchestrator as requested.

## Suggested PR

Title: Make core HTTP asynchronous and bound I/O work

Description:

Replace blocking OpenRouter discovery with async HTTP and cap core HTTP
requests at 10 seconds, including response bodies. Move model initialization,
model scanning, metadata I/O and dictation audio writes onto blocking workers.
Add hanging-server tests, provider deadline tests and CI positive controls for
the Clippy I/O guard; document the GPUI executor contract.

Fixes #36

## PR #151 conflict resolution

Merged the current main into the PR branch without rebasing. Keep main's
removed analytics module removed, retain the new delivery, recovery, STT and
transcription-queue modules, and retain crate-wide denied blocking HTTP types
and filesystem methods. Main's expanded filesystem denylist is unchanged.
The guard probe now also exercises OpenOptions, directory creation/removal
and canonicalization; its async positive control remains in place.

Keep main's async transcript/settings helpers and blocking-worker boundaries.
Use the PR's shared model initializer from both engines, retaining main's
poisoned-mutex recovery and publishing only fully constructed engines. Keep
main's stronger simultaneous OpenRouter request test and the PR's successful
model decoding / HTTP error regression coverage. Remove the duplicate CI guard
invocation introduced by the merge. Format every Rust file changed by the PR.

The new delivery webhook had a 30-second timeout. A stalled-webhook regression
failed after 10.58 seconds before the fix and passes after using the core's
10-second HTTP deadline for both connecting and the complete request. Keep
redirects disabled. The separate 60-second SFTP process deadline is unchanged:
SFTP is a subprocess operation, outside the HTTP deadline contract.

No new dependency or lockfile change was introduced beyond main. Existing
locked dependencies were used. The Linux runner needed the distribution's
ALSA and Clang development packages before the native dependencies compiled.
The existing unused-mut and dead-code warnings remain outside this change.
Full fresh-context PR review remains assigned to the maintainer's machine.

Run the guard and the core suite sequentially in the same checkout:

```bash
bash scripts/check-async-io-guard.sh && cargo test --locked -p tingskriver-core
```

The guard temporarily creates an example using the `reqwest/blocking` feature.
A simultaneous ordinary `cargo test` discovers that example without the feature
and fails with E0433. Its EXIT trap removes the example before the next command.

Final merge verification:

- `cargo test --locked -p tingskriver-core`: 867 passed, 0 failed,
  10 ignored across unit, integration and documentation suites. Last result:
  `test result: ok. 2 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 2.40s`.
- `bash scripts/check-async-io-guard.sh`: async positive control and all nine
  negative controls pass. Last line:
  `Guard rejected: std::fs::canonicalize("unused")`.
- `cargo clippy --locked -p tingskriver-core --all-targets -- -A warnings -A clippy::all`:
  passes with the crate's explicit I/O denials active. This checks I/O guards,
  not the unrelated Clippy backlog. Last line:
  `Finished dev profile [unoptimized + debuginfo] target(s) in 1m 48s`.
- All 53 changed Rust files pass targeted `rustfmt --edition 2021 --check`.
  `git diff --check` is clean. No workspace build/test was run.
