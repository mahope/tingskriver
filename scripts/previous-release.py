#!/usr/bin/env python3
"""Select an installed predecessor able to see the target's public update channel."""
import json
import sys

from auto_release import parse_version, version_key


def previous(releases, version):
    prerelease = bool(parse_version(version)[3])
    candidates = []
    for release in releases:
        tag = release["tag_name"]
        if release.get("draft") or not tag.startswith("app-v"):
            continue
        candidate = tag[len("app-v"):]
        try:
            parsed = parse_version(candidate)
        except ValueError:
            continue
        if bool(parsed[3]) != prerelease or version_key(candidate) >= version_key(version):
            continue
        if any(asset["name"] == "Tingskriver-Setup.exe" for asset in release.get("assets", [])):
            candidates.append(candidate)
    if not candidates:
        raise ValueError(f"No published installer predecessor for the {version} update channel")
    return "app-v" + max(candidates, key=version_key)


if __name__ == "__main__":
    pages = json.load(sys.stdin)
    print(previous([release for page in pages for release in page], sys.argv[1]))
