#!/usr/bin/env python3
"""Decide whether a change needs the Rust and installer builds.

    ci-scope.py --event-name push --ref-name <branch> [--before <sha>]

Prints `rust=true|false` and, when GITHUB_OUTPUT is set, appends the same line
there. Everything is treated as Rust-relevant unless every changed path is in
the skip list below, so a new top-level directory, Cargo.toml, Cargo.lock,
workspace config, shared scripts or a change to rust.yml itself always runs
the full build.

`main` and tags always get `rust=true`: the automatic release needs a
successful Rust push run for every commit on main (scripts/auto_release.py).
Other branches are compared with the merge-base with origin/main, so a later
docs-only push to a branch that changed Rust still builds.
"""
import argparse
import os
import subprocess
import sys

# Directory prefixes and exact files that no Rust job reads. docs/schemas is
# included by tingskriver-core (include_str!) and tested in rust.yml, so it is
# carved out below.
SKIP_PREFIXES = (
    "site/",
    "docs/",
    "apps/android/",
    ".github/ISSUE_TEMPLATE/",
)
SKIP_FILES = {
    "README.md",
    "CONTRIBUTING.md",
    "CLAUDE.md",
    ".github/issue_template.md",
    ".github/pull_request_template.md",
}
KEEP_PREFIXES = ("docs/schemas/",)
# Workflows that do not call into or share files with the Rust build. Anything
# else under .github/ (rust.yml, branch-build.yml, windows-build.yml, scripts)
# triggers it.
SKIP_WORKFLOWS = {
    "android.yml",
    "android-benchmark.yml",
    "android-release.yml",
    "pages.yml",
    "wer.yml",
    "search-benchmark.yml",
    "ydeevne-port.yml",
    "recording-soak.yml",
}


def is_skippable(path: str) -> bool:
    if path.startswith(KEEP_PREFIXES):
        return False
    if path in SKIP_FILES or path.startswith(SKIP_PREFIXES):
        return True
    if path.startswith(".github/workflows/"):
        return path.rsplit("/", 1)[-1] in SKIP_WORKFLOWS
    return False


def needs_rust(paths: list[str]) -> bool:
    """True when the change set is empty (unknown) or touches a non-skippable path."""
    paths = [p for p in paths if p]
    return not paths or any(not is_skippable(p) for p in paths)


def git(*args: str) -> str:
    return subprocess.run(
        ["git", *args], check=True, capture_output=True, text=True
    ).stdout


def changed_paths(base: str) -> list[str]:
    return git("diff", "--name-only", "--no-renames", f"{base}...HEAD").splitlines()


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--event-name", default="push")
    ap.add_argument("--ref-name", default="")
    ap.add_argument("--ref-type", default="branch")
    args = ap.parse_args()

    if args.ref_name == "main" or args.ref_type == "tag" or args.event_name == "workflow_dispatch":
        result = True
    else:
        try:
            git("fetch", "--no-tags", "--quiet", "origin", "main")
            result = needs_rust(changed_paths("origin/main"))
        except subprocess.CalledProcessError as err:
            print(f"::warning::cannot compute the change set ({err}); building everything")
            result = True

    line = f"rust={'true' if result else 'false'}"
    print(line)
    out = os.environ.get("GITHUB_OUTPUT")
    if out:
        with open(out, "a", encoding="utf-8") as fh:
            fh.write(line + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
