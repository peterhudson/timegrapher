#!/usr/bin/env bash
# The workstation runners' job-started hook (agents#48): runs before any
# step of every job, and a non-zero exit fails the job there. ci.yml's
# mode job sends only our own code here, but a pull request's run uses the
# pull request's own ci.yml, so a fork could name these runners directly.
# This check lives on the machine, root-owned, where no pull request can
# change it. It lets a job through only from this repository, and only for
# a push to main, a run started by hand, or a pull request from one of the
# repository's own branches.
# tg-runner.sh install copies it to /usr/local/lib/tg-runner/ and sets
# ACTIONS_RUNNER_HOOK_JOB_STARTED in each runner's service (not in the
# runner's own folder, which its account can write).
# Offline tests: .github/ci/test_job_started_hook.py.
set -u
REPO="${TG_RUNNER_REPO:-peterhudson/timegrapher}"
refuse() { echo "tg-runner: refusing this job: $*" >&2; exit 1; }

[ "${GITHUB_REPOSITORY:-}" = "$REPO" ] || refuse "repository ${GITHUB_REPOSITORY:-unset} is not $REPO"
[ -n "${GITHUB_EVENT_PATH:-}" ] && [ -r "$GITHUB_EVENT_PATH" ] || refuse "no event file"
case "${GITHUB_EVENT_NAME:-}" in
  push)
    [ "${GITHUB_REF:-}" = refs/heads/main ] || refuse "a push to ${GITHUB_REF:-unset}, not main" ;;
  workflow_dispatch) ;;
  pull_request)
    # "<head repo> <author>"; a deleted fork has no head repo.
    read -r head author <<< "$(/usr/bin/python3 -I -c '
import json, sys
pr = json.load(open(sys.argv[1])).get("pull_request") or {}
repo = (pr.get("head") or {}).get("repo") or {}
print(repo.get("full_name") or "-", (pr.get("user") or {}).get("login") or "-")
' "$GITHUB_EVENT_PATH" 2>/dev/null)" || refuse "unreadable event file"
    [ "${head:--}" = "$REPO" ] || refuse "a pull request from ${head:-a deleted fork}, not a branch of $REPO"
    [ "${author:-}" != "dependabot[bot]" ] || refuse "a Dependabot pull request" ;;
  *) refuse "event ${GITHUB_EVENT_NAME:-unset}" ;;
esac
echo "tg-runner: ${GITHUB_EVENT_NAME} on $REPO, allowed"
