#!/usr/bin/env python3
"""Capture the real GPUI About page in both themes using isolated mock data.

Requires an already-built app, Xvfb, xdotool, xclip and ImageMagick. No downloads.
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
                    for route in ("sidebar", "settings"):
                        env = dict(os.environ, DISPLAY=":" + number.decode(),
                                   TINGSKRIVER_APPEARANCE=theme,
                                   TINGSKRIVER_DATA_DIR=str(Path(temporary) / (theme + "-" + route)),
                                   XDG_DATA_HOME=str(Path(temporary) / theme / "data"),
                                   XDG_CONFIG_HOME=str(Path(temporary) / theme / "config"),
                                   XDG_CACHE_HOME=str(Path(temporary) / theme / "cache"),
                                   TINGSKRIVER_UPDATE_FEED="off",
                                   LIBGL_ALWAYS_SOFTWARE="1")
                        app = subprocess.Popen([str(binary), "--page", "settings"], env=env,
                                               stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
                        try:
                            window = subprocess.check_output(
                                ["xdotool", "search", "--sync", "--onlyvisible", "--name", "^Tingskriver$"],
                                env=env, timeout=30).decode().splitlines()[0]
                            time.sleep(3)
                            subprocess.run(["xdotool", "windowfocus", "--sync", window], env=env, check=True, timeout=10)
                            # Exercise both independent entry points from Settings.
                            open_keys = ["Tab", "Return"] if route == "sidebar" else ["Tab", "Tab", "Return"]
                            copy_keys = ["Tab", "Return"] if route == "sidebar" else ["Return"]
                            subprocess.run(["xdotool", "key", "--delay", "200", *open_keys], env=env, check=True, timeout=10)
                            time.sleep(0.5)
                            # Settings transfers focus to Copy when its own button unmounts.
                            subprocess.run(["xdotool", "key", "--delay", "200", *copy_keys], env=env, check=True, timeout=10)
                            time.sleep(0.5)
                            report = subprocess.check_output(["xclip", "-selection", "clipboard", "-o"], env=env, timeout=5).decode()
                            assert report.startswith("Tingskriver "), "Keyboard navigation or copy failed"
                            assert "GPU-status:" in report and "Whisper:" in report and "Parakeet:" in report
                            assert "@" not in report and "/home/" not in report and "C:\\Users" not in report
                            if app.poll() is not None:
                                raise RuntimeError("App exited before capture")
                            subprocess.run(["import", "-window", window,
                                            str(destination / (theme + ".png"))], env=env, check=True, timeout=10)
                        finally:
                            app.terminate()
                            app.wait(timeout=10)
            finally:
                display.terminate()
                display.wait(timeout=10)
    print("About keyboard routes: 4 passed")
    # Both captures must exist, be readable, and show distinct themes.
    for theme in ("light", "dark"):
        subprocess.run(["identify", str(destination / (theme + ".png"))], check=True, timeout=10)
    if (destination / "light.png").read_bytes() == (destination / "dark.png").read_bytes():
        raise RuntimeError("Light and dark captures are identical")


if __name__ == "__main__":
    capture(Path(sys.argv[1]).resolve(), Path(sys.argv[2]).resolve())
