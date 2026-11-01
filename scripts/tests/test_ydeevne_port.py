"""CLI regression tests using synthetic, never reference-machine, measurements."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / 'ydeevne-port.py'


class GateTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.baseline = self.root / 'baseline.json'
        self.measurement = self.root / 'measurement.md'
        self.write(self.baseline, {'resultater': {'varmstart_ms': 100}})

    def write(self, path, data):
        path.write_text(json.dumps(data), encoding='utf-8')

    def run_gate(self, value=100, *args):
        self.write(self.measurement, {'resultater': {'varmstart_ms': value}})
        return self.invoke(*args)

    def invoke(self, *args):
        return subprocess.run([sys.executable, str(SCRIPT), str(self.measurement),
                               '--baseline', str(self.baseline), *args],
                              capture_output=True, text=True, encoding='utf-8',
                              env={**os.environ, 'PYTHONIOENCODING': 'ascii'})

    def test_manipulated_baseline_copy(self):
        data = json.loads(self.baseline.read_text())
        data['resultater']['varmstart_ms'] *= 1.25
        self.write(self.measurement, data)
        self.assertEqual(self.invoke().returncode, 1)

    def test_relative_boundaries(self):
        for value, code in [(125, 1), (115, 0), (120, 0), (75, 0)]:
            with self.subTest(value=value):
                result = self.run_gate(value)
                self.assertEqual(result.returncode, code, result.stderr)

    def test_missing_baseline(self):
        self.baseline.unlink()
        self.assertEqual(self.run_gate().returncode, 2)
        self.assertEqual(self.run_gate(100, '--tillad-uden-baseline').returncode, 0)

    def test_unmeasured_and_no_common_keys(self):
        for data in [{'status': 'ikke_maalt', 'resultater': {}},
                     {'resultater': {'koldstart_ms': 100}}]:
            self.write(self.baseline, data)
            self.assertEqual(self.run_gate().returncode, 2)

    def test_unknown_key_warns(self):
        self.write(self.measurement, {'resultater': {'varmstart_ms': 100, 'unknown': 2}})
        result = self.invoke()
        self.assertEqual(result.returncode, 0)
        self.assertIn('Advarsel', result.stderr)
        self.assertIn('unknown', result.stderr)

    def test_invalid_json(self):
        self.measurement.write_text('{ broken', encoding='utf-8')
        result = self.invoke('--tillad-uden-baseline')
        self.assertEqual(result.returncode, 2)
        self.assertIn('JSON', result.stderr)
        self.assertNotIn('Traceback', result.stderr)

    def test_markdown_json_block(self):
        self.measurement.write_text('## Maskine\n\n## Maskinlæsbart\n\n```json\n'
                                    '{"resultater":{"varmstart_ms":115}}\n```\nSlut',
                                    encoding='utf-8-sig')
        self.assertEqual(self.invoke().returncode, 0)

    def test_partial_measurement(self):
        self.write(self.baseline, {'resultater': {'varmstart_ms': 100, 'koldstart_ms': 200}})
        result = self.run_gate()
        self.assertEqual(result.returncode, 0)
        self.assertIn('ikke målt', result.stdout)

    def test_invalid_values_fail_closed(self):
        for value in [-1, True, '100', float('nan'), float('inf')]:
            with self.subTest(value=value):
                self.assertEqual(self.run_gate(value, '--tillad-uden-baseline').returncode, 2)

    def test_zero_baseline(self):
        self.write(self.baseline, {'resultater': {'varmstart_ms': 0}})
        self.assertEqual(self.run_gate(0).returncode, 0)
        self.assertEqual(self.run_gate(1).returncode, 1)

    def test_ci_scope_and_threshold(self):
        self.write(self.baseline, {'resultater': {'diktering_local_transcribe_p50_ms': 100,
                                                'varmstart_ms': 100}})
        for value, expected in [(140, 0), (141, 1)]:
            self.write(self.measurement, {'resultater': {
                'diktering_local_transcribe_p50_ms': value, 'varmstart_ms': 999}})
            self.assertEqual(self.invoke('--profil', 'ci').returncode, expected)

    def test_allow_flag_does_not_hide_regression_or_bad_baseline(self):
        self.assertEqual(self.run_gate(125, '--tillad-uden-baseline').returncode, 1)
        self.baseline.write_text('broken', encoding='utf-8')
        self.assertEqual(self.run_gate(100, '--tillad-uden-baseline').returncode, 2)

    def test_null_values_and_ci_without_eligible_measurements(self):
        self.write(self.measurement, {'resultater': {'varmstart_ms': None}})
        self.assertEqual(self.invoke().returncode, 2)
        self.assertEqual(self.run_gate(100, '--profil', 'ci').returncode, 2)

    def test_provider_keys(self):
        self.write(self.baseline, {'resultater': {'diktering_syv_start_p50_ms': 100}})
        self.write(self.measurement, {'resultater': {'diktering_syv_start_p50_ms': 125}})
        result = self.invoke()
        self.assertEqual(result.returncode, 1)
        self.assertNotIn('Advarsel', result.stderr)

    def test_bad_schema_and_multiple_json_blocks(self):
        for text in ['[]', '{}', '{"resultater": []}',
                     '```json\n{"resultater":{}}\n```\n'
                     '```json\n{"resultater":{}}\n```']:
            self.measurement.write_text(text, encoding='utf-8')
            result = self.invoke('--tillad-uden-baseline')
            self.assertEqual(result.returncode, 2)
            self.assertNotIn('Traceback', result.stderr)

    def test_readme_requires_markers(self):
        readme = self.root / 'README.md'
        readme.write_text('Unrelated documentation', encoding='utf-8')
        result = self.invoke('--skriv-readme', '--readme', str(readme))
        self.assertEqual(result.returncode, 2)
        self.assertEqual(readme.read_text(), 'Unrelated documentation')

    def test_readme_sync(self):
        readme = self.root / 'README.md'
        readme.write_text('Before\n<!-- ydeevne:start -->\nstale\n<!-- ydeevne:slut -->\nAfter\n')
        args = ['--readme', str(readme)]
        self.assertEqual(self.invoke('--tjek-readme', *args).returncode, 1)
        self.assertEqual(self.invoke('--skriv-readme', *args).returncode, 0)
        self.assertEqual(self.invoke('--tjek-readme', *args).returncode, 0)
        text = readme.read_text()
        self.assertTrue(text.startswith('Before\n'))
        self.assertTrue(text.endswith('\nAfter\n'))
        self.assertIn('varmstart_ms', text)
        self.write(self.baseline, {'resultater': {'varmstart_ms': 101}})
        self.assertEqual(self.invoke('--tjek-readme', *args).returncode, 1)


if __name__ == '__main__':
    unittest.main()
