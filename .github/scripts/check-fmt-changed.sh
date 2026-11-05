#!/usr/bin/env bash
# rustfmt --check on the .rs files a branch changed against <base>, and
# nothing else. Files nobody touched may stay unformatted (docs/agenter.md).
#
#   .github/scripts/check-fmt-changed.sh <base-ref>     (e.g. origin/main)
#
# rustfmt follows `mod` declarations, so a changed mod.rs or lib.rs also
# checks its children. skip_children would stop that but is a nightly setting,
# so every "Diff in <file>" block is matched against the changed list and
# diffs in other files are ignored. --check never writes: the working tree
# stays untouched.

set -uo pipefail

base="${1:?usage: check-fmt-changed.sh <base-ref>}"
edition="${RUSTFMT_EDITION:-2021}"
root="$(git rev-parse --show-toplevel)"
cd "$root"

mapfile -t changed < <(git diff --name-only --diff-filter=ACMR --no-renames "$base...HEAD" -- '*.rs' | while read -r f; do [ -f "$f" ] && echo "$f"; done)

if [ "${#changed[@]}" -eq 0 ]; then
  echo "No changed .rs files against $base."
  exit 0
fi

printf 'Checking %d changed file(s):\n' "${#changed[@]}"
printf '  %s\n' "${changed[@]}"

out="$(rustfmt --edition "$edition" --check --color never "${changed[@]}" 2>&1)"
status=$?

# A rustfmt failure that is not a formatting diff (parse error) must fail too.
if [ "$status" -ne 0 ] && ! grep -q '^Diff in ' <<<"$out"; then
  printf '%s\n' "$out"
  echo "::error::rustfmt failed without reporting a diff"
  exit 1
fi

declare -A mine=()
for f in "${changed[@]}"; do mine["$root/$f"]=1; done

failed=()
while IFS= read -r file; do
  if [ -n "${mine[$file]:-}" ]; then
    failed+=("${file#"$root"/}")
  fi
done < <(grep '^Diff in ' <<<"$out" | sed -E 's/^Diff in (.*):[0-9]+:?$/\1/' | sort -u)

if [ "${#failed[@]}" -eq 0 ]; then
  echo "All changed files are rustfmt-clean."
  exit 0
fi

for f in "${failed[@]}"; do
  echo "::error file=$f::not formatted; run: rustfmt --edition $edition $f"
done
# Show the diffs for the changed files only.
awk -v files="$(printf '%s\n' "${failed[@]}")" '
  BEGIN { n = split(files, a, "\n"); for (i = 1; i <= n; i++) want[a[i]] = 1 }
  /^Diff in / { p = 0; for (f in want) if (index($0, f ":") > 0) p = 1 }
  p { print }
' <<<"$out"
exit 1
