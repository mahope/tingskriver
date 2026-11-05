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
sed -n '/^#!\[/p' crates/tingskriver-core/src/lib.rs > "$probe"
cat >> "$probe" <<'RS'
async fn permitted() {
    let _ = tokio::fs::read("unused").await;
    let _ = reqwest::Client::new().get("http://127.0.0.1:1").send().await;
}
fn main() { let _ = permitted; }
RS
if ! cargo clippy --locked -p tingskriver-core --features reqwest/blocking --example async_io_guard_probe -- -A warnings -A clippy::all > "$log" 2>&1; then
    cat "$log"
    exit 1
fi
echo "Guard permits async HTTP and file I/O"
for expression in 'reqwest::blocking::get("http://127.0.0.1:1")' 'reqwest::blocking::Client::new()' 'std::fs::read("unused")' 'std::fs::File::open("unused")'; do
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
