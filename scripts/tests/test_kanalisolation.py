"""Black-box checks for the kanalisolation tool; opt in with KANALISOLATION_BINARY."""
import json
import math
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
import wave

RATE = 48000
THRESHOLDS = Path(__file__).resolve().parents[2] / 'docs' / 'kanalisolation-graenser.json'
PHASES = ['both:0:3', 'mic:3:6', 'system:6:9']


def write_wav(path, samples):
    with wave.open(str(path), 'wb') as wav:
        wav.setnchannels(1)
        wav.setsampwidth(2)
        wav.setframerate(RATE)
        wav.writeframes(b''.join(int(max(-1, min(1, s)) * 32767).to_bytes(2, 'little', signed=True) for s in samples))


def tracks(leak_into_mic=0.0, leak_into_system=0.0):
    limits = json.loads(THRESHOLDS.read_text())
    mic, system = [], []
    for i in range(RATE * 9):
        phase = i // (RATE * 3)
        t = i / RATE
        m = 0.25 * math.sin(2 * math.pi * limits['mic_hz'] * t) if phase <= 1 else 0.0
        s = 0.25 * math.sin(2 * math.pi * limits['system_hz'] * t) if phase != 1 else 0.0
        mic.append(m + s * leak_into_mic)
        system.append(s + m * leak_into_system)
    return mic, system


@unittest.skipUnless(os.environ.get('KANALISOLATION_BINARY'), 'build the kanalisolation example and set KANALISOLATION_BINARY')
class Kanalisolation(unittest.TestCase):
    def run_tool(self, leak_into_mic=0.0, leak_into_system=0.0, extra=()):
        binary = str(Path(os.environ['KANALISOLATION_BINARY']).resolve())
        with tempfile.TemporaryDirectory() as directory:
            mic, system = tracks(leak_into_mic, leak_into_system)
            write_wav(Path(directory) / 'mic.wav', mic)
            write_wav(Path(directory) / 'system.wav', system)
            return subprocess.run([binary, directory + '/mic.wav', directory + '/system.wav', str(THRESHOLDS), *PHASES, *extra],
                                  capture_output=True, text=True)

    def test_clean_tracks_pass(self):
        result = self.run_tool()
        self.assertEqual(result.returncode, 0, result.stdout)
        self.assertIn('PASS', result.stdout)

    def test_leakage_fails_in_both_directions(self):
        for leaks in [(0.3, 0.0), (0.0, 0.3)]:
            result = self.run_tool(*leaks)
            self.assertEqual(result.returncode, 1, result.stdout)
            self.assertIn('FAIL', result.stdout)

    def test_json_report_and_bad_input(self):
        report = json.loads(self.run_tool(extra=['--json']).stdout)
        self.assertEqual(report['failures'], [])
        self.assertEqual(len(report['phases']), 3)
        binary = str(Path(os.environ['KANALISOLATION_BINARY']).resolve())
        self.assertEqual(subprocess.run([binary, 'missing.wav', 'missing.wav', str(THRESHOLDS), 'both:0:3'],
                                        capture_output=True).returncode, 2)


if __name__ == '__main__':
    unittest.main()
