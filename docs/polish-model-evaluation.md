# Dictation polish model evaluation

The unit and fake-HTTP tests verify boundaries, response handling and fallback.
They do **not** establish that a model follows the Danish polish prompt.
The numeric guard compares complete tokens, including decimal and grouping
separators, and requires every input occurrence to remain in the output, except
for the superseded number in an adjacent numeric self-correction such as
"13 nej 14" (also with commas around "nej"). Correction chains may drop each
superseded occurrence; the final number, unrelated repetitions and dictionary
names remain protected. Unit tests run spoken "tretten nej fjorten" through
layer 1 before checking this exception. Other correction forms keep the
conservative fallback. Swapped decimal parts or a missing unrelated repeated
number therefore use layer 1.

Run the separate, opt-in model acceptance test against an already installed
Ollama model (no model is downloaded by this test):

```bash
export CARGO_TARGET_DIR="$HOME/dev/cargo-target/tingskriver"
export POLISH_EVAL_MODEL='<installed model tag>'
export POLISH_EVAL_ENDPOINT='http://localhost:11434'
cargo test --locked -p tingskriver-core --test polish_model_eval -- --ignored --nocapture
```

Record the model tag and immutable model digest from the server inventory,
server version, hardware and complete per-case output with any evaluation.
Warm the model first if measuring warm inference; report cold runs separately.
Sampling remains the production path's configuration, so repeated runs can vary.

The test sends the production prompt and escaped transcript through the real
HTTP client. It checks the two instruction/question inputs and ten Danish
self-corrections against explicit expected text. Exact comparison deliberately
also catches punctuation changes, which should be inspected when a run fails.
Each request has the production two-second limit. Provider errors, truncation,
empty output and timeout fail the evaluation; layer-1 fallback cannot mask them.
All cases run before the final assertion, with actual output and elapsed time
printed for every synthetic input. Ordinary CI skips this external-service test.

## Result on 2026-11-10

Model acceptance is **not verified** in the review-fix environment: there is no
Ollama executable, installed GGUF model or listening Ollama service on localhost.
No model response or passing quality result is claimed. The P2 finding remains
partially open until the command above has passed on an identified target model.
Use `Refs #77`, rather than `Fixes #77`, while that acceptance evidence is missing.
