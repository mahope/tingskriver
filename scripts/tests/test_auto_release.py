"""Tests for the pure parts of scripts/auto_release.py (synthetic data only)."""
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import auto_release as ar  # noqa: E402

CARGO = """[workspace]
resolver = "2"
members = ["crates/a"]

# Shared workspace settings
[workspace.package]
version = "0.5.0-dev.3"
edition = "2021"

[workspace.dependencies]
anyhow = "1.0"
tokio = { version = "1.32.0", features = ["full"] }

[profile.release]
lto = "thin"
"""


class VersionTests(unittest.TestCase):
    def test_precedence(self):
        order = ["0.5.0-dev.2", "0.5.0-dev.3", "0.5.0-dev.10", "0.5.0", "0.5.1-dev.1", "0.5.1", "0.10.0"]
        keys = [ar.version_key(v) for v in order]
        self.assertEqual(keys, sorted(keys))

    def test_prerelease_is_below_its_release(self):
        self.assertLess(ar.version_key("0.5.0-dev.99"), ar.version_key("0.5.0"))

    def test_parse_rejects_garbage(self):
        for bad in ("0.5", "v0.5.0", "01.2.3", "0.5.0-", ""):
            with self.assertRaises(ValueError):
                ar.parse_version(bad)

    def test_next_dev_steps_the_counter(self):
        self.assertEqual(ar.next_version("0.5.0-dev.3"), "0.5.0-dev.4")
        self.assertEqual(ar.next_version("0.5.0-dev.9"), "0.5.0-dev.10")
        self.assertEqual(ar.next_version("0.5.0-rc.1"), "0.5.0-rc.2")

    def test_next_never_goes_backwards(self):
        for v in ("0.5.0-dev.3", "0.5.0-dev.9", "1.2.3"):
            nxt = ar.next_version(v)
            self.assertGreater(ar.version_key(nxt), ar.version_key(v))

    def test_stable_is_never_produced_here(self):
        # After a manual stable release the pre-release series moves on to the next patch.
        self.assertEqual(ar.next_version("0.5.0"), "0.5.1-dev.1")
        for v in ("0.5.0-dev.3", "0.5.0", "1.2.3"):
            self.assertIn("-", ar.next_version(v))

    def test_prerelease_without_counter_is_an_error(self):
        with self.assertRaises(ValueError):
            ar.next_version("0.5.0-dev")

    def test_tags(self):
        self.assertEqual(ar.version_from_tag("app-v0.5.0-dev.3"), "0.5.0-dev.3")
        self.assertEqual(ar.version_from_tag("v0.5.0-dev.2"), "0.5.0-dev.2")
        self.assertIsNone(ar.version_from_tag("android-v0.0.1"))
        self.assertIsNone(ar.version_from_tag("vendor-1"))

    def test_highest_looks_at_both_tag_families_and_cargo(self):
        tags = ["v0.4.1", "v0.5.0-dev.2", "app-v0.5.0-dev.3", "android-v9.9.9"]
        self.assertEqual(ar.highest_version(tags, "0.5.0-dev.3"), "0.5.0-dev.3")
        self.assertEqual(ar.highest_version(tags, "0.5.0-dev.5"), "0.5.0-dev.5")
        self.assertEqual(ar.highest_version(tags + ["v0.5.0-dev.7"], "0.5.0-dev.3"), "0.5.0-dev.7")

    def test_a_burned_tag_is_never_reused(self):
        tags = ["app-v0.5.0-dev.3", "app-v0.5.0-dev.4"]  # dev.4 tagged but never published
        self.assertEqual(ar.next_version(ar.highest_version(tags, "0.5.0-dev.4")), "0.5.0-dev.5")


class DecideTests(unittest.TestCase):
    TAGS = ["v0.5.0-dev.2", "app-v0.5.0-dev.3", "android-v0.0.1"]

    def decide(self, tags=None, cargo="0.5.0-dev.3", published=("app-v0.5.0-dev.3",), changes=5):
        return ar.decide(tags if tags is not None else self.TAGS, cargo, list(published), changes)

    def test_changes_since_the_last_release(self):
        action, reason, version, last_tag, last_pub = self.decide()
        self.assertEqual((action, reason, version, last_tag, last_pub),
                         ("release", "ok", "0.5.0-dev.4", "app-v0.5.0-dev.3", "app-v0.5.0-dev.3"))

    def test_nothing_new(self):
        self.assertEqual(self.decide(changes=0)[:2], ("skip", "no-changes"))

    def test_first_ever_release_without_any_app_tag(self):
        action, _, version, last_tag, last_pub = self.decide(tags=["v0.5.0-dev.2"], published=(), changes=None)
        self.assertEqual((action, version, last_tag, last_pub), ("release", "0.5.0-dev.4", None, None))

    def test_a_failed_attempt_does_not_loop(self):
        # dev.4 was tagged (bump commit on main) but never published; nothing merged since.
        tags = self.TAGS + ["app-v0.5.0-dev.4"]
        action, reason, version, last_tag, last_pub = self.decide(tags=tags, cargo="0.5.0-dev.4", changes=0)
        self.assertEqual((action, reason), ("fail", "last-attempt-failed"))
        self.assertEqual((last_tag, last_pub), ("app-v0.5.0-dev.4", "app-v0.5.0-dev.3"))

    def test_after_a_failed_attempt_new_changes_get_the_next_number(self):
        tags = self.TAGS + ["app-v0.5.0-dev.4"]
        action, _, version, _, last_pub = self.decide(tags=tags, cargo="0.5.0-dev.4", changes=2)
        self.assertEqual((action, version, last_pub), ("release", "0.5.0-dev.5", "app-v0.5.0-dev.3"))

    def test_a_tauri_tag_counts_for_the_number_but_not_as_a_gpui_release(self):
        action, _, version, last_tag, _ = self.decide(tags=self.TAGS + ["v0.5.0-dev.7"], changes=3)
        self.assertEqual((action, version, last_tag), ("release", "0.5.0-dev.8", "app-v0.5.0-dev.3"))

    def test_stable_published_by_hand_continues_with_the_next_prerelease_series(self):
        tags = self.TAGS + ["app-v0.5.0"]
        action, _, version, _, _ = self.decide(tags=tags, cargo="0.5.0", published=("app-v0.5.0",), changes=4)
        self.assertEqual((action, version), ("release", "0.5.1-dev.1"))


    def test_bump_pushed_but_tag_missing_does_not_loop(self):
        action, reason, *_ = ar.decide(self.TAGS, "0.5.0-dev.4", ["app-v0.5.0-dev.3"], 0, stray_bump=True)
        self.assertEqual((action, reason), ("fail", "bump-without-tag"))

    def test_changes_after_a_stray_bump_get_a_new_version(self):
        action, _, version, *_ = ar.decide(self.TAGS, "0.5.0-dev.4", ["app-v0.5.0-dev.3"], 1, stray_bump=True)
        self.assertEqual((action, version), ("release", "0.5.0-dev.5"))

    def test_manual_stable_in_the_making_is_not_overtaken(self):
        # Cargo.toml says 0.5.1 (stable) but app-v0.5.1 is not tagged yet: the sign-off may be pending.
        action, reason, *_ = ar.decide(self.TAGS, "0.5.1", ["app-v0.5.0-dev.3"], 3)
        self.assertEqual((action, reason), ("skip", "stable-pending"))

    def test_a_pulled_release_is_not_reported_as_a_failed_build(self):
        action, reason, *_ = ar.decide(self.TAGS, "0.5.0-dev.3", [], 0, drafts=["app-v0.5.0-dev.3"])
        self.assertEqual((action, reason), ("skip", "release-is-draft"))


def git(cwd, *args, **env):
    import os
    import subprocess
    subprocess.run(["git", "-c", "commit.gpgsign=false", "-c", "user.name=x", "-c", "user.email=x@example.com",
                    "-c", "tag.gpgsign=false", *args], cwd=cwd, check=True, capture_output=True,
                   env={**os.environ, **env})


class GitHistoryTests(unittest.TestCase):
    """The history parsing against a real repository (no network)."""

    def setUp(self):
        import tempfile
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.repo = self.tmp.name
        git(self.repo, "init", "-q", "-b", "main")

    def commit(self, subject, email="dev@example.com"):
        git(self.repo, "commit", "-q", "--allow-empty", "-m", subject,
            GIT_AUTHOR_NAME="x", GIT_AUTHOR_EMAIL=email, GIT_COMMITTER_NAME="x", GIT_COMMITTER_EMAIL=email)

    def test_a_tab_in_a_subject_is_fine(self):
        self.commit("Fix\tthing (#5)")
        commits = ar.log_commits("HEAD", cwd=self.repo)
        self.assertEqual([c[1] for c in commits], ["Fix\tthing (#5)"])

    def test_changes_stop_at_the_tag_and_ignore_nothing_else(self):
        self.commit("Merge pull request #1 from o/a")
        git(self.repo, "tag", "-a", "app-v0.5.0-dev.3", "-m", "t")
        self.commit("Fix\tthing (#5)")
        self.commit("Release v0.4.1")  # a person's commit with a similar subject still counts
        changes, stray = ar.changes_since_last_attempt("app-v0.5.0-dev.3", cwd=self.repo)
        self.assertEqual(len(changes), 2)
        self.assertFalse(stray)

    def test_a_bump_commit_without_its_tag_is_found(self):
        self.commit("Merge pull request #1 from o/a")
        git(self.repo, "tag", "-a", "app-v0.5.0-dev.3", "-m", "t")
        self.commit("Merge pull request #2 from o/b")
        self.commit("Release 0.5.0-dev.4", email=ar.BOT_EMAIL)  # pushed, tag push failed
        changes, stray = ar.changes_since_last_attempt("app-v0.5.0-dev.3", cwd=self.repo)
        self.assertEqual((len(changes), stray), (0, True))
        self.commit("Merge pull request #3 from o/c")
        changes, stray = ar.changes_since_last_attempt("app-v0.5.0-dev.3", cwd=self.repo)
        self.assertEqual((len(changes), stray), (1, True))

    def test_a_released_tag_on_the_bump_commit_is_quiet(self):
        self.commit("Merge pull request #1 from o/a")
        self.commit("Release 0.5.0-dev.4", email=ar.BOT_EMAIL)
        git(self.repo, "tag", "-a", "app-v0.5.0-dev.4", "-m", "t")
        self.assertEqual(ar.changes_since_last_attempt("app-v0.5.0-dev.4", cwd=self.repo), ([], False))


class BumpCommitTests(unittest.TestCase):
    def test_only_the_bots_release_commit_counts(self):
        self.assertTrue(ar.is_bump_commit("Release 0.5.0-dev.4", ar.BOT_EMAIL))
        self.assertFalse(ar.is_bump_commit("Release 0.5.0-dev.4", "someone@example.com"))
        self.assertFalse(ar.is_bump_commit("Release v0.4.1", ar.BOT_EMAIL))
        self.assertFalse(ar.is_bump_commit("Release notes for the thing", ar.BOT_EMAIL))


class CargoTests(unittest.TestCase):
    def test_read(self):
        self.assertEqual(ar.read_workspace_version(CARGO), "0.5.0-dev.3")

    def test_set_touches_only_the_workspace_version(self):
        new = ar.set_workspace_version(CARGO, "0.5.0-dev.4")
        self.assertIn('version = "0.5.0-dev.4"', new)
        self.assertIn('tokio = { version = "1.32.0", features = ["full"] }', new)
        self.assertEqual(new.count("0.5.0-dev"), 1)
        self.assertEqual(ar.read_workspace_version(new), "0.5.0-dev.4")
        self.assertEqual(new.replace("0.5.0-dev.4", "0.5.0-dev.3"), CARGO)

    def test_missing_section(self):
        with self.assertRaises(ValueError):
            ar.read_workspace_version('[package]\nversion = "1.0.0"\n')


class BumpDiffTests(unittest.TestCase):
    GOOD = (
        'diff --git a/Cargo.lock b/Cargo.lock\nindex 1..2 100644\n--- a/Cargo.lock\n+++ b/Cargo.lock\n'
        '@@ -1 +1 @@\n-version = "0.5.0-dev.3"\n+version = "0.5.0-dev.4"\n'
        '@@ -9 +9 @@\n-version = "0.5.0-dev.3"\n+version = "0.5.0-dev.4"\n'
    )

    def test_good(self):
        self.assertEqual(ar.check_bump_diff(self.GOOD, "0.5.0-dev.3", "0.5.0-dev.4"), [])

    def test_unrelated_dependency_change_is_caught(self):
        bad = self.GOOD + '@@ -20 +20 @@\n-version = "1.0.1"\n+version = "1.0.2"\n'
        self.assertTrue(ar.check_bump_diff(bad, "0.5.0-dev.3", "0.5.0-dev.4"))

    def test_no_change_is_caught(self):
        self.assertTrue(ar.check_bump_diff("", "0.5.0-dev.3", "0.5.0-dev.4"))

    def test_unbalanced_is_caught(self):
        bad = '@@ -1 +1 @@\n-version = "0.5.0-dev.3"\n'
        self.assertTrue(ar.check_bump_diff(bad, "0.5.0-dev.3", "0.5.0-dev.4"))


class ChangelogTests(unittest.TestCase):
    def test_pr_numbers(self):
        self.assertEqual(ar.pr_number("Merge pull request #167 from mahope/issue/36-async-io-v2"), 167)
        self.assertEqual(ar.pr_number("Add the thing (#12)"), 12)
        self.assertIsNone(ar.pr_number("Fix typo in issue #12 text"))
        self.assertIsNone(ar.pr_number("Release 0.5.0-dev.4"))

    def test_sections(self):
        self.assertEqual(ar.section_for("Add offline About page"), "Tilføjet")
        self.assertEqual(ar.section_for("Fix: crash on resume"), "Rettet")
        self.assertEqual(ar.section_for("Remove the Tauri updater"), "Fjernet")
        self.assertEqual(ar.section_for("Make core HTTP asynchronous"), "Ændret")
        self.assertEqual(ar.section_for(""), "Ændret")

    def test_render(self):
        text = ar.render_changelog(
            "0.5.0-dev.4", "2026-11-05", "app-v0.5.0-dev.3",
            [("Add offline About page", 177), ("Fix crash on resume", 170), ("Make HTTP async", None)],
            "mahope/tingskriver",
        )
        self.assertTrue(text.startswith("# Tingskriver 0.5.0-dev.4\n"))
        self.assertIn("## Tilføjet\n\n- Add offline About page ([#177](https://github.com/mahope/tingskriver/pull/177))", text)
        self.assertIn("## Rettet", text)
        self.assertIn("- Make HTTP async\n", text)
        self.assertTrue(text.endswith("\n") and not text.endswith("\n\n"))

    def test_render_with_no_entries_still_has_content(self):
        text = ar.render_changelog("0.5.0-dev.4", "2026-11-05", None, [], "mahope/tingskriver")
        self.assertIn("## Ændret", text)

    def test_release_preserves_exact_changelog(self):
        from types import SimpleNamespace
        from unittest.mock import patch
        import tempfile

        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "notes.md"
            path.write_text("# Version\n\nÆndringer.\n", encoding="utf-8")
            args = SimpleNamespace(tag="app-v0.5.3", version="0.5.3",
                                   timeout_minutes=1, changelog=str(path))
            with patch.object(ar, "dispatch_and_wait"), patch.object(ar, "verify_release", return_value=[]), \
                    patch.object(ar, "gh_json", return_value={"body": path.read_text(encoding="utf-8") + (ar.ROOT / "installer/release-notes.md").read_text(encoding="utf-8")}), \
                    patch.object(ar, "run") as run:
                self.assertEqual(ar.cmd_release(args), 0)
                run.assert_not_called()
            with patch.object(ar, "dispatch_and_wait"), patch.object(ar, "verify_release", return_value=[]), \
                    patch.object(ar, "gh_json", return_value={"body": "Extra text" + ar.RELEASE_NOTES_FOOTER}):
                self.assertEqual(ar.cmd_release(args), 1)

    def test_release_timeout_covers_all_jobs(self):
        from unittest.mock import patch
        with patch.object(ar, "cmd_release", return_value=0) as release:
            ar.main(["release", "--tag", "app-v0.5.3", "--version", "0.5.3"])
        self.assertGreater(release.call_args.args[0].timeout_minutes, 120 + 30 + 25)


def run_(workflow, event="push", status="completed", conclusion="success"):
    return {"workflowName": workflow, "event": event, "status": status, "conclusion": conclusion}


class CiVerdictTests(unittest.TestCase):
    def test_green(self):
        self.assertEqual(ar.evaluate_ci([run_("Rust"), run_("WER regression"), run_("Search benchmark", conclusion="skipped")])[0], "green")

    def test_red_beats_pending(self):
        runs = [run_("Rust", conclusion="failure"), run_("WER regression", status="in_progress", conclusion=None)]
        self.assertEqual(ar.evaluate_ci(runs)[0], "red")

    def test_cancelled_is_not_green(self):
        self.assertEqual(ar.evaluate_ci([run_("Rust"), run_("Ydeevne-port", conclusion="cancelled")])[0], "red")

    def test_pending(self):
        self.assertEqual(ar.evaluate_ci([run_("Rust", status="in_progress", conclusion=None)])[0], "pending")

    def test_scheduled_and_manual_runs_are_ignored(self):
        runs = [run_("Rust"), run_("Three-hour recording soak", event="schedule", conclusion="failure")]
        self.assertEqual(ar.evaluate_ci(runs)[0], "green")

    def test_no_runs_at_all(self):
        self.assertEqual(ar.evaluate_ci([])[0], "missing")
        self.assertEqual(ar.evaluate_ci([run_("Rust", event="schedule")])[0], "missing")

    def test_rust_must_have_run(self):
        self.assertEqual(ar.evaluate_ci([run_("WER regression")])[0], "missing")


if __name__ == "__main__":
    unittest.main()
