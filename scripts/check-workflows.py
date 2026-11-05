#!/usr/bin/env python3
"""Static checks of the GitHub Actions workflows that PR CI cannot exercise.

A scheduled or reusable workflow can fail at start-up (a job asks for a token
permission the caller does not grant) while every pull request stays green.
This reproduces that class of failure before merge (#192):

  * every workflow declares top-level `permissions` (reusable-only ones inherit
    the caller's);
  * a workflow with `schedule` also has `workflow_dispatch`, so it can be run
    by hand after a change (gh workflow run <file>);
  * for each job that calls a local reusable workflow, the permissions the
    callee asks for (workflow level and per job) are not above what the caller
    job grants;
  * `needs:` names an existing job.

    python3 scripts/check-workflows.py [workflow-dir]
"""
import pathlib
import sys

try:
    import yaml
except ImportError:
    sys.exit("check-workflows.py needs PyYAML: pip install pyyaml (or apt install python3-yaml)")

LEVEL = {"none": 0, "read": 1, "write": 2}


def expand(perms):
    """Normalise a `permissions:` value to {scope: level}; None when absent."""
    if perms is None:
        return None
    if perms == "read-all":
        return {"*": 1}
    if perms == "write-all":
        return {"*": 2}
    return {k: LEVEL[str(v)] for k, v in perms.items()}


def triggers(doc):
    # PyYAML (YAML 1.1) reads the key `on` as True.
    on = doc.get("on", doc.get(True))
    if isinstance(on, str):
        return {on: None}
    if isinstance(on, list):
        return {k: None for k in on}
    return on or {}


def granted(perms, scope):
    if perms is None:
        return None
    return perms.get(scope, perms.get("*", 0))


def check(workflows):
    """workflows: {file name: parsed document}. Returns a list of problems."""
    problems = []
    for name, doc in sorted(workflows.items()):
        jobs = doc.get("jobs") or {}
        on = triggers(doc)
        top = expand(doc.get("permissions"))
        if top is None and set(on) != {"workflow_call"}:
            problems.append(f"{name}: no top-level `permissions:`")
        if "schedule" in on and "workflow_dispatch" not in on:
            problems.append(f"{name}: has `schedule` but no `workflow_dispatch`, so it cannot be tested by hand")
        for job_id, job in jobs.items():
            for need in ([job.get("needs")] if isinstance(job.get("needs"), str) else job.get("needs") or []):
                if need not in jobs:
                    problems.append(f"{name}: job `{job_id}` needs unknown job `{need}`")
            uses = job.get("uses", "")
            if not uses.startswith("./.github/workflows/"):
                continue
            callee_name = uses.rsplit("/", 1)[-1]
            callee = workflows.get(callee_name)
            if callee is None:
                problems.append(f"{name}: job `{job_id}` calls missing workflow {callee_name}")
                continue
            caller = expand(job.get("permissions")) or top or {}
            asked = {}
            for perms in [expand(callee.get("permissions"))] + [
                expand(j.get("permissions")) for j in (callee.get("jobs") or {}).values()
            ]:
                for scope, level in (perms or {}).items():
                    asked[scope] = max(asked.get(scope, 0), level)
            for scope, level in sorted(asked.items()):
                if level > (granted(caller, scope) or 0):
                    wanted = {v: k for k, v in LEVEL.items()}[level]
                    problems.append(
                        f"{name}: job `{job_id}` calls {callee_name}, which needs `{scope}: {wanted}` that the caller does not grant"
                    )
    return problems


def load(directory):
    out = {}
    for path in sorted(pathlib.Path(directory).glob("*.y*ml")):
        out[path.name] = yaml.safe_load(path.read_text(encoding="utf-8")) or {}
    return out


def main():
    directory = sys.argv[1] if len(sys.argv) > 1 else ".github/workflows"
    problems = check(load(directory))
    for p in problems:
        print(f"::error::{p}")
    if problems:
        return 1
    print("workflow checks passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
