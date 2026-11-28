#!/usr/bin/env bash
# Which runner the heavy Linux jobs of ci.yml use (agents#48): the
# self-hosted runners labelled tg-linux on Peter's workstation, or GitHub's
# ubuntu-24.04. Writes linux= to $GITHUB_OUTPUT as JSON for runs-on.
#
# The workstation is used only when all of these hold:
#   - the code is ours: a push to main, a run started by hand, or a pull
#     request from a branch of this repository (never a fork's, never
#     Dependabot's). TRUSTED is worked out in ci.yml from the event.
#   - the switch is not off: repository variable SELF_HOSTED_LINUX
#     ("off" sends every run to GitHub; `tg-runner pause` sets it).
#   - at least one tg-linux runner is online, read live from the runners
#     API with RUNNER_STATUS_TOKEN (a read-only token; GitHub's own token
#     cannot list runners). No token, an API error or no runner online
#     means GitHub's runners, so nothing waits on a sleeping machine.
# Inputs: TRUSTED, SWITCH, REPO, GH_TOKEN. Offline tests: test_linux_runner.py.
set -u
hosted='"ubuntu-24.04"'
self='["self-hosted","tg-linux"]'
pick="$hosted"
if [ "${TRUSTED:-}" != true ]; then
  echo "Not our own branch: GitHub's runner."
elif [ "${SWITCH:-}" = off ]; then
  echo "SELF_HOSTED_LINUX is off: GitHub's runner."
elif [ -z "${GH_TOKEN:-}" ]; then
  echo "No RUNNER_STATUS_TOKEN: GitHub's runner."
elif ! online="$(gh api "repos/$REPO/actions/runners?per_page=100" \
    --jq '[.runners[] | select(.status == "online") | select(any(.labels[]; .name == "tg-linux"))] | length')"; then
  echo "Could not read the runners: GitHub's runner."
elif [ "${online:-0}" -gt 0 ] 2>/dev/null; then
  echo "$online tg-linux runner(s) online: the workstation."
  pick="$self"
else
  echo "No tg-linux runner online: GitHub's runner."
fi
echo "linux=$pick"
echo "linux=$pick" >> "$GITHUB_OUTPUT"
