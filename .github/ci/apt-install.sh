#!/usr/bin/env bash
# Install Ubuntu packages for a CI job, trying again when the package
# mirror can't be reached. A runner whose DNS or mirror hiccups (#1467)
# otherwise fails the job before any test runs.
# Usage: .github/ci/apt-install.sh <package>...
set -u
# Nothing to do when every package is already there: the workstation's
# self-hosted runners have them preinstalled and no sudo (agents#48).
installed="$(dpkg-query -W -f='${Status}\n' "$@" 2>/dev/null | grep -c '^install ok installed$')"
if [ "$installed" -eq "$#" ]; then
  echo "already installed: $*"
  exit 0
fi
# Only the install decides: a third-party list the runner image carries
# can fail `apt-get update` while Ubuntu's own indexes load
# (packages.microsoft.com answered 403 on 27 Nov, #2464).
for attempt in 1 2 3; do
  sudo apt-get update || echo "apt-get update reported an error; trying the install anyway"
  if sudo apt-get install -y "$@"; then
    exit 0
  fi
  if [[ $attempt -lt 3 ]]; then
    echo "apt-get attempt $attempt failed; trying again in 30 s"
    sleep 30
  fi
done
echo "apt-get failed 3 times"
exit 1
