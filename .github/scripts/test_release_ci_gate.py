"""Offline release gate contract tests. No network, build, tag or publishing."""

import copy
import io
import os
import pathlib
import re
import unittest
from contextlib import redirect_stdout
from unittest.mock import patch

import release_ci_gate as gate


SHA = "a" * 40


def successful_run():
    return dict(id=42, head_sha=SHA, event="push", head_branch="main",
                path=gate.WORKFLOW, repository={"full_name": gate.REPOSITORY},
                head_repository={"full_name": gate.REPOSITORY},
                run_attempt=1, status="completed", conclusion="success",
                html_url="https://github.com/example/actions/runs/42")


def successful_jobs():
    jobs = [dict(name=name, head_sha=SHA, run_attempt=1, status="completed",
                 conclusion="success") for name in gate.REQUIRED_JOBS]
    jobs[-1]["steps"] = [dict(name=gate.EQUIVALENCE_STEP, status="completed",
                              conclusion="success")]
    return jobs


class FakeAPI:
    def __init__(self, runs=None, jobs=None):
        self.runs = [successful_run()] if runs is None else runs
        self.jobs = successful_jobs() if jobs is None else jobs
        self.current = copy.deepcopy(self.runs[0]) if self.runs else None
        self.calls = []

    def get(self, path):
        self.calls.append(path)
        if path.startswith("/actions/workflows/"):
            return {"workflow_runs": copy.deepcopy(self.runs)}
        if "/jobs?" in path:
            return {"jobs": copy.deepcopy(self.jobs)}
        return copy.deepcopy(self.current)


class GateTests(unittest.TestCase):
    def test_publication_discriminator(self):
        self.assertTrue(gate.publication_required("push", "refs/tags/v0.1.0", False))
        self.assertTrue(gate.publication_required("workflow_dispatch", "refs/heads/main", True))
        for event, ref, release in [
            ("pull_request", "refs/pull/1/merge", True),
            ("workflow_dispatch", "refs/heads/main", False),
            ("workflow_dispatch", "refs/heads/experiment", True),
            ("workflow_dispatch", "refs/tags/v0.1.0", False),
            ("workflow_dispatch", "refs/tags/v0.1.0", True),
        ]:
            self.assertFalse(gate.publication_required(event, ref, release))
        for args in [("push", "refs/heads/main", False),
                     ("pull_request_target", "refs/heads/main", True),
                     ("workflow_dispatch", "refs/heads/main", "true")]:
            with self.assertRaises(gate.GateError):
                gate.publication_required(*args)

    def test_workflow_publication_predicate_and_build_dependency_stay_in_sync(self):
        workflow = pathlib.Path(__file__).parents[1] / "workflows/release.yml"
        text = workflow.read_text(encoding="utf-8")
        predicate = re.search(r"^  release:\n    if: >-\n(.*?)\n    needs:", text, re.M | re.S)
        self.assertIsNotNone(predicate)
        # Intentionally pin this small expression, rather than implement an
        # independent GitHub expression evaluator. Review helper + controls if
        # publication semantics change; a tag dispatch must never bypass it.
        self.assertEqual(" ".join(predicate[1].split()),
                         "(github.event_name == 'push' && github.ref_type == 'tag') || "
                         "(github.event_name == 'workflow_dispatch' && inputs.release && "
                         "github.ref == 'refs/heads/main')")
        # Every build waits for the gate, whatever else it needs.
        self.assertRegex(text, r"(?m)^  build:\n    needs: (ci-gate|\[([\w-]+, )*ci-gate(, [\w-]+)*\])\n")

    def test_equivalence_job_is_the_last_required_job(self):
        # successful_jobs() and the step tests below put the step on jobs[-1].
        self.assertEqual(gate.REQUIRED_JOBS[-1], gate.EQUIVALENCE_JOB)

    def test_required_jobs_are_the_ci_workflow_jobs(self):
        workflow = pathlib.Path(__file__).parents[1] / "workflows/ci.yml"
        text = workflow.read_text(encoding="utf-8")
        self.assertIn("os: [ubuntu-24.04, windows-latest, macos-latest]", text)
        self.assertIn("\n  mcp-renderer:\n", text)
        self.assertIn("name: regress (${{ matrix.check }})", text)
        self.assertIn("check: [engine, app, mcp]", text)
        self.assertIn(f"- name: {gate.EQUIVALENCE_STEP}\n", text)

    def test_seven_jobs_and_required_step_succeed(self):
        for event in ["push", "workflow_dispatch"]:
            api = FakeAPI()
            api.runs[0]["event"] = event
            run, names = gate.check(api, SHA, log=lambda _: None)
            self.assertEqual(run["id"], 42)
            self.assertEqual(tuple(names), gate.REQUIRED_JOBS)
            self.assertIn("filter=latest", api.calls[1])

    def test_no_matching_run_and_wrong_identity_fail(self):
        for field, value in [("head_sha", "b" * 40), ("event", "pull_request"),
                             ("head_branch", "topic"), ("path", "other.yml"),
                             ("repository", {"full_name": "fork/repo"}),
                             ("head_repository", {"full_name": "fork/repo"})]:
            with self.subTest(field=field):
                run = successful_run()
                run[field] = value
                with self.assertRaisesRegex(gate.GateError, "No trusted main CI"):
                    gate.check(FakeAPI([run]), SHA)
        with self.assertRaises(gate.GateError):
            gate.check(FakeAPI([]), SHA)

    def test_latest_failed_or_unknown_run_does_not_reuse_older_green(self):
        for status, conclusion in [("completed", "failure"), ("completed", "skipped"),
                                   ("completed", None), ("future_status", "success")]:
            newer = successful_run()
            newer.update(id=43, status=status, conclusion=conclusion)
            with self.subTest(status=status, conclusion=conclusion):
                with self.assertRaises(gate.GateError):
                    gate.check(FakeAPI([successful_run(), newer]), SHA, log=lambda _: None)

    def test_pending_wait_is_bounded_and_never_accepts_older_green(self):
        newer = successful_run()
        newer.update(id=43, status="in_progress", conclusion=None)
        sleeps = []
        ticks = iter([0, 0, 0, 31])
        with self.assertRaisesRegex(gate.GateError, "still pending"):
            gate.check(FakeAPI([successful_run(), newer]), SHA, wait_seconds=30,
                       sleep=sleeps.append, monotonic=lambda: next(ticks), log=lambda _: None)
        self.assertEqual(sleeps, [30])

    def test_every_required_job_missing_duplicate_bad_status_or_sha_fails(self):
        for index in range(len(gate.REQUIRED_JOBS)):
            for defect in ["missing", "duplicate", "skipped", "failed", "unknown", "sha"]:
                jobs = successful_jobs()
                if defect == "missing":
                    jobs.pop(index)
                elif defect == "duplicate":
                    jobs.append(copy.deepcopy(jobs[index]))
                elif defect == "sha":
                    jobs[index]["head_sha"] = "b" * 40
                elif defect == "unknown":
                    jobs[index]["status"] = "future_status"
                else:
                    jobs[index]["conclusion"] = defect
                with self.subTest(index=index, defect=defect):
                    with self.assertRaises(gate.GateError):
                        gate.check(FakeAPI(jobs=jobs), SHA, log=lambda _: None)

    def test_old_regress_without_equivalence_or_skipped_step_fails(self):
        for defect in ["absent", "empty", "duplicate", "skipped", "pending"]:
            jobs = successful_jobs()
            if defect == "absent":
                del jobs[-1]["steps"]
            elif defect == "empty":
                jobs[-1]["steps"] = []
            elif defect == "duplicate":
                jobs[-1]["steps"] *= 2
            elif defect == "skipped":
                jobs[-1]["steps"][0]["conclusion"] = "skipped"
            else:
                jobs[-1]["steps"][0]["status"] = "in_progress"
            with self.subTest(defect=defect):
                with self.assertRaisesRegex(gate.GateError, "equivalence|step inventory"):
                    gate.check(FakeAPI(jobs=jobs), SHA, log=lambda _: None)

    def test_partial_rerun_rejected_full_rerun_accepted(self):
        run = successful_run()
        run["run_attempt"] = 2
        jobs = successful_jobs()
        with self.assertRaisesRegex(gate.GateError, "rerun all"):
            gate.check(FakeAPI([run], jobs), SHA, log=lambda _: None)
        for job in jobs:
            job["run_attempt"] = 2
        self.assertEqual(gate.check(FakeAPI([run], jobs), SHA, log=lambda _: None)[0], run)

    def test_rerun_or_new_run_during_inspection_fails(self):
        api = FakeAPI()
        api.current["run_attempt"] = 2
        with self.assertRaisesRegex(gate.GateError, "changed during"):
            gate.check(api, SHA, log=lambda _: None)
        api = FakeAPI()
        original_get = api.get

        def newer(path):
            if path == "/actions/runs/42":
                api.runs.append(dict(successful_run(), id=43))
            return original_get(path)

        api.get = newer
        with self.assertRaisesRegex(gate.GateError, "changed during"):
            gate.check(api, SHA, log=lambda _: None)

    def test_pagination_and_malformed_inventories(self):
        class Paginated:
            def get(self, path):
                page = int(path.rsplit("=", 1)[1])
                return {"jobs": [{}] * (100 if page == 1 else 1), "total_count": 101}

        self.assertEqual(len(gate.pages(Paginated(), "/jobs?filter=latest", "jobs")), 101)
        for payload in [{}, {"jobs": None}, {"jobs": [None]},
                        {"jobs": [], "total_count": 1}]:
            api = FakeAPI()
            api.get = lambda _, value=payload: value
            with self.assertRaises(gate.GateError):
                gate.pages(api, "/jobs", "jobs")

    def test_api_error_and_invalid_candidate_fail_closed(self):
        api = FakeAPI()
        api.get = lambda _: (_ for _ in ()).throw(OSError("API unavailable"))
        with self.assertRaises(OSError):
            gate.check(api, SHA)
        with self.assertRaises(gate.GateError):
            gate.check(FakeAPI(), "main")

    def test_nonpublishing_entrypoint_never_calls_api_or_git(self):
        with patch.dict(os.environ, {"GITHUB_EVENT_NAME": "pull_request",
                                     "GITHUB_REF": "refs/pull/1/merge",
                                     "RELEASE_REQUESTED": "false",
                                     "GITHUB_STEP_SUMMARY": ""}, clear=True):
            with patch.object(gate, "ActionsAPI") as api:
                with patch.object(gate.subprocess, "check_output") as git:
                    with redirect_stdout(io.StringIO()) as output:
                        self.assertEqual(gate.main(), 0)
                    self.assertIn("NOT REQUIRED", output.getvalue())
                git.assert_not_called()
            api.assert_not_called()

    def test_publishing_entrypoint_success_and_failures(self):
        env = {"GITHUB_EVENT_NAME": "workflow_dispatch", "GITHUB_REF": "refs/heads/main",
               "RELEASE_REQUESTED": "true", "GITHUB_REPOSITORY": gate.REPOSITORY,
               "GITHUB_SHA": SHA, "GH_TOKEN": "offline", "GITHUB_STEP_SUMMARY": ""}
        for defect in [None, "fork", "checkout", "API"]:
            with self.subTest(defect=defect):
                current_env = dict(env)
                if defect == "fork":
                    current_env["GITHUB_REPOSITORY"] = "fork/repo"
                api = FakeAPI()
                if defect == "API":
                    api.runs = []
                with patch.dict(os.environ, current_env, clear=True):
                    with patch.object(gate, "ActionsAPI", return_value=api):
                        with patch.object(gate.subprocess, "check_output",
                                          return_value=("b" * 40 if defect == "checkout" else SHA)):
                            with redirect_stdout(io.StringIO()) as output:
                                self.assertEqual(gate.main(), int(defect is not None))
                            self.assertIn("CI accepted" if defect is None else "FAILED", output.getvalue())


if __name__ == "__main__":
    unittest.main()
