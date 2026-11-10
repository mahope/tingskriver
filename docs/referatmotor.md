# Structured meeting reports (core only)

`tingskriver_core::summary::structured` produces a typed `Report` without UI,
database writes or provider selection. It is deliberately not connected to
`processor.rs` or `service.rs`; that integration, persistence and UI remain for
the follow-up to issue #44, after the concurrent pipeline changes.

## Contract

Call `generate(model, budget, segments, own_notes, cancellation).await` with an
implementation of `map_reduce::Model`, a `map_reduce::Budget`, transcript
segments, the original note strings and a `CancellationToken`.

The result contains:

- `content.summary`: summary in the source language.
- `content.decisions`: decisions.
- `content.tasks`: `who`, `what`, `deadline`. Unknown owners and deadlines are
  explicit JSON nulls. Deadlines are source text, not inferred calendar dates.
- `content.open_questions`: unresolved questions.
- `origin: "ai"`: provenance for all generated content.
- `notes`: each original string, byte for byte, with `origin: "own"`.

Own notes bypass the model, including map/reduce, and are never rewritten,
trimmed, deduplicated or included in the model's context budget. They do not
influence AI content. Empty and repeated notes remain intact.

## Schema and budgets

`structured/schema.json` is the JSON Schema for model responses. `parse` checks
required fields, types (including nullable task fields), array items and unknown
properties before deserializing. The dependency-free validator implements only
the schema vocabulary used in this file; it is not a general JSON Schema engine.
Changes to the schema vocabulary must also extend validation and its tests.

Malformed JSON, schema violations and oversized final answers cause exactly one
retry of the same bounded prompt. An invalid answer is not appended to the retry
prompt. Model/transport errors return immediately. Validation errors do not
include source text or model output; adapters must also sanitize their errors.

Long transcripts use the existing hierarchical `map_reduce::prepare`, including
its overlap, output bounds and failure when reduction does not shrink. Every
final attempt reserves the schema, framing and answer budget. Token estimates
are the supplied `Budget` estimator, not an exact tokenizer guarantee. Intermediate
summaries use a source-language prompt for both map and reduce, so the final
step receives source-language facts even after hierarchical reduction. The
existing English report pipeline keeps its English prompt. The model adapter must
enforce the requested context/output limits at inference time too. `Builtin`
already implements `Model`; other providers can implement the same interface.

## Cancellation and restart

Cancellation drops the in-flight future and returns an error. The model adapter
must support cancellation by future drop, or receive the same cancellation token
for native work that outlives that future. The engine has no side effects and
publishes no partial results. Restart with a fresh token, then replace the previous
report as one value. Do not append each run to a stored report. Persistence-level
concurrent-run ordering and deduplication are the future service's responsibility.

## Verification

`cargo test --locked -p tingskriver-core summary::structured` exercises verbatim
notes and provenance, schema rejection, one JSON retry, a synthetic 90-minute
transcript (150 words/minute) with every model call checked against the budget,
source-language preservation through map and reduce for Danish and English,
and cancellation during generation followed by restart without duplicate items.
These are deterministic fake-model tests, not an evaluation of a real model's
factual accuracy or extraction quality.
