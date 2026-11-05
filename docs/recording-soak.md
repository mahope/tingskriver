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

## Channel isolation with pilot tones (#189)

Tones that find gaps do not find leakage between the channels. The
`recording_soak isolation` mode records three phases through the same production
capture and writer: both pilots (523 Hz on the microphone, 997 Hz on the system),
only the microphone pilot, only the system pilot. It then analyses the saved
tracks (`audio::isolation`) and prints the isolation per phase in dB, the clipped
share and the duration difference. A muted or mis-routed channel fails as a missing
pilot, not as a good isolation. The recording is never touched: the analysis reads
the finished files.

The microphone track passes through the production capture filters (high-pass,
noise suppression, loudness normalisation in `pipeline.rs`) before it is saved, and
the analysis measures what is saved. A normaliser can raise the microphone pilot and
a noise suppressor can dampen a pure tone, so read the reported pilot level and
isolation figures with that in mind; they have not been compared with and without
the filters yet.

```sh
cargo run --locked -p tingskriver-core --example recording_soak -- \
  isolation /tmp/isolation 2 docs/kanalisolation-graenser.json
```

`scripts/recording-soak.py` runs it after the soak variants (`--isolation-seconds`,
default 3 per phase, 0 skips it); the `Rust` workflow runs it on every push. The
thresholds are only in `docs/kanalisolation-graenser.json`, which the tools and
[the release checklist](release-checkliste.md#kanalisolation-før-hver-release) share.

For a recording from a real machine, run the `kanalisolation` example on the two
saved WAV tracks and the phase times (`both:0:10 mic:10:20 system:20:30`); it exits
0 for pass, 1 for leakage/clipping/duration and 2 for unusable input, and `--json`
prints the report as JSON. `scripts/tests/test_kanalisolation.py` runs it on clean
and leaking WAV fixtures (`KANALISOLATION_BINARY`). The method is inspired by the
anarlog Linux audio test (MIT, Fastrepl, Inc.); no code is copied.

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
