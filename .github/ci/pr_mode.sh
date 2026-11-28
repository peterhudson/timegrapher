#!/usr/bin/env bash
# Draft or ready, read live: the `mode` job of ci.yml and of release.yml.
# Writes full= and quick= to $GITHUB_OUTPUT. Off a pull request everything
# runs. On a pull request the API's draft state and head decide, not the
# event's: an event can carry the pull request as it was a few seconds
# before (#2368). A run whose event names an older head than the pull
# request has now runs nothing: a push made that head, and its own run
# (which cancels this one) tests it. If the API can't be reached, the event
# decides. Inputs: EVENT, EVENT_DRAFT, EVENT_HEAD, PR_URL, GH_TOKEN.
# Offline tests: test_draft_first.py.
set -u
full=true quick=false
if [ "$EVENT" = pull_request ]; then
  draft="$EVENT_DRAFT" head="$EVENT_HEAD"
  if live="$(gh api "$PR_URL" --jq '"\(.draft) \(.head.sha)"')"; then
    read -r draft head <<< "$live"
  else
    echo "Could not read the PR live; using the event's draft state."
  fi
  echo "event: draft=$EVENT_DRAFT head=$EVENT_HEAD; now: draft=$draft head=$head"
  if [ "$head" != "$EVENT_HEAD" ]; then
    echo "The PR has a newer head; its own run tests it."
    full=false
  elif [ "$draft" = true ]; then
    full=false quick=true
  fi
fi
echo "full=$full quick=$quick"
echo "full=$full" >> "$GITHUB_OUTPUT"
echo "quick=$quick" >> "$GITHUB_OUTPUT"
