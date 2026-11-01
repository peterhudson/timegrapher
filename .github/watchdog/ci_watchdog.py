"""Re-run CI jobs that GitHub's hosted runners failed, and flag stalled CI.

A failed or cancelled CI job is re-run only when GitHub's own annotation on it
says the runner was never acquired or was lost; a job that ran and failed is
never re-run. A job on main that failed on its own is flagged on an open
"CI failed on main" issue: main's CI, unlike a pull request's, has no one
waiting on it, and pull requests read only a cut of the corpus (ci.yml). Runs from ci-watchdog.yml with main's copy of this file and the
workflow token: it reads no pull-request code. Standard library only.

  ci_watchdog.py completed RUN_ID   a CI run just finished
  ci_watchdog.py sweep              look for CI jobs queued too long, and
                                    re-check recent failed CI runs
"""

import datetime
import json
import os
import sys
import urllib.error
import urllib.parse
import urllib.request


REPOSITORY = "peterhudson/timegrapher"
WORKFLOW = ".github/workflows/ci.yml"
# The first run plus two automatic re-runs.
MAX_ATTEMPTS = 3
# Hosted runners normally start a CI job within ten minutes.
STALL_MINUTES = 45
# The sweep re-checks failed CI runs created this recently, in case GitHub
# starts no workflow_run event when a run the workflow token re-ran finishes.
RECHECK_HOURS = 6
FAILED = {"failure", "cancelled", "timed_out", "startup_failure"}
# GitHub's annotations for a hosted runner that never started a job or was
# lost during one. A test failure or a timeout-minutes cancel carries none.
INFRA_MESSAGES = (
    "was not acquired by Runner",
    "lost communication with the server",
    "runner has received a shutdown signal",
    "hosted runner encountered an error",
)
MAIN_ISSUE = "<!-- ci-watchdog:main -->"
MAIN_FAILED_ISSUE = "<!-- ci-watchdog:main-failed -->"
# GitHub's annotation on a job someone cancelled by hand.
HAND_CANCEL = "was canceled by"


class API:
    def __init__(self, repository, token):
        self.base = f"https://api.github.com/repos/{repository}"
        self.token = token

    def request(self, method, path, body=None):
        data = None if body is None else json.dumps(body).encode()
        request = urllib.request.Request(self.base + path, data=data, method=method, headers={
            "Accept": "application/vnd.github+json",
            "Authorization": f"Bearer {self.token}",
            "X-GitHub-Api-Version": "2022-11-28",
        })
        with urllib.request.urlopen(request, timeout=30) as response:
            raw = response.read()
            return json.loads(raw) if raw else None

    def get(self, path):
        return self.request("GET", path)

    def post(self, path, body=None):
        return self.request("POST", path, {} if body is None else body)


def pages(api, path, key=None):
    """Every row of a list endpoint, up to 1,000."""
    rows = []
    separator = "&" if "?" in path else "?"
    for page in range(1, 11):
        payload = api.get(f"{path}{separator}per_page=100&page={page}")
        batch = payload if key is None else payload[key]
        rows.extend(batch)
        if len(batch) < 100:
            break
    return rows


def is_ci(run):
    return (run.get("path") == WORKFLOW
            and run.get("repository", {}).get("full_name") == REPOSITORY)


def on_main(run):
    return run.get("event") in {"push", "workflow_dispatch"} and run.get("head_branch") == "main"


def infra_message(annotations):
    """GitHub's message that the runner failed the job, or None."""
    for annotation in annotations:
        message = annotation.get("message") or ""
        if any(text in message for text in INFRA_MESSAGES):
            return message.strip()
    return None


def superseded(api, run):
    """A newer CI run for the same pull-request branch replaced this one."""
    if run.get("event") != "pull_request":
        return False
    branch = urllib.parse.quote(run.get("head_branch") or "", safe="")
    newer = pages(api, f"/actions/workflows/ci.yml/runs?event=pull_request&branch={branch}",
                  "workflow_runs")
    head = run.get("head_repository", {}).get("full_name")
    return any(other["id"] > run["id"] and other.get("head_repository", {}).get("full_name") == head
               for other in newer)


def plan(run, jobs, annotations, newer):
    """("rerun", why), ("give-up", why) or ("skip", why) for a completed CI run.

    annotations maps a job id to that job's check-run annotations.
    """
    if not is_ci(run):
        return "skip", "not this repository's CI workflow"
    if run.get("status") != "completed" or run.get("conclusion") not in FAILED:
        return "skip", f"run is {run.get('status')}/{run.get('conclusion')}"
    if newer:
        return "skip", "a newer run for the same branch replaced it"
    failed = [job for job in jobs if job.get("conclusion") in FAILED]
    if not failed:
        return "skip", "no failed jobs"
    reasons = []
    for job in failed:
        message = infra_message(annotations.get(job["id"], []))
        if message is None:
            return "skip", f"{job['name']} failed on its own ({job.get('conclusion')})"
        reasons.append(f"`{job['name']}`: {message}")
    why = "; ".join(reasons)
    if run.get("run_attempt", 1) >= MAX_ATTEMPTS:
        return "give-up", why
    return "rerun", why


def failed_on_its_own(jobs, annotations):
    """The names of the failed jobs that neither GitHub's runners failed nor
    someone cancelled by hand."""
    def hand_cancelled(job):
        return job.get("conclusion") == "cancelled" and any(
            HAND_CANCEL in (a.get("message") or "") for a in annotations.get(job["id"], []))
    return [job["name"] for job in jobs if job.get("conclusion") in FAILED
            and infra_message(annotations.get(job["id"], [])) is None
            and not hand_cancelled(job)]


def stalled(jobs, now):
    """(job name, minutes) for each job still queued after STALL_MINUTES."""
    late = []
    for job in jobs:
        if job.get("status") != "queued" or not job.get("created_at"):
            continue
        minutes = int((now - parse_time(job["created_at"])).total_seconds() // 60)
        if minutes >= STALL_MINUTES:
            late.append((job["name"], minutes))
    return late


def parse_time(text):
    return datetime.datetime.fromisoformat(text.replace("Z", "+00:00"))


def pull_request(api, run):
    """The open pull request whose head is this run's commit, or None."""
    # GitHub lists the pull requests open on the run's branch now, which can
    # be a newer one than the run tested, so check the state and the head.
    for listed in run.get("pull_requests") or []:
        pr = api.get(f"/pulls/{listed['number']}")
        if pr.get("state") == "open" and pr.get("head", {}).get("sha") == run.get("head_sha"):
            return pr["number"]
    # Runs from forks list no pull requests; match the head commit instead.
    for pr in pages(api, "/pulls?state=open"):
        if pr.get("head", {}).get("sha") == run.get("head_sha"):
            return pr["number"]
    return None


def flag(api, run, kind, text, title="CI stalled", main_issue=MAIN_ISSUE):
    """Post text once per run, attempt and kind: on the PR, or main's issue
    (the open issue the watchdog opened with main_issue in it, else a new one
    titled title).

    The marker is looked for in every issue the watchdog opened and every
    comment updated since the run began, so closing the "CI stalled" issue
    does not bring the same flag back on a new one.
    """
    marker = f"<!-- ci-watchdog:{kind}:{run['id']}:{run.get('run_attempt', 1)} -->"
    body = f"{marker}\n{text}"
    since = urllib.parse.quote(run.get("created_at") or "", safe="")
    ours = pages(api, f"/issues?state=all&creator=github-actions%5Bbot%5D&since={since}")
    comments = pages(api, f"/issues/comments?since={since}")
    if any(marker in (row.get("body") or "") for row in ours + comments):
        return "already flagged"
    number = None if on_main(run) else pull_request(api, run)
    if number is None:
        # Not the since-filtered list: an open issue untouched since the run
        # began is still the one to comment on.
        issues = [issue for issue in pages(api, "/issues?state=open&creator=github-actions%5Bbot%5D")
                  if main_issue in (issue.get("body") or "") and "pull_request" not in issue]
        if not issues:
            api.post("/issues", {"title": title, "body": f"{main_issue}\n{body}"})
            return "opened an issue"
        number = issues[0]["number"]
    api.post(f"/issues/{number}/comments", {"body": body})
    return f"commented on #{number}"


def run_link(run):
    return (f"[CI run {run['id']}, attempt {run.get('run_attempt', 1)}]({run.get('html_url')}) "
            f"on `{run.get('head_sha', '')[:7]}`")


def completed(api, run_id, log=print):
    run = api.get(f"/actions/runs/{run_id}")
    if not is_ci(run):
        log(f"run {run_id}: not this repository's CI workflow")
        return "skip"
    if run.get("event") == "pull_request" and pull_request(api, run) is None:
        log(f"run {run_id}: its pull request is closed")
        return "skip"
    jobs = pages(api, f"/actions/runs/{run_id}/jobs?filter=latest", "jobs")
    annotations = {job["id"]: pages(api, f"/check-runs/{job['id']}/annotations")
                   for job in jobs if job.get("conclusion") in FAILED}
    action, why = plan(run, jobs, annotations, superseded(api, run))
    log(f"run {run_id} attempt {run.get('run_attempt')}: {action}: {why}")
    if action == "rerun":
        # The Release gate wants every job from one attempt on main.
        api.post(f"/actions/runs/{run_id}/{'rerun' if on_main(run) else 'rerun-failed-jobs'}")
    elif on_main(run) and run.get("conclusion") in FAILED:
        names = failed_on_its_own(jobs, annotations)
        if names:
            jobs_text = ", ".join(f"`{name}`" for name in names)
            log(flag(api, run, "failed",
                     f"{run_link(run)}: {jobs_text} failed. Pull requests read only a cut "
                     f"of the corpus, so main is where the whole corpus is read.",
                     title="CI failed on main", main_issue=MAIN_FAILED_ISSUE))
    if action == "give-up":
        log(flag(api, run, "give-up",
                 f"{run_link(run)}: GitHub's hosted runners failed it on all {MAX_ATTEMPTS} "
                 f"attempts ({why}). The CI watchdog re-runs it no further."))
    return action


def sweep(api, now, log=print):
    found = 0
    for status in ("queued", "in_progress"):
        for run in pages(api, f"/actions/workflows/ci.yml/runs?status={status}", "workflow_runs"):
            if not is_ci(run):
                continue
            if run.get("event") == "pull_request" and pull_request(api, run) is None:
                continue
            late = stalled(pages(api, f"/actions/runs/{run['id']}/jobs?filter=latest", "jobs"), now)
            for name, minutes in late:
                found += 1
                log(f"run {run['id']}: {name} queued {minutes} minutes")
            if late:
                waits = ", ".join(f"`{name}` for {minutes} minutes" for name, minutes in late)
                log(flag(api, run, "stalled",
                         f"{run_link(run)}: waiting for a GitHub-hosted runner: {waits}. "
                         f"CI jobs normally start within 10 minutes."))
    since = (now - datetime.timedelta(hours=RECHECK_HOURS)).strftime("%Y-%m-%dT%H:%M:%SZ")
    for run in pages(api, f"/actions/workflows/ci.yml/runs?status=completed&created=%3E%3D{since}",
                     "workflow_runs"):
        if is_ci(run) and run.get("conclusion") in FAILED:
            try:
                completed(api, run["id"], log)
            except urllib.error.HTTPError as error:
                # Most likely a re-run the workflow_run job started meanwhile.
                log(f"run {run['id']}: {error}")
    return found


def main(argv):
    api = API(os.environ.get("GITHUB_REPOSITORY", REPOSITORY), os.environ["GITHUB_TOKEN"])
    if len(argv) == 3 and argv[1] == "completed":
        completed(api, int(argv[2]))
    elif len(argv) == 2 and argv[1] == "sweep":
        sweep(api, datetime.datetime.now(datetime.timezone.utc))
    else:
        raise SystemExit(__doc__)


if __name__ == "__main__":
    main(sys.argv)
