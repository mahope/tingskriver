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
and cumulative user + kernel CPU ticks. Every five minutes, at time zero and after the last callback,
the log includes RSS, CPU ticks and WAV bytes. Peak sampled RSS growth must be
strictly below 100 decimal MB. CPU and disk size are diagnostic measurements,
without an arbitrary CPU regression threshold. Two tracks use about 2.1 GB for
three hours; allow at least 4 GB free space. Raw audio stays local and is not
uploaded as an artifact. `--seconds 8 --interval 1` exercises the same control
flow quickly, but does not establish the three-hour performance budget.

The fake device emits 10 ms callbacks into **production `AudioCapture`**,
including microphone filtering/normalization and `RecordingState` hand-off to
the bounded disk writer. Both tracks carry nonzero square tones with 10 ms
markers encoding a 16-bit second index (unique throughout the three-hour run).
Verification streams every recovered sample through the production
recovery scanner and source reader and compares it with the expected signal.
This detects lost, repeated, reordered and zero-filled samples, including gaps
shorter than 5 ms. Each track must match the recording clock within 0.5 seconds;
the generator also checks its monotonic wall clock. The crash variant uses an
actual process kill without joining or flushing the writer. Recovery checks
the production checkpoint detector and recovery command,
repairs headers, encodes/mixes the audio and must retain all but less than two
seconds on each track. Normal completion exercises the production startup
pending-finalization scan. Both variants decode the final `audio.mp4` and check
its duration. FFmpeg must be on PATH (installed by the nightly workflow).
The recorder waits for the final RSS measurement before releasing its capture
state; a missing acknowledgment fails after 60 seconds.

Scope: this exercises 48 kHz mono capture, storage, checkpointing, finalization
and recovery without hardware. The expected microphone samples are generated
with the same production filters in bounded blocks. For normal completion,
the generated block count defines the signal length; any track-end alignment
tail is checked separately for silence, even when callback skew exceeds 10 ms.
After a crash, the scanner can append whole callbacks of silence to align
unequal track ends. The verifier derives the signal boundary from the file
sources before that padding; all earlier samples are checked. The loss limit
uses each track's last file endpoint plus its recording offset, excluding
scanner padding, so a longer track cannot hide loss on the shorter track.
It does not exercise OS device callbacks, other sample rates/resampling, UI
startup/recovery dialogs or transcription. It proves process-crash recovery,
not power-loss
survival. Ordinary CI continues to run the existing focused audio unit tests.

## Time budget after the recording

`finish` (scan, verify every sample, encode `microphone.m4a`, `system.m4a` and
`audio.mp4`, decode them again) is linear in the recording length. The three
AAC encodes dominate, and FFmpeg's encoder uses one core each. They run side
by side now; before, one after another. Measured on a 16-core Linux
machine with other jobs running (single runs, so +-15 %):

| Recording | Before (serial) | After (parallel) |
|-----------|-----------------|------------------|
| 4.5 min   | not measured    | 8.6 s            |
| 9 min     | not measured    | 16.9 s           |
| 10 min    | 36-39 s         | not measured     |
| 20 min    | 70-83 s         | not measured     |
| 30 min    | 119-120 s       | 51-55 s          |

The after column is 0.029-0.032 s per recorded second at 4.5, 9 and 30 minutes
(the two short runs were measured side by side), so no non-linear effect shows
up in that range. That extrapolates to 700-740 s for three hours before and
about 310-350 s after. The extrapolation is a factor of 6 beyond the longest
measurement, and a GitHub runner with fewer cores would encode the three
streams less in parallel, which is why the limit is 1800 s and not close to
the estimate. The old script allowed `finish` 600 s, so it sat at the limit and
failed on the night of 2026-11-05 although the code was unchanged on that
path (the encode path has no diff between the commit before #164 and `main`).
`recover` and `finish` now get 1200 s: the limit only has to catch a hang, and
it is still more than three times the measured 310-350 s.
`recover` was not timed separately; it runs the same scan and encode path.
Stopping `finish` early cannot lose audio: raw tracks are deleted only after
the outputs verify as `ok`, outputs are written to `.{name}.tmp` files and
renamed, and a killed run leaves the raw tracks plus stale temp files that the
next run overwrites. The three encodes use one core each. On a small machine they compete with other
work after stop, such as transcription or a new recording started before the
previous one has finished; that costs time, not audio, because capture never
waits for the disk writer. The job's `timeout-minutes: 345` (20,700 s) covers
3 h + 1.5 h of recording (16,200 s) and both limits (2,400 s), leaving about
35 minutes for checkout, packages and a cold build.

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
