#!/usr/bin/env bash
# The workstation's self-hosted CI runners (agents#48): install, pause,
# resume and status. ci.yml sends its heavy Linux jobs (the Linux test
# halves, quick and the three regress checks) here only for our own branches
# and main, never a fork's pull request, and only while a runner is online;
# otherwise GitHub's runners take them (.github/ci/linux_runner.sh).
#
#   sudo bash tg-runner.sh install TOKEN [COUNT]
#       Once. TOKEN is the registration token from GitHub, Settings >
#       Actions > Runners > New self-hosted runner (it lasts an hour and
#       registers any number). COUNT runners (default 4), each its own
#       service under the account `tgrunner`, which has no sudo and cannot
#       see /home/peter. Installs the build packages, and this script as
#       /usr/local/bin/tg-runner.
#   tg-runner pause          Stop now (before recording a take). Jobs on the
#                            runners are cut off and their runs started again
#                            on GitHub's runners.
#   tg-runner pause --drain  Send no more jobs here, let running ones
#                            finish (up to 45 min), then stop.
#   tg-runner resume         Start again.
#   tg-runner status         Services, jobs running, and the switch.
#   sudo tg-runner uninstall TOKEN
#       Remove the runners (TOKEN from the same page, "Remove") and the
#       account.
#
# If the machine drops off after a run's mode job picked it, that run's
# Linux jobs wait (up to GitHub's 24 h). Fix: "Re-run all jobs" on the
# run's page (not "Re-run failed jobs"), and mode then picks GitHub.
#
# pause and resume run as you, not root: they use sudo for the services and
# your own `gh` login (if any) for the repository switch SELF_HOSTED_LINUX
# and the re-runs. Without gh they still stop and start the runners; ci.yml
# then sees them offline and uses GitHub's runners for new runs.
set -euo pipefail

REPO=peterhudson/timegrapher
USER_NAME=tgrunner
HOME_DIR=/home/$USER_NAME
CONF=/etc/tg-runner.conf
HOOK=/usr/local/lib/tg-runner/job-started.sh
LABEL=tg-linux
# Packages the Linux jobs need beyond what a runner brings (ci.yml's apt
# steps skip themselves when these are present).
PACKAGES=(git curl ca-certificates jq python3 build-essential pkg-config
  libasound2-dev pandoc tar unzip xz-utils)

say() { printf '%s\n' "$*"; }
die() { printf 'tg-runner: %s\n' "$*" >&2; exit 1; }

count() {
  if [ -r "$CONF" ]; then . "$CONF"; fi
  echo "${COUNT:-4}"
}

unit() { echo "actions.runner.${REPO/\//-}.$LABEL-$1.service"; }

units() {
  local n
  for n in $(seq 1 "$(count)"); do unit "$n"; done
}

have_gh() { command -v gh >/dev/null && gh auth status >/dev/null 2>&1; }

switch() {
  if have_gh; then
    if gh variable set SELF_HOSTED_LINUX --body "$1" -R "$REPO" >/dev/null; then
      say "switch SELF_HOSTED_LINUX: $1"
    else
      say "could not set SELF_HOSTED_LINUX (your gh login needs admin on $REPO); carrying on"
    fi
  else
    say "no gh login: the switch is left as it is (stopped runners read as offline anyway)"
  fi
}

busy() { pgrep -u "$USER_NAME" -f Runner.Worker >/dev/null 2>&1; }

# Runs with a job that was on, or waiting for, these runners: cancel them
# and run them again from the start, so the mode job picks GitHub's runners.
rerun_cut_off() {
  have_gh || { say "no gh login: re-run any cut-off CI run by hand (Re-run all jobs)"; return 0; }
  local id st left
  for st in in_progress queued; do
    for id in $(gh run list -R "$REPO" --workflow ci.yml --status "$st" --limit 30 --json databaseId --jq '.[].databaseId'); do
      left="$(gh api "repos/$REPO/actions/runs/$id/jobs?per_page=100" \
        --jq "[.jobs[] | select(.labels | index(\"$LABEL\")) | select(.status != \"completed\" or (.conclusion != \"success\" and .conclusion != \"skipped\"))] | length")"
      [ "${left:-0}" -gt 0 ] || continue
      say "run $id had jobs here: cancelling and starting it again"
      gh run cancel "$id" -R "$REPO" >/dev/null || true
      for _ in $(seq 1 30); do
        [ "$(gh api "repos/$REPO/actions/runs/$id" --jq .status)" = completed ] && break
        sleep 10
      done
      if [ "$(gh api "repos/$REPO/actions/runs/$id" --jq .status)" != completed ]; then
        gh api -X POST "repos/$REPO/actions/runs/$id/force-cancel" >/dev/null || true
        sleep 15
      fi
      gh run rerun "$id" -R "$REPO" >/dev/null || say "could not re-run $id: Re-run all jobs on its page"
    done
  done
}

cmd_pause() {
  switch off
  if [ "${1:-}" = --drain ]; then
    say "waiting for running jobs to finish (up to 45 min)"
    local quiet=0 waited=0
    while [ "$waited" -lt 2700 ] && [ "$quiet" -lt 60 ]; do
      if busy; then quiet=0; else quiet=$((quiet + 10)); fi
      sleep 10; waited=$((waited + 10))
    done
  fi
  # Disabled as well as stopped, so a reboot keeps them paused.
  # shellcheck disable=SC2046
  sudo systemctl disable --now $(units)
  say "runners stopped"
  rerun_cut_off
  say "paused. Resume with: tg-runner resume"
}

cmd_resume() {
  # shellcheck disable=SC2046
  sudo systemctl enable --now $(units)
  switch on
  say "runners started"
}

cmd_status() {
  local u
  for u in $(units); do printf '%-60s %s\n' "$u" "$(systemctl is-active "$u" || true)"; done
  if busy; then say "jobs running: $(pgrep -u "$USER_NAME" -fc Runner.Worker)"; else say "jobs running: 0"; fi
  if have_gh; then say "switch SELF_HOSTED_LINUX: $(gh variable get SELF_HOSTED_LINUX -R "$REPO" 2>/dev/null || echo 'not set (on)')"; fi
}

runner_tarball() {
  local release version sha file
  release="$(curl -fsSL https://api.github.com/repos/actions/runner/releases/latest)"
  version="$(jq -r .tag_name <<< "$release")"; version="${version#v}"
  sha="$(jq -r .body <<< "$release" | sed -n 's/.*<!-- BEGIN SHA linux-x64 -->\([0-9a-f]\{64\}\)<!-- END SHA linux-x64 -->.*/\1/p' | head -1)"
  [ -n "$sha" ] || die "no SHA-256 for linux-x64 in the runner release notes"
  file="$HOME_DIR/actions-runner-linux-x64-$version.tar.gz"
  curl -fsSL -o "$file" "https://github.com/actions/runner/releases/download/v$version/actions-runner-linux-x64-$version.tar.gz"
  echo "$sha  $file" | sha256sum -c - >&2 || die "runner download does not match its SHA-256"
  echo "$file"
}

cmd_install() {
  [ "$(id -u)" = 0 ] || die "install runs as root: sudo bash $0 install TOKEN [COUNT]"
  local token="${1:-}" n="${2:-4}" tarball dir
  [ -n "$token" ] || die "give the registration token from Settings > Actions > Runners > New self-hosted runner"
  case "$n" in ''|*[!0-9]*) die "COUNT must be a number";; esac
  local threads=$(( $(nproc) / n )); [ "$threads" -ge 1 ] || threads=1
  # A job checks free disk only when it starts, so the floor leaves room
  # for every runner to build from cold at once (about 40 GB each).
  local floor=$(( 100 + 40 * n ))

  say "== packages"
  apt-get update
  apt-get install -y "${PACKAGES[@]}"

  say "== account $USER_NAME (no sudo, home only its own)"
  id "$USER_NAME" >/dev/null 2>&1 || useradd --create-home --home-dir "$HOME_DIR" --shell /bin/bash "$USER_NAME"
  chmod 700 "$HOME_DIR"
  if [ -d /home/peter ] && [ $(( 8#$(stat -c %a /home/peter) & 8#007 )) -ne 0 ]; then
    say "note: /home/peter is readable by other accounts; the runner services are kept out of it anyway (InaccessiblePaths)"
  fi

  say "== job-started hook (refuses forks' and other repositories' jobs)"
  install -d -o root -g root -m 755 "$(dirname "$HOOK")"
  install -o root -g root -m 755 "$(dirname "$0")/job-started.sh" "$HOOK"

  local free; free="$(df --output=avail -BG "$(dirname "$HOME_DIR")" | tail -1 | tr -dc '0-9')"
  say "== disk: ${free} GB free on the disk that holds $HOME_DIR"
  say "   up to about 70 GB per runner (60 GB build folder, 7 GB audio); a job starts only with ${floor} GB free (100 GB plus 40 GB per runner)"
  if [ "$free" -lt "$floor" ]; then
    say "   warning: under ${floor} GB free now, so jobs here would refuse to start; use fewer runners or free some space"
  fi

  say "== runner"
  tarball="$(runner_tarball)"
  for i in $(seq 1 "$n"); do
    dir="$HOME_DIR/runner-$i"
    if [ -f "$dir/.runner" ]; then say "runner $i is already registered; leaving it"; continue; fi
    install -d -o "$USER_NAME" -g "$USER_NAME" "$dir" "$HOME_DIR/cache/$i"
    sudo -u "$USER_NAME" tar -xzf "$tarball" -C "$dir"
    if [ "$i" = 1 ]; then "$dir/bin/installdependencies.sh"; fi
    (cd "$dir" && sudo -u "$USER_NAME" ./config.sh --unattended --replace \
      --url "https://github.com/$REPO" --token "$token" \
      --name "$LABEL-$i" --labels "$LABEL" --work _work)
    (cd "$dir" && ./svc.sh install "$USER_NAME")
    # Root-owned, so the runner account cannot change it: the job-started
    # hook that refuses anything but our own code (job-started.sh), each
    # job's build folder and corpus audio (.github/ci/self_hosted_prep.sh)
    # and share of the cores; Peter's work first, and none of his files,
    # media or temporary files in reach.
    mkdir -p "/etc/systemd/system/$(unit "$i").d"
    cat > "/etc/systemd/system/$(unit "$i").d/tg-runner.conf" <<EOF
[Service]
Environment=ACTIONS_RUNNER_HOOK_JOB_STARTED=$HOOK
Environment=TG_RUNNER_REPO=$REPO
Environment=TG_CACHE=$HOME_DIR/cache/$i
Environment=CARGO_BUILD_JOBS=$threads
Environment=NEXTEST_TEST_THREADS=$threads
Environment=TG_MIN_FREE_GB=$floor
Nice=10
CPUWeight=20
IOWeight=20
MemoryMax=16G
InaccessiblePaths=-/home/peter -/media -/mnt -/run/media
PrivateTmp=yes
NoNewPrivileges=yes
EOF
  done
  rm -f "$tarball"
  printf 'COUNT=%s\n' "$n" > "$CONF"
  install -m 755 "$0" /usr/local/bin/tg-runner
  systemctl daemon-reload
  # shellcheck disable=SC2046
  systemctl enable $(units)
  # shellcheck disable=SC2046
  systemctl restart $(units)
  say "== done: $n runners, $threads threads each. Check: tg-runner status"
}

cmd_uninstall() {
  [ "$(id -u)" = 0 ] || die "uninstall runs as root"
  local token="${1:-}" i dir
  [ -n "$token" ] || die "give the removal token from Settings > Actions > Runners"
  for i in $(seq 1 "$(count)"); do
    dir="$HOME_DIR/runner-$i"
    [ -d "$dir" ] || continue
    (cd "$dir" && ./svc.sh stop || true; ./svc.sh uninstall || true)
    rm -rf "/etc/systemd/system/$(unit "$i").d"
    (cd "$dir" && sudo -u "$USER_NAME" ./config.sh remove --token "$token" || true)
  done
  systemctl daemon-reload
  userdel -r "$USER_NAME" 2>/dev/null || true
  rm -f "$CONF" /usr/local/bin/tg-runner
  rm -rf "$(dirname "$HOOK")"
  say "removed"
}

case "${1:-}" in
  install) shift; cmd_install "$@" ;;
  pause) shift; cmd_pause "$@" ;;
  resume) cmd_resume ;;
  status) cmd_status ;;
  uninstall) shift; cmd_uninstall "$@" ;;
  *) sed -n '2,/^set -euo/p' "$0" | sed '$d'; exit 2 ;;
esac
