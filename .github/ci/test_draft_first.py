"""Offline tests of ci.yml's draft rule (#2444): a draft pull request runs
only the quick job, and every other job runs only off a draft; and of
release.yml's (#2490): a draft pull request builds no packages. Reads the
workflows as text (no YAML library on every runner); no network or git."""

import os
import pathlib
import re
import shutil
import subprocess
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
WORKFLOW = (ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
RELEASE = (ROOT / ".github/workflows/release.yml").read_text(encoding="utf-8")
MODE_SCRIPT = ROOT / ".github/ci/pr_mode.sh"
FULL = "${{ needs.mode.outputs.full == 'true' }}"
PR_TYPES = "types: [opened, synchronize, reopened, ready_for_review]\n"


def jobs(workflow=WORKFLOW):
    """{job id: its text} for each job under `jobs:`."""
    body = workflow.split("\njobs:\n", 1)[1]
    parts = re.split(r"(?m)^  ([\w-]+):\n", body)
    return dict(zip(parts[1::2], parts[2::2]))


def job_if(text):
    found = re.search(r"(?m)^    if: (.*)$", text)
    return found.group(1) if found else None


class DraftRule(unittest.TestCase):
    def test_the_draft_state_events_start_a_run(self):
        self.assertIn(PR_TYPES, WORKFLOW)
        # A run on converted_to_draft would skip the required checks on a
        # head that may already hold their real results.
        self.assertNotIn("converted_to_draft]", WORKFLOW)

    def test_every_job_but_mode_and_quick_runs_only_off_a_draft(self):
        self.assertIn(f"if: &full-suite {FULL}", WORKFLOW)
        found = jobs()
        self.assertIn("quick", found)
        summaries = {"test-linux-result": "test-linux", "test-macos-result": "test-macos"}
        for name, text in found.items():
            if name in ("mode", "quick"):
                continue
            rule = job_if(text)
            if name in summaries:
                # Its halves ran off a draft, so it runs only when they did:
                # run there, it would fail on their "skipped".
                self.assertEqual(rule, f"${{{{ !cancelled() && needs.{summaries[name]}.result != 'skipped' }}}}")
                continue
            # A job with no `if`, or one that reads the event's draft
            # state, would run on a draft.
            self.assertIn(rule, ("*full-suite", f"&full-suite {FULL}"), name)
            self.assertRegex(text, r"(?m)^    needs: mode$", name)

    def test_mode_reads_the_pull_request_live(self):
        # The event's draft state and head can be a few seconds old (#2368):
        # mode reads both from the API and runs nothing for an older head.
        # Release's mode runs the same script.
        script = MODE_SCRIPT.read_text(encoding="utf-8")
        self.assertIn('gh api "$PR_URL" --jq \'"\\(.draft) \\(.head.sha)"\'', script)
        self.assertIn('if [ "$head" != "$EVENT_HEAD" ]; then', script)
        for workflow in (WORKFLOW, RELEASE):
            text = jobs(workflow)["mode"]
            self.assertIsNone(job_if(text))
            self.assertNotRegex(text, r"(?m)^    needs: ")
            # ci.yml's also takes linux_runner.sh (agents#48); only scripts.
            self.assertRegex(text, r"sparse-checkout: (\|\n            )?\.github/ci/pr_mode\.sh\n")
            self.assertIn("        run: bash .github/ci/pr_mode.sh\n", text)
            self.assertNotIn("github.event.pull_request.draft", workflow.replace(
                "EVENT_DRAFT: ${{ github.event.pull_request.draft }}", ""))

    def test_jobs_made_after_others_are_matrix_jobs(self):
        # A job with `needs:` is made only when what it needs ends. If it
        # were skipped under its own name on the draft run, that "skipped"
        # would stand for it, and read as passing, until then. A skipped
        # matrix job takes its unexpanded name instead. Only quick, which
        # is no required check, may skip under its own name.
        for name, text in jobs().items():
            if name != "quick" and re.search(r"(?m)^    needs: ", text):
                job_name = re.search(r"(?m)^    name: (.*)$", text).group(1)
                self.assertIn("${{ matrix.", job_name, name)
                self.assertRegex(text, r"(?m)^    strategy:\n(      .*\n)*?      matrix:\n", name)

    def test_quick_runs_only_on_a_draft_and_is_no_required_check(self):
        text = jobs()["quick"]
        # Exactly: `!draft` here would run it beside the full suite.
        self.assertEqual(job_if(text), "${{ needs.mode.outputs.quick == 'true' }}")
        name = re.search(r"(?m)^    name: (.*)$", text).group(1)
        gate = (ROOT / ".github/scripts/release_ci_gate.py").read_text(encoding="utf-8")
        self.assertNotIn(f'"{name}"', gate)
        self.assertFalse(name.startswith(("test (", "regress (", "mcp-renderer")), name)


class ReleaseDraftRule(unittest.TestCase):
    """release.yml builds a pull request's packages only once it is ready."""

    def test_marking_ready_starts_a_run(self):
        pr = RELEASE.split("  pull_request:\n", 1)[1].split("\n  workflow_dispatch:", 1)[0]
        self.assertIn("    " + PR_TYPES, pr)
        self.assertNotIn("converted_to_draft", RELEASE)

    def test_every_build_job_waits_for_mode(self):
        found = jobs(RELEASE)
        self.assertEqual(job_if(found["ci-gate"]), f"&build {FULL}")
        self.assertEqual(job_if(found["docs"]), "*build")
        for name in ("ci-gate", "docs"):
            self.assertRegex(found[name], r"(?m)^    needs: mode$", name)
        self.assertRegex(found["version"], r"(?m)^    needs: \[mode, drafts\]$")
        self.assertEqual(job_if(found["version"]),
                         "${{ !cancelled() && needs.mode.outputs.full == 'true' "
                         "&& needs.drafts.result != 'failure' }}")
        # build and release need those (or build), and name each result, so
        # a draft skips them too and neither runs on its own.
        self.assertRegex(found["build"], r"(?m)^    needs: \[ci-gate, version, docs\]$")
        self.assertEqual(job_if(found["build"]),
                         "${{ !cancelled() && needs.ci-gate.result == 'success' && "
                         "needs.version.result == 'success' && needs.docs.result == 'success' }}")
        self.assertRegex(found["release"], r"(?m)^    needs: \[version, build\]$")

    def test_the_shared_mode_script_starts_a_run(self):
        # A change to the script decides what Release builds, so it runs
        # Release too.
        paths = RELEASE.split("  pull_request:\n", 1)[1].split("\n  workflow_dispatch:", 1)[0]
        self.assertIn('      - ".github/ci/pr_mode.sh"\n', paths)

    def test_only_pull_request_runs_are_cancelled(self):
        # Tag and Actions-tab runs share the group `release-main` and are
        # never cancelled: two drafts must never pick the same version.
        self.assertIn("\nconcurrency:\n  group: release-${{ github.event_name == 'pull_request' && github.ref || 'main' }}\n"
                      "  cancel-in-progress: ${{ github.event_name == 'pull_request' }}\n", RELEASE)


class ReleaseTokens(unittest.TestCase):
    """Only the runs that can draft a release get a write token (#2426)."""

    RELEASE_CAPABLE = ("github.event_name == 'workflow_dispatch' && inputs.release "
                       "&& github.ref == 'refs/heads/main'")

    def test_only_drafts_and_release_can_write(self):
        found = jobs(RELEASE)
        writers = {n for n, text in found.items() if "contents: write" in text}
        self.assertEqual(writers, {"drafts", "release"})
        self.assertIn("\npermissions:\n  contents: read\n", RELEASE)
        self.assertEqual(job_if(found["drafts"]), self.RELEASE_CAPABLE)
        # release's own `if` is pinned by test_release_ci_gate.py.
        self.assertIn(self.RELEASE_CAPABLE, found["release"])

    def test_drafts_runs_no_file_of_the_branch(self):
        text = jobs(RELEASE)["drafts"]
        self.assertNotIn("uses:", text)
        self.assertNotIn("needs:", text)
        self.assertNotRegex(text, r"(packaging|\.github)/")


def step_script(job, marker):
    """The `run: |` text after `marker` in release.yml's `job`, unindented."""
    block = jobs(RELEASE)[job].split(marker, 1)[1]
    block = block.split("        run: |\n", 1)[1]
    lines = []
    for line in block.split("\n"):
        if line and not line.startswith("          "):
            break
        lines.append(line[10:])
    return "\n".join(lines)


@unittest.skipIf(os.name == "nt" or not shutil.which("bash"), "needs bash")
class VersionForADraft(unittest.TestCase):
    """version picks a draft's number from the drafts job's list (#2426)."""

    def pick(self, used):
        with tempfile.TemporaryDirectory() as tmp:
            out = pathlib.Path(tmp, "out")
            env = dict(os.environ, GITHUB_OUTPUT=str(out), GITHUB_STEP_SUMMARY=str(pathlib.Path(tmp, "sum")),
                       RUNNER_TEMP=tmp, GITHUB_REF_TYPE="branch", GITHUB_REF="refs/heads/main",
                       RELEASE_REQUESTED="true", USED=used)
            done = subprocess.run(["bash", "-e", "-c", step_script("version", "      - id: pick\n")],
                                  env=env, cwd=ROOT, capture_output=True, text=True)
            return done.returncode, out.read_text().strip() if out.exists() else ""

    def test_an_empty_list_stops_the_run(self):
        for used in ("", " ", "   "):
            code, out = self.pick(used)
            self.assertNotEqual(code, 0, repr(used))
            self.assertEqual(out, "")

    def test_the_list_gives_the_next_number(self):
        code, out = self.pick("v0.1.0 v0.1.1 ")
        self.assertEqual(code, 0)
        self.assertRegex(out, r"^version=\d+\.\d+\.\d+$")


def tag_script():
    """The `run: |` text of release.yml's "Tag already released" step."""
    block = jobs(RELEASE)["mode"].split("      - name: Tag already released\n", 1)[1]
    block = block.split("        run: |\n", 1)[1]
    lines = []
    for line in block.split("\n"):
        if line and not line.startswith("          "):
            break
        lines.append(line[10:])
    return "\n".join(lines)


@unittest.skipIf(os.name == "nt" or not shutil.which("bash"), "needs bash")
class TagAlreadyReleased(unittest.TestCase):
    """A tag a published release holds builds nothing (#2206)."""

    def run_step(self, gh_body):
        with tempfile.TemporaryDirectory() as tmp:
            gh = pathlib.Path(tmp, "gh")
            gh.write_text("#!/bin/sh\n" + gh_body)
            gh.chmod(0o755)
            out = pathlib.Path(tmp, "out")
            env = dict(os.environ, PATH=f"{tmp}{os.pathsep}{os.environ['PATH']}",
                       GITHUB_OUTPUT=str(out), GITHUB_STEP_SUMMARY=str(pathlib.Path(tmp, "sum")),
                       RUNNER_TEMP=tmp, GITHUB_REPOSITORY="o/r", TAG="v1.1.0", GH_TOKEN="x")
            done = subprocess.run(["bash", "-e", "-c", tag_script()], env=env,
                                  capture_output=True, text=True)
            return done.returncode, out.read_text().strip() if out.exists() else ""

    def test_the_four_answers(self):
        self.assertEqual(self.run_step("echo false\n"), (0, "published=true"))
        self.assertEqual(self.run_step("echo true\n"), (0, "published=false"))
        self.assertEqual(self.run_step("echo 'gh: Not Found (HTTP 404)' >&2; exit 1\n"),
                         (0, "published=false"))
        code, out = self.run_step("echo 'gh: Server Error (HTTP 502)' >&2; exit 1\n")
        self.assertNotEqual(code, 0)
        self.assertEqual(out, "")

    def test_mode_says_full_only_without_a_published_release(self):
        self.assertIn("      full: ${{ steps.mode.outputs.full == 'true' && "
                      "steps.tag.outputs.published != 'true' }}\n", jobs(RELEASE)["mode"])
        self.assertIn("        if: github.ref_type == 'tag'\n", jobs(RELEASE)["mode"])


class TestTimes(unittest.TestCase):
    """Each test job keeps nextest's JUnit report (#2490)."""

    def test_nextest_writes_a_junit_report(self):
        config = (ROOT / ".config/nextest.toml").read_text(encoding="utf-8")
        self.assertIn('\n[profile.default.junit]\npath = "junit.xml"\n', config)

    def test_every_test_job_uploads_it(self):
        steps = WORKFLOW.split("    steps: &test-steps\n", 1)[1].split("\n\n", 1)[0]
        self.assertIn("      - name: Test times\n", steps)
        upload = steps.split("      - name: Test times\n", 1)[1].split("\n      - ", 1)[0]
        self.assertIn("path: target/nextest/*/junit.xml\n", upload)
        self.assertIn("retention-days: 7\n", upload)
        self.assertIn("!cancelled()", upload)
        for name in ("test", "test-linux", "test-macos"):
            text = jobs()[name]
            self.assertTrue("steps: &test-steps" in text or "steps: *test-steps" in text, name)
        # A re-run job replaces its own report instead of failing.
        self.assertIn("overwrite: true\n", upload)

    def test_each_test_job_names_its_report_apart(self):
        # Two jobs uploading one name would fail the second upload. The
        # name is OS plus half; every test job has two halves.
        expr = "test-times-${{ matrix.os }}-${{ matrix.part == '2/2' && '2' || '1' }}"
        self.assertIn(f"name: {expr}\n", WORKFLOW)
        found = jobs()
        # The Windows legs are its matrix's `include` (the '' leg is excluded).
        include = found["test"].split("        include:\n", 1)[1]
        windows = re.findall(r"(?m)^            part: (\S+)$", include)
        self.assertEqual(windows, ["1/2", "2/2"])
        legs = [("windows-latest", part) for part in windows]
        for name, os_name in (("test-linux", "ubuntu-24.04"), ("test-macos", "macos-latest")):
            self.assertIn(f"        os: [{os_name}]\n        part: [1/2, 2/2]\n", found[name])
            legs += [(os_name, "1/2"), (os_name, "2/2")]
        names = [f"test-times-{o}-{'2' if part == '2/2' else '1'}" for o, part in legs]
        self.assertEqual(len(set(names)), 6, names)


def mode_script():
    """The text of the mode jobs' script."""
    return MODE_SCRIPT.read_text(encoding="utf-8")


# (event, event draft, event head, what `gh api` prints or None if it fails,
#  expected output line)
MODE_CASES = [
    ("push", "", "", "false abc", "full=true quick=false"),
    ("workflow_dispatch", "", "", "true abc", "full=true quick=false"),
    # #2368: the ready_for_review event still named the old head.
    ("pull_request", "false", "old", "false new", "full=false quick=false"),
    # #2368: the push's synchronize event still said draft.
    ("pull_request", "true", "new", "false new", "full=true quick=false"),
    ("pull_request", "true", "abc", "true abc", "full=false quick=true"),
    ("pull_request", "true", "abc", None, "full=false quick=true"),
    ("pull_request", "false", "abc", None, "full=true quick=false"),
    # Marked ready, then back to draft before the run started.
    ("pull_request", "false", "abc", "true abc", "full=false quick=true"),
]


@unittest.skipIf(os.name == "nt" or not shutil.which("bash"), "needs bash")
class ModeDecides(unittest.TestCase):
    """Runs mode's own script from ci.yml with a fake `gh` on PATH."""

    def run_mode(self, event, draft, head, live):
        with tempfile.TemporaryDirectory() as tmp:
            gh = pathlib.Path(tmp, "gh")
            gh.write_text("#!/bin/sh\n" + ("exit 1\n" if live is None else f"echo '{live}'\n"))
            gh.chmod(0o755)
            out = pathlib.Path(tmp, "out")
            env = dict(os.environ, PATH=f"{tmp}{os.pathsep}{os.environ['PATH']}",
                       GITHUB_OUTPUT=str(out), EVENT=event, EVENT_DRAFT=draft,
                       EVENT_HEAD=head, PR_URL="repos/o/r/pulls/1", GH_TOKEN="x")
            subprocess.run(["bash", "-c", mode_script()], env=env, check=True,
                           capture_output=True, text=True)
            return " ".join(out.read_text().split())

    def test_the_eight_cases(self):
        for event, draft, head, live, want in MODE_CASES:
            with self.subTest(event=event, draft=draft, head=head, live=live):
                self.assertEqual(self.run_mode(event, draft, head, live), want)


if __name__ == "__main__":
    unittest.main()
