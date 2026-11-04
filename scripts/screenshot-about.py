#!/usr/bin/env python3
"""Capture the real GPUI About page in both themes using isolated mock data.

Requires an already-built app, Xvfb, xdotool and ImageMagick. No downloads.
Usage: python3 scripts/screenshot-about.py target/debug/tingskriver-app /tmp/about-shots
"""
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time


def capture(binary, destination):
    destination.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="tingskriver-about-") as temporary:
        with tempfile.TemporaryFile() as display_file:
            display = subprocess.Popen(
                ["Xvfb", "-displayfd", str(display_file.fileno()), "-screen", "0", "1280x1000x24"],
                pass_fds=(display_file.fileno(),), stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
            )
            try:
                deadline = time.monotonic() + 10
                number = b""
                while not number and time.monotonic() < deadline:
                    display_file.seek(0)
                    number = display_file.read().strip()
                    time.sleep(0.1)
                if not number:
                    raise RuntimeError("Xvfb did not become ready")
                for theme in ("light", "dark"):
                    env = dict(os.environ, DISPLAY=":" + number.decode(),
                               TINGSKRIVER_APPEARANCE=theme,
                               TINGSKRIVER_DATA_DIR=str(Path(temporary) / theme),
                               LIBGL_ALWAYS_SOFTWARE="1")
                    app = subprocess.Popen([str(binary), "--page", "about"], env=env,
                                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
                    try:
                        window = subprocess.check_output(
                            ["xdotool", "search", "--sync", "--onlyvisible", "--name", "^Tingskriver$"],
                            env=env, timeout=30).decode().splitlines()[0]
                        time.sleep(3)
                        if app.poll() is not None:
                            raise RuntimeError("App exited before capture")
                        subprocess.run(["import", "-window", window,
                                        str(destination / (theme + ".png"))], env=env, check=True)
                    finally:
                        app.terminate()
                        app.wait(timeout=10)
            finally:
                display.terminate()
                display.wait(timeout=10)
    # Both captures must exist, be readable, and show distinct themes.
    for theme in ("light", "dark"):
        subprocess.run(["identify", str(destination / (theme + ".png"))], check=True)
    if (destination / "light.png").read_bytes() == (destination / "dark.png").read_bytes():
        raise RuntimeError("Light and dark captures are identical")


if __name__ == "__main__":
    capture(Path(sys.argv[1]).resolve(), Path(sys.argv[2]).resolve())
