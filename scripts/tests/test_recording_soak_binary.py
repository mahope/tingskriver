"""Black-box checks for the sample verifier; opt in with RECORDING_SOAK_BINARY."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


@unittest.skipUnless(os.environ.get('RECORDING_SOAK_BINARY'), 'build recording_soak and set RECORDING_SOAK_BINARY')
class SampleVerifier(unittest.TestCase):
    def test_detects_zero_gap_and_repairs_stale_header(self):
        binary = str(Path(os.environ['RECORDING_SOAK_BINARY']).resolve())
        with tempfile.TemporaryDirectory() as directory:
            folder = Path(directory)
            subprocess.run([binary, 'record', directory, '4'], check=True)
            subprocess.run([binary, 'verify', directory, '4'], check=True)
            segment = next((folder / '.checkpoints').glob('system_*.wav'))
            # A process can die after writing PCM but before updating its header.
            with segment.open('r+b') as stream:
                stream.seek(40)
                stream.write((0).to_bytes(4, 'little'))
            subprocess.run([binary, 'verify', directory, '4'], check=True)
            with segment.open('r+b') as stream:
                stream.seek(44 + 48_000 * 2)
                stream.write(bytes(480 * 2))  # 10 ms of missing system audio
            result = subprocess.run([binary, 'verify', directory, '4'], capture_output=True)
            self.assertNotEqual(result.returncode, 0, 'zero-gap regression was missed')
            self.assertIn(b'marker/gap', result.stderr)

    def test_rejects_repeated_whole_seconds(self):
        binary = str(Path(os.environ['RECORDING_SOAK_BINARY']).resolve())
        with tempfile.TemporaryDirectory() as directory:
            subprocess.run([binary, 'record', directory, '4'], check=True)
            segment = next((Path(directory) / '.checkpoints').glob('system_*.wav'))
            original = segment.read_bytes()
            size = 48_000 * 2
            first, second = original[44:44 + size], original[44 + size:44 + 2 * size]
            for corruption in [first * 4, second + first + original[44 + 2 * size:]]:
                with self.subTest(corruption=corruption[:8]):
                    segment.write_bytes(original[:44] + corruption)
                    result = subprocess.run([binary, 'verify', directory, '4'], capture_output=True)
                    self.assertNotEqual(result.returncode, 0, 'whole-second corruption was missed')
                    self.assertIn(b'marker/gap', result.stderr)
            segment.write_bytes(original)

    def test_finalizes_playable_audio(self):
        binary = str(Path(os.environ['RECORDING_SOAK_BINARY']).resolve())
        with tempfile.TemporaryDirectory() as directory:
            subprocess.run([binary, 'record', directory, '4'], check=True)
            subprocess.run([binary, 'verify', directory, '4'], check=True)
            subprocess.run([binary, 'finish', directory, '4'], check=True)
            self.assertTrue((Path(directory) / 'audio.mp4').is_file())
