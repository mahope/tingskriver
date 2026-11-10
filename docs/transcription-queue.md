# Persistent meeting transcription queue

`tingskriver_core::transcription_queue` is a UI-independent queue for saved WAV
tracks or chunks. It does not run during recording. SQLite uses WAL and FULL
synchronous commits; each job stores its meeting, track paths, language,
microphone label, offset, attempts, due time, state and eventual result.
Audio and API keys are never stored in the queue database. Transcript versions
are private meeting data and belong in the application's local data directory.

## Host integration contract

1. Open one `Queue` in the application data directory. Only one application
   process may own that database. Before starting workers, call
   `recover_interrupted()`: running jobs become pending, or failed when their
   attempt budget is exhausted. Pending backoff survives unchanged.
2. After recording/finalization (or pause-based chunking), enqueue `JobSpec` with
   saved WAV paths. Set `microphone_label` from settings. The defaults are `Mig`
   and language `da`. `offset_ms` puts a chunk back on the meeting timeline.
   Set `hotwords = Hotwords::from_sources(matched_event, &dictionary)` using
   only the matched calendar event and the dictation store's dictionary.
   Names of non-declined participants come first, then dictionary targets,
   then their old spellings (aliases). Email addresses are never hints.
   Missing snapshots in older jobs default to empty. Hints are private data
   stored with the job; the host must apply the same consent policy as audio.
   The worker sends hints only through `TranscriptionRequest.prompt` to the
   attempted chain links, never to unused reserves or a separate service.
   Whole hints are selected within a conservative 224 UTF-8 byte budget
   (including separators), preserving case and Danish letters. This also
   bounds byte-level tokens for Whisper's 224-token prompt window; larger
   provider windows are deliberately not filled. Custom endpoints with a
   smaller limit are not yet configurable. The built-in local adapter does
   not consume prompt hints because its inference trait has no prompt field.
3. Poll `run_once` from a host-owned task. It returns false if nothing is due;
   the host can wait before polling again. Shut down/cancel workers before
   recovery. A cancelled in-flight call remains running until recovery.
4. Supply the shared `ProviderService`, current `ProvidersConfig`, and a local
   callback that loads the resolved link's Parakeet/Whisper model. The
   `transcribe_local` adapter accepts an already loaded `TranscriptionProvider`,
   decodes saved WAV bytes to 16 kHz and passes the requested language through.
   It transcribes each VAD speech range separately and keeps its original
   timestamps, including leading silence and pauses. Custom local callbacks
   must likewise return timestamped segments rather than text alone.
   No-key links are skipped by the shared chain; the default reserve is local.
5. Forward `QueueEvent` to the host event bus (suggested name
   `transcription-queue-status`). Events contain job/meeting IDs, status,
   attempts, due time and current track, never transcript text or errors from
   the server. `statuses()` restores the status view after restarting. Render
   `failed` as “Fejlede, prøv igen”; use inline progress, never a modal.

Each atomic claim increments the attempt count. A failed chain schedules the
whole job again, up to five attempts, with 5/10/20/40 second exponential backoff
plus 0–25% jitter. A provider's `Retry-After` is a lower bound, including when
other links fail afterwards. Scheduling starts after the call has finished.
The common provider client handles HTTP timeouts and `Retry-After` parsing.
Provider cooldowns are stored in SQLite even when a reserve succeeds. Later
tracks and jobs skip those providers until their deadline, also after restart.
Results are committed before the completed event. A crash in that interval
leaves a completed version, which the host can retrieve on startup.

Microphone requests never ask for diarization; system requests do. The shared
client only sends `diarize` to providers that support it. Both use the explicit
language and `/audio/transcriptions`, never the translation endpoint. Requests
use `verbose_json` independently of diarization; nonempty text without valid
segment timestamps fails that link instead of inventing timing. Results are
merged by `transcript_merge`; missing system speakers remain unknown.
Stored results include raw per-track deliveries/provenance and merged segments.

## Run again and versions

Enqueue a new `JobSpec` for the same meeting, optionally with another language
and a validated provider configuration snapshot in `providers`. Credentials are
resolved when the job runs. `versions(meeting_id)` returns successful versions
in creation order; failed reruns never replace a previous result. Multiple
chunks have separate jobs/versions: aggregation and selection of a complete
meeting version remain the host's responsibility. A retry may re-submit a
previously successful track if the other track failed.

## Scope and verification

This change supplies the core API and tests only. App startup/shutdown wiring,
recording-stop enqueueing, the local model loader, event forwarding and the
“Kør igen” controls/version selector still need host/UI integration. Existing
recording/retranscription paths are unchanged. This is not yet an automatic
end-to-end queue in the desktop app.
The host must also supply the matched calendar event and dictionary snapshot;
core does not fetch calendar feeds or open the dictionary database itself.

Run `cargo test --locked -p tingskriver-core transcription_queue`. Tests cover
HTTP 500/429/hanging responses, bounded retries, speaker merging, explicit
Danish local requests, preserved versions and forced termination of a separate
process after its SQLite claim is committed. Local inference itself needs an
installed Parakeet/Whisper model; the automated tests use a local provider
double. Mads must still check an actual Danish meeting with two other speakers
on Windows and Syv.ai.

`cargo test --locked -p tingskriver-core hotwords` covers name/target/alias
priority, exact-byte boundaries, Danish spelling, deduplication, legacy jobs,
and the multipart prompt body with an unused reserve receiving no request.
