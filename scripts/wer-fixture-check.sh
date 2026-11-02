#!/usr/bin/env bash
# No models, generated audio or downloads. Run from repository root.
set -euo pipefail
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/dev/cargo-target/tingskriver}"
out="${1:-/tmp/tingskriver-wer-fixture}"
mkdir -p "$out"
for variant in good bad; do
  cargo run --locked -p tingskriver-core --example wer_run -- \
    --manifest testsaet/dansk-wer/fixtures/manifest.json --engine fixture \
    --hypotheses "testsaet/dansk-wer/fixtures/$variant" --out "$out/$variant.json"
done
python3 scripts/wer-gate.py --rapport "$out/good.json" \
  --baseline testsaet/dansk-wer/fixtures/baseline.json --markdown "$out/good.md"
set +e
python3 scripts/wer-gate.py --rapport "$out/bad.json" \
  --baseline testsaet/dansk-wer/fixtures/baseline.json --markdown "$out/bad.md"
status=$?
set -e
if [ "$status" -ne 1 ]; then
  echo "Positive control failed: expected regression exit 1, received $status" >&2
  exit 1
fi
echo "Positive control passed: removed words trigger regression exit 1."
