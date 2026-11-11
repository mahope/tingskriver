#!/usr/bin/env python3
"""Automatic pre-release of the GPUI app (Velopack). See docs/auto-release.md.

The scheduled workflow .github/workflows/auto-release.yml calls the sub
commands below. Everything that can be decided without GitHub (version maths,
changelog text, CI verdict, diff checks) is a plain function and is covered by
scripts/tests/test_auto_release.py.

  plan        decide whether to release, and which version (writes GITHUB_OUTPUT)
  bump        write the new version into [workspace.package] in Cargo.toml
  check-bump  verify that the bump changed nothing but the version lines
  changelog   write docs/changelog/<version>.md from the merged pull requests
  release     dispatch app-release.yml on the tag, wait for it, verify the
              published release and add the changelog to its description
  verify      verify an already published release (feed, assets, not latest)
"""

import argparse
import datetime
import json
import os
import re
import subprocess
import sys
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CARGO_TOML = ROOT / "Cargo.toml"
TAG_PREFIX = "app-v"
RELEASE_WORKFLOW = "app-release.yml"
REQUIRED_WORKFLOW = "Rust"
BAD_CONCLUSIONS = {"failure", "cancelled", "timed_out", "action_required", "startup_failure", "stale"}
BOT_EMAIL = "41898282+github-actions[bot]@users.noreply.github.com"
_BUMP_SUBJECT = re.compile(r"^Release \d+\.\d+\.\d+\S*$")

_SEMVER = re.compile(
    r"^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?$"
)


def is_bump_commit(subject, email):
    """Our own `Release <version>` commit; a person's commit with a similar subject does not count."""
    return email == BOT_EMAIL and bool(_BUMP_SUBJECT.match(subject))


# --------------------------------------------------------------------------
# Version maths (pure)
# --------------------------------------------------------------------------

def parse_version(text):
    m = _SEMVER.match(text)
    if not m:
        raise ValueError(f"not a semantic version: {text!r}")
    major, minor, patch = (int(m.group(i)) for i in (1, 2, 3))
    pre = tuple(m.group(4).split(".")) if m.group(4) else ()
    return major, minor, patch, pre


def version_key(text):
    """SemVer precedence: a pre-release sorts before the release it leads to."""
    major, minor, patch, pre = parse_version(text)
    if not pre:
        return (major, minor, patch, 1, ())
    ids = tuple((0, int(p), "") if p.isdigit() else (1, 0, p) for p in pre)
    return (major, minor, patch, 0, ids)


def version_from_tag(tag):
    """app-v0.5.0-dev.3 and v0.5.0-dev.2 are versions; android-v0.0.1 is not ours."""
    for prefix in (TAG_PREFIX, "v"):
        if tag.startswith(prefix):
            candidate = tag[len(prefix):]
            if _SEMVER.match(candidate):
                return candidate
    return None


def highest_version(tags, cargo_version):
    """Highest version published or tagged anywhere (both tag families), and in Cargo.toml."""
    versions = [v for v in (version_from_tag(t) for t in tags) if v]
    versions.append(cargo_version)
    return max(versions, key=version_key)


def next_version(current):
    """One micro step, always a pre-release: -dev.N becomes -dev.N+1.

    After a stable x.y.z (released by hand) the next pre-release series starts at
    x.y.(z+1)-dev.1. Stable versions are never produced here (docs/release-checkliste.md).
    """
    major, minor, patch, pre = parse_version(current)
    if pre:
        last = pre[-1]
        if not last.isdigit():
            raise ValueError(f"cannot step pre-release {current}: its last identifier is not a number")
        stepped = pre[:-1] + (str(int(last) + 1),)
        return f"{major}.{minor}.{patch}-{'.'.join(stepped)}"
    return f"{major}.{minor}.{patch + 1}-dev.1"


def read_workspace_version(text):
    section = re.search(r"(?ms)^\[workspace\.package\]\s*$(.*?)(?=^\[|\Z)", text)
    if not section:
        raise ValueError("no [workspace.package] section in Cargo.toml")
    m = re.search(r'(?m)^version\s*=\s*"([^"]+)"\s*$', section.group(1))
    if not m:
        raise ValueError("no version in [workspace.package]")
    return m.group(1)


def set_workspace_version(text, new):
    section = re.search(r"(?ms)^\[workspace\.package\]\s*$(.*?)(?=^\[|\Z)", text)
    if not section:
        raise ValueError("no [workspace.package] section in Cargo.toml")
    body, count = re.subn(
        r'(?m)^(version\s*=\s*)"[^"]+"(\s*)$', lambda m: f'{m.group(1)}"{new}"{m.group(2)}', section.group(1), count=1
    )
    if count != 1:
        raise ValueError("no version in [workspace.package]")
    return text[: section.start(1)] + body + text[section.end(1):]


def check_bump_diff(diff_text, old, new):
    """Return a list of problems; empty when the diff only moves `version = old` to new."""
    problems = []
    removed = added = 0
    for line in diff_text.splitlines():
        if line.startswith(("diff --git", "index ", "--- ", "+++ ", "@@")):
            continue
        if line.startswith("-"):
            removed += 1
            if line != f'-version = "{old}"':
                problems.append(f"unexpected removed line: {line[:80]}")
        elif line.startswith("+"):
            added += 1
            if line != f'+version = "{new}"':
                problems.append(f"unexpected added line: {line[:80]}")
    if removed != added:
        problems.append(f"{removed} lines removed but {added} added")
    if removed == 0:
        problems.append("the version did not change")
    return problems


# --------------------------------------------------------------------------
# Pull requests and changelog (pure)
# --------------------------------------------------------------------------

_MERGE = re.compile(r"^Merge pull request #(\d+) ")
_SQUASH = re.compile(r"\(#(\d+)\)\s*$")

_ADDED = {
    "add", "adds", "build", "builds", "create", "introduce", "implement", "support", "enable", "allow",
    "show", "publish", "import", "export", "tilføj", "byg", "opret", "vis", "udgiv", "importér", "eksportér",
}
_FIXED = {
    "fix", "fixes", "fixed", "repair", "correct", "resolve", "prevent", "avoid", "reject", "preserve",
    "ret", "rette", "undgå", "forhindr", "stop",
}
_REMOVED = {"remove", "removes", "drop", "delete", "retire", "fjern"}
SECTIONS = ("Tilføjet", "Ændret", "Rettet", "Fjernet")


def pr_number(subject):
    m = _MERGE.match(subject) or _SQUASH.search(subject)
    return int(m.group(1)) if m else None


def section_for(title):
    words = re.split(r"[\s:]+", title.strip().lower(), maxsplit=1)
    first = words[0] if words else ""
    if first in _FIXED:
        return "Rettet"
    if first in _REMOVED:
        return "Fjernet"
    if first in _ADDED:
        return "Tilføjet"
    return "Ændret"


def render_changelog(version, date, previous, entries, repo):
    """entries: list of (title, pr_number or None). Newest first."""
    lines = [f"# Tingskriver {version}", ""]
    since = f" siden {previous}" if previous else ""
    lines.append(f"Udgivet {date}. Genereret automatisk ud fra de flettede pull requests{since}.")
    lines.append("")
    grouped = {s: [] for s in SECTIONS}
    for title, number in entries:
        ref = f" ([#{number}](https://github.com/{repo}/pull/{number}))" if number else ""
        grouped[section_for(title)].append(f"- {title}{ref}")
    if not any(grouped.values()):
        grouped["Ændret"].append("- Interne ændringer uden brugerrettet beskrivelse")
    for name in SECTIONS:
        if grouped[name]:
            lines.append(f"## {name}")
            lines.append("")
            lines.extend(grouped[name])
            lines.append("")
    return "\n".join(lines).rstrip() + "\n"


# --------------------------------------------------------------------------
# CI verdict (pure)
# --------------------------------------------------------------------------

def evaluate_ci(runs):
    """runs: workflow runs for one commit, as from `gh run list --json`.

    Returns (state, detail): green | pending | red | missing.
    Only push runs count; scheduled or manual workflows say nothing about the commit.
    """
    push = [r for r in runs if r.get("event") == "push"]
    if not push:
        return "missing", "no push workflow runs for the commit"
    bad = [r for r in push if r.get("status") == "completed" and r.get("conclusion") in BAD_CONCLUSIONS]
    if bad:
        return "red", ", ".join(f"{r.get('workflowName')}: {r.get('conclusion')}" for r in bad)
    pending = [r for r in push if r.get("status") != "completed"]
    if pending:
        return "pending", ", ".join(f"{r.get('workflowName')}: {r.get('status')}" for r in pending)
    if not any(r.get("workflowName") == REQUIRED_WORKFLOW and r.get("conclusion") == "success" for r in push):
        return "missing", f"workflow {REQUIRED_WORKFLOW} has no successful run for the commit"
    return "green", ""


# --------------------------------------------------------------------------
# Plumbing
# --------------------------------------------------------------------------

def run(*cmd, check=True, cwd=ROOT):
    result = subprocess.run(cmd, cwd=cwd, capture_output=True, text=True, encoding="utf-8")
    if check and result.returncode != 0:
        raise RuntimeError(f"{' '.join(cmd[:3])} failed ({result.returncode}): {result.stderr.strip()[:500]}")
    return result


def gh_json(*args):
    return json.loads(run("gh", *args).stdout or "null")


def log(message):
    print(message, flush=True)


def set_output(**values):
    path = os.environ.get("GITHUB_OUTPUT")
    lines = "".join(f"{k}={v}\n" for k, v in values.items())
    if path:
        with open(path, "a", encoding="utf-8") as fh:
            fh.write(lines)
    log("outputs: " + ", ".join(f"{k}={v}" for k, v in values.items()))


def repo_name():
    return os.environ.get("GITHUB_REPOSITORY") or gh_json("repo", "view", "--json", "nameWithOwner")["nameWithOwner"]


def release_states():
    """(published, drafts): tags of non-draft and of draft GPUI releases."""
    releases = gh_json("release", "list", "--limit", "300", "--json", "tagName,isDraft")
    app = [r for r in releases if r["tagName"].startswith(TAG_PREFIX)]
    return [r["tagName"] for r in app if not r["isDraft"]], [r["tagName"] for r in app if r["isDraft"]]


def published_app_tags():
    return release_states()[0]


def log_commits(rev_range, limit=None, cwd=ROOT):
    """First-parent commits (sha, subject, author email, commit time), newest first.

    NUL separates the fields: a subject may contain a tab.
    """
    cmd = ["git", "log", "--first-parent", "--format=%H%x00%ae%x00%ct%x00%s"]
    if limit:
        cmd += ["-n", str(limit)]
    out = run(*cmd, rev_range, cwd=cwd).stdout
    commits = []
    for line in out.splitlines():
        sha, email, ts, subject = line.split("\0", 3)
        commits.append((sha, subject, email, int(ts)))
    return commits


def commits_since(tag, ref="HEAD"):
    """Commits after `tag`, newest first, without our own release commits (for the changelog)."""
    return [(sha, subject) for sha, subject, email, _ in log_commits(f"{tag}..{ref}") if not is_bump_commit(subject, email)]


def changes_since_last_attempt(tag, ref="HEAD", cwd=ROOT):
    """Changes after the most recent release attempt: the newest app tag or a newer bump commit.

    Returns (changes, stray_bump): `stray_bump` is True when a Release commit newer than the tag
    was found, i.e. the bump reached main but its tag did not.
    """
    changes = []
    for sha, subject, email, _ in log_commits(f"{tag}..{ref}" if tag else ref, None if tag else 200, cwd=cwd):
        if is_bump_commit(subject, email):
            return changes, True
        changes.append((sha, subject))
    return changes, False


def gate_commit():
    """The commit CI must be green for: HEAD, or below it when HEAD is a bump commit without CI."""
    for sha, subject, email, ts in log_commits("HEAD", 20):
        if not is_bump_commit(subject, email):
            return sha, ts
    raise RuntimeError("no commit below the release commits found")


def decide(tags, cargo_version, published, changes_since_last_attempt, stray_bump=False, drafts=()):
    """What to do, from facts only (pure). Returns (action, reason, version, last_tag, last_published).

    action is "release", "skip" or "fail". `changes_since_last_attempt` is the number of commits on
    main after the newest app-v* tag or a newer Release commit, or None when there is no such tag.
    """
    highest = highest_version(tags, cargo_version)
    version = next_version(highest)
    app_tags = sorted((t for t in tags if t.startswith(TAG_PREFIX) and version_from_tag(t)),
                      key=lambda t: version_key(version_from_tag(t)))
    last_tag = app_tags[-1] if app_tags else None
    pub = sorted((t for t in published if version_from_tag(t)), key=lambda t: version_key(version_from_tag(t)))
    last_published = pub[-1] if pub else None
    if version in (version_from_tag(t) for t in tags):
        raise RuntimeError(f"version {version} is already tagged; tags are never reused")
    # A stable version in Cargo.toml without its tag is a manual release in the making (the performance
    # sign-off may sit between the commit and the tag): do not publish pre-releases on top of it.
    if not parse_version(cargo_version)[3] and TAG_PREFIX + cargo_version not in tags:
        return "skip", "stable-pending", version, last_tag, last_published
    if last_tag is not None and changes_since_last_attempt == 0:
        if stray_bump:
            # The bump commit reached main, but its tag did not. Do not bump again for the same content.
            return "fail", "bump-without-tag", version, last_tag, last_published
        if last_tag in drafts:
            return "skip", "release-is-draft", version, last_tag, last_published
        if last_tag != last_published:
            # The last attempt left a tag without a release and nothing new has landed since.
            # Do not burn another version for the same content: a person looks at the failed build.
            return "fail", "last-attempt-failed", version, last_tag, last_published
        return "skip", "no-changes", version, last_tag, last_published
    return "release", "ok", version, last_tag, last_published


# --------------------------------------------------------------------------
# Sub commands
# --------------------------------------------------------------------------

def cmd_plan(args):
    cargo_version = read_workspace_version(CARGO_TOML.read_text(encoding="utf-8"))
    run("git", "fetch", "--tags", "--force", "origin")
    tags = run("git", "tag", "--list").stdout.split()
    published, drafts = release_states()
    newest_tag = max((t for t in tags if t.startswith(TAG_PREFIX) and version_from_tag(t)),
                     key=lambda t: version_key(version_from_tag(t)), default=None)
    changes, stray_bump = changes_since_last_attempt(newest_tag)
    action, reason, version, last_tag, last_published = decide(
        tags, cargo_version, published, len(changes), stray_bump, drafts)
    log(f"Cargo.toml {cargo_version}; newest app tag {last_tag}; last published GPUI release {last_published}; "
        f"next version {version}; {len(changes)} change(s) since the last attempt"
        f"{' (a Release commit without a tag was found)' if stray_bump else ''}")
    if action == "fail" and reason == "last-attempt-failed":
        building = gh_json("run", "list", "--workflow", RELEASE_WORKFLOW, "--branch", last_tag, "--limit", "5",
                           "--json", "status")
        if any(r["status"] != "completed" for r in building):
            log(f"{last_tag} is still being built by {RELEASE_WORKFLOW}. Trying again at the next run.")
            set_output(release="false", reason="release-building")
            return 0
    if action == "fail" and reason == "last-attempt-failed":
        log(f"ERROR The newest tag {last_tag} has no published release, and nothing has been merged since. "
            f"Look at the failed {RELEASE_WORKFLOW} run for {last_tag}. Fix it and run that workflow on "
            f"the tag again, or merge a change to get a new version. Not burning another version number.")
        set_output(release="false", reason=reason)
        return 1
    if action == "fail":
        log(f"ERROR A Release commit for {cargo_version} is on main without its tag, and nothing has been merged "
            f"since. Tag it by hand (git tag -a app-v{cargo_version} <the Release commit>; git push origin "
            f"app-v{cargo_version}) and run {RELEASE_WORKFLOW} on the tag. Not bumping again.")
        set_output(release="false", reason=reason)
        return 1
    if action == "skip":
        log(f"Nothing to release ({reason}).")
        set_output(release="false", reason=reason)
        return 0
    tag = TAG_PREFIX + version

    gate_sha, gate_ts = gate_commit()
    deadline = time.time() + args.wait_minutes * 60
    while True:
        runs = gh_json("run", "list", "--commit", gate_sha, "--limit", "100", "--json",
                       "workflowName,event,status,conclusion")
        state, detail = evaluate_ci(runs)
        young = time.time() - gate_ts < 20 * 60
        log(f"CI for {gate_sha[:8]}: {state} {detail}")
        if state == "green":
            break
        if state == "red":
            log("CI on main is red. No release.")
            set_output(release="false", reason="ci-red")
            return 0
        if state == "missing" and not young:
            log("CI on main has not produced a green Rust run for this commit. No release.")
            set_output(release="false", reason="ci-missing")
            return 0
        if time.time() > deadline:
            log("CI on main is still running after the wait. Trying again at the next run.")
            set_output(release="false", reason="ci-pending")
            return 0
        time.sleep(60)

    # The push of the bump commit would be rejected if main moved while CI was awaited.
    # Say so now and leave quietly; the next run starts over on the new main.
    run("git", "fetch", "origin", "main")
    head = run("git", "rev-parse", "HEAD").stdout.strip()
    moved = run("git", "rev-parse", "origin/main").stdout.strip()
    if head != moved:
        log(f"main moved from {head[:8]} to {moved[:8]} while CI was awaited. Trying again at the next run.")
        set_output(release="false", reason="main-moved")
        return 0

    set_output(release="true", version=version, tag=tag, previous=last_published or "",
               current=cargo_version, gate=gate_sha, reason="ok")
    return 0


def cmd_bump(args):
    text = CARGO_TOML.read_text(encoding="utf-8")
    old = read_workspace_version(text)
    if version_key(args.version) <= version_key(old):
        raise RuntimeError(f"refusing to go from {old} to {args.version}: versions never go backwards")
    CARGO_TOML.write_text(set_workspace_version(text, args.version), encoding="utf-8", newline="\n")
    log(f"Cargo.toml: {old} -> {args.version}")
    return 0


def cmd_check_bump(args):
    problems = []
    changed = run("git", "diff", "--name-only").stdout.split()
    if sorted(changed) != ["Cargo.lock", "Cargo.toml"]:
        problems.append(f"expected only Cargo.toml and Cargo.lock to change, got {changed}")
    for name in ("Cargo.toml", "Cargo.lock"):
        problems += [f"{name}: {p}" for p in check_bump_diff(run("git", "diff", "-U0", name).stdout, args.old, args.new)]
    if problems:
        for p in problems:
            log(f"ERROR {p}")
        return 1
    log("Version bump touches only the workspace version lines in Cargo.toml and Cargo.lock.")
    return 0


def cmd_changelog(args):
    repo = repo_name()
    commits = commits_since(args.since) if args.since else commits_since("HEAD~30")
    entries, seen = [], set()
    for sha, subject in commits:
        if subject.startswith(("Merge branch", "Merge remote", "wip")):
            continue
        number = pr_number(subject)
        if number is not None:
            if number in seen:
                continue
            seen.add(number)
            try:
                title = gh_json("pr", "view", str(number), "--json", "title")["title"]
            except RuntimeError as exc:  # an issue number, or a PR we cannot read: use the commit subject
                log(f"WARNING no pull request #{number}: {str(exc)[:120]}; using the commit subject")
                title, number = subject, None
        else:
            title = subject
        entries.append((title, number))
    date = datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%d")
    text = render_changelog(args.version, date, args.since, entries, repo)
    out = Path(args.out)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(text, encoding="utf-8", newline="\n")
    log(f"{out}: {len(entries)} entries")
    print(text)
    return 0


def dispatch_and_wait(tag, timeout_minutes):
    started = datetime.datetime.now(datetime.timezone.utc)
    run("gh", "workflow", "run", RELEASE_WORKFLOW, "--ref", tag)
    log(f"Dispatched {RELEASE_WORKFLOW} on {tag}")
    run_id = None
    for _ in range(30):
        time.sleep(10)
        runs = gh_json("run", "list", "--workflow", RELEASE_WORKFLOW, "--event", "workflow_dispatch",
                       "--limit", "20", "--json", "databaseId,headBranch,createdAt")
        for r in runs:
            created = datetime.datetime.fromisoformat(r["createdAt"].replace("Z", "+00:00"))
            if r["headBranch"] == tag and created >= started - datetime.timedelta(minutes=2):
                run_id = r["databaseId"]
                break
        if run_id:
            break
    if not run_id:
        raise RuntimeError(f"the {RELEASE_WORKFLOW} run for {tag} never appeared")
    log(f"Release run: {run_id}")
    deadline = time.time() + timeout_minutes * 60
    while time.time() < deadline:
        state = gh_json("run", "view", str(run_id), "--json", "status,conclusion,url")
        if state["status"] == "completed":
            log(f"Release run finished: {state['conclusion']} {state['url']}")
            if state["conclusion"] != "success":
                raise RuntimeError(f"{RELEASE_WORKFLOW} on {tag} ended with {state['conclusion']}: {state['url']}")
            return run_id
        time.sleep(60)
    raise RuntimeError(f"{RELEASE_WORKFLOW} on {tag} did not finish within {timeout_minutes} minutes")


def feed_assets(tags):
    """Velopack assets of the given releases, as the app joins them."""
    assets = []
    for tag in tags:
        with tempfile.TemporaryDirectory() as tmp:
            res = run("gh", "release", "download", tag, "--pattern", "releases.win.json", "--dir", tmp, check=False)
            path = Path(tmp) / "releases.win.json"
            if res.returncode != 0 or not path.exists():
                continue
            assets.extend(json.loads(path.read_text(encoding="utf-8-sig")).get("Assets", []))
    return assets


def verify_release(tag, version):
    problems = []
    rel = gh_json("release", "view", tag, "--json", "tagName,isDraft,isPrerelease,assets,url")
    names = {a["name"] for a in rel["assets"]}
    if rel["isDraft"]:
        problems.append("the release is still a draft")
    expect_pre = "-" in version
    if rel["isPrerelease"] != expect_pre:
        problems.append(f"isPrerelease is {rel['isPrerelease']}, expected {expect_pre}")
    required = {"releases.win.json", "Tingskriver-Setup.exe", f"TingskriverApp-{version}-full.nupkg"}
    previous = [t for t in published_app_tags() if t != tag and version_key(version_from_tag(t) or "0.0.0") < version_key(version)]
    if previous:
        required.add(f"TingskriverApp-{version}-delta.nupkg")
    missing = sorted(required - names)
    if missing:
        problems.append(f"missing assets: {missing}")

    ten_newest = [r["tagName"] for r in gh_json("release", "list", "--limit", "10", "--json", "tagName,isDraft") if not r["isDraft"]]
    if tag not in ten_newest:
        problems.append("the release is not among the ten newest releases, which is all the app's feed reads")
    feed = feed_assets(ten_newest)
    mine = [a for a in feed if a.get("Version") == version]
    types = {a.get("Type") for a in mine}
    if "Full" not in types:
        problems.append("the feed (ten newest releases joined) has no Full package for the new version")
    if previous and "Delta" not in types:
        problems.append("the feed has no Delta package for the new version")
    higher = [a["Version"] for a in feed if a.get("Version") and version_key(a["Version"]) > version_key(version)]
    if higher:
        problems.append(f"the feed offers higher versions than the new release: {sorted(set(higher))}")

    latest = run("gh", "api", f"repos/{repo_name()}/releases/latest", "--jq", ".tag_name", check=False).stdout.strip()
    if latest.startswith(TAG_PREFIX):
        problems.append(f"{latest} is marked Latest; the Tauri updater still reads releases/latest (installer/README.md)")
    log(f"Release {rel['url']}: assets {sorted(names)}; feed versions {sorted({a.get('Version') for a in feed})}; latest {latest}")
    return problems


def cmd_verify(args):
    problems = verify_release(args.tag, args.version)
    for p in problems:
        log(f"ERROR {p}")
    return 1 if problems else 0


RELEASE_NOTES_FOOTER = "\n<!-- installation-notes -->\n"


def cmd_release(args):
    dispatch_and_wait(args.tag, args.timeout_minutes)
    problems = verify_release(args.tag, args.version)
    if problems:
        for p in problems:
            log(f"ERROR {p}")
        return 1
    if args.changelog:
        existing = gh_json("release", "view", args.tag, "--json", "body")["body"] or ""
        changelog = Path(args.changelog).read_text(encoding="utf-8")
        if existing.split(RELEASE_NOTES_FOOTER, 1)[0] != changelog:
            log("ERROR release description differs from the committed changelog")
            return 1
    log(f"Released {args.version}.")
    return 0


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="command", required=True)

    p = sub.add_parser("plan")
    p.add_argument("--wait-minutes", type=int, default=90, help="how long to wait for CI that is still running")
    p.set_defaults(func=cmd_plan)

    p = sub.add_parser("bump")
    p.add_argument("--version", required=True)
    p.set_defaults(func=cmd_bump)

    p = sub.add_parser("check-bump")
    p.add_argument("--old", required=True)
    p.add_argument("--new", required=True)
    p.set_defaults(func=cmd_check_bump)

    p = sub.add_parser("changelog")
    p.add_argument("--version", required=True)
    p.add_argument("--since", default="", help="tag of the previous published release")
    p.add_argument("--out", required=True)
    p.set_defaults(func=cmd_changelog)

    p = sub.add_parser("release")
    p.add_argument("--tag", required=True)
    p.add_argument("--version", required=True)
    p.add_argument("--changelog", default="")
    p.add_argument("--timeout-minutes", type=int, default=210)
    p.set_defaults(func=cmd_release)

    p = sub.add_parser("verify")
    p.add_argument("--tag", required=True)
    p.add_argument("--version", required=True)
    p.set_defaults(func=cmd_verify)

    args = parser.parse_args(argv)
    return args.func(args)


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Exception as exc:  # noqa: BLE001 - one line in the Actions log, non-zero exit
        print(f"ERROR {exc}", file=sys.stderr)
        sys.exit(1)
