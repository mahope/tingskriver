import importlib.util
import pathlib
import unittest

SPEC = importlib.util.spec_from_file_location(
    "ci_scope", pathlib.Path(__file__).resolve().parents[2] / ".github/scripts/ci-scope.py"
)
ci_scope = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(ci_scope)


class CiScopeTests(unittest.TestCase):
    def test_site_docs_and_readme_skip_rust(self):
        self.assertFalse(ci_scope.needs_rust(["site/index.html", "docs/agenter.md", "README.md"]))

    def test_unrelated_workflows_skip_rust(self):
        self.assertFalse(ci_scope.needs_rust([".github/workflows/pages.yml", "site/a.css"]))

    def test_android_only_skips_rust(self):
        self.assertFalse(ci_scope.needs_rust(["apps/android/app/build.gradle"]))

    def test_workspace_config_and_lock_run_rust(self):
        for path in ["Cargo.toml", "Cargo.lock", "deny.toml", "rust-toolchain.toml", "crates/x/src/a.rs"]:
            self.assertTrue(ci_scope.needs_rust([path]), path)

    def test_shared_workflow_files_and_scripts_run_rust(self):
        for path in [
            ".github/workflows/rust.yml",
            ".github/workflows/branch-build.yml",
            ".github/workflows/windows-build.yml",
            ".github/scripts/annotate-errors.sh",
            "scripts/check-gpui-lock.py",
        ]:
            self.assertTrue(ci_scope.needs_rust([path]), path)

    def test_delivery_schema_in_docs_runs_rust(self):
        self.assertTrue(ci_scope.needs_rust(["docs/schemas/delivery-v1.schema.json"]))

    def test_one_rust_path_among_docs_runs_rust(self):
        self.assertTrue(ci_scope.needs_rust(["docs/a.md", "site/b.css", "frontend/src/page.tsx"]))

    def test_unknown_new_top_level_dir_runs_rust(self):
        self.assertTrue(ci_scope.needs_rust(["newdir/file"]))

    def test_empty_change_set_runs_rust(self):
        self.assertTrue(ci_scope.needs_rust([]))


if __name__ == "__main__":
    unittest.main()
