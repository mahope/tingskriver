import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import device
import lock_corpus
import wave


class DeviceTests(unittest.TestCase):
    def test_reject_emulator(self):
        with patch('device.adb', side_effect=['device', 'arm64-v8a', '1']):
            with self.assertRaises(ValueError):
                device.collect(Path('/unused'), None, None, None, None, 'midrange', 'a'*40)

    def test_corpus_lock_validates_audio_and_hashes(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            recipe = json.loads(Path(lock_corpus.__file__).with_name('corpus.json').read_text())
            for clip in recipe['clips']:
                with wave.open(str(root / (clip['id'] + '.wav')), 'wb') as audio:
                    audio.setparams((1, 2, 16000, 0, 'NONE', 'not compressed'))
                    audio.writeframes(b'\x00\x00' * 240000)
            locked = lock_corpus.lock(root, 'synthetic test consent')
            self.assertEqual(len(locked['clips'][0]['sha256']), 64)
            with self.assertRaises(ValueError):
                lock_corpus.lock(root, '')
            (root / 'quiet-01.wav').write_bytes(b'not a wave file')
            with self.assertRaises(wave.Error):
                lock_corpus.lock(root, 'test')
