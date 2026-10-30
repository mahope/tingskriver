#!/usr/bin/env bash
# Run a command and, if it fails, turn its error blocks into GitHub
# annotations. Job logs need admin rights to download, but annotations are
# public, so this is what scripts/ci-status.sh can show agents.
#
#   .github/scripts/annotate-errors.sh cargo test --workspace --locked
#
# A block starts at a rustc/cargo `error` line, a test panic or a failed
# test's captured output, and runs until the next blank line (max 40 lines).
# At most 9 blocks are annotated (GitHub keeps 10 error annotations per step).

set -uo pipefail

log="$(mktemp)"
"$@" 2>&1 | tee "$log"
status="${PIPESTATUS[0]}"

if [ "$status" -ne 0 ]; then
  python="$(command -v python3 || command -v python)"
  "$python" - "$log" <<'PY'
import re
import sys

start = re.compile(
    r"^(error(\[\w+\])?:|error: |thread '.*' panicked|---- .* stdout ----|failures:$)"
)
blocks, current = [], None
for raw in open(sys.argv[1], encoding="utf-8", errors="replace"):
    line = raw.rstrip("\r\n")
    if start.match(line):
        if current:
            blocks.append(current)
        current = [line]
    elif current is not None:
        if line.strip() == "" or len(current) >= 40:
            blocks.append(current)
            current = None
        else:
            current.append(line)
if current:
    blocks.append(current)

# Drop the summary line cargo prints after the real errors.
blocks = [b for b in blocks if not b[0].startswith("error: could not compile")] or blocks


def escape(text):
    return text.replace("%", "%25").replace("\r", "%0D").replace("\n", "%0A")


for block in blocks[:9]:
    title = escape(block[0][:200]).replace(",", "%2C").replace(":", "%3A")
    print(f"::error title={title}::{escape(chr(10).join(block))}")
if not blocks:
    print("::error::Command failed without a recognisable error line; see the job log")
PY
fi

exit "$status"
