"""Offline CI watchdog tests. No network: the GitHub API is a fake."""

import datetime
import unittest
import urllib.parse

import ci_watchdog as watchdog


NOT_ACQUIRED = "The job was not acquired by Runner of type hosted even after multiple attempts"
CAPACITY = "Due to capacity constraints, jobs targeting macOS arm64 runners may experience longer queue times."


def ci_run(**changes):
    run = dict(id=50, path=watchdog.WORKFLOW, repository={"full_name": watchdog.REPOSITORY},
               head_repository={"full_name": watchdog.REPOSITORY}, head_sha="b" * 40,
               head_branch="feature", event="pull_request", pull_requests=[{"number": 7}],
               run_attempt=1, status="completed", conclusion="failure",
               created_at="2026-10-21T16:00:00Z",
               html_url="https://github.com/example/actions/runs/50")
    run.update(changes)
    return run


def job(id, name, conclusion="success", status="completed", created_at=None):
    return dict(id=id, name=name, status=status, conclusion=conclusion, created_at=created_at)


def macos_stall():
    """Today's #319 run: four jobs green, macOS never given a runner."""
    jobs = [job(1, "test (ubuntu-24.04)"), job(2, "test (windows-latest)"),
            job(3, "test (macos-latest)", "cancelled"), job(4, "mcp-renderer"), job(5, "regress")]
    annotations = {3: [dict(message=NOT_ACQUIRED), dict(message=CAPACITY)]}
    return jobs, annotations


class FakeAPI:
    def __init__(self, run, jobs, annotations, newer=(), comments=(), issues=(), pulls=(),
                 closed=(), moved=()):
        self.run, self.jobs, self.annotations = run, jobs, annotations
        self.newer, self.comments, self.issues, self.pulls = list(newer), list(comments), list(issues), list(pulls)
        self.closed, self.moved = set(closed), set(moved)
        self.posts = []

    def get(self, path):
        path = path.split("per_page=")[0].rstrip("?&")
        if path == f"/actions/runs/{self.run['id']}":
            return self.run
        if path == f"/actions/runs/{self.run['id']}/jobs?filter=latest":
            return {"jobs": self.jobs}
        if path.startswith("/check-runs/"):
            return self.annotations.get(int(path.split("/")[2]), [])
        if path.startswith("/actions/workflows/ci.yml/runs?event=pull_request"):
            return {"workflow_runs": [self.run] + self.newer}
        if path.startswith("/actions/workflows/ci.yml/runs?status="):
            status = path.split("status=")[1].split("&")[0]
            return {"workflow_runs": [self.run] if status == self.run["status"] else []}
        # Like GitHub, since= keeps rows updated at or after it; a row with no
        # updated_at stands for one just written.
        since = urllib.parse.unquote(path.split("since=")[1].split("&")[0]) if "since=" in path else ""
        recent = lambda rows: [row for row in rows if row.get("updated_at", "9999") >= since]
        if path.startswith("/issues/comments?since="):
            # Every comment in the repository: pull requests and issues alike.
            return recent(self.comments)
        if path.startswith("/issues?state=all"):
            return recent(self.issues)
        if path.startswith("/issues?state=open"):
            return [issue for issue in self.issues if issue.get("state") == "open"]
        if path.startswith("/pulls/"):
            number = int(path.split("/")[2])
            head = "c" * 40 if number in self.moved else self.run["head_sha"]
            return dict(number=number, state="closed" if number in self.closed else "open",
                        head=dict(sha=head))
        if path.startswith("/pulls?"):
            return self.pulls
        raise AssertionError(f"unexpected GET {path}")

    def post(self, path, body=None):
        self.posts.append((path, body))


def quiet(_):
    pass


class Plan(unittest.TestCase):
    def test_runner_never_acquired_is_rerun(self):
        jobs, annotations = macos_stall()
        action, why = watchdog.plan(ci_run(conclusion="cancelled"), jobs, annotations, False)
        self.assertEqual(action, "rerun")
        self.assertIn("test (macos-latest)", why)

    def test_lost_runner_is_rerun(self):
        jobs = [job(1, "regress", "failure")]
        annotations = {1: [dict(message="The self-hosted runner: GitHub Actions 7 lost communication "
                                        "with the server.")]}
        self.assertEqual(watchdog.plan(ci_run(), jobs, annotations, False)[0], "rerun")

    def test_a_job_that_failed_on_its_own_is_never_rerun(self):
        jobs = [job(1, "regress", "failure")]
        self.assertEqual(watchdog.plan(ci_run(), jobs, {1: []}, False)[0], "skip")
        notice_only = {1: [dict(message=CAPACITY)]}
        self.assertEqual(watchdog.plan(ci_run(), jobs, notice_only, False)[0], "skip")

    def test_a_timeout_is_not_infrastructure(self):
        jobs = [job(1, "test (windows-latest)", "cancelled")]
        timeout = {1: [dict(message="The job has exceeded the maximum execution time of 45m0s")]}
        self.assertEqual(watchdog.plan(ci_run(conclusion="cancelled"), jobs, timeout, False)[0], "skip")

    def test_one_real_failure_beside_a_stall_blocks_the_rerun(self):
        jobs, annotations = macos_stall()
        jobs[0]["conclusion"] = "failure"
        self.assertEqual(watchdog.plan(ci_run(), jobs, annotations, False)[0], "skip")

    def test_superseded_runs_are_left_alone(self):
        jobs, annotations = macos_stall()
        self.assertEqual(watchdog.plan(ci_run(conclusion="cancelled"), jobs, annotations, True)[0], "skip")

    def test_two_automatic_reruns_then_give_up(self):
        jobs, annotations = macos_stall()
        self.assertEqual(watchdog.plan(ci_run(run_attempt=2), jobs, annotations, False)[0], "rerun")
        self.assertEqual(watchdog.plan(ci_run(run_attempt=3), jobs, annotations, False)[0], "give-up")

    def test_green_pending_and_foreign_runs_are_skipped(self):
        jobs, annotations = macos_stall()
        for run in (ci_run(conclusion="success"), ci_run(status="in_progress", conclusion=None),
                    ci_run(path=".github/workflows/release.yml"),
                    ci_run(repository={"full_name": "someone/fork"})):
            self.assertEqual(watchdog.plan(run, jobs, annotations, False)[0], "skip")


class Completed(unittest.TestCase):
    def test_pull_request_reruns_only_failed_jobs(self):
        jobs, annotations = macos_stall()
        api = FakeAPI(ci_run(), jobs, annotations)
        self.assertEqual(watchdog.completed(api, 50, log=quiet), "rerun")
        self.assertEqual(api.posts, [("/actions/runs/50/rerun-failed-jobs", None)])

    def test_main_reruns_every_job_for_the_release_gate(self):
        jobs, annotations = macos_stall()
        api = FakeAPI(ci_run(event="push", head_branch="main", pull_requests=[]), jobs, annotations)
        watchdog.completed(api, 50, log=quiet)
        self.assertEqual(api.posts, [("/actions/runs/50/rerun", None)])

    def test_a_newer_push_on_the_branch_stops_the_rerun(self):
        jobs, annotations = macos_stall()
        newer = ci_run(id=51)
        api = FakeAPI(ci_run(), jobs, annotations, newer=[newer])
        self.assertEqual(watchdog.completed(api, 50, log=quiet), "skip")
        self.assertEqual(api.posts, [])

    def test_a_newer_run_from_another_fork_does_not_count(self):
        jobs, annotations = macos_stall()
        other = ci_run(id=51, head_repository={"full_name": "someone/fork"})
        api = FakeAPI(ci_run(), jobs, annotations, newer=[other])
        self.assertEqual(watchdog.completed(api, 50, log=quiet), "rerun")

    def test_giving_up_comments_once_on_the_pull_request(self):
        jobs, annotations = macos_stall()
        api = FakeAPI(ci_run(run_attempt=3), jobs, annotations)
        watchdog.completed(api, 50, log=quiet)
        [(path, body)] = api.posts
        self.assertEqual(path, "/issues/7/comments")
        self.assertIn("all 3 attempts", body["body"])
        self.assertIn(NOT_ACQUIRED, body["body"])
        api.comments, api.posts = [body], []
        watchdog.completed(api, 50, log=quiet)
        self.assertEqual(api.posts, [])

    def test_a_fork_pull_request_is_found_by_its_head(self):
        jobs, annotations = macos_stall()
        run = ci_run(run_attempt=3, pull_requests=[],
                     head_repository={"full_name": "someone/fork"})
        api = FakeAPI(run, jobs, annotations, pulls=[dict(number=9, head=dict(sha=run["head_sha"]))])
        watchdog.completed(api, 50, log=quiet)
        self.assertEqual(api.posts[0][0], "/issues/9/comments")


class Sweep(unittest.TestCase):
    NOW = datetime.datetime(2026, 10, 21, 17, 0, tzinfo=datetime.timezone.utc)

    def queued_run(self, created_at):
        run = ci_run(status="queued", conclusion=None)
        jobs = [job(1, "regress"), job(3, "test (macos-latest)", None, "queued", created_at)]
        return run, jobs

    def test_a_job_queued_45_minutes_is_flagged_once(self):
        run, jobs = self.queued_run("2026-10-21T16:10:00Z")
        api = FakeAPI(run, jobs, {})
        self.assertEqual(watchdog.sweep(api, self.NOW, log=quiet), 1)
        [(path, body)] = api.posts
        self.assertEqual(path, "/issues/7/comments")
        self.assertIn("`test (macos-latest)` for 50 minutes", body["body"])
        api.comments, api.posts = [body], []
        watchdog.sweep(api, self.NOW, log=quiet)
        self.assertEqual(api.posts, [])

    def test_a_normal_queue_is_not_flagged(self):
        run, jobs = self.queued_run("2026-10-21T16:30:00Z")
        api = FakeAPI(run, jobs, {})
        self.assertEqual(watchdog.sweep(api, self.NOW, log=quiet), 0)
        self.assertEqual(api.posts, [])

    def test_a_stall_on_main_opens_an_issue_then_comments_on_it(self):
        run, jobs = self.queued_run("2026-10-21T16:00:00Z")
        run.update(event="push", head_branch="main", pull_requests=[])
        api = FakeAPI(run, jobs, {})
        watchdog.sweep(api, self.NOW, log=quiet)
        [(path, body)] = api.posts
        self.assertEqual(path, "/issues")
        self.assertIn(watchdog.MAIN_ISSUE, body["body"])
        # GitHub lists an issue's comments without its body.
        api.issues, api.posts = [dict(number=12, state="open", body=body["body"])], []
        watchdog.sweep(api, self.NOW, log=quiet)
        self.assertEqual(api.posts, [])
        api.run = dict(run, id=60)
        watchdog.sweep(api, self.NOW, log=quiet)
        self.assertEqual([path for path, _ in api.posts], ["/issues/12/comments"])

    def test_a_later_stall_comments_on_an_open_issue_untouched_since(self):
        run, jobs = self.queued_run("2026-10-21T16:00:00Z")
        run.update(event="push", head_branch="main", pull_requests=[])
        older = dict(number=12, state="open", updated_at="2026-10-20T09:00:00Z",
                     body=f"{watchdog.MAIN_ISSUE}\n<!-- ci-watchdog:stalled:40:1 -->")
        api = FakeAPI(run, jobs, {}, issues=[older])
        watchdog.sweep(api, self.NOW, log=quiet)
        self.assertEqual([path for path, _ in api.posts], ["/issues/12/comments"])

    def test_a_queued_run_of_a_closed_pull_request_is_not_flagged(self):
        run, jobs = self.queued_run("2026-10-21T16:00:00Z")
        api = FakeAPI(run, jobs, {}, closed={7})
        watchdog.sweep(api, self.NOW, log=quiet)
        self.assertEqual(api.posts, [])

    def test_closing_the_issue_does_not_bring_the_same_flag_back(self):
        run, jobs = self.queued_run("2026-10-21T16:00:00Z")
        run.update(event="push", head_branch="main", pull_requests=[])
        api = FakeAPI(run, jobs, {})
        watchdog.sweep(api, self.NOW, log=quiet)
        [(_, body)] = api.posts
        api.issues, api.posts = [dict(number=12, state="closed", body=body["body"])], []
        watchdog.sweep(api, self.NOW, log=quiet)
        self.assertEqual(api.posts, [])
        # A later stall opens a fresh issue.
        api.run = dict(run, id=60)
        watchdog.sweep(api, self.NOW, log=quiet)
        self.assertEqual([path for path, _ in api.posts], ["/issues"])

    def test_a_closed_pull_requests_run_is_left_alone(self):
        jobs, annotations = macos_stall()
        for attempt in (1, 3):
            api = FakeAPI(ci_run(run_attempt=attempt), jobs, annotations, closed={7})
            watchdog.sweep(api, self.NOW, log=quiet)
            self.assertEqual(api.posts, [])

    def test_a_new_pull_request_on_the_same_branch_is_not_this_runs(self):
        # GitHub lists #8, opened on the branch after #7 merged, on #7's old runs.
        jobs, annotations = macos_stall()
        api = FakeAPI(ci_run(run_attempt=3, pull_requests=[{"number": 8}]), jobs, annotations,
                      moved={8})
        watchdog.sweep(api, self.NOW, log=quiet)
        self.assertEqual(api.posts, [])

    def test_a_failed_run_the_event_missed_is_rerun(self):
        jobs, annotations = macos_stall()
        api = FakeAPI(ci_run(run_attempt=2), jobs, annotations)
        watchdog.sweep(api, self.NOW, log=quiet)
        self.assertEqual(api.posts, [("/actions/runs/50/rerun-failed-jobs", None)])

    def test_a_real_failure_on_main_opens_an_issue_once(self):
        run = ci_run(event="push", head_branch="main", pull_requests=[])
        jobs = [job(1, "test (ubuntu-24.04)"), job(2, "regress (app)", "failure")]
        api = FakeAPI(run, jobs, {2: []})
        self.assertEqual(watchdog.completed(api, 50, log=quiet), "skip")
        [(path, body)] = api.posts
        self.assertEqual((path, body["title"]), ("/issues", "CI failed on main"))
        self.assertIn(watchdog.MAIN_FAILED_ISSUE, body["body"])
        self.assertNotIn(watchdog.MAIN_ISSUE, body["body"])
        self.assertIn("`regress (app)` failed", body["body"])
        self.assertNotIn("test (ubuntu-24.04)", body["body"])
        # The sweep's re-check of the same run posts nothing more.
        api.issues, api.posts = [dict(number=13, state="open", body=body["body"])], []
        watchdog.completed(api, 50, log=quiet)
        self.assertEqual(api.posts, [])
        # A later failure comments on the open issue, not on a stall issue.
        stall = dict(number=12, state="open", body=f"{watchdog.MAIN_ISSUE}\nstalled")
        api.issues.append(stall)
        api.run = dict(run, id=60)
        watchdog.completed(api, 60, log=quiet)
        self.assertEqual([path for path, _ in api.posts], ["/issues/13/comments"])

    def test_a_main_run_the_runners_failed_is_rerun_not_flagged(self):
        jobs, annotations = macos_stall()
        api = FakeAPI(ci_run(event="push", head_branch="main", pull_requests=[]), jobs, annotations)
        self.assertEqual(watchdog.completed(api, 50, log=quiet), "rerun")
        self.assertEqual(api.posts, [("/actions/runs/50/rerun", None)])

    def test_a_main_run_cancelled_by_hand_is_not_flagged(self):
        run = ci_run(event="push", head_branch="main", pull_requests=[], conclusion="cancelled")
        jobs = [job(1, "regress (mcp)", "cancelled")]
        cancelled = {1: [dict(message="The run was canceled by @someone.")]}
        api = FakeAPI(run, jobs, cancelled)
        watchdog.completed(api, 50, log=quiet)
        self.assertEqual(api.posts, [])
        # A job that hit its time limit is flagged.
        limit = {1: [dict(message="The job has exceeded the maximum execution time of 45m0s")]}
        api = FakeAPI(run, jobs, limit)
        watchdog.completed(api, 50, log=quiet)
        self.assertEqual([path for path, _ in api.posts], ["/issues"])

    def test_a_real_failure_on_a_pull_request_is_not_flagged(self):
        api = FakeAPI(ci_run(), [job(1, "regress (app)", "failure")], {1: []})
        watchdog.completed(api, 50, log=quiet)
        self.assertEqual(api.posts, [])

    def test_the_sweep_leaves_real_failures_alone(self):
        jobs = [job(1, "regress", "failure")]
        api = FakeAPI(ci_run(), jobs, {1: []})
        watchdog.sweep(api, self.NOW, log=quiet)
        self.assertEqual(api.posts, [])


if __name__ == "__main__":
    unittest.main()
