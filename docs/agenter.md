# Agents: building, checking and reading CI

Agents work on Linux and cannot build or run the Windows app. These commands are the loop instead. Details are in [BUILDING.md](BUILDING.md#workspace-tests-and-ci-for-agents), the crate boundaries in [arkitektur.md](arkitektur.md).

```bash
# Platform independent tests
cargo test -p tingskriver-core

# Type-check the Windows code (#[cfg(windows)]) from Linux
cargo xwin check --target x86_64-pc-windows-msvc -p tingskriver-core -p tingskriver-overlay -p tingskriver-app

# Dependency licences (deny.toml)
cargo deny --locked check licenses bans

# Push, then wait for the GitHub Actions result of the newest commit
git push && scripts/ci-status.sh "$(git branch --show-current)"
```

CI (`.github/workflows/rust.yml`) runs the same checks on every push: the Windows workspace build and tests on `windows-latest`, the core tests and the "core has no UI crates" check on Linux, `cargo xwin check` and `cargo deny`. The branch build (`.github/workflows/branch-build.yml`) builds the Windows installer and uploads it as a workflow artifact.

`scripts/ci-status.sh` reads results through the public GitHub API without a token. On failure it prints the failed jobs and steps and the compiler errors from the job annotations, so the job logs are not needed.
