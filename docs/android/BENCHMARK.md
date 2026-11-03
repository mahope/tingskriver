# Android Whisper baseline: protocol and current report (#133)

## Status: NOT MEASURED

2026-11-03: `adb devices` returned an empty device list on agent-worker. No
physical phone, speech model, native benchmark driver or consented recording
was available. Milestone 2 is **open**. Do not enable native IME integration or
cloud fallback on the strength of these changes. No WER, latency, PSS, energy
or thermal result has been observed. Existing SPEC thresholds remain unchanged.

This change supplies a corpus-lock utility, USB driver launcher, offline scorer,
synthetic regression tests and a separate path-filtered CI job. **It is not an
end-to-end Whisper runner:** the persistent arm64 driver described below still
has to be implemented/built and audited before collection can run. No native
library or dependency was added to the IME, desktop or root Cargo workspace.

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

## Device method and driver contract (implementation outstanding)

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

Required driver at `/data/local/tmp/tingskriver-bench/driver` accepts
`--manifest PATH --output PATH`. It must load only local WAV/models, force Danish
`da`, transcription rather than translation, deterministic decoding, fixed
threads/seed, no prompts or contextual carry-over, and networking disabled.
Do not suppress silence results in the driver: score the actual returned text.

The driver writes one JSON document containing the corpus lock fields,
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
- Sample PSS with Android `dumpsys meminfo <pid>` at <=100 ms intervals across
  load and inference. Use TOTAL PSS, convert KiB to decimal MB (KiB * 1024/1e6),
  and report sampling interval/overhead. Sampled peak may miss short spikes.
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

Use an otherwise idle test phone, keep all private files outside the repo, and
stage only audited driver, frozen models, manifest and WAVs under the fixed adb
path. USB launcher (uses adb's default single device, rejects emulators):

```sh
python3 scripts/android-benchmark/device.py /private/bench/midrange.json
# Repeat with the recent phone attached, tier=recent in its local manifest.
```

The launcher runs the driver in the foreground, attaches allowlisted hardware/OS
properties and writes private results locally. It does not build/provide the
driver or auto-install/download anything. Failure is exit 2 with sanitized output.
Never upload these raw JSON files to CI: hypotheses may contain personal data.

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

Tests were written first; initial discovery failed because `benchmark` did not
exist. The implementation then passed all 9 synthetic contract tests, including
bad WER/latency/PSS/silence rejection, incomplete rows, non-finite values, invalid
hashes, emulators, metadata collection, CLI exit codes and actual generated WAV hashing. These
are tests of tools, not measured Danish Whisper accuracy. CI compiles the Python
tools and runs the same tests without audio/models/dependencies/network access.

Android build/test/lint attempt:
`./gradlew --no-daemon :app:assembleDebug :app:testDebugUnitTest :app:lintDebug`
failed before configuration: Gradle requires JVM >=17, current JVM is 11.
No APK/JVM/lint success is claimed from that attempt.
Python compilation and `git diff --check` passed. Desktop Cargo/frontend build,
tests and lint were not run: no desktop code changed and frontend dependencies
are absent. The full relevant benchmark suite was run once after the final
code change; the combined Android build/JVM/lint gate is environment-blocked.

Suggested PR title: `Add Android Whisper benchmark tools and device protocol`.
Suggested description: Adds consented-corpus locking, offline WER/CER and resource
scoring, a USB driver launcher and dependency-free contract CI. Nine synthetic
tests pass; Android validation is blocked by Java 11. No physical measurements
or persistent native driver are supplied, so #133 and milestone 2 remain open.
No PR was created by this worker.

Outstanding: consented recordings and final hashes, audited engine/model pinning,
native persistent driver, power instrument, both physical phones, all actual
measurements and sustained thermal run, aggregate publication and model/SPEC
decision. Issue #133 must remain open until those are complete.
