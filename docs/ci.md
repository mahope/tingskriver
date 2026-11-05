# CI: scope, lint gates and measurements

Issues #187 (lint only what changed) and #192 (measure, scope by path, test scheduled workflows).

## What runs when

| Workflow | Trigger | Jobs |
|---|---|---|
| `rust.yml` | every push | `scope` (1 min), `workflows` (checks, 1 min), `lint`, `windows`, `linux-core`, `windows-gpui-app`, `cargo-deny`, `xwin-check` |
| `branch-build.yml` | push to any branch except `main` | `scope`, Windows installer, device switching tests |
| `wer.yml`, `search-benchmark.yml`, `ydeevne-port.yml` | own path filters | unchanged, except `wer.yml` now cancels superseded runs on branches |

**Scope** (`.github/scripts/ci-scope.py`): the `scope` job decides whether the Rust and installer jobs run.

- `main` and tags always run everything. The automatic release needs a successful `Rust` push run for every commit on `main`, and `main` keeps the full check.
- Other branches are compared with the merge-base with `origin/main`. The builds are skipped only when *every* changed path is in the skip list: `site/`, `docs/` (except `docs/schemas/`, which core includes with `include_str!`), `apps/android/`, `README.md`, `CONTRIBUTING.md`, `CLAUDE.md`, issue/PR templates and workflows that share no file with the Rust build (`pages.yml`, `wer.yml`, `android*.yml`, ...). Anything else, including `Cargo.toml`, `Cargo.lock`, `deny.toml`, `.github/scripts/`, `rust.yml`, `branch-build.yml`, `windows-build.yml` and new top-level directories, runs the builds.
- A later docs-only push to a branch that changed Rust still builds, because the diff is against `main`, not against the previous push.
- A skipped job counts as passed for required checks, and `scripts/ci-status.sh` treats `skipped` as success.
- If the change set cannot be computed the builds run.

To add a path to the skip list, edit `SKIP_PREFIXES`, `SKIP_FILES` or `SKIP_WORKFLOWS` in `ci-scope.py` and the test in `scripts/tests/test_ci_scope.py`.

## Lint gates (#187)

- **rustfmt on changed files**: `.github/scripts/check-fmt-changed.sh <base>` runs `rustfmt --check` on the `.rs` files changed against the base and fails only on diffs in those files. `rustfmt` follows `mod` declarations, so a changed `mod.rs` also reports its children; those reports are filtered out by file name. (`skip_children=true` is accepted on the current stable toolchain, but it is a nightly setting, so the gate does not rely on it.) `--check` never writes, so the working tree is untouched. Untouched, unformatted files do not fail. Tested in `scripts/tests/test_fmt_changed.sh` (a new unformatted file fails, untouched files and untouched children do not, the working tree is unchanged).
- **clippy ratchet**: `.github/clippy-crates.txt` lists the crates that must pass `cargo clippy --all-targets -- -D warnings` (`--no-deps`, so the unclean `tingskriver-core` is not linted through its dependents). Add a crate with one line when it is clean; never remove one to get green. Today: `tingskriver-overlay`, `tingskriver-host` (Linux targets; `#[cfg(windows)]` code is covered by `cargo xwin check`, not by clippy).

Run both locally:

```bash
.github/scripts/check-fmt-changed.sh origin/main
.github/scripts/clippy-ratchet.sh
```

## Scheduled and reusable workflows (#192)

`scripts/check-workflows.py` runs in the `workflows` job on every push. It fails when a workflow lacks top-level `permissions`, when a `schedule` workflow has no `workflow_dispatch`, when a job that calls a local reusable workflow grants less than the callee asks for (workflow or job level), or when `needs:` names an unknown job. That is the failure that PR CI cannot see: a cron run that dies at start-up.

After any change to `permissions:` or a reusable workflow, also run the scheduled workflow by hand: `gh workflow run recording-soak.yml --ref <branch>` (the other `workflow_dispatch` workflows as relevant) and check that it starts. This PR changes no `permissions:` blocks.

## Measurements

`scripts/ci-minutes.sh <workflow.yml> [runs]` prints wall time and runner minutes (sum of job durations) for the newest successful runs and the medians. It needs one API call per run, so use a `GITHUB_TOKEN` instead of the shared unauthenticated quota.

Baseline, 5/11-26, the 10 newest successful push runs of each workflow (one API call each). Wall time is run creation to last update, so it **includes queue time**; runner minutes were not available within the unauthenticated quota and are listed as "not measured" rather than guessed.

| Workflow | Median wall | Fastest | Slowest | Runner minutes |
|---|---|---|---|---|
| `rust.yml` | 25.7 min | 11.4 | 38.2 | not measured, run `scripts/ci-minutes.sh rust.yml` |
| `branch-build.yml` | 33.7 min | 19.2 | 51.0 | not measured, run `scripts/ci-minutes.sh branch-build.yml` |

Queue on the same day (the 100 newest runs, 17:40 UTC): 20 `Rust` runs and 18 `Branch build` runs queued or pending, plus 33 queued `WER regression` runs. Every push triggers one `Rust` and one `Branch build` run, and two Windows jobs in `Rust` plus the installer build compete for the Windows runners, which is why the wall time is mostly waiting.

Expected effect (estimated from the structure, not yet measured; fill in the "after" row from `ci-minutes.sh` once this has been on `main` for a while):

| Change | Effect |
|---|---|
| Push that only touches `site/`, `docs/` etc. on a branch | `Rust` and `Branch build` finish in about a minute on Linux and take no Windows runner time (before: both full builds, median 26 and 34 min wall) |
| `wer.yml` cancels superseded branch runs | at most one running and no stale queued WER run per branch and event |
| fmt/clippy gate | one extra Linux job in parallel with the Windows jobs; it does not extend the critical path (the Windows jobs are the longest) |

Parallelism was not raised. Raising it on Windows runners needs a test on those runners first (#192).
