import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('soak', Path(__file__).parents[1] / 'recording-soak.py')
soak = importlib.util.module_from_spec(spec)
spec.loader.exec_module(soak)


class Budgets(unittest.TestCase):
    def test_memory_regression(self):
        soak.check_memory([10_000_000, 109_999_999])
        with self.assertRaises(AssertionError):
            soak.check_memory([10_000_000, 110_000_000])

    def test_transient_peak_is_not_hidden(self):
        with self.assertRaises(AssertionError):
            soak.check_memory([10, 100_000_010, 10])


if __name__ == '__main__':
    unittest.main()
