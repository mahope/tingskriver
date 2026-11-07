#!/usr/bin/env bash
# Clippy with -D warnings on the crates listed in .github/clippy-crates.txt.
# --no-deps lints only the listed crates, not the workspace crates they depend
# on (tingskriver-core is not clean yet; see docs/agenter.md).
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"
mapfile -t crates < <(grep -vE '^\s*(#|$)' .github/clippy-crates.txt)
[ "${#crates[@]}" -gt 0 ] || { echo "::error::.github/clippy-crates.txt is empty"; exit 1; }

args=()
for c in "${crates[@]}"; do args+=(-p "$c"); done
echo "clippy -D warnings on: ${crates[*]}"
cargo clippy --locked --no-deps --all-targets "${args[@]}" -- -D warnings
