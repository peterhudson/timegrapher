"""Require successful CI for the exact commit before release-capable builds.

Read-only Actions API access; no release, tag or rerun operations. Keep the job
and step contract below synchronized with ci.yml and docs/install.md.
"""

import json
import os
import re
import subprocess
import sys
import time
import urllib.request


REPOSITORY = "peterhudson/timegrapher"
WORKFLOW = ".github/workflows/ci.yml"
REQUIRED_JOBS = (
    "test (ubuntu-24.04)",
    "test (windows-latest)",
    "test (macos-latest)",
    "mcp-renderer",
    "regress",
)
EQUIVALENCE_STEP = "Required real-recording analyze/session equivalence"
PENDING = {"queued", "in_progress", "waiting", "pending", "requested"}


class GateError(Exception):
    pass


def publication_required(event, ref, release):
    if event == "push":
        if not ref.startswith("refs/tags/v"):
            raise GateError("Release push must name a v* tag")
        return True
    if event == "workflow_dispatch":
        if not isinstance(release, bool):
            raise GateError("workflow_dispatch release input must be boolean")
        return release and ref == "refs/heads/main"
    if event == "pull_request":
        return False
    raise GateError(f"Unsupported release event: {event}")


class ActionsAPI:
    def __init__(self, repository, token):
        self.base = f"https://api.github.com/repos/{repository}"
        self.token = token

    def get(self, path):
        request = urllib.request.Request(self.base + path, headers={
            "Accept": "application/vnd.github+json",
            "Authorization": f"Bearer {self.token}",
            "X-GitHub-Api-Version": "2022-11-28",
        })
        with urllib.request.urlopen(request, timeout=30) as response:
            return json.load(response)


def pages(api, path, key):
    """Do not silently accept a truncated run/job inventory."""
    result = []
    separator = "&" if "?" in path else "?"
    for page in range(1, 11):
        payload = api.get(f"{path}{separator}per_page=100&page={page}")
        rows = payload.get(key)
        if not isinstance(rows, list) or not all(isinstance(r, dict) for r in rows):
            raise GateError(f"Malformed {key} API response")
        result.extend(rows)
        if len(rows) < 100:
            if payload.get("total_count", len(result)) > len(result):
                raise GateError(f"Incomplete {key} API response")
            return result
    raise GateError(f"Too many {key}; refusing a truncated inventory")


def latest_run(api, candidate):
    runs = pages(api, "/actions/workflows/ci.yml/runs?head_sha=" + candidate,
                 "workflow_runs")
    eligible = []
    for run in runs:
        if (run.get("head_sha") == candidate
                and run.get("event") in {"push", "workflow_dispatch"}
                and run.get("head_branch") == "main"
                and run.get("path") == WORKFLOW
                and run.get("repository", {}).get("full_name") == REPOSITORY
                and run.get("head_repository", {}).get("full_name") == REPOSITORY):
            if not isinstance(run.get("id"), int):
                raise GateError("Malformed CI run ID")
            eligible.append(run)
    if not eligible:
        raise GateError("No trusted main CI run for this exact commit. Run CI on "
                        "this main revision first (including Markdown-only changes).")
    # Never fall back to an older green run when a newer run fails or is pending.
    return max(eligible, key=lambda run: run["id"])


def validate_jobs(run, jobs):
    attempt = run.get("run_attempt")
    if not isinstance(attempt, int) or attempt < 1:
        raise GateError("Malformed CI run attempt")
    accepted = []
    for name in REQUIRED_JOBS:
        matches = [job for job in jobs if job.get("name") == name]
        if len(matches) != 1:
            raise GateError(f"Expected exactly one CI job {name!r}; found {len(matches)}")
        job = matches[0]
        if job.get("run_attempt") != attempt:
            raise GateError(f"{name}: partial/stale attempt; rerun all CI jobs")
        if job.get("head_sha") != run["head_sha"]:
            raise GateError(f"{name}: candidate SHA mismatch")
        if job.get("status") != "completed" or job.get("conclusion") != "success":
            raise GateError(f"{name}: requires completed/success, got "
                            f"{job.get('status')}/{job.get('conclusion')}")
        if name == "regress":
            steps = job.get("steps")
            if not isinstance(steps, list):
                raise GateError("regress: missing step inventory")
            equivalent = [step for step in steps if step.get("name") == EQUIVALENCE_STEP]
            if (len(equivalent) != 1 or equivalent[0].get("status") != "completed"
                    or equivalent[0].get("conclusion") != "success"):
                raise GateError("regress: required real-recording equivalence step "
                                "must be present and completed/success")
        accepted.append(name)
    return accepted


def check(api, candidate, wait_seconds=2400, sleep=time.sleep, monotonic=time.monotonic,
          log=print):
    if not re.fullmatch(r"[0-9a-f]{40}", candidate):
        raise GateError("Candidate must be a full commit SHA")
    deadline = monotonic() + wait_seconds
    while True:
        run = latest_run(api, candidate)
        log(f"CI run {run['id']} ({run.get('html_url')}), head {candidate}, attempt {run.get('run_attempt')}: "
            f"{run.get('status')}/{run.get('conclusion')}")
        status = run.get("status")
        if status in PENDING:
            if monotonic() >= deadline:
                raise GateError("Latest exact-candidate CI is still pending; retry after it finishes")
            sleep(min(30, max(0, deadline - monotonic())))
            continue
        if status != "completed" or run.get("conclusion") != "success":
            raise GateError("Latest exact-candidate CI must be completed/success")
        jobs = pages(api, f"/actions/runs/{run['id']}/jobs?filter=latest", "jobs")
        for job in jobs:
            if job.get("name") in REQUIRED_JOBS:
                log(f"{job['name']}: head {job.get('head_sha')}, attempt "
                    f"{job.get('run_attempt')}, {job.get('status')}/{job.get('conclusion')}")
        accepted = validate_jobs(run, jobs)
        # A rerun/new run beginning during inspection invalidates this snapshot.
        current = api.get(f"/actions/runs/{run['id']}")
        newest = latest_run(api, candidate)
        if (current.get("run_attempt") != run["run_attempt"]
                or current.get("status") != "completed"
                or current.get("conclusion") != "success"
                or newest["id"] != run["id"]
                or newest.get("run_attempt") != run["run_attempt"]):
            raise GateError("CI changed during inspection; retry after its latest full run")
        return run, accepted


def main():
    summary = []
    try:
        event_name = os.environ["GITHUB_EVENT_NAME"]
        ref = os.environ["GITHUB_REF"]
        # inputs.release preserves booleans; github.event.inputs does not.
        release = json.loads(os.environ.get("RELEASE_REQUESTED", "false"))
        required = publication_required(event_name, ref, release)
        if not required:
            summary.append("Release CI gate: NOT REQUIRED (nonpublishing packaging run).")
        else:
            if os.environ["GITHUB_REPOSITORY"] != REPOSITORY:
                raise GateError("Publishing gate is restricted to the upstream repository")
            candidate = subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()
            if candidate != os.environ["GITHUB_SHA"]:
                raise GateError("Checked-out commit differs from the release candidate GITHUB_SHA")
            summary.append(f"Release candidate: `{candidate}`; ref `{ref}`.")
            run, jobs = check(ActionsAPI(REPOSITORY, os.environ["GH_TOKEN"]), candidate,
                              log=summary.append)
            summary.append(f"CI accepted: {run['html_url']} (head `{run['head_sha']}`, "
                           f"attempt {run['run_attempt']}).")
            summary.extend(f"- {name}: completed/success" for name in jobs)
            summary.append(f"- regress / {EQUIVALENCE_STEP}: completed/success")
    except Exception as error:
        summary.append(f"Release CI gate FAILED: {error}")
        return_code = 1
    else:
        return_code = 0
    finally:
        report = "\n".join(summary) + "\n"
        print(report, end="")
        if os.environ.get("GITHUB_STEP_SUMMARY"):
            with open(os.environ["GITHUB_STEP_SUMMARY"], "a", encoding="utf-8") as output:
                output.write(report)
    return return_code


if __name__ == "__main__":
    sys.exit(main())
