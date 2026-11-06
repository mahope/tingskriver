#!/usr/bin/env python3
"""Render the real GPUI settings with synthetic data on Linux/Xvfb.

Build first:
cargo build --locked -p tingskriver-app -p tingskriver-mcp -p tingskriver-core \
    --bin tingskriver-app --bin tingskriver-mcp --example mcp_preview
Then run this script with --update to review new baselines, or without it to
compare using FFmpeg SSIM. No downloaded browser or Python packages are used.
"""
import argparse
import ctypes
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
BASELINES = ROOT / "docs/screenshots/mcp-native"


def run(*args, **kwargs):
    return subprocess.run(args, check=True, **kwargs)


def stop(process):
    if process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=10)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--update", action="store_true")
    parser.add_argument("--output", type=Path, default=Path("/tmp/tingskriver-mcp-screenshots"))
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    if args.update:
        BASELINES.mkdir(parents=True, exist_ok=True)
    # Copy executables so screenshots never contain a developer's private path.
    with tempfile.TemporaryDirectory(prefix="tingskriver-mcp-preview-", dir="/var/tmp") as temp:
        directory = Path(temp)
        for binary in ["tingskriver-app", "tingskriver-mcp"]:
            shutil.copy2(ROOT / "target/debug" / binary, directory / binary)
        with (args.output / "xvfb.log").open("w") as log:
            xvfb = subprocess.Popen(["Xvfb", "-displayfd", "1", "-screen", "0", "1440x1000x24", "-nolisten", "tcp"], stdout=subprocess.PIPE, stderr=log, text=True)
            try:
                display = ":" + xvfb.stdout.readline().strip()
                assert re.fullmatch(r":\d+", display), "Xvfb did not allocate a display"
                env = dict(os.environ, DISPLAY=display)
                x11 = ctypes.CDLL("libX11.so.6")
                xtst = ctypes.CDLL("libXtst.so.6")
                x11.XOpenDisplay.restype = ctypes.c_void_p
                connection = x11.XOpenDisplay(display.encode())
                assert connection, "Cannot connect to Xvfb"
                x11.XDefaultRootWindow.argtypes = [ctypes.c_void_p]
                x11.XDefaultRootWindow.restype = ctypes.c_ulong
                root = x11.XDefaultRootWindow(connection)
                x11.XWarpPointer.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_ulong, ctypes.c_int, ctypes.c_int, ctypes.c_uint, ctypes.c_uint, ctypes.c_int, ctypes.c_int]
                x11.XFlush.argtypes = [ctypes.c_void_p]
                x11.XCloseDisplay.argtypes = [ctypes.c_void_p]
                xtst.XTestFakeButtonEvent.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_int, ctypes.c_ulong]
                for mode in ["empty", "enabled", "error", "missing"]:
                    if mode == "missing":
                        (directory / "tingskriver-mcp").rename(directory / "server-not-installed")
                    data = directory / mode
                    run(str(ROOT / "target/debug/examples/mcp_preview"), str(data), mode, stdout=subprocess.DEVNULL)
                    # Fixed bounds make the real scrolling settings deterministic.
                    (data / "gpui-window.json").write_text('{"x":0,"y":0,"width":1440,"height":1000,"maximized":false}')
                    for appearance in ["light", "dark"]:
                        app_env = dict(env, TINGSKRIVER_DATA_DIR=str(data), TINGSKRIVER_APPEARANCE=appearance, XDG_CONFIG_HOME=str(directory / "config"), XDG_CACHE_HOME=str(directory / "cache"))
                        with (args.output / f"{mode}-{appearance}.log").open("w") as app_log:
                            app = subprocess.Popen([str(directory / "tingskriver-app"), "--page", "settings"], env=app_env, stdout=app_log, stderr=app_log)
                            try:
                                time.sleep(4)
                                assert app.poll() is None, "Native app exited; inspect the fixture log"
                                for section in ["controls", "log"]:
                                    if section == "log":
                                        x11.XWarpPointer(connection, 0, root, 0, 0, 0, 0, 1000, 700)
                                        for _ in range(6):
                                            xtst.XTestFakeButtonEvent(connection, 5, 1, 0)
                                            xtst.XTestFakeButtonEvent(connection, 5, 0, 0)
                                        x11.XFlush(connection)
                                        time.sleep(1)
                                    name = f"{mode}-{appearance}-{section}.png"
                                    output = args.output / name
                                    run("ffmpeg", "-loglevel", "error", "-y", "-f", "x11grab", "-video_size", "1440x1000", "-draw_mouse", "0", "-i", display, "-frames:v", "1", str(output))
                                    assert output.stat().st_size > 15000, "Screenshot looks empty"
                                    baseline = BASELINES / name
                                    if args.update:
                                        shutil.copy2(output, baseline)
                                    else:
                                        comparison = subprocess.run(["ffmpeg", "-i", str(baseline), "-i", str(output), "-lavfi", "ssim", "-f", "null", "-"], capture_output=True, text=True, check=True)
                                        score = re.search(r"All:([0-9.]+)", comparison.stderr)
                                        assert score and float(score[1]) >= 0.985, f"Visual regression: {name}"
                                    print(name, flush=True)
                            finally:
                                stop(app)
                x11.XCloseDisplay(connection)
            finally:
                stop(xvfb)


if __name__ == "__main__":
    main()
