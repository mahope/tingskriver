"""Exercise the gate against real merge bases in isolated Git repositories."""
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / 'check-delivery-schema-version.py'
spec = importlib.util.spec_from_file_location('schema_gate', SCRIPT)
gate = importlib.util.module_from_spec(spec)
sys.dont_write_bytecode = True
spec.loader.exec_module(gate)


class SchemaVersionTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.previous = Path.cwd()
        os.chdir(self.temp.name)
        self.addCleanup(os.chdir, self.previous)
        self.run_git('init', '-q')
        self.run_git('config', 'user.email', 'test@example.invalid')
        self.run_git('config', 'user.name', 'Test')
        self.path = Path('docs/schemas/delivery-v1.schema.json')
        self.path.parent.mkdir(parents=True)
        self.schema = {'$id': 'urn:delivery:1', 'type': 'object',
                       'properties': {'skemaversion': {'const': 1}}}
        self.write()
        self.run_git('add', str(self.path))
        self.run_git('commit', '-qm', 'Initial schema')
        self.base = gate.git('rev-parse', 'HEAD')

    def run_git(self, *args):
        subprocess.run(['git', *args], check=True, capture_output=True)

    def write(self):
        self.path.write_text(json.dumps(self.schema), encoding='utf-8')

    def test_unchanged_and_formatting(self):
        self.assertEqual(gate.check(self.base), [])
        self.path.write_text(json.dumps(self.schema, indent=2))
        self.assertEqual(gate.check(self.base), [])

    def test_change_without_bump_fails_even_after_commit(self):
        self.schema['required'] = ['titel']
        self.write()
        self.run_git('add', str(self.path))
        self.run_git('commit', '-qm', 'Change contract')
        self.assertEqual(len(gate.check(self.base)), 1)

    def test_id_change_requires_version_bump(self):
        for key in ['id', 'version']:
            with self.subTest(key=key):
                if key == 'id':
                    self.schema['$id'] = 'urn:delivery:2'
                else:
                    self.schema['$id'] = 'urn:delivery:1'
                    self.schema['properties']['skemaversion']['const'] = 2
                self.write()
                self.assertEqual(len(gate.check(self.base)), 1 if key == 'id' else 0)

    def test_removal_and_downgrade_are_not_bumps(self):
        for version in [None, 0, True]:
            self.schema.pop('$id', None)
            self.schema['properties']['skemaversion']['const'] = version
            self.write()
            self.assertEqual(len(gate.check(self.base)), 1)

    def test_new_filename_preserves_old_contract(self):
        new = self.path.with_name('delivery-v2.schema.json')
        new.write_text(json.dumps(self.schema))
        self.assertEqual(len(gate.check(self.base)), 1)
        self.schema['properties']['skemaversion']['const'] = 2
        new.write_text(json.dumps(self.schema))
        self.assertEqual(gate.check(self.base), [])
        self.path.unlink()
        self.assertEqual(len(gate.check(self.base)), 1)

    def test_missing_base_fails_closed(self):
        result = subprocess.run([sys.executable, str(SCRIPT), '--base', 'missing'], capture_output=True)
        self.assertEqual(result.returncode, 1)

    def test_new_version_must_exceed_all_published_versions(self):
        published = self.path.with_name('delivery-v3.schema.json')
        self.schema['properties']['skemaversion']['const'] = 3
        published.write_text(json.dumps(self.schema))
        self.run_git('add', str(published))
        self.run_git('commit', '-qm', 'Publish version three')
        base = gate.git('rev-parse', 'HEAD')
        new = self.path.with_name('delivery-v2.schema.json')
        for version in [None, True, 2, 3, 4]:
            with self.subTest(version=version):
                self.schema['properties']['skemaversion']['const'] = version
                new.write_text(json.dumps(self.schema))
                self.assertEqual(len(gate.check(base)), 1)
        new.unlink()
        new = self.path.with_name('delivery-v4.schema.json')
        new.write_text(json.dumps(self.schema))
        self.assertEqual(gate.check(base), [])


if __name__ == '__main__':
    unittest.main()
