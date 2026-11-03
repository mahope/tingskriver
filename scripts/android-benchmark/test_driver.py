from pathlib import Path
import unittest


class DriverTests(unittest.TestCase):
    def test_parse_native_observation_preserves_silence_hallucinations(self):
        import device
        row = device.parse_observation('12.5\n25.5\n40.0\n...\n')
        self.assertEqual(row, dict(cold_ms=12.5, warm_ms=25.5,
                                   peak_pss_mb=40.0, hypothesis='...\n'))

    def test_invalid_native_metrics_fail_closed(self):
        import device
        for value in ('nan', '-1', 'inf'):
            with self.assertRaises(ValueError):
                device.parse_observation(f'{value}\n20\n30\ntext')

    def test_native_driver_and_build_are_supplied(self):
        root = Path(__file__).parent
        self.assertTrue((root / 'native/driver.cpp').is_file())
        self.assertTrue((root / 'native/CMakeLists.txt').is_file())

    def test_collection_runs_native_matrix_and_preserves_missing_energy(self):
        import hashlib
        import json
        import tempfile
        import wave
        from unittest.mock import patch
        import device
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            driver = root / 'driver'
            driver.write_bytes(b'test binary')
            driver.with_suffix('.json').write_text(json.dumps(dict(
                engine_revision='a'*40, abi='arm64-v8a',
                driver_sha256=hashlib.sha256(driver.read_bytes()).hexdigest())))
            with wave.open(str(root / 'quiet.wav'), 'wb') as audio:
                audio.setparams((1, 2, 16000, 0, 'NONE', 'not compressed'))
                audio.writeframes(b'\0\0' * 240000)
            clip = dict(id='quiet', reference='test', kind='quiet', duration_s=15,
                        sha256=hashlib.sha256((root / 'quiet.wav').read_bytes()).hexdigest())
            manifest = root / 'manifest.json'
            manifest.write_text(json.dumps(dict(consent='synthetic consent', clips=[clip])))
            for model in device.MODELS:
                (root / f'ggml-{model}.bin').write_bytes(b'model')
            def adb(*args):
                command = args[-1]
                return {'get-state': 'device', 'getprop ro.product.cpu.abi': 'arm64-v8a',
                        'getprop ro.kernel.qemu': '', 'getprop ro.product.model': 'phone',
                        'getprop ro.build.version.release': '16'}.get(command,
                        '10\n20\n30\nactual text' if '/driver ' in command else '')
            output = root / 'results.json'
            with patch('device.adb', side_effect=adb) as calls, \
                 patch('device.stage', return_value='b'*64), \
                 patch('device.thermal', return_value=(25, 0)):
                device.collect(output, manifest, root, root, driver, 'midrange', 'a'*40, 0)
            data = json.loads(output.read_text())
            self.assertEqual(len(data['runs']), 60)
            self.assertEqual({r['repeat'] for r in data['runs']}, set(range(20)))
            self.assertTrue(all(r['energy_mwh'] is None for r in data['runs']))
            self.assertTrue(all(r['hypothesis'] == 'actual text' for r in data['runs']))
            self.assertEqual(sum('/driver ' in c.args[-1] for c in calls.call_args_list), 60)

    def test_native_decode_protocol_and_pcm_conversion_with_fake_engine(self):
        import subprocess
        import tempfile
        import device
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            native = Path(__file__).parent / 'native'
            subprocess.run(['c++', '-std=c++17', '-pthread', '-Wall', '-Wextra', '-Werror',
                            '-I', str(native / 'test'), str(native / 'driver.cpp'),
                            '-o', str(root / 'driver')], check=True)
            pcm = root / 'test.pcm'
            pcm.write_bytes(b'\x00\x80\x00\x40' + b'\0\0' * 239998)
            result = subprocess.run([str(root / 'driver'), 'fake-model', str(pcm)],
                                    check=True, capture_output=True, text=True, timeout=30)
            row = device.parse_observation(result.stdout)
            self.assertEqual(row['hypothesis'], '...')
            self.assertGreater(row['peak_pss_mb'], 0)
