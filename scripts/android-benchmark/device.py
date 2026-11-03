#!/usr/bin/env python3
"""Collect offline native Whisper observations on a physical arm64 phone."""
import argparse
import hashlib
import json
import math
from pathlib import Path
import random
import re
import subprocess
import tempfile
import wave

ROOT = '/data/local/tmp/tingskriver-bench'
MODELS = ('base', 'small', 'small-q5_1')


def adb(*args):
    return subprocess.run(['adb', *args], check=True, capture_output=True,
                          text=True, timeout=7200).stdout


def parse_observation(raw):
    fields = raw.split('\n', 3)
    values = [float(x) for x in fields[:3]]
    if len(fields) != 4 or any(not math.isfinite(x) or x < 0 for x in values):
        raise ValueError('Invalid native observation')
    return dict(zip(('cold_ms', 'warm_ms', 'peak_pss_mb', 'hypothesis'),
                    [*values, fields[3]]))


def thermal():
    battery = adb('shell', 'dumpsys battery')
    temperature = re.search(r'^\s*temperature:\s*(\d+)\s*$', battery, re.M)
    status = re.search(r'Thermal Status:\s*([0-6])\b',
                       adb('shell', 'dumpsys thermalservice'))
    if not temperature or not status:
        raise ValueError('Thermal readings unavailable')
    return int(temperature[1]) / 10, int(status[1])


def stage(path, name):
    digest = hashlib.sha256(path.read_bytes()).hexdigest()
    adb('push', str(path), f'{ROOT}/{name}')
    remote = adb('shell', f'sha256sum {ROOT}/{name}').split()[0]
    if remote != digest:
        raise ValueError('Device file hash mismatch')
    return digest


def collect(output, manifest, audio_dir, model_dir, driver, tier, revision,
            cooldown_s=60):
    if output.exists() or not re.fullmatch(r'[0-9a-f]{40}', revision):
        raise ValueError('New output and full engine revision required')
    state = adb('get-state').strip()
    if state != 'device':
        raise ValueError('A single authorized physical USB device is required')
    abi = adb('shell', 'getprop ro.product.cpu.abi').strip()
    if abi != 'arm64-v8a':
        raise ValueError('arm64-v8a required')
    if adb('shell', 'getprop ro.kernel.qemu').strip() == '1':
        raise ValueError('Emulators cannot supply acceptance measurements')
    metadata = json.loads(driver.with_suffix('.json').read_text())
    if (metadata['engine_revision'] != revision or metadata['abi'] != 'arm64-v8a'
            or metadata['driver_sha256'] != hashlib.sha256(driver.read_bytes()).hexdigest()):
        raise ValueError('Driver build provenance mismatch')
    hardware = adb('shell', 'getprop ro.product.model').strip()
    os_version = adb('shell', 'getprop ro.build.version.release').strip()
    data = json.loads(manifest.read_text())
    if not data.get('consent') or not data.get('clips'):
        raise ValueError('Locked consented corpus required')
    adb('shell', f'mkdir -p {ROOT}')
    driver_hash = stage(driver, 'driver')
    adb('shell', f'chmod 700 {ROOT}/driver')
    models = {}
    for model in MODELS:
        path = model_dir / f'ggml-{model}.bin'
        models[model] = (stage(path, f'{model}.bin'), path.stat().st_size)
    with tempfile.TemporaryDirectory() as directory:
        for index, clip in enumerate(data['clips']):
            path = audio_dir / (clip['id'] + '.wav')
            if not re.fullmatch(r'[a-z0-9-]+', clip['id']):
                raise ValueError('Invalid clip identifier')
            if hashlib.sha256(path.read_bytes()).hexdigest() != clip['sha256']:
                raise ValueError('Audio hash mismatch')
            with wave.open(str(path), 'rb') as audio:
                if (audio.getnchannels(), audio.getsampwidth(), audio.getframerate(),
                    audio.getnframes(), audio.getcomptype()) != (1, 2, 16000, 240000, 'NONE'):
                    raise ValueError('Expected 15 seconds mono PCM16 at 16 kHz')
                pcm = Path(directory) / 'clip.pcm'
                pcm.write_bytes(audio.readframes(240000))
            stage(pcm, f'clip-{index}.pcm')
    data.update(build=metadata, engine_revision=revision, driver_sha256=driver_hash, runs=[],
                method=dict(threads=4, order_seed=133, pss_interval_ms=50,
                            cooldown_s=cooldown_s, energy='not collected'))
    schedule = [(model, index, repeat) for model in MODELS
                for index in range(len(data['clips'])) for repeat in range(20)]
    random.Random(133).shuffle(schedule)
    for model, index, repeat in schedule:
        # Foreground device wait; never a detached host process.
        adb('shell', f'sleep {cooldown_s}')
        before, _ = thermal()
        raw = adb('shell', f'{ROOT}/driver {ROOT}/{model}.bin {ROOT}/clip-{index}.pcm')
        after, status = thermal()
        row = parse_observation(raw)
        row.update(tier=tier, hardware=hardware, os=os_version, abi=abi,
                   model=model, model_sha256=models[model][0], model_bytes=models[model][1],
                   clip=data['clips'][index]['id'], repeat=repeat,
                   energy_mwh=None, temperature_start_c=before,
                   temperature_end_c=after, thermal_status=status)
        data['runs'].append(row)
        # Preserve completed observations on USB failure; scorer rejects incomplete runs.
        output.write_text(json.dumps(data, ensure_ascii=False, indent=2) + '\n')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('output', type=Path)
    parser.add_argument('--manifest', type=Path, required=True)
    parser.add_argument('--audio-dir', type=Path, required=True)
    parser.add_argument('--model-dir', type=Path, required=True)
    parser.add_argument('--driver', type=Path, required=True)
    parser.add_argument('--tier', choices=('midrange', 'recent'), required=True)
    parser.add_argument('--engine-revision', required=True)
    args = parser.parse_args()
    try:
        collect(args.output, args.manifest, args.audio_dir, args.model_dir,
                args.driver, args.tier, args.engine_revision)
    except (subprocess.SubprocessError, ValueError, KeyError, OSError, wave.Error):
        parser.exit(2, 'Collection failed; check USB device and local benchmark setup\n')
