#!/usr/bin/env python3
"""Capture all seven update states in the real GPUI app, offline, in both themes.

Usage: python3 scripts/screenshot-updates.py target/debug/tingskriver-app /tmp/update-shots
Requires Xvfb, xdotool, ImageMagick and a debug build; no downloads.
"""
import os
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import time

STATES = ("idle", "checking", "available", "downloading", "ready", "error", "blocked")


def capture(binary, destination):
    destination.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="tingskriver-updates-") as temporary:
        with tempfile.TemporaryFile() as display_file:
            display = subprocess.Popen(
                ["Xvfb", "-displayfd", str(display_file.fileno()), "-screen", "0", "1280x1400x24"],
                pass_fds=(display_file.fileno(),), stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
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
                    for state in STATES:
                        env = dict(os.environ, DISPLAY=":" + number.decode(),
                                   TINGSKRIVER_APPEARANCE=theme,
                                   TINGSKRIVER_DATA_DIR=str(Path(temporary) / (theme + state)),
                                   XDG_DATA_HOME=str(Path(temporary) / "data"),
                                   XDG_CONFIG_HOME=str(Path(temporary) / "config"),
                                   XDG_CACHE_HOME=str(Path(temporary) / "cache"),
                                   TINGSKRIVER_UPDATE_FEED="off", TINGSKRIVER_MOCK_UPDATE=state,
                                   LIBGL_ALWAYS_SOFTWARE="1")
                        app = subprocess.Popen([str(binary), "--page", "about"], env=env,
                                               stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
                        try:
                            window = subprocess.check_output(
                                ["xdotool", "search", "--sync", "--onlyvisible", "--name", "^Tingskriver$"],
                                env=env, timeout=30).decode().splitlines()[0]
                            subprocess.run(["xdotool", "windowsize", "--sync", window, "1200", "1300"],
                                           env=env, check=True, timeout=10)
                            time.sleep(2)
                            if app.poll() is not None:
                                raise RuntimeError("App exited before capture")
                            subprocess.run(["import", "-window", window,
                                            str(destination / (theme + "-" + state + ".png"))],
                                           env=env, check=True, timeout=10)
                        finally:
                            app.terminate()
                            try:
                                app.wait(timeout=10)
                            except subprocess.TimeoutExpired:
                                app.kill()
                                app.wait(timeout=10)
                    settings_dir = Path(temporary) / (theme + "-settings")
                    env.pop("TINGSKRIVER_MOCK_UPDATE")
                    env["TINGSKRIVER_DATA_DIR"] = str(settings_dir)
                    for restart in (False, True):
                        app = subprocess.Popen([str(binary), "--page", "settings"], env=env,
                                               stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
                        try:
                            window = subprocess.check_output(
                                ["xdotool", "search", "--sync", "--onlyvisible", "--name", "^Tingskriver$"],
                                env=env, timeout=30).decode().splitlines()[0]
                            subprocess.run(["xdotool", "windowfocus", "--sync", window], env=env, check=True, timeout=10)
                            time.sleep(1)
                            # Sidebar About -> Settings About -> updates toggle.
                            subprocess.run(["xdotool", "key", "--delay", "200", "Tab", "Tab", "Tab", "Return"],
                                           env=env, check=True, timeout=10)
                            time.sleep(0.5)
                            saved = json.loads((settings_dir / "updates.json").read_text())["preferences"]
                            assert saved["enabled"] == restart, "Keyboard toggle or persistence failed"
                            if restart:
                                subprocess.run(["xdotool", "key", "--delay", "200", "Tab", "Return"],
                                               env=env, check=True, timeout=10)
                                time.sleep(0.5)
                                saved = json.loads((settings_dir / "updates.json").read_text())["preferences"]
                                assert saved["weekly"], "Frequency control did not respond to keyboard"
                            subprocess.run(["import", "-window", window,
                                            str(destination / (theme + "-settings-" + str(restart) + ".png"))],
                                           env=env, check=True, timeout=10)
                        finally:
                            app.terminate()
                            try:
                                app.wait(timeout=10)
                            except subprocess.TimeoutExpired:
                                app.kill()
                                app.wait(timeout=10)
            finally:
                display.terminate()
                display.wait(timeout=10)
    for theme in ("light", "dark"):
        pictures = [(destination / (theme + "-" + state + ".png")).read_bytes() for state in STATES]
        assert len(set(pictures)) == len(STATES), "Update states rendered identically"
    print("Captured 14 distinct update screenshots; keyboard settings survive restart in both themes")


if __name__ == "__main__":
    capture(Path(sys.argv[1]).resolve(), Path(sys.argv[2]).resolve())
