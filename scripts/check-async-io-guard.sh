#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
probe=crates/tingskriver-core/examples/async_io_guard_probe.rs
if [[ -e "$probe" ]]; then
    echo "Refusing to overwrite $probe" >&2
    exit 1
fi
mkdir -p crates/tingskriver-core/examples
log=$(mktemp)
trap 'rm -f "$probe" "$log"' EXIT
for expression in 'reqwest::blocking::get("http://127.0.0.1:1")' 'std::fs::read("unused")'; do
    sed -n '/^#!\[/p' crates/tingskriver-core/src/lib.rs > "$probe"
    printf 'async fn blocked() { let _ = %s; }\nfn main() { let _ = blocked; }\n' "$expression" >> "$probe"
    # Legacy core Clippy errors must not masquerade as a successful I/O guard.
    if cargo clippy --locked -p tingskriver-core --features reqwest/blocking --example async_io_guard_probe -- -A warnings -A clippy::all > "$log" 2>&1; then
        echo "Guard failed to reject: $expression" >&2
        exit 1
    fi
    if ! grep -Eq 'use of a disallowed (method|type)' "$log" ||
       ! grep -q 'examples/async_io_guard_probe.rs:' "$log"; then
        cat "$log"
        exit 1
    fi
    echo "Guard rejected: $expression"
done
