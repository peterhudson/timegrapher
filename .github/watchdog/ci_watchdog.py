"""Re-run CI jobs that GitHub's hosted runners failed, and flag stalled CI.

A failed or cancelled CI job is re-run only when GitHub's own annotation on it
says the runner was never acquired or was lost; a job that ran and failed is
never re-run. Runs from ci-watchdog.yml with main's copy of this file and the
workflow token: it reads no pull-request code. Standard library only.

  ci_watchdog.py completed RUN_ID   a CI run just finished
  ci_watchdog.py sweep              look for CI jobs queued too long
"""

import datetime
import json
import os
import sys
import urllib.parse
import urllib.request


REPOSITORY = "peterhudson/timegrapher"
WORKFLOW = ".github/workflows/ci.yml"
# The first run plus two automatic re-runs.
MAX_ATTEMPTS = 3
# Hosted runners normally start a CI job within ten minutes.
STALL_MINUTES = 45
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
    """The open pull request this run tested, or None."""
    for pr in run.get("pull_requests") or []:
        return pr["number"]
    # Runs from forks list no pull requests; match the head commit instead.
    for pr in pages(api, "/pulls?state=open"):
        if pr.get("head", {}).get("sha") == run.get("head_sha"):
            return pr["number"]
    return None


def flag(api, run, kind, text):
    """Post text once per run, attempt and kind: on the PR, or main's issue."""
    marker = f"<!-- ci-watchdog:{kind}:{run['id']}:{run.get('run_attempt', 1)} -->"
    body = f"{marker}\n{text}"
    number = None if on_main(run) else pull_request(api, run)
    if number is None:
        issues = [issue for issue in pages(api, "/issues?state=open&creator=github-actions%5Bbot%5D")
                  if MAIN_ISSUE in (issue.get("body") or "") and "pull_request" not in issue]
        if not issues:
            api.post("/issues", {"title": "CI stalled", "body": f"{MAIN_ISSUE}\n{body}"})
            return "opened an issue"
        number = issues[0]["number"]
    comments = pages(api, f"/issues/{number}/comments")
    if any(marker in (comment.get("body") or "") for comment in comments):
        return f"already flagged on #{number}"
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
    jobs = pages(api, f"/actions/runs/{run_id}/jobs?filter=latest", "jobs")
    annotations = {job["id"]: pages(api, f"/check-runs/{job['id']}/annotations")
                   for job in jobs if job.get("conclusion") in FAILED}
    action, why = plan(run, jobs, annotations, superseded(api, run))
    log(f"run {run_id} attempt {run.get('run_attempt')}: {action}: {why}")
    if action == "rerun":
        # The Release gate wants every job from one attempt on main.
        api.post(f"/actions/runs/{run_id}/{'rerun' if on_main(run) else 'rerun-failed-jobs'}")
    elif action == "give-up":
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
            late = stalled(pages(api, f"/actions/runs/{run['id']}/jobs?filter=latest", "jobs"), now)
            for name, minutes in late:
                found += 1
                log(f"run {run['id']}: {name} queued {minutes} minutes")
            if late:
                waits = ", ".join(f"`{name}` for {minutes} minutes" for name, minutes in late)
                log(flag(api, run, "stalled",
                         f"{run_link(run)}: waiting for a GitHub-hosted runner: {waits}. "
                         f"CI jobs normally start within 10 minutes."))
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
