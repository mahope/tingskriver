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
