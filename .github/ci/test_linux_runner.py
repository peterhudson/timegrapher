"""Offline tests of the workstation runner rule (agents#48): linux_runner.sh
picks the self-hosted tg-linux runners only for our own code, with the
switch on and a runner online, and ci.yml sends only the heavy Linux jobs
there. A fake `gh` stands in for the API; no network."""

import os
import pathlib
import re
import shutil
import subprocess
import tempfile
import unittest

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parents[1]
SCRIPT = HERE / "linux_runner.sh"
WORKFLOW = (ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
HOSTED = '"ubuntu-24.04"'
SELF = '["self-hosted","tg-linux"]'
PICK = "runs-on: ${{ fromJSON(needs.mode.outputs.linux) }}"


def jobs():
    body = WORKFLOW.split("\njobs:\n", 1)[1]
    parts = re.split(r"(?m)^  ([\w-]+):\n", body)
    return dict(zip(parts[1::2], parts[2::2]))


@unittest.skipIf(os.name == "nt" or not shutil.which("bash"), "needs bash")
class Script(unittest.TestCase):
    def run_script(self, gh_output="2", gh_status=0, **env):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = pathlib.Path(tmp)
            calls = tmp / "calls"
            fake = tmp / "gh"
            fake.write_text(f'#!/bin/sh\necho "$@" >> "{calls}"\necho "{gh_output}"\nexit {gh_status}\n')
            fake.chmod(0o755)
            out = tmp / "out"
            full = {"PATH": f"{tmp}:{os.environ['PATH']}", "GITHUB_OUTPUT": str(out),
                    "REPO": "peterhudson/timegrapher", "TRUSTED": "true", "SWITCH": "",
                    "GH_TOKEN": "t"}
            full.update(env)
            subprocess.run(["bash", str(SCRIPT)], env=full, check=True, capture_output=True)
            called = calls.read_text() if calls.exists() else ""
            return out.read_text().strip(), called

    def test_our_code_with_a_runner_online_goes_to_the_workstation(self):
        out, called = self.run_script()
        self.assertEqual(out, f"linux={SELF}")
        self.assertIn("repos/peterhudson/timegrapher/actions/runners", called)

    def test_a_fork_never_goes_to_the_workstation_and_the_api_is_not_asked(self):
        out, called = self.run_script(TRUSTED="false")
        self.assertEqual(out, f"linux={HOSTED}")
        self.assertEqual(called, "")

    def test_the_switch_off_no_token_no_runner_or_no_api_mean_github(self):
        for env in ({"SWITCH": "off"}, {"GH_TOKEN": ""}, {"gh_output": "0"},
                    {"gh_status": 1, "gh_output": ""}, {"gh_output": "garbage"}):
            with self.subTest(env=env):
                out, _ = self.run_script(**env)
                self.assertEqual(out, f"linux={HOSTED}")


class Workflow(unittest.TestCase):
    def test_only_the_heavy_linux_jobs_can_run_on_the_workstation(self):
        found = jobs()
        movable = {"test-linux", "quick", "regress"}
        for name, text in found.items():
            with self.subTest(job=name):
                if name in movable:
                    self.assertIn(PICK, text)
                    self.assertIn("permissions:\n      contents: read\n", text)
                else:
                    self.assertNotIn("needs.mode.outputs.linux", text)
                    self.assertNotIn("self-hosted", re.findall(r"(?m)^    runs-on: .*$", text)[0])

    def test_forks_and_dependabot_are_never_trusted(self):
        mode = jobs()["mode"]
        self.assertIn("github.event.pull_request.head.repo.full_name == github.repository", mode)
        self.assertIn("!github.event.pull_request.head.repo.fork", mode)
        self.assertIn("github.event.pull_request.user.login != 'dependabot[bot]'", mode)
        self.assertIn("linux: ${{ steps.runner.outputs.linux }}", mode)
        self.assertIn(".github/ci/linux_runner.sh", mode)

    def test_github_runner_only_steps_are_skipped_on_the_workstation(self):
        for step in re.findall(r"(?ms)^      - uses: Swatinem/rust-cache@v2\n        if: ([^\n]*)", WORKFLOW):
            self.assertIn("runner.environment == 'github-hosted'", step)
        self.assertRegex(WORKFLOW, r"Make disk room for the corpus audio\n        if: [^\n]*runner.environment == 'github-hosted'")
        self.assertRegex(WORKFLOW, r"Corpus audio cache\n        id: audio\n        if: [^\n]*runner.environment == 'github-hosted'")


if __name__ == "__main__":
    unittest.main()
