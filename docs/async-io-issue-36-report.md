# Async core I/O (#36)

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

The 10-second total deadline also applies to large downloads and remote/local
HTTP inference. Operations that previously ran for minutes can now return a
timeout. This follows the issue's explicit upper bound; existing cancellation,
error reporting and download retry/resume paths remain in place.

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
