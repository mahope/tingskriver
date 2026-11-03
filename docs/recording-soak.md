# Three-hour recording soak (#91)

The separate `Three-hour recording soak` Actions workflow runs nightly at
22:20 UTC and on `workflow_dispatch`. It does not run on pushes or PRs.
It records two 48 kHz mono tracks for three real hours, then starts another
three-hour recording and kills the recorder process halfway through (4.5 hours
total, plus build and verification). Failures produce a nonzero exit and a red
workflow. Metrics are retained as a JSONL artifact, including on failure.

No new packages are required. Build with the existing Cargo lockfile:

```sh
cargo build --release --locked -p tingskriver-core --example recording_soak
python3 scripts/recording-soak.py \
  --binary target/release/examples/recording_soak --output /tmp/recording-soak
```

Use a fresh output directory each time. Linux `/proc` supplies recorder RSS
and cumulative user + kernel CPU ticks. Every five minutes (including time zero)
the log includes RSS, CPU ticks and WAV bytes. Peak sampled RSS growth must be
strictly below 100 decimal MB. CPU and disk size are diagnostic measurements,
without an arbitrary CPU regression threshold. Two tracks use about 2.1 GB for
three hours; allow at least 4 GB free space. Raw audio stays local and is not
uploaded as an artifact. `--seconds 8 --interval 1` exercises the same control
flow quickly, but does not establish the three-hour performance budget.

The fake device emits 10 ms callbacks into the **production bounded disk
writer**. Both tracks carry nonzero square tones with distinct 10 ms markers
every second. Verification streams every recovered sample through the production
recovery scanner and source reader and compares it with the expected signal.
This detects lost, repeated, reordered and zero-filled samples, including gaps
shorter than 5 ms. Each track must match the recording clock within 0.5 seconds;
the generator also checks its monotonic wall clock. The crash variant uses an
actual process kill without joining or flushing the writer. Recovery repairs
headers and must retain all but less than two seconds on each track.

Scope: this isolates storage, checkpointing and recovery from hardware. It does
not exercise OS device callbacks, microphone filters/resampling, UI, encoded
mixed playback or transcription. It proves process-crash recovery, not power-loss
survival. Ordinary CI continues to run the existing focused audio unit tests.

## Manual hardware variant (Windows)

Use only synthetic audio, never real voices. Play a generated continuous tone
with an audible marker each second through a loopback-capable device. Capture
that device and a microphone fed the same test source for three hours in the app.
Record initial working set and then working set, CPU and recording-folder size
every five minutes with Task Manager/Performance Monitor. Require less than
100 MB growth. Stop normally, inspect both saved tracks and mixed playback for
marker alignment (<0.5 seconds drift) and system-track zero runs (>5 ms).
Repeat with Task Manager ending the app at 90 minutes; restart, recover the
recording and compare the final marker with the kill time (<=2 seconds lost).
Record device names, sample rates, app commit, measurements and result. This
manual protocol is required to establish hardware and full-app behavior.
