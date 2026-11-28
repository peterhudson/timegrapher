"""Offline tests of the workstation runners' job-started hook (agents#48):
only this repository's push to main, runs started by hand and pull requests
from its own branches get past it. No network."""

import json
import os
import pathlib
import shutil
import subprocess
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
HOOK = ROOT / ".github/self-hosted/job-started.sh"
INSTALL = (ROOT / ".github/self-hosted/tg-runner.sh").read_text(encoding="utf-8")
REPO = "peterhudson/timegrapher"

# The hook runs on Linux only (the workstation); /usr/bin/python3 and bash.
NEEDS_BASH = unittest.skipIf(os.name == "nt" or not shutil.which("bash"), "needs bash")


def pr(head_repo):
    return {"pull_request": {"head": {"repo": head_repo and {"full_name": head_repo}}}}


class Hook(unittest.TestCase):
    def run_hook(self, event_name, event, repo=REPO, ref="refs/heads/main", event_file=True):
        with tempfile.TemporaryDirectory() as tmp:
            path = pathlib.Path(tmp) / "event.json"
            if event_file:
                path.write_text(event if isinstance(event, str) else json.dumps(event))
            env = {"PATH": os.environ["PATH"], "GITHUB_REPOSITORY": repo,
                   "GITHUB_EVENT_NAME": event_name, "GITHUB_REF": ref,
                   "GITHUB_EVENT_PATH": str(path)}
            return subprocess.run(["bash", str(HOOK)], env=env, capture_output=True).returncode

    @NEEDS_BASH
    def test_our_own_code_gets_through(self):
        self.assertEqual(self.run_hook("push", {}), 0)
        self.assertEqual(self.run_hook("workflow_dispatch", {}, ref="refs/heads/some-branch"), 0)
        self.assertEqual(self.run_hook("pull_request", pr(REPO), ref="refs/pull/1/merge"), 0)

    @NEEDS_BASH
    def test_everything_else_fails_the_job(self):
        cases = {
            "a fork's pull request": ("pull_request", pr("someone/timegrapher"), {}),
            "a deleted fork's pull request": ("pull_request", pr(None), {}),
            "a pull request event with no pull request": ("pull_request", {}, {}),
            "Dependabot's pull request": ("pull_request", {"pull_request": {
                "head": {"repo": {"full_name": REPO}}, "user": {"login": "dependabot[bot]"}}}, {}),
            "an unreadable event file": ("pull_request", "not json {", {}),
            "pull_request_target": ("pull_request_target", pr(REPO), {}),
            "a push to another branch": ("push", {}, {"ref": "refs/heads/other"}),
            "another repository": ("push", {}, {"repo": "someone/else"}),
            "no event file": ("push", {}, {"event_file": False}),
        }
        for what, (name, event, extra) in cases.items():
            with self.subTest(what):
                self.assertNotEqual(self.run_hook(name, event, **extra), 0)

    def test_install_sets_the_hook_where_the_runner_account_cannot_change_it(self):
        self.assertIn("install -o root -g root -m 755", INSTALL)
        self.assertIn("\nHOOK=/usr/local/lib/tg-runner/job-started.sh\n", INSTALL)
        # In the root-owned service drop-in, not the runner's own .env.
        self.assertIn("\nEnvironment=ACTIONS_RUNNER_HOOK_JOB_STARTED=$HOOK\n", INSTALL)
        self.assertNotIn(">> \"$dir/.env\"", INSTALL)

    def test_install_leaves_disk_for_every_runner_building_at_once(self):
        self.assertIn("local floor=$(( 100 + 40 * n ))", INSTALL)
        self.assertIn("\nEnvironment=TG_MIN_FREE_GB=$floor\n", INSTALL)


if __name__ == "__main__":
    unittest.main()
