#!/usr/bin/env python3
"""Exercise the native watchdog against an intentionally parked process."""
from pathlib import Path
import json
import subprocess

build = subprocess.run(
    ["cargo", "test", "--locked", "-p", "tingskriver-app", "--features",
     "screenshot-tests", "--test", "screenshots", "--no-run", "--message-format=json"],
    cwd=Path(__file__).resolve().parents[1], stdout=subprocess.PIPE,
    text=True, encoding="utf-8", check=True,
)
artifacts = [json.loads(line) for line in build.stdout.splitlines() if line.startswith("{")]
executables = [item["executable"] for item in artifacts
               if item.get("reason") == "compiler-artifact"
               and item["target"]["name"] == "screenshots" and item.get("executable")]
assert len(executables) == 1, "Expected one screenshot executable"
# Run the executable directly so timeout cleanup cannot leave a Cargo child parked.
result = subprocess.run([executables[0], "--watchdog-self-test"],
                        stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                        text=True, encoding="utf-8", timeout=5, check=False)
assert result.returncode == 1, "Watchdog must terminate the parked process with failure"
assert "Native capture timed out" in result.stdout, result.stdout
print("Native watchdog rejected simulated stuck capture")
