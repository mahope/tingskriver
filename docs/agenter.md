# Agents: building, checking and reading CI

Agents work on Linux and cannot build or run the Windows app. These commands are the loop instead. Details are in [BUILDING.md](BUILDING.md#workspace-tests-and-ci-for-agents), the crate boundaries in [arkitektur.md](arkitektur.md).

```bash
# Platform independent tests
cargo test -p tingskriver-core

# Type-check the Windows code (#[cfg(windows)]) from Linux
cargo xwin check --target x86_64-pc-windows-msvc -p tingskriver-core -p tingskriver-overlay -p tingskriver-host -p tingskriver-app

# Dependency licences (deny.toml)
cargo deny --locked check licenses bans

# Push, then wait for the GitHub Actions result of the newest commit
git push && scripts/ci-status.sh "$(git branch --show-current)"
```

CI (`.github/workflows/rust.yml`) runs the same checks on every push: the Windows workspace build and tests on `windows-latest`, the core tests and the "core has no UI crates" check on Linux, `cargo xwin check`, `cargo deny` and the lint gates below. The branch build (`.github/workflows/branch-build.yml`) builds the Windows installer and uploads it as a workflow artifact.

Two lint gates run on every push (details in [ci.md](ci.md)). Because the code inherited from the fork is not clean (about 1,350 rustfmt hunks in 147 files and clippy errors in `tingskriver-core`, 30/10-26) and one big reformat would conflict with every open branch, they only look at what you change:

- **rustfmt**: every `.rs` file your branch changes against `origin/main` must be rustfmt-clean. Check with `.github/scripts/check-fmt-changed.sh origin/main` or `rustfmt --edition 2021 <file>` on the files you touch. Untouched files may stay unformatted. Format only files you touch: never the whole tree.
- **clippy**: `cargo clippy --all-targets -- -D warnings` must pass for every crate in `.github/clippy-crates.txt` (`.github/scripts/clippy-ratchet.sh` runs the list). Today that is `tingskriver-overlay` and `tingskriver-host`. When a crate or new module is clean, add its line; never remove a crate to get green. Keep new code in other crates clippy-clean as well.

Pushes that only change `site/`, `docs/` and similar skip the Rust and installer builds on branches; `main` always runs everything.

`scripts/ci-status.sh` reads results through the public GitHub API without a token. On failure it prints the failed jobs and steps and the compiler errors from the job annotations, so the job logs are not needed.
