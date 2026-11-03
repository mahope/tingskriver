# Issue #71: isolate native speech inference

Implemented a persistent `tingskriver-stt` worker for Parakeet, Whisper and
Silero VAD. Model management, recording, shortcuts, tray and UI stay in the app.
Private pipes carry bounded frames, request IDs and raw little-endian f32 PCM.
A broken pipe fails the current call in Danish, drops/reaps the worker, and
reloads it on the next call. Windows workers run without a console and belong
to a kill-on-close Job Object.

Parakeet retains the existing two-instance model pool. Whisper keeps one warm
worker and creates a second only for overlapping inference. VAD startup errors,
including a native abort while loading ONNX Runtime, are isolated from the app;
automatic VAD selection uses the existing energy detector. Syv.ai's existing
fallback selection remains in core. Windows Tauri and Velopack packaging stage
the worker; Velopack includes the same pinned, checksum-verified ONNX Runtime
as the existing Tauri bundler.

No Cargo manifest or lockfile changes and no new dependencies. All Cargo TOML
files were parsed to check duplicate keys. Changed workflows parse as YAML.

## Validation

- `cargo build --workspace --locked`: passed (exit 0). Linux linking used a
  local linker name for the already installed `libxkbcommon-x11.so.0`; no
  system packages were installed. The existing FFmpeg binary was used in the
  ignored local bundler cache.
- `cargo test --workspace --locked`: attempted; blocked during compilation
  by `rustc-LLVM ERROR: IO failure on output stream: No space left on device`.
  The complete workspace suite did not run. Remaining compilation jobs were
  stopped after that failure. Re-run on a runner with enough disk space.
- STT transport/lifecycle tests: 6 passed (standalone optimized compilation
  of the production codec and client). Includes kill/restart, request-ID
  mismatch, 15-second PCM integrity, bounded frames and IEEE bit patterns.
- Real-worker integration tests: 3 passed; the ignored ORT-failure child test
  was invoked separately by its parent and passed. The integration executable
  was newer than the unchanged test source. It exercises worker exit/restart,
  real framing and forced ORT startup failure with energy fallback.
- `cargo clippy --workspace --locked` failed on the pre-existing
  `clippy::inherent_to_string_shadow_display` error at
  `audio/capture/backend_config.rs:55`. The file is identical to main.
  A final core-only Clippy run confirms the same error (exit 101), 127 existing
  warnings and no warnings in the new STT modules. The new VAD initializer
  warning from the first run was fixed.
- New Rust files pass targeted `rustfmt --check`; `git diff --check` passes.

Optimized client against the actual updated debug worker, 20 warmed roundtrips
of 240,000 f32 samples (15 seconds): **p50 0.36 ms, p95 5.43 ms, max 66.47 ms**.
This was measured on Linux during concurrent server builds. The p95 is below
50 ms, but the maximum was not; this is not proof that every request meets
50 ms under load. Before the zero-copy PCM change, the same test exceeded
50 ms at p95. The benchmark uses an echo operation, excluding model inference
and cold model loading. Reproduce with the commands in `BUILDING.md`.

The tests use synthetic audio only. Lifecycle tests kill an IPC fixture during
a call and verify the Danish error and a new process on the next call. An
integration test exercises the real worker exiting during request handling,
then verifies automatic restart. Another launches a separate app test process
with forced ORT startup failure, confirms the worker reports that failure,
and verifies energy VAD still returns speech. These tests do not exercise
real Parakeet/Whisper model inference or a Windows GUI session.

## Remaining verification

The full workspace tests need a runner with more disk space; baseline Clippy
is still red. Windows CI/installer and an actual native model crash while dictating still
need validation on Windows. The Linux transport benchmark is not a Windows
latency measurement. Review is left to the orchestrator as requested; no PR
was created.

## Suggested PR

Title: Isolate local speech inference in a restartable worker

Move Parakeet, Whisper and Silero inference out of the app process. Preserve
warm model instances, fail only the current call after a worker crash, restart
on the next request, and use energy VAD when ONNX startup fails. Package the
worker in both Windows installers and add lifecycle, framing and ORT-failure
tests plus a repeatable 15-second PCM transport benchmark.

Fixes #71.
