#!/usr/bin/env bash
# First step after the checkout on a self-hosted runner (agents#48). The
# checkout has just cleaned the workspace, as on GitHub's runners. The build
# folder and the corpus audio live outside it, one set per runner, so builds
# stay warm without the Actions cache: `target` is a link to this runner's
# build folder. TG_CACHE comes from the runner's service (tg-runner.sh install).
# A build folder past TG_TARGET_MAX_GB (default 60) starts again empty, and
# the job stops if free disk stays under TG_MIN_FREE_GB (default 100).
set -eu
cache="${TG_CACHE:-$HOME/tg-cache/${RUNNER_NAME:-runner}}"
max="${TG_TARGET_MAX_GB:-60}"
mkdir -p "$cache/target"
used="$(du -s --block-size=1G "$cache/target" | cut -f1)"
echo "build folder $cache/target: ${used} GB (limit ${max})"
if [ "$used" -gt "$max" ]; then
  echo "over the limit: starting it again empty"
  rm -rf "$cache/target"
  mkdir -p "$cache/target"
fi
# A floor of free disk (TG_MIN_FREE_GB: 100 here, and tg-runner.sh install
# sets 100 GB plus 40 per runner, as this is checked only at the start):
# the workstation's disk also holds Peter's takes. Below it, this runner's build folder goes
# first; if that is not enough, the job fails rather than fill the disk.
floor="${TG_MIN_FREE_GB:-100}"
free_gb() { df --output=avail -BG "$cache" | tail -1 | tr -dc '0-9'; }
if [ "$(free_gb)" -lt "$floor" ]; then
  echo "under ${floor} GB free: emptying this runner's build folder"
  rm -rf "$cache/target"
  mkdir -p "$cache/target"
fi
if [ "$(free_gb)" -lt "$floor" ]; then
  echo "only $(free_gb) GB free, under the ${floor} GB floor: not running here" >&2
  exit 1
fi
rm -rf target
ln -s "$cache/target" target
echo "TG_CACHE=$cache" >> "$GITHUB_ENV"
