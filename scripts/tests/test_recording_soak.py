import importlib.util
from pathlib import Path
import unittest
from unittest.mock import Mock, patch
import tempfile

spec = importlib.util.spec_from_file_location('soak', Path(__file__).parents[1] / 'recording-soak.py')
soak = importlib.util.module_from_spec(spec)
spec.loader.exec_module(soak)


class Budgets(unittest.TestCase):
    def test_memory_regression(self):
        soak.check_memory([10_000_000, 109_999_999])
        with self.assertRaises(AssertionError):
            soak.check_memory([10_000_000, 110_000_000])

    def test_crash_variant_must_reach_forced_stop(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            def exited_recorder(*args, **kwargs):
                (root / 'crash' / 'ready').touch()
                return Mock(poll=Mock(return_value=0), returncode=0)
            with patch.object(soak.subprocess, 'Popen', side_effect=exited_recorder), patch.object(soak.subprocess, 'run'):
                with self.assertRaisesRegex(AssertionError, 'forced stop'):
                    soak.run(Path('/fake-recorder'), root, 8, crash=True)

    def test_transient_peak_is_not_hidden(self):
        with self.assertRaises(AssertionError):
            soak.check_memory([10, 100_000_010, 10])


if __name__ == '__main__':
    unittest.main()
