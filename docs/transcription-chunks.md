# Long recording transcription

Import and retranscription share the silence splitter and chunk checkpoints.
VAD runs on saved audio, with the fallback from [batch VAD](vad.md). Recording
and dictation are unchanged.

## Budgets and joins

Whisper uses a conservative 25-second budget; Parakeet uses 60 seconds. These
are local inference profiles, not the maximum durations advertised for a model
on a particular GPU. Both budgets include one second of shared audio at each
split. The splitter searches for a quiet 100 ms window in the last three seconds
of the budget, falling back to the hard limit for continuous speech. No chunk
exceeds the selected budget. Separate VAD intervals retain their original gaps.

Only suffix/prefix words from overlapping chunks are compared, ignoring case
and surrounding punctuation. Matching boundary words are retained once, with
the original punctuation of the earlier chunk. Repeated words in nonoverlapping
chunks are retained. The next displayed segment starts at or after the previous
segment's end, on the original recording clock, in seconds for the player.

The current Whisper/Parakeet engine APIs used by these workflows return text,
not word timestamps. Text matching is consequently a heuristic: different
recognition at the same join, or an ambiguous repeated phrase, cannot be resolved
reliably without timed words. Segment times are audio bounds, not inferred word
times. This change does **not** establish the word-timing acceptance criterion
of #48, and should not auto-close that issue.

## Retry and interrupted runs

Each successful provider response (including empty text) is committed to a
separate JSON checkpoint through a synced temporary file and atomic rename.
The checkpoint directory is synced on Unix. There is no checkpoint for a failed
or interrupted request. Repeating the operation reuses completed chunks and
calls the provider for unfinished chunks, then rebuilds the full transcript.
An interrupted write before rename cannot appear as a completed chunk.

Checkpoint identity includes the full source file contents, actual loaded
model, provider, language, application version and chunk schema. Each record
also checks its original time bounds. Changed inputs/settings cannot reuse the
old results. Source fingerprinting runs on a blocking worker with a 64 KiB
buffer. This is an internal cache key, not a security or integrity hash.

Retranscription checkpoints live under the meeting's `.transcription-chunks/`;
import checkpoints live under that directory in the recordings root, so retrying
an import can reuse work despite receiving a new meeting folder. A retry still
decodes and runs VAD, and import still copies the source into its new folder.
An interrupted import may leave its previous staging folder on disk; this change
does not delete those audio files. Checkpoints contain local transcript text,
not audio, and are removed after a successful operation. Failure and cancellation
retain them. Corrupt checkpoints fail explicitly; remove the affected numbered
JSON record to retry just that chunk. Existing database transcripts are only
replaced by the final transaction once every chunk has succeeded.

The batch paths still decode the source into memory. They consume VAD segments
when building chunks rather than cloning all unsplit audio. Each processed
chunk is moved into the provider and released instead of retaining another
copy until the batch ends. This is not a streaming decoder or a constant-memory
transcription pipeline.

## Validation

`cargo test --locked -p tingskriver-core --lib chunk_checkpoint` covers exact
joins across a synthetic two-hour known-word sequence, punctuation, repeated
phrases across pauses, changed inputs/settings, empty results, corrupt records,
and reuse after a subprocess exits without running destructors. The splitter
test in `audio::import` requires actual overlap and hard chunk limits.

The known-word sequence is a fake-provider output fixture, not two hours of
recognized speech. Real speech, real provider word timestamps, playback checks
on Windows and the licensed corpus expected from #30 remain necessary before
claiming that no recognized words are lost or duplicated at real joins.
