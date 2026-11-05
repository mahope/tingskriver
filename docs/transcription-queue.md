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

## Speaker names and truncated reruns

When a job completes and the meeting already has a *current* version of the
same chunk (same `offset_ms`), the new result is reconciled with it
(`transcript_merge::reconcile`, a reimplementation of the idea in anarlog's
`batch_refine`, MIT, Fastrepl, Inc.):

- New system speaker numbers ("Taler N") are mapped to the old numbers by
  summed time overlap. A new speaker keeps an old number only from 0.6 of its
  own time; the mapping is one-to-one and a collision goes to the best match.
  Everyone else gets the lowest number no old speaker used, so a number never
  silently changes owner and a speaker below 0.6 stays unnamed.
- Names confirmed with `set_speaker_name(job_id, number, name)` and corrections
  stored with `add_speaker_correction` are copied to the new version in the
  same transaction as the result. A correction is moved to a new segment when
  it covers at least 0.6 of it, and overrides the provider's clustering.
- If the new text loses at least 200 letters/digits and keeps under 50 %, the
  result is stored as a version with `rejected` (`reason: "truncated"` and a
  Danish `message`) and is never current. `current_version(meeting, offset)`
  returns the newest version that is not rejected; `Version::is_rejected()`
  tells the UI to show the message. The comparison is Unicode-aware (æøå count).
  The `completed` queue event (and `statuses()`) carries the same Danish text
  in `rejected`, so the host can show it without reading the stored result.
- Only system segments labelled "Taler N" are renumbered; an undiarised system
  side ("Ukendt taler") is left as is, and corrections do not apply to it.
  Names and corrections are copied as they are, also for numbers the new
  version does not use, so a later rerun can map back to them.

## Scope and verification

This change supplies the core API and tests only. App startup/shutdown wiring,
recording-stop enqueueing, the local model loader, event forwarding and the
“Kør igen” controls/version selector still need host/UI integration. Existing
recording/retranscription paths are unchanged. This is not yet an automatic
end-to-end queue in the desktop app.

Run `cargo test --locked -p tingskriver-core -- transcription_queue transcript_merge`. Tests cover
HTTP 500/429/hanging responses, bounded retries, speaker merging, explicit
Danish local requests, preserved versions and forced termination of a separate
process after its SQLite claim is committed. Local inference itself needs an
installed Parakeet/Whisper model; the automated tests use a local provider
double. Mads must still check an actual Danish meeting with two other speakers
on Windows and Syv.ai.
