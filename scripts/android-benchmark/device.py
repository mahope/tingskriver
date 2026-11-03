#!/usr/bin/env python3
"""Invoke a separately audited, persistent arm64 benchmark driver over USB adb.

The driver is not shipped yet. See docs/android/BENCHMARK.md for its contract.
No app installation, network download, model build or cloud fallback occurs here.
"""
import argparse
import json
from pathlib import Path
import subprocess


def adb(*args):
    return subprocess.run(['adb', *args], check=True, capture_output=True,
                          text=True, timeout=7200).stdout


def collect(output):
    state = adb('get-state').strip()
    if state != 'device':
        raise ValueError('A single authorized physical USB device is required')
    abi = adb('shell', 'getprop ro.product.cpu.abi').strip()
    if abi != 'arm64-v8a':
        raise ValueError('arm64-v8a required')
    if adb('shell', 'getprop ro.kernel.qemu').strip() == '1':
        raise ValueError('Emulators cannot supply acceptance measurements')
    # Fixed paths prevent local input becoming Android shell source.
    adb('shell', '/data/local/tmp/tingskriver-bench/driver --manifest '
        '/data/local/tmp/tingskriver-bench/manifest.json --output '
        '/data/local/tmp/tingskriver-bench/results.json')
    raw = adb('shell', 'cat /data/local/tmp/tingskriver-bench/results.json')
    data = json.loads(raw)
    hardware = adb('shell', 'getprop ro.product.model').strip()
    os_version = adb('shell', 'getprop ro.build.version.release').strip()
    for row in data['runs']:
        row.update(hardware=hardware, os=os_version, abi=abi)
    output.write_text(json.dumps(data, ensure_ascii=False, indent=2) + '\n')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    try:
        collect(args.output)
    except (subprocess.SubprocessError, ValueError, KeyError, OSError):
        parser.exit(2, 'Collection failed; check USB device and local driver setup\n')
