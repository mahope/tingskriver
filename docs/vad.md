# Batch VAD before transcription

`tingskriver_core::vad_trim` removes silence **after recording**. The capture
pipeline is untouched. No dependency was added.

## API

```rust,ignore
use tingskriver_core::vad_trim::{trim, trim_file, TrimConfig};
let cfg = TrimConfig::default();
let audio = trim(&samples_16k_mono, &cfg)?;
let original_ms = audio.to_original_ms(1234.0);
let from_wav = trim_file(std::path::Path::new("speech-16k-mono.wav"), &cfg)?;
```

`trim` returns `Result<TrimmedAudio>`. Samples must be finite 16 kHz mono f32.
`TrimmedAudio.samples` concatenates padded speech intervals without overlaps.
`segments` contains `orig_start`, `orig_end` (exclusive), and `trimmed_start`,
all in **samples**, not milliseconds. Output audio remains 16 kHz mono.
Invalid thresholds and zero minimum durations fail before runtime startup.

`trim_file` uses the existing Symphonia decoder packet by packet and requires
**decoded** 16 kHz mono (WAV, FLAC, or another supported container). It rejects
other rates/channels rather than interpreting them on the wrong clock. Convert
other recordings first with the existing decoding/resampling facilities.
`trim_pcm_file` accepts headerless little-endian f32 PCM with the same format.
Both make two streaming passes: detect intervals, then collect selected audio.
Memory contains the speech output, interval index, and a decoder packet/frame,
not the entire original file. Thus an all-speech file still requires memory
proportional to its speech. Inputs must remain unchanged between passes.
Silero also retains the current utterance internally. The legacy meeting
retranscription path still decodes/resamples its source into memory first; it
does not yet call the streaming file API.

## Backend and fallback

`Auto` (default) and `Silero` first consult `ensure_onnx_runtime_available()`.
If unavailable, they select energy VAD. If the runtime probe succeeds but Silero
session construction returns an error, they also select energy VAD. `Energy`
never constructs an ONNX session. Selection and a fixed reason are logged,
without audio, transcript content, paths, or runtime error strings.

`select_backend(preference, probe)` and `trim_with_runtime_probe(samples, cfg,
probe)` make runtime absence testable without changing the environment or the
process-global Windows runtime state. A false probe is the missing-ORT seam.
A recoverable session-start error falls back; an operating-system abort or
illegal CPU instruction cannot be recovered within Rust. The application's
startup runtime guard must reject unsafe runtime loading first. Inference or
input errors propagate rather than silently treating an incomplete scan as
silence.

Silero reuses `audio::vad::ContinuousVadProcessor::with_config`; no new model,
integration, download, or runtime is introduced. Its thresholds and durations
are configurable without changing the old constructor's behavior.

Energy VAD uses 30 ms frames, a 200 Hz high-pass RMS, a slowly adapting noise
floor, and a first-difference energy ratio to reject narrow low-frequency hum.
The threshold raises both the absolute RMS gate and its required ratio above
the noise floor. Only nonspeech frames update that floor, so sustained speech
does not teach the detector to suppress itself. The same interval builder
bridges pauses, rejects short events, pads intervals, merges overlapping pads,
and retains speech at EOF. This is a conservative fallback, not a semantic
speech classifier: loud broadband noise/music may still pass, and very quiet
or narrow-band speech may be missed. Device-specific calibration is not proved
by the synthetic tests.

## Parameters

| Parameter | Default | Reason |
| --- | --- | --- |
| `threshold` | `0.5` | Silero's existing positive threshold; energy sensitivity midpoint (not a calibrated probability) |
| `min_speech_ms` | `150` | Reject brief clicks while retaining short utterances |
| `min_silence_ms` | `500` | Bridge short within-sentence pauses |
| `speech_pad_ms` | `200` | Retain consonant onsets/endings and frame-boundary context |
| `backend` | `Auto` | Prefer Silero; remain usable without ONNX Runtime |

The meeting compatibility adapter keeps the caller's existing 2000 ms silence
minimum. Silero's negative threshold is `max(threshold - 0.15, 0)`.
In the energy backend the RMS gate is `max(0.0015 + threshold * 0.003,
noise_floor * (2 + threshold * 4))`; the initial floor is `0.0003`.
First-difference energy must exceed 2% of raw energy to reject mains hum.
Synthetic noise speech bursts at amplitudes 0.012 through 0.4 are covered.

## Time mapping

For a trimmed position inside a segment:

`original_sample = orig_start + trimmed_sample - trimmed_start`

`to_original_ms` is monotonic and right-continuous: an exact join maps to the
start of the next original segment. `end_to_original_ms` maps that same join to
the end of the previous segment, for exclusive word/segment endpoints. Times
outside the trimmed duration clamp to its endpoints; an empty map returns zero.
Words spanning a join include the removed original pause in their duration.

`transcribe_trimmed(&dyn TranscriptionProvider, samples, cfg, language)` skips
providers on empty speech and otherwise makes **one** call on the concatenated
audio, preserving linguistic context and avoiding per-segment request overhead.
It returns the existing text-only `TranscriptResult` and the audio/time map.
The core provider trait has no timestamp fields, so no timestamps are invented.

`TimedTranscriptionProvider` is a separate optional adapter contract.
`transcribe_trimmed_timed` maps every returned word and segment back, using
start/end boundary rules, and rejects nonfinite/reversed timestamps. Existing
Whisper/Parakeet providers do not implement this optional contract yet.

## Meeting and dictation integration

`audio/transcription/worker.rs::transcribe_chunk_with_provider` trims after
resampling and before any of its three engine variants. Empty speech returns
empty text without a transcription call. Existing chunk-level start/end times
remain original; this worker does not expose word timestamps.

`audio/retranscription.rs` imports the new
`vad_trim::meeting::get_speech_chunks_with_progress` adapter instead of the
Silero-only helper. Its original per-segment provider calls, splitting and
original timestamps remain intact. Cancellation is checked once per input
second, plus at completion; progress is reported every 5% and at 100%. Segment
count is reported at completion (zero while scanning). This path retains its
existing "no speech" error/persistence behavior rather than replacing saved
transcripts with an empty result. Engine initialization is after VAD.

No `dictation/**` file is changed. The owning branch should trim the 16 kHz
samples at the beginning of `dictation/service.rs::transcribe` (currently around
line 551), before the local Parakeet branch, WAV encoding for Syv, and the local
fallback branch. A future TranscriptionProvider adapter can call
`transcribe_trimmed`; with today's direct local/HTTP APIs call `trim` once,
return empty on no speech, and share the trimmed samples across all routes.
`dictation/syv.rs` receives encoded WAV, so trimming belongs before its call,
not inside that HTTP function. Timed providers should use the timed wrapper.

## Validation and remaining evidence

Run `cargo test -p tingskriver-core vad_trim --lib`.
The tests cover 30 s silence/low noise with zero fake-provider calls; five
pause-containing synthetic clips with 100% known speech-sample coverage; word
and segment timestamp restoration within 100 ms; monotonic/boundary mapping;
missing-runtime selection; parameters; EOF/padding; streaming PCM and WAV;
wrong file rate; invalid samples; cancellation/progress; Silero silent startup.

There is **no real speech fixture in this repository**. Synthetic bursts prove
interval coverage, not word recognition or Silero recall. The Silero smoke test
may fall back if session startup fails, so it is not evidence of real-speech
Silero behavior. Issue #30 is expected to provide a licensed speech corpus.
Until those tests exist, issue #23 should be referenced, not auto-closed.

On Windows, after #30 provides licensed recordings:

1. Use at least five 16 kHz mono clips with known words, original word times,
   quiet/loud voices and several long pauses. Keep them outside Git until their
   redistribution licence is confirmed.
2. Run the `vad_trim` tests with the application's bundled ONNX Runtime DLL
   (`ORT_DYLIB_PATH`), then use `trim` with `BackendPreference::Silero` on those
   clips. Assert `result.backend == Backend::Silero`, not a fallback. Compare
   detected original intervals with all annotated words and listen at cuts.
3. Repeat with `trim_with_runtime_probe(..., || false)` and assert energy is
   selected. Compare retained words and mapped timestamps (maximum 100 ms error).
4. Transcribe the trimmed clips with a real provider, compare words against the
   untrimmed baseline, and confirm no lost words. Separately submit 30 s digital
   silence/low room noise and confirm no provider calls.
5. Record/stop/retranscribe a meeting; check playback alignment and cancellation.
   The recording indicator and saved tracks must behave as before.
