import importlib.util
import pathlib
import unittest

import yaml

ROOT = pathlib.Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("check_workflows", ROOT / "scripts/check-workflows.py")
cw = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(cw)


def docs(**files):
    return {name: yaml.safe_load(text) for name, text in files.items()}


class CheckWorkflowsTests(unittest.TestCase):
    def test_repository_workflows_pass(self):
        self.assertEqual(cw.check(cw.load(ROOT / ".github/workflows")), [])

    def test_callee_job_permission_above_caller_is_reported(self):
        wf = docs(**{
            "caller.yml": "on: {schedule: [{cron: '0 0 * * *'}], workflow_dispatch: {}}\npermissions: {contents: read}\njobs:\n  c:\n    uses: ./.github/workflows/callee.yml\n",
            "callee.yml": "on: workflow_call\npermissions: {contents: read}\njobs:\n  j:\n    permissions: {contents: read, id-token: write}\n    runs-on: ubuntu-latest\n    steps: []\n",
        })
        problems = cw.check(wf)
        self.assertEqual(len(problems), 1)
        self.assertIn("id-token: write", problems[0])

    def test_caller_job_permissions_can_grant_more(self):
        wf = docs(**{
            "caller.yml": "on: push\npermissions: {contents: read}\njobs:\n  c:\n    permissions: {contents: read, id-token: write}\n    uses: ./.github/workflows/callee.yml\n",
            "callee.yml": "on: workflow_call\npermissions: {contents: read}\njobs:\n  j:\n    permissions: {id-token: write}\n    runs-on: ubuntu-latest\n    steps: []\n",
        })
        self.assertEqual(cw.check(wf), [])

    def test_schedule_without_dispatch_is_reported(self):
        wf = docs(**{"s.yml": "on:\n  schedule: [{cron: '0 0 * * *'}]\npermissions: {contents: read}\njobs: {}\n"})
        self.assertIn("workflow_dispatch", cw.check(wf)[0])

    def test_missing_top_level_permissions_is_reported(self):
        wf = docs(**{"p.yml": "on: push\njobs: {}\n"})
        self.assertIn("permissions", cw.check(wf)[0])

    def test_unknown_needs_is_reported(self):
        wf = docs(**{"n.yml": "on: push\npermissions: {contents: read}\njobs:\n  a:\n    needs: b\n    runs-on: ubuntu-latest\n    steps: []\n"})
        self.assertIn("unknown job", cw.check(wf)[0])


if __name__ == "__main__":
    unittest.main()
