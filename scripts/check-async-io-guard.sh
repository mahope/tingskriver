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
    printf '#![deny(clippy::disallowed_methods)]\nfn main() { let _ = %s; }\n' "$expression" > "$probe"
    # Legacy core Clippy errors must not masquerade as a successful I/O guard.
    if cargo clippy --locked -p tingskriver-core --features reqwest/blocking --example async_io_guard_probe -- -A warnings -A clippy::all -D clippy::disallowed_methods -D clippy::disallowed_types > "$log" 2>&1; then
        echo "Guard failed to reject: $expression" >&2
        exit 1
    fi
    if ! grep -q 'use of a disallowed method' "$log"; then
        cat "$log"
        exit 1
    fi
    echo "Guard rejected: $expression"
done
