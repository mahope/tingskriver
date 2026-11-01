#!/usr/bin/env python3
import importlib.util
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

sys.dont_write_bytecode = True

spec = importlib.util.spec_from_file_location("wer_gate", Path(__file__).with_name("wer-gate.py"))
gate = importlib.util.module_from_spec(spec)
spec.loader.exec_module(gate)


def report(error_count):
    score = dict(reference_words=1000, substitutions=error_count, insertions=0, deletions=0)
    return dict(schema_version=1, normalization="unicode-alphanumeric-lower-v1", dataset_kind="fixture-text", engine="fixture", date="2026-11-01", git_sha="a" * 40,
                clips=[dict(id="test", reference=" ".join(["ord"] * 1000), hypothesis=" ".join(["fejl"] * error_count + ["ord"] * (1000 - error_count)), score=score, wer=error_count / 1000)], total=score.copy(), wer=error_count / 1000)


class GateTests(unittest.TestCase):
    def test_exactly_one_pp_passes(self):
        self.assertFalse(gate.compare(report(100), report(110))[0])

    def test_one_point_one_pp_fails(self):
        self.assertTrue(gate.compare(report(100), report(111))[0])

    def test_improvement_passes(self):
        self.assertFalse(gate.compare(report(100), report(90))[0])

    def test_missing_clip_rejected(self):
        baseline = report(100)
        extra = gate.json.loads(gate.json.dumps(baseline["clips"][0]))
        extra["id"] = "second"
        baseline["clips"].append(extra)
        baseline["total"] = {key: value * 2 for key, value in baseline["total"].items()}
        gate.validate(baseline)
        with self.assertRaises(gate.InvalidReport):
            gate.compare(baseline, report(100))

    def test_new_clip_rejected(self):
        current = report(100)
        extra = gate.json.loads(gate.json.dumps(current["clips"][0]))
        extra["id"] = "new"
        current["clips"].append(extra)
        current["total"] = {key: value * 2 for key, value in current["total"].items()}
        gate.validate(current)
        with self.assertRaises(gate.InvalidReport):
            gate.compare(report(100), current)

    def test_metadata_and_reference_changes_rejected(self):
        for key in ("engine", "normalization", "dataset_kind"):
            current = report(100)
            current[key] = "different"
            with self.assertRaises(gate.InvalidReport):
                gate.compare(report(100), current)
        current = report(100)
        current["clips"][0]["reference"] = " ".join(["andet"] * 1000)
        with self.assertRaises(gate.InvalidReport):
            gate.compare(report(100), current)

    def test_inconsistent_and_nonfinite_scores_rejected(self):
        for value in (-1, True, float("nan"), float("inf"), None):
            current = report(100)
            current["wer"] = value
            with self.assertRaises(gate.InvalidReport):
                gate.validate(current)
        current = report(100)
        current["total"]["deletions"] = 2
        with self.assertRaises(gate.InvalidReport):
            gate.validate(current)

    def test_cli_exit_codes_and_baseline_creation(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            path, baseline = root / "report.json", root / "baseline.json"
            path.write_text(gate.json.dumps(report(100)))
            command = [sys.executable, str(Path(__file__).with_name("wer-gate.py")), "--rapport", str(path), "--baseline", str(baseline)]
            result = subprocess.run(command, capture_output=True, text=True)
            self.assertEqual(result.returncode, 2)
            self.assertIn("--skriv-baseline", result.stderr)
            self.assertEqual(subprocess.run(command + ["--skriv-baseline"], capture_output=True).returncode, 0)
            for count, expected in ((110, 0), (111, 1), (90, 0)):
                path.write_text(gate.json.dumps(report(count)))
                self.assertEqual(subprocess.run(command, capture_output=True).returncode, expected)


if __name__ == "__main__":
    unittest.main()
