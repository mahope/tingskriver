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

Whisper token alignments and Parakeet token emission times pass through the
isolated speech worker and are grouped into words, preserving subwords,
punctuation and UTF-8 characters split across tokens. Times are relative to the
chunk in checkpoints and offset to the original audio clock during assembly.
Parakeet token ends use the next emission (the last token uses one 80 ms encoder
step), bounded by chunk duration; they are alignment estimates, not measured
phoneme ends. Alignments beyond padded model input are clipped to the source
duration. Invalid or out-of-range checkpoint times fail explicitly.

Only suffix/prefix words whose time intervals intersect are compared, ignoring
case and surrounding punctuation. A repeated word before or after the shared
audio, or at a distinct time within it, is retained. Matching words retain the
earlier punctuation and alignment. Any remaining intersecting intervals are
made monotone using the preceding word's end, rather than the preceding chunk's
end. Different recognition or alignment at a join remains ambiguous.

Import and retranscription persist one existing transcript segment per timed
word, so both database and transcripts.json retain word positions without a
new database schema. The player receives these intervals in seconds through
its existing audio_start_time/audio_end_time fields. This increases the number
of transcript rows compared with text-only chunks. Dictation remains unchanged.

## Retry and interrupted runs

Each successful provider response (including empty text) is committed to a
separate JSON checkpoint through a synced temporary file and atomic rename.
The checkpoint directory is synced on Unix. There is no checkpoint for a failed
or interrupted request. Repeating the operation reuses completed chunks and
calls the provider for unfinished chunks, then rebuilds the full transcript.
An interrupted write before rename cannot appear as a completed chunk.

Checkpoint identity includes the full source file contents, actual loaded
model, provider, language, application version and chunk schema. Each record
also checks its original time bounds and validates its word alignments. The timed
schema uses a new cache identity; earlier text-only checkpoints are not reused. Changed inputs/settings cannot reuse the
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
joins and every word position across a synthetic two-hour known-word sequence,
punctuation, repetitions outside and inside shared audio, checkpointed word
alignments, conversion to player seconds, changed inputs/settings, empty results,
corrupt records and reuse after a subprocess exits without running destructors.
`cargo test --locked -p tingskriver-core --lib stt::words` covers subword grouping
and UTF-8 characters split across model tokens. The splitter
test in `audio::import` requires actual overlap and hard chunk limits.

The known-word sequence is a fake-provider output fixture, not two hours of
recognized speech. Real speech with provider alignments, interactive playback checks
on Windows and the licensed corpus expected from #30 remain necessary before
claiming that no recognized words are lost or duplicated at real joins.
