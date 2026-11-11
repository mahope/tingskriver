#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
examples=crates/tingskriver-core/examples
mkdir -p "$examples"
log=$(mktemp)
probes=()
trap 'rm -f "${probes[@]}" "$log"' EXIT

# Every probe is its own example target. Cargo decides freshness by mtime and
# treats a source as unchanged when its mtime equals the previous check's start
# time, which happens on filesystems with coarse timestamps. Rewriting a single
# probe file could therefore reuse the previous probe's result instead of
# linting the new code.
write_probe() {
    local name=$1 path="$examples/$1.rs"
    if [[ -e "$path" ]]; then
        echo "Refusing to overwrite $path" >&2
        exit 1
    fi
    probes+=("$path")
    sed -n '/^#!\[/p' crates/tingskriver-core/src/lib.rs > "$path"
    cat >> "$path"
}

clippy_probe() {
    cargo clippy --locked -p tingskriver-core --features reqwest/blocking --example "$1" -- -A warnings -A clippy::all > "$log" 2>&1
}

write_probe async_io_guard_probe <<'RS'
async fn permitted() {
    let _ = tokio::fs::read("unused").await;
    let _ = reqwest::Client::new().get("http://127.0.0.1:1").send().await;
}
fn main() { let _ = permitted; }
RS
if ! clippy_probe async_io_guard_probe; then
    cat "$log"
    exit 1
fi
echo "Guard permits async HTTP and file I/O"
index=0
for expression in 'reqwest::blocking::get("http://127.0.0.1:1")' 'reqwest::blocking::Client::new()' 'std::fs::read("unused")' 'std::fs::File::open("unused")' 'std::fs::OpenOptions::new().read(true).open("unused")' 'std::fs::create_dir("unused")' 'std::fs::remove_dir("unused")' 'std::fs::remove_dir_all("unused")' 'std::fs::canonicalize("unused")'; do
    index=$((index + 1))
    name=async_io_guard_probe_$index
    write_probe "$name" <<< "$(printf 'async fn blocked() { let _ = %s; }\nfn main() { let _ = blocked; }' "$expression")"
    # Legacy core Clippy errors must not masquerade as a successful I/O guard.
    if clippy_probe "$name"; then
        echo "Guard failed to reject: $expression" >&2
        exit 1
    fi
    if ! grep -Eq 'use of a disallowed (method|type)' "$log" ||
       ! grep -q "examples/$name.rs:" "$log"; then
        cat "$log"
        exit 1
    fi
    echo "Guard rejected: $expression"
done
