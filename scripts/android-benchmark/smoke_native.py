#!/usr/bin/env python3
"""Exercise the real native executable's safe failure paths without models/audio."""
from pathlib import Path
import subprocess
import sys
import tempfile

with tempfile.TemporaryDirectory() as directory:
    pcm = Path(directory) / 'synthetic.pcm'
    for samples in (b'bad PCM', b'\0\0' * 240000):
        pcm.write_bytes(samples)
        result = subprocess.run([sys.argv[1], str(Path(directory) / 'absent.bin'), str(pcm)],
                                capture_output=True, timeout=30)
        assert result.returncode == 2, 'native driver must reject invalid input/model'
        assert result.stdout == b'', 'failed observation must never produce measurements'
print('Native invalid PCM and missing-model controls passed')
