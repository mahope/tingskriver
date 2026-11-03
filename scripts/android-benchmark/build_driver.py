#!/usr/bin/env python3
"""Build from an already audited local whisper.cpp checkout and installed NDK."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess


def build(source, ndk, revision, output):
    if not re.fullmatch('[0-9a-f]{40}', revision) or output.exists():
        raise ValueError('Full revision and new build directory required')
    actual = subprocess.check_output(['git', '-C', str(source), 'rev-parse', 'HEAD'], text=True).strip()
    dirty = subprocess.check_output(['git', '-C', str(source), 'status', '--porcelain'], text=True)
    if actual != revision or dirty:
        raise ValueError('Engine checkout must be clean at the specified revision')
    toolchain = ndk / 'build/cmake/android.toolchain.cmake'
    if not toolchain.is_file():
        raise ValueError('Installed Android NDK required')
    subprocess.run(['cmake', '-S', str(Path(__file__).parent / 'native'), '-B', str(output),
                    f'-DWHISPER_SOURCE={source.resolve()}', f'-DCMAKE_TOOLCHAIN_FILE={toolchain.resolve()}',
                    '-DANDROID_ABI=arm64-v8a', '-DANDROID_PLATFORM=android-28',
                    '-DANDROID_STL=c++_static', '-DCMAKE_BUILD_TYPE=Release',
                    '-DCMAKE_POLICY_VERSION_MINIMUM=3.5'], check=True)
    subprocess.run(['cmake', '--build', str(output), '--parallel', '2'], check=True)
    binary = output / 'driver'
    metadata = dict(engine_revision=revision,
                    driver_sha256=hashlib.sha256(binary.read_bytes()).hexdigest(),
                    ndk=(ndk / 'source.properties').read_text(), abi='arm64-v8a',
                    api=28, backend='CPU', threads=4, build_type='Release')
    binary.with_suffix('.json').write_text(json.dumps(metadata, indent=2) + '\n')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source', type=Path, required=True)
    parser.add_argument('--ndk', type=Path, required=True)
    parser.add_argument('--revision', required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    build(args.source, args.ndk, args.revision, args.output)
