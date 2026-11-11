import importlib.util
import pathlib
import sys
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
SPEC = importlib.util.spec_from_file_location("previous_release", ROOT / "scripts/previous-release.py")
pr = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(pr)


def release(version, **extra):
    return dict(tag_name=f"app-v{version}", draft=False,
                assets=[{"name": "Tingskriver-Setup.exe"}], **extra)


class PreviousReleaseTests(unittest.TestCase):
    def test_stable_uses_highest_lower_stable_installer(self):
        releases = [release(v) for v in ("0.5.2", "0.5.10", "0.5.11", "0.5.11-dev.1")]
        self.assertEqual(pr.previous(releases, "0.5.11"), "app-v0.5.10")

    def test_prerelease_needs_a_predecessor_that_can_see_prereleases(self):
        releases = [release(v) for v in ("0.5.2", "0.5.3-dev.2", "0.5.3-dev.10")]
        self.assertEqual(pr.previous(releases, "0.5.3-dev.11"), "app-v0.5.3-dev.10")
        with self.assertRaises(ValueError):
            pr.previous([release("0.5.2")], "0.5.3-dev.1")

    def test_drafts_other_tags_and_missing_installers_do_not_count(self):
        draft = release("0.5.2")
        draft["draft"] = True
        missing = release("0.5.2")
        missing["assets"] = []
        other = release("0.5.2")
        other["tag_name"] = "v0.5.2"
        self.assertEqual(pr.previous([draft, missing, other, release("0.5.1")], "0.5.3"),
                         "app-v0.5.1")
        with self.assertRaises(ValueError):
            pr.previous([draft, missing, other], "0.5.3")


if __name__ == "__main__":
    unittest.main()
