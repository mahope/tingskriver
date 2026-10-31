#!/usr/bin/env python3
"""Fail when the workspace resolves GPUI differently from gpui-component.

GPUI is pinned to exactly the revision gpui-component's own Cargo.lock uses
(docs/gpui.md, "Versionslåsning"). Mixed GPUI versions compile into E0277
errors far from the cause, so CI checks three things:

1. Cargo.lock holds one version of every gpui* package.
2. `cargo tree -d --depth 0` lists no gpui* package (no duplicates on any
   target).
3. Every gpui* package that both lock files contain has the same version and
   checksum in the workspace and in gpui-component's Cargo.lock.

Usage: scripts/check-gpui-lock.py [--lock Cargo.lock] [--component-lock PATH]
       [--skip-cargo-tree]

Without --component-lock the gpui-component source is located through
`cargo metadata`, so the crate must be fetched (any cargo build does that).
Exit code 0 when everything matches, 1 on a mismatch, 2 on usage errors.
"""

import argparse
import json
import re
import subprocess
import sys
import tomllib
from collections import defaultdict
from pathlib import Path

GPUI_NAME = re.compile(r"^gpui")
COMPONENT = "gpui-component"


def load_packages(lock_path):
    with open(lock_path, "rb") as fh:
        return tomllib.load(fh).get("package", [])


def gpui_versions(packages):
    versions = defaultdict(dict)
    for pkg in packages:
        if GPUI_NAME.match(pkg["name"]):
            versions[pkg["name"]][pkg["version"]] = pkg.get("checksum")
    return versions


def component_lock_from_metadata(workspace_dir):
    out = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--locked"],
        cwd=workspace_dir,
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    for pkg in json.loads(out)["packages"]:
        if pkg["name"] == COMPONENT:
            return Path(pkg["manifest_path"]).parent / "Cargo.lock"
    raise SystemExit(f"{COMPONENT} is not in the dependency graph")


def duplicates_in_cargo_tree(workspace_dir):
    out = subprocess.run(
        ["cargo", "tree", "--locked", "-d", "--depth", "0", "--target", "all",
         "-e", "normal,build,dev", "--prefix", "none"],
        cwd=workspace_dir,
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    return sorted({line.split(" (")[0] for line in out.splitlines() if GPUI_NAME.match(line)})


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--lock", default="Cargo.lock")
    parser.add_argument("--component-lock")
    parser.add_argument("--skip-cargo-tree", action="store_true")
    args = parser.parse_args()

    lock = Path(args.lock).resolve()
    if not lock.is_file():
        print(f"no lock file at {lock}", file=sys.stderr)
        return 2
    ours = gpui_versions(load_packages(lock))
    errors = []

    for name, versions in sorted(ours.items()):
        if len(versions) > 1:
            errors.append(f"{name} has {len(versions)} versions in {lock.name}: {', '.join(sorted(versions))}")

    if not args.skip_cargo_tree:
        for line in duplicates_in_cargo_tree(lock.parent):
            errors.append(f"cargo tree -d lists a duplicate: {line}")

    component_versions = ours.get(COMPONENT, {})
    if len(component_versions) != 1:
        errors.append(f"expected exactly one {COMPONENT} in {lock.name}, found {len(component_versions)}")
    else:
        component_lock = Path(args.component_lock) if args.component_lock else component_lock_from_metadata(lock.parent)
        if not component_lock.is_file():
            print(f"{COMPONENT} ships no Cargo.lock at {component_lock}", file=sys.stderr)
            return 2
        theirs = gpui_versions(load_packages(component_lock))
        compared = 0
        for name, versions in sorted(theirs.items()):
            if name == COMPONENT or name not in ours:
                continue
            for version, checksum in versions.items():
                compared += 1
                if version not in ours[name]:
                    errors.append(
                        f"{name}: {COMPONENT} locks {version}, the workspace has {', '.join(sorted(ours[name]))}"
                    )
                elif checksum and ours[name][version] != checksum:
                    errors.append(f"{name} {version}: checksum differs from {COMPONENT}'s lock")
        if "gpui-pre" not in ours or compared == 0:
            errors.append(f"no GPUI package shared with {COMPONENT}'s lock; is gpui still gpui-pre?")
        print(f"compared {compared} GPUI packages with {component_lock}")

    if errors:
        for error in errors:
            print(f"::error::{error}")
        return 1
    gpui = ", ".join(f"{name} {next(iter(v))}" for name, v in sorted(ours.items()) if name in ("gpui-pre", COMPONENT))
    print(f"one GPUI version, matching {COMPONENT}'s lock ({gpui})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
