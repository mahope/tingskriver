# Android Whisper baseline: protocol and current report (#133)

## Status: NOT MEASURED

2026-11-03: `adb devices` returned an empty device list on agent-worker. No
physical phone, speech model or consented recording
was available. Milestone 2 is **open**. Do not enable native IME integration or
cloud fallback on the strength of these changes. No WER, latency, PSS, energy
or thermal result has been observed. Existing SPEC thresholds remain unchanged.

This change supplies a native CPU Whisper driver, an arm64 NDK build script,
a corpus-lock utility, USB collection, offline scoring and regression CI.
The native driver builds against the existing locked whisper-rs-sys 0.11.1
source (whisper.cpp 1.7.1) on this Linux host. An Android cross-build and actual
inference require an installed NDK and frozen models; neither is available here.
No native library or dependency was added to the IME, desktop or Cargo workspace.

| Required run metadata | Current value |
| --- | --- |
| Midrange phone / SoC / RAM / OS build | unavailable |
| Recent phone / SoC / RAM / OS build | unavailable |
| whisper.cpp full engine revision / build flags / NDK | not selected |
| Multilingual base SHA-256 / bytes | unavailable |
| Multilingual small SHA-256 / bytes | unavailable |
| Multilingual small-q5_1 SHA-256 / bytes | unavailable |
| Consent record / WAV SHA-256 lock | not recorded |

Never fill these cells with guessed values or published desktop estimates.
The model and engine revision are benchmark candidates, not a production choice.

## Fixed consented corpus

`scripts/android-benchmark/corpus.json` freezes six 15-second prompts/conditions:
two quiet Danish passages, their two noisy equivalents and two silence cases.
Place names and landmarks exercise proper nouns without referring to private
people. These are authored test sentences, not customer material. This small
pilot cannot establish population-wide Danish accuracy; expand speakers and
accents in a new corpus version before making broader claims.

Obtain explicit speaker consent for local benchmark processing, retention and
aggregate publication; keep the consent record outside this public repository.
A voice remains personal data even when the words are fictional. Do not commit
voices, transcripts, consent documents, serials, private paths or raw adb logs.
No recordings have been obtained in this task.

1. On each consented speaker's quiet recording, read each quiet passage within
   15 s without time stretching; pad the end to exactly 15 s. Use mono signed
   PCM16, 16 kHz WAV. Store as `quiet-01.wav` and `quiet-02.wav` outside the repo.
2. Copy the speech for `noise-01.wav` / `noise-02.wav`. Mix deterministic seeded
   non-speech noise at 10 dB SNR (speech-active RMS, no clipping). Record the
   seed, mixing tool/revision and active sample ranges in the local protocol.
   Freeze the resulting WAV hashes, rather than regenerating between phones.
3. `silence-01.wav`: 15 s digital zero. `silence-02.wav`: 15 s consented empty-room
   microphone noise, with no voices. Listen locally to verify every reference,
   condition and absence of unintended personal information.
4. Lock the same six WAVs once and keep the lock immutable between all runs:

```sh
python3 scripts/android-benchmark/lock_corpus.py /private/bench/audio \
  /private/bench/manifest.json --consent consent-record-001
sha256sum /private/bench/manifest.json
```

The lock rejects missing files, existing output and anything other than exactly
240,000 mono PCM16 samples at 16 kHz. It hashes actual WAV bytes. It cannot verify
that consent is legally valid, that a person read the reference correctly, or
that the noise mix follows the protocol; a human must do those checks.

## Device method and native driver

Use two **physical** arm64-v8a phones: one midrange and one recent. Record model,
SoC, RAM, Android version/security patch/build, battery health, ambient temperature,
free RAM, compiler/NDK, thread count, CPU/GPU backend, decoding flags and driver
binary SHA-256 in a local method record. Do not publish device identifiers.

Select an audited immutable whisper.cpp commit and record its full 40-hex revision.
Build a release arm64 driver separately, without installing a keyboard. Freeze
multilingual `base`, `small`, and `small-q5_1` from the same source model family;
never `.en`. Record license, immutable download origin, conversion/quantization
command and tool revision, SHA-256 and actual file size for each artifact. No
upstream repository was cloned and no new package was installed in this task.

The native executable takes `MODEL PCM16_PATH` and emits cold milliseconds,
warm milliseconds, peak PSS MB (one per line), then the unmodified hypothesis.
`device.py` creates the result JSON, rather than requiring a second Android
runtime or JSON library. Each invocation loads one model once, then decodes in
that same resident process. Danish `da`, greedy decoding, zero temperature and
fallback increment, four threads, CPU, no prompt and no context carry-over are
fixed in source. No network API, download or cloud fallback is used. Enable
Android airplane mode and disable Wi-Fi/Bluetooth before running.

Build with an **already audited local clean whisper.cpp checkout** at the chosen
full commit and an **already installed NDK**. The script never clones or installs
anything. Record the chosen NDK version and use that exact version on reruns.
CMake statically links whisper/ggml and the C++ runtime for arm64-v8a/API 28:

```sh
python3 scripts/android-benchmark/build_driver.py \
  --source /private/whisper.cpp --revision FULL_40_HEX_COMMIT \
  --ndk /installed/android-ndk --output /private/bench/build-arm64
```

Keep `build-arm64/driver.json` beside `driver`. Collection verifies its binary
hash, ABI and engine revision. It verifies pushed driver/model/PCM hashes using
Android `sha256sum`. The corpus WAV hashes are verified before conversion to
PCM16. The native process converts PCM16 to float **inside** the warm window.
The build metadata is retained in the private results; do not commit it blindly.

The collector writes one JSON document containing the corpus lock fields,
`engine_revision`, and a `runs` array. For each phone/model/clip, supply exactly
20 indexed observations (`repeat` 0..19). Each observation contains:

```json
{
  "tier": "midrange", "hardware": "phone model", "os": "Android version",
  "abi": "arm64-v8a", "model": "base", "model_sha256": "64 lowercase hex",
  "model_bytes": 147000000, "clip": "quiet-01", "repeat": 0,
  "hypothesis": "local private transcription",
  "cold_ms": 1500, "warm_ms": 2500, "peak_pss_mb": 400,
  "energy_mwh": 1.2, "temperature_start_c": 25,
  "temperature_end_c": 26, "thermal_status": 0
}
```

Numbers above illustrate fields only; they are **not measurements**.
For every observation run a cold arm followed by a warm arm:

- Cold: new process, time from just before model open to ready-to-decode using
  monotonic time. This is process-cold, **not** filesystem-cache-cold; do not
  root/drop caches. Report that distinction.
- Warm: same resident process/model, feed the completed 15-second PCM buffer;
  clock from stop/buffer-ready to final text, including preprocessing and decode.
  Exclude the 15 s capture, include all post-stop work. No process launch in this
  window. A standalone whisper CLI invocation is not a warm measurement.
- The driver samples its own `/proc/self/smaps_rollup` Pss every 50 ms across
  load and inference, converting KiB to decimal MB (KiB * 1024/1e6). Missing PSS
  fails the observation. This avoids repeated adb/dumpsys overhead; quantify the
  sampler overhead in the method record. Sampled peak may miss short spikes.
- Measure per-observation energy over both cold and warm arms with a calibrated
  external power meter, integral watts*time converted to mWh. Disconnect charging
  for timing/thermal runs; report meter setup and battery operating point. If
  only battery charge-counter estimates are possible, label them as estimates
  with resolution and a separate repeated long-batch measurement. Unsupported
  energy is missing, never zero. Do not claim energy measured via `batterystats`
  alone. The scorer requires finite nonnegative numeric energy.
- Record battery temperature before/after (Celsius), ambient temperature, and
  Android thermal status (0..6), plus sustained-throttle observations. Cool to
  the same baseline before each cold/warm pair; randomize and log model/clip
  execution order. Also run a separate 10-minute resident-model sustained loop
  and publish aggregate thermal/latency drift; the scorer does not certify that
  sustained experiment. Airplane mode, screen state and battery band must match.

Use an otherwise idle test phone and keep all private files outside the repo.
The collector stages only the supplied driver, models and converted corpus PCM
under `/data/local/tmp/tingskriver-bench`. It overwrites that dedicated staging
area; use no other files there. USB collection uses adb's default single device
and rejects emulators:

```sh
python3 scripts/android-benchmark/device.py /private/bench/midrange.json \
  --manifest /private/bench/manifest.json --audio-dir /private/bench/audio \
  --model-dir /private/bench/models --driver /private/bench/build-arm64/driver \
  --tier midrange --engine-revision FULL_40_HEX_COMMIT
# Repeat with the recent phone attached and --tier recent.
```

Models must be named `ggml-base.bin`, `ggml-small.bin`, `ggml-small-q5_1.bin`.
Execution order is seeded (133) and preserved in result order. There is a 60 s
foreground cooldown before each observation; this is not proof of a matched
thermal baseline. Discard/repeat observations outside the recorded baseline.
Battery temperature and thermal status are read via dumpsys; unavailable fields
fail collection. Completed rows are saved after each observation so USB failure
does not lose the run. Use a new output file for every run.

**Energy requires external instrumentation.** The collector sets `energy_mwh`
to JSON null and records `energy: not collected`. Align power-meter readings to
the recorded execution order, retain the calibration/time record privately and
fill each row only with its measured energy before scoring. Null is deliberately
rejected by the scorer. No battery/energy estimates or fabricated zeroes are used.
The separate sustained 10-minute test and ambient/battery controls remain manual
protocol steps. Raw JSON hypotheses may contain personal data; never upload to CI.

## Scoring and acceptance

Merge the two local documents into a single document: identical consent/corpus
and engine revision, concatenate `runs`. Verify model hashes match between
phones. Preserve build flags, hardware metadata, hashes and method record locally;
publish their non-personal subset alongside aggregate results after review.

```sh
python3 scripts/android-benchmark/benchmark.py /private/bench/combined.json \
  --audio-dir /private/bench/audio --output /private/bench/summary.json
```

Exit 0: complete matrix meeting the four SPEC metrics. Exit 1: complete matrix
with a failing target. Exit 2: incomplete/invalid data or audio hash mismatch.
The summary includes no references, hypotheses or arbitrary metadata strings.
Do not treat exit 0 as a substitute for the consent and method audit above.

WER/CER use NFC, lowercase, Unicode alphanumeric tokens, punctuation removed.
CER excludes whitespace. Both are corpus-weighted Levenshtein edits divided by
reference length; quiet and noisy subsets are separate, not a mean of per-clip
percentages. Empty references are excluded from WER/CER and scored separately:
any non-whitespace output on either silence case fails, including punctuation.
Nearest-rank p50/p95 are over 15 s speech observations per device/model. Energy
is total over all observations; temperature/thermal status and PSS report maxima.
The 20 repeats characterize timing variance, not 20 independent speakers.

Required SPEC gates per device/model:

- Quiet WER <= 0.15. No noisy-WER or CER target is currently specified.
- Warm stop-to-text p95 <= 3000 ms on 15 s speech.
- Base sampled peak PSS <= 700 decimal MB. Small has no SPEC memory threshold.
- Zero silence outputs. There is no energy or thermal numeric SPEC threshold;
  report values and investigate throttling rather than inventing a passing gate.

If any model fails, retain the failed result. Before milestone 5, propose a
separate explicit SPEC amendment stating the affected device tier, observed
metric, proposed revised threshold/support floor/model choice and trade-off.
Do not silently relax thresholds or activate cloud fallback. With no measurements,
there is no evidence to justify a numeric SPEC change now.

## Verification report

The review's P1 (missing executable/build path) was valid. Native source, a local
NDK cross-build path, verified staging and full device collection now replace the
unimplemented executable contract. Regression tests first failed for the missing
native source/parser, then passed after implementation. Host compilation against
the existing Cargo-cached Whisper source and real-executable invalid-PCM and
missing-model smoke tests pass. A fake-engine native test additionally exercises
PCM conversion, fixed decoding flags and preservation of silence hallucinations.
CI is configured to build using the locked Cargo dependency and cross-compile
with the runner's preinstalled NDK; that job has not been observed here.

No new package was installed and no foreign repository was cloned. The native
library used for host verification was already present under the existing
Cargo.lock; the build runs its existing CMake configuration. No model was loaded
successfully and no phone measurement is claimed. Android NDK/physical inference,
energy calibration and sustained thermal validation remain unverified locally.

Validation commands:

- `python3 -m compileall -q scripts/android-benchmark` and `git diff --check` pass.
- `python3 -m unittest discover -s scripts/android-benchmark -v`: 13 tests pass.
- Native CMake host build and `smoke_native.py`: pass. Upstream CMake emits its
  existing minimum-version/ccache warnings; driver builds with `-Werror`.
- Android assemble/JVM tests/lint: blocked before configuration by Java 11
  (Gradle requires >=17).
- `cargo test --workspace --locked --offline --no-fail-fast`: failed during
  build, before tests: missing Tauri `llama-helper` sidecar, and native llama
  compilation exhausted the shared filesystem. A benchmark test compile also
  hit that temporary disk shortage; it passed when rerun after Cargo exited.
- `pnpm --dir frontend build` and `pnpm --dir frontend lint`: blocked because
  node_modules are absent. No frontend test script is defined.

These broader failures are outside this benchmark diff. Full-repository checks
are **not green**; no desktop/Android application success is claimed.

Outstanding: consented recordings and final hashes, audited engine/model pinning,
both physical phones, calibrated power instrument, all actual measurements,
sustained thermal run, aggregate publication and model/SPEC decision. Issue #133
and milestone 2 remain open. Do not infer acceptance from tool tests.
