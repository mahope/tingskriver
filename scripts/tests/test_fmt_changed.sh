#!/usr/bin/env bash
# Tests .github/scripts/check-fmt-changed.sh in a throwaway git repository.
#   bash scripts/tests/test_fmt_changed.sh
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
script="$here/.github/scripts/check-fmt-changed.sh"
tmp="$(mktemp -d)"
out_dir="$(mktemp -d)"
trap 'rm -rf "$tmp" "$out_dir"' EXIT
cd "$tmp"
git init -q -b main .
git config user.email t@example.invalid
git config user.name t
git config commit.gpgsign false
mkdir -p src/sub

# Baseline: an unformatted root that declares an unformatted child.
printf 'mod sub;\nfn   root( ){ }\n' > src/lib.rs
printf 'pub fn   child( ){ }\n' > src/sub/mod.rs
git add . && git commit -qm base
git checkout -q -b topic

fail=0
expect() { # expect <0|1> <name>
  local want="$1" name="$2" got=0
  bash "$script" main >"$out_dir/out.txt" 2>&1 || got=1
  if [ "$got" != "$want" ]; then
    echo "FAIL: $name (expected exit $want, got $got)"; cat "$out_dir/out.txt"; fail=1
  else
    echo "ok: $name"
  fi
}

expect 0 "no changes"

printf 'fn new_file( ){ }\n' > src/new.rs
git add . && git commit -qm unformatted
expect 1 "a new unformatted file fails"

printf 'fn new_file() {}\n' > src/new.rs
git add . && git commit -qm formatted
expect 0 "untouched unformatted files do not fail"

printf 'mod sub;\nfn root() {}\n' > src/lib.rs
git add . && git commit -qm "format root"
expect 0 "a formatted root with an unformatted untouched child passes"

printf 'fn   broken( ){ }\n' > src/new.rs
git add . && git commit -qm "break it again"
status_before="$(git status --porcelain; git rev-parse HEAD; sha256sum src/*.rs src/sub/*.rs)"
expect 1 "an unformatted edit in an already formatted file fails"
status_after="$(git status --porcelain; git rev-parse HEAD; sha256sum src/*.rs src/sub/*.rs)"
[ "$status_before" = "$status_after" ] || { echo "FAIL: the gate changed the working tree"; fail=1; }

exit "$fail"
