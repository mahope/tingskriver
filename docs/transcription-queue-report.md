# Issue #52: transcription queue report

The core queue is implemented and pushed on `issue/52-transskriptionskoe`.
The desktop application is not yet wired to use it. No PR was created.

Implementation commit: `f1918df340640dd7fd192c9106c14e5b58734673`.

## Changes

- A dedicated SQLite queue with FULL synchronous WAL commits, atomic claims,
  attempts, due times, terminal failure after five attempts, exponential
  backoff/jitter and recovery of interrupted jobs. Stale attempts cannot write
  results after recovery/reclaim.
- The shared transcription provider chain, explicit `da` by default,
  microphone label from the job/settings, system diarization, existing track
  merging, offsets for chunk input and a Parakeet/Whisper provider adapter.
- Reruns create new jobs with optional provider/language overrides and preserve
  earlier successful versions, including when the rerun fails.
- Status/progress callbacks and a status snapshot API; no UI dependency or
  modal dialogs.
- `Retry-After` now survives generic HTTP errors (including 503), in addition
  to the existing 429 classification. The queue uses the largest requested
  wait among failed chain links.

The implementation contract and limitations are in
[transcription-queue.md](transcription-queue.md).

## Verification

Tests were written before connecting the queue implementation. The initial
run failed because the queue API was absent. A later fake-server 503 test
failed on the lost `Retry-After` value before the provider client was fixed.

| Command | Result |
| --- | --- |
| `cargo build --locked -p tingskriver-core` | Pass; 5 existing warnings |
| `cargo test --locked -p tingskriver-core` | 690 passed, 0 failed, 8 ignored across unit, integration and doc tests |
| `cargo test --locked -p tingskriver-core transcription_queue --lib` | Final run: 10 passed, 0 failed |
| `cargo deny --locked check licenses bans` | `bans ok, licenses ok` |
| `python3 scripts/check-gpui-lock.py` | Pass |
| Targeted `rustfmt --check` on the new queue files and changed error type | Pass |
| `git diff --check` | Pass |
| `cargo clippy --locked -p tingskriver-core --all-targets` | Fails on 3 existing errors in unchanged files; no queue diagnostics |

The final targeted run includes the strengthened crash test: a separate
process commits a running claim, is forcibly killed, then a new queue instance
recovers and completes the job through the worker. The full suite passed
before that test-only strengthening; production code did not change afterwards.

Fake-server tests exercise 500, 429, 503, timeout before headers and timeout
while reading a response body. They verify retry bounds, increasing backoff,
`Retry-After`, exhaustion and no successful version after failure. Other tests
cover restart-persistent backoff, competing claims, stale-attempt fencing,
speaker labels, Danish requests, actual WAV decoding into a local provider
double, and rerun overrides/version preservation.

Clippy errors are `inherent_to_string_shadow_display` in
`audio/capture/backend_config.rs:55` and `unused_io_amount` in
`whisper_engine/whisper_engine.rs:1691` and `:1891`. Both files are byte-for-byte
unchanged relative to `origin/main` (`git diff --quiet origin/main -- <files>`
returned 0). This baseline is also documented in `docs/agenter.md`. Clippy
reported 131 library warnings and 139 test warnings, with duplicates; new queue
files had no diagnostics. These checks were not weakened or bypassed.

No dependencies were added or upgraded. Only the existing lockfile was used;
no new-package installation decision was needed.

## Remaining work and acceptance limits

- Host integration: recover at startup, enqueue after finalization/chunking,
  poll/cancel the worker, load the selected local model, forward events and
  connect the rerun/version selector. Existing recording and retranscription
  paths remain unchanged, so desktop users do not automatically use this queue.
- The queue accepts saved WAV tracks/chunks. Preparing those inputs from final
  compressed recordings, grouping multiple chunks into a selected meeting
  version, retention/deletion and multi-process ownership are not implemented.
  One application process must own the database; recovery runs before workers.
- The manual Windows/Syv.ai meeting with two other speakers is not performed.
  The fake server proves label/diarization wiring, not acoustic quality.
- No-key local selection and language `da` are tested, including decoding and
  forwarding to a local provider double. Real Parakeet/Whisper Danish inference
  still needs an installed model and audio fixture/manual validation.
- Windows workspace build/tests and final fresh-context review remain with CI
  and the orchestrator. At the implementation push, performance CI passed;
  Rust, branch build and WER workflows were still running.

## Suggested PR

Title: **Add a persistent transcription queue with retries and reruns**

Add a SQLite-backed core transcription queue with crash recovery, bounded
retries, Retry-After handling, shared provider fallback, track merging and
immutable rerun versions. Verify it with fake HTTP servers, local provider
doubles and a forced process termination test. Host/UI integration and actual
model/Syv.ai validation remain as described above.

Core scope of #52. The orchestrator should decide issue closure after reviewing
the remaining application integration.
