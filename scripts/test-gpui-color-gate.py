#!/usr/bin/env python3
"""Prove that a changed production palette token fails the golden gate."""
import os
from pathlib import Path
import subprocess


def main():
    root = Path(__file__).resolve().parents[1]
    theme = root / "crates/tingskriver-app/src/theme.rs"
    original = theme.read_bytes()
    token = b"background: hsl(40.0, 14.0, 96.0)"
    assert original.count(token) == 1, "Locate the light background token before mutating"
    mutated = original.replace(token, b"background: hsl(120.0, 100.0, 50.0)")
    output = root / "target/screenshots/color-mutation"
    output.mkdir(parents=True, exist_ok=True)
    environment = dict(os.environ, TINGSKRIVER_SCREENSHOT_OUTPUT=str(output))
    try:
        theme.write_bytes(mutated)
        with (output / "gate.log").open("w", encoding="utf-8") as log:
            result = subprocess.run(
                ["cargo", "test", "--locked", "-p", "tingskriver-app", "--features",
                 "screenshot-tests", "--test", "screenshots"],
                cwd=root, env=environment, stdout=log, stderr=subprocess.STDOUT,
                timeout=300, check=False,
            )
        log = (output / "gate.log").read_text(encoding="utf-8")
        assert result.returncode != 0, "Changed background unexpectedly passed"
        assert "changed pixels" in log, "Failure must come from golden comparison, not build/render errors"
        assert (output / "diff-meetings-light.png").is_file(), "Missing difference artifact"
        print("Changed production background rejected by golden comparison")
    finally:
        assert theme.read_bytes() == mutated, "Theme changed concurrently; refusing to overwrite"
        theme.write_bytes(original)
        assert theme.read_bytes() == original


if __name__ == "__main__":
    main()
