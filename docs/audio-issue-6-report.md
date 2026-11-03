# Issue #6: durable separate recording tracks

The base commit already records microphone and system audio as separate PCM WAV
segments, with shared-clock offsets in `tracks.json`, a bounded nonblocking
storage channel, storage alarms, and final encoding after stop. Its missing
piece for the two-second power-loss budget was `sync_data`: files reached the
OS every second, but reached disk only every five seconds.

Reduced the disk sync interval to one second, leaving room for the next writer
commit within the two-second budget on a healthy disk. Capture still never
blocks on storage. Slow or failing disks can exceed the budget and continue to
use the existing backlog/drop/error reporting. Hardware power-loss durability
depends on the filesystem and drive honoring sync requests.

The new regression test was run first against the unchanged implementation and
failed with `Microphone must reach the disk every second`. After the fix it
passes for both sources and checks that sample data and updated WAV sizes are
present before the sync callback runs. No dependencies, manifests or lockfiles
were changed.

## Validation (Linux sandbox, 2026-11-03)

- `cargo build --workspace --locked`: passed. Local Tauri staging followed
  `BUILDING.md`: existing system FFmpeg, empty test-only llama-helper, and a
  freshly built real STT worker in the ignored binaries directory. The first
  build failed before staging because llama-helper was missing.
- `cargo test --workspace --locked`: the first run ended unsuccessfully while
  the disk filled; its log also recorded a calendar network-test failure and
  did not contain a complete summary. That test passed independently.
  After removing only this worktree's own incremental cache, the full command
  was rerun with `RUST_TEST_THREADS=2`: **877 passed, 0 failed, 9 ignored** across
  27 test executables/doc-test groups. No shared build directories were deleted.
- Focused track-writer tests: **13 passed**, including simulated one-hour flat
  buffering, bounded buffering during disk failure, crash-readable WAV segments
  and retry after failed sync. Recording-writer tests: **10 passed**, including
  nonblocking overflow alarms, slow storage, matching track ends and long-recording
  stop latency (193 ms for 300 seconds of synthetic audio in this run).
- Built the existing `recording_soak` example and ran
  `scripts/recording-soak.py --binary target/debug/examples/recording_soak
  --output /tmp/tingskriver-6-soak --seconds 20 --interval 5`: passed normal
  finalization and actual process-kill recovery. Both recovered tracks retained
  9.970 seconds against 10.043 seconds of wall time (about **72 ms loss**).
  Normal tracks differed by one sample; crash tracks by three samples at 48 kHz.
  This short run does not establish ten-minute drift or hour-long RSS behavior.
- All **8** Python soak-verifier tests passed with `RECORDING_SOAK_BINARY` set.
- `cargo clippy --workspace --locked`: failed on the pre-existing
  `clippy::inherent_to_string_shadow_display` at
  `audio/capture/backend_config.rs:55`; that file is unchanged from main.
  Core emitted 127 existing warnings; none referenced the changed track writer.
- Targeted `rustfmt --check` and `git diff --check`: passed. All eight Cargo
  TOML/lock files parsed without duplicate keys. No new packages were installed.

## Remaining manual acceptance

On Windows, record ten minutes of synthetic hardware audio, verify no FFmpeg
process during capture, kill with `taskkill /F`, restart and recover. Check
loss against wall time (at most two seconds) and track drift (under 100 ms).
Measure full-app buffer/RSS behavior over 60 minutes. The existing simulated
hour test and short Linux crash run do not establish those hardware criteria or
survival of an actual power cut.

## Suggested PR

Title: `Sync both recording tracks to disk every second`

The separate raw recording tracks already landed with the capture/transcription
split, but their five-second disk-sync interval exceeded the two-second
power-loss budget. Sync each track every second and add a regression test that
checks both PCM data and WAV headers before sync. Workspace build and 877 tests
pass; Clippy remains blocked by an unchanged baseline error. Windows hardware
acceptance remains manual.

Fixes #6
