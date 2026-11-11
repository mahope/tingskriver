# Crash recovery timing investigation

## Evidence before the change

The reported Windows failure was `fed 9.73s, recovered 3.02s`. It does not
include the queue depth, completed writes, or disk latency, so it cannot by
itself distinguish a stalled writer from a durability defect.

The old test runs capture for ten wall-clock seconds, then calls
`scan_raw_tracks(..., true)` while the writer thread remains alive. It neither
terminates the thread nor synchronizes with its progress. `StorageSender::send`
only enqueues audio; its success does not acknowledge a write. The queue holds
12,000 chunks (roughly a minute for two tracks). The test counts queued audio
as fed audio, even if the writer has not been scheduled or is blocked in I/O.
Repair also writes headers concurrently with the live writer.

The production path already addresses commit drift: `TrackWriter::tick`
commits when either one second has elapsed or one second of samples is
pending. `WavSegmentStorage` uses unbuffered `File::write_all` for PCM followed
by header updates, calls `sync_data` at the sync interval, syncs the previous
segment before rollover, and syncs the last segment at stop. There is no
userspace buffered writer awaiting a missing flush. This is code evidence,
not a measurement of Windows disk durability.

Before editing the test, 30 consecutive Linux runs passed: 15 without extra
load and 15 alongside four workers repeatedly hashing, writing and fsyncing
1 MiB each. Every invocation used `--test-threads=1`. Thus the original Windows
failure was **not naturally reproduced** on Linux.

A temporary diagnostic used the existing `spawn_writer_with` storage factory
to park the writer at its first dequeued chunk, using rendezvous channels
rather than sleeps. Capture queued 9.73 seconds on both tracks. Before
resuming the writer, the same scanner saw zero seconds; after resuming and
joining, it recovered all 9.73 seconds and the summary reported zero loss:

```text
diagnostic: fed 9.73s, queued 19.45s (both tracks), stalled snapshot 0.00s, after resume 9.73s
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 791 filtered out; finished in 0.24s
```

This demonstrates the harness failure mechanism (a): its old two-second
assertion would fail before the writer resumed even though all enqueued
audio was intact. It does **not** establish whether Windows was blocked in
I/O or delayed by scheduling. No durability defect (b) was demonstrated.
The temporary diagnostic was removed after the experiment.

## Correction

Only the test changes; the production writers are unchanged. The test now
drives the same `TrackWriter` and real `WavSegmentStorage` synchronously with
an explicit clock. It scans after each 10 ms chunk over ten seconds, checks
that both tracks exist, and asserts **at most 96,000 missing samples per
track at 48 kHz** (exactly two seconds). No finish, stop, forced commit or
header repair runs before these snapshots.

The test exercises both normal clock advancement and a frozen clock, where
sample-count commits must protect audio arriving in a burst. Checking every
chunk also prevents an implementation with an excessive commit interval
from passing just because the final snapshot happens to land after a commit.
The existing threaded recording-writer tests continue to cover queue
draining, slow storage, storage errors and recovery. The revised test does
not measure scheduling latency or claim to simulate physical power loss.

## Sensitivity check

Temporarily changing `COMMIT_INTERVAL` from one to three seconds made the
revised test fail at the first snapshot exceeding its unchanged loss budget:

```text
burst false, Microphone: fed 2.02s, recovered 0.01s
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 790 filtered out; finished in 0.01s
```

The production file was restored byte for byte after this experiment; it is
not part of the diff.

## Repetitions and targeted suite

Each repetition used:

```sh
export CARGO_TARGET_DIR=$HOME/dev/cargo-target/tingskriver
cargo test --locked -p tingskriver-core crash_recovery_loses_at_most_two_seconds -- --test-threads=1
```

| Version | No extra load | CPU and disk load | Total failures | Test duration |
| --- | --- | --- | --- | --- |
| Original | 0/15 failed | 0/15 failed | 0/30 | 10.05–11.64 s |
| Corrected | 0/15 failed | 0/15 failed | 0/30 | 0.11–0.18 s |

The four load workers used scoped Python threads, SHA-256, and independent
temporary files with repeated 1 MiB writes and `fsync`. The foreground script
joined every worker on completion. No load processes were left running.

Last relevant output from the corrected repetition:

```text
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 790 filtered out; finished in 0.12s
after failures: 0/30
```

`cargo test --locked -p tingskriver-core finalize`:

```text
test result: ok. 18 passed; 0 failed; 0 ignored; 0 measured; 773 filtered out; finished in 2.42s
```

`rustfmt --edition 2021 --check` on the changed Rust file and
`git diff --check` both pass. Existing core warnings concern unused items in
unmodified files; this change adds no warnings.

The final full `cargo test --locked -p tingskriver-core` run exited zero,
including the threaded recording-writer tests, integration tests and doctests.
Core unit-test output and the final output line were:

```text
test result: ok. 786 passed; 0 failed; 5 ignored; 0 measured; 0 filtered out; finished in 31.11s
test result: ok. 2 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 3.44s
```

## Proposed pull request

Title: `Make the crash recovery loss test deterministic`

Body:

The test scanned a live writer after ten wall-clock seconds, counting queued
audio as written and racing header repair against ongoing writes. Drive the
production track writers and real WAV storage with a controlled clock instead,
checking both tracks after every chunk with the unchanged two-second limit.

Validation: 30 runs before and after (15 loaded each), 18 finalize tests and
the full core suite passed. A three-second commit-interval mutation fails the
test. Windows and physical power loss remain unverified. No UI components
were changed.

The architecture's two-second power-loss budget explicitly assumes a healthy
disk and separately reports storage stalls. No finite loss budget can hold
while a writer is indefinitely unscheduled or a disk stops completing writes.

## Verification environment

Only `tingskriver-core` is built and tested. Cargo uses the existing lockfile
and the requested shared target directory. No new Rust dependencies were
introduced. The first build failed because `alsa.pc` was missing; installing
the explicitly requested Ubuntu `libasound2-dev` package resolved that build
prerequisite (`pkg-config` was already installed).

Windows scheduling, Windows filesystem behavior and physical power loss have
not been verified on this Linux host.
