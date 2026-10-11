#!/usr/bin/env bash
# The virtual microphone stand. Each persona has its own PulseAudio server
# (PULSE_SERVER) with a null sink "stand" whose monitor is remapped as the
# default input "USB_Stand_Mic". Playing a recording into the sink is the
# watch sitting on the stand; the app, `doctor`, `record` and the MCP server
# hear it live. This never touches a real microphone.
#
#   stand.sh place POSITION   put the watch on the stand in POSITION (DU, DD, CU, CD, CL, CR)
#   stand.sh remove           take the watch off (the stand hears silence)
#   stand.sh status           what is on the stand
#
# Needs STAND_DIR (the persona's stand folder, holding map.txt with lines
# POSITION=/path/file.flac) and PULSE_SERVER, both set by round.py.
set -euo pipefail
dir=${STAND_DIR:?STAND_DIR is not set}
pidfile=$dir/player.pid
now=$dir/now.txt

stop() {
  if [[ -f $pidfile ]]; then
    kill -- -"$(cat "$pidfile")" 2>/dev/null || kill "$(cat "$pidfile")" 2>/dev/null || true
    rm -f "$pidfile"
  fi
}

case ${1:-status} in
  place)
    pos=${2:?which position? DU DD CU CD CL CR}
    pos=${pos^^}
    file=$(sed -n "s/^$pos=//p" "$dir/map.txt" | head -1)
    if [[ -z $file ]]; then
      echo "There is no recording for $pos on this stand; it has: $(cut -d= -f1 "$dir/map.txt" | tr '\n' ' ')" >&2
      exit 2
    fi
    stop
    # Loop the take so the watch keeps ticking for as long as it sits there.
    setsid bash -c "while true; do paplay --device=stand '$file' || sleep 1; done" >/dev/null 2>&1 &
    echo $! > "$pidfile"
    # When the loop started, so a live reading can be matched to its stretch
    # of the file (seconds since placing, modulo the file's length).
    echo "$pos $(basename "$file") placed $(date -u +%H:%M:%S)" > "$now"
    echo "$(date -u +%FT%TZ) place $pos $(basename "$file")" >> "$dir/log.txt"
    echo "The watch is on the stand, $pos."
    ;;
  remove)
    stop
    echo "empty" > "$now"
    echo "$(date -u +%FT%TZ) remove" >> "$dir/log.txt"
    echo "The stand is empty."
    ;;
  status)
    cat "$now" 2>/dev/null || echo "empty"
    ;;
  *)
    echo "usage: stand.sh place POSITION | remove | status" >&2
    exit 2
    ;;
esac
