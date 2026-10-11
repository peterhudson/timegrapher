#!/usr/bin/env bash
# Drive the desktop app on the persona's virtual screen (Xvfb) the way a
# person would: look, click, type. Set up by round.py: DISPLAY, APP_BIN,
# APP_HOME (the persona's home folder, where the app keeps its settings),
# SHOTS (where screenshots go) and PULSE_SERVER (the persona's stand).
#
#   app.sh start [FILE]          open the app (FILE: a recording or folder, as `timegrapher-app FILE`)
#   app.sh shot NAME             screenshot the screen to SHOTS/NAME.png; prints the path
#   app.sh click X Y             left click at X,Y (pixels on the screenshot)
#   app.sh rclick X Y            right click
#   app.sh dclick X Y            double click
#   app.sh move X Y              move the pointer there (hover), then wait a moment
#   app.sh drag X1 Y1 X2 Y2      press at X1,Y1, move to X2,Y2, release
#   app.sh scroll X Y up|down N  turn the scroll wheel N notches over X,Y
#   app.sh type TEXT             type TEXT into whatever has focus
#   app.sh key KEYS              press keys, e.g. Return, Escape, ctrl+l, Tab
#   app.sh wait SECONDS          wait (the app keeps running)
#   app.sh size W H              resize the app's window
#   app.sh quit                  close the app's window as a person would
#   app.sh stop                  kill the app if it does not close
set -euo pipefail
: "${DISPLAY:?}" "${APP_BIN:?}" "${APP_HOME:?}" "${SHOTS:?}"
pidfile=$APP_HOME/app.pid
# The app's window: by its process, else by its title (egui does not always set the pid).
win() {
  local w
  w=$(xdotool search --pid "$(cat "$pidfile" 2>/dev/null || echo 0)" --onlyvisible 2>/dev/null | tail -1)
  [[ -n $w ]] || w=$(xdotool search --onlyvisible --name '^Timegrapher' 2>/dev/null | head -1)
  echo "$w"
}
settle() { sleep "${1:-0.6}"; }

case ${1:-} in
  start)
    shift
    if [[ -f $pidfile ]] && kill -0 "$(cat "$pidfile")" 2>/dev/null; then
      echo "The app is already open." ; exit 0
    fi
    mkdir -p "$APP_HOME"
    (cd "$APP_HOME" && HOME=$APP_HOME XDG_DATA_HOME=$APP_HOME/.local/share XDG_CONFIG_HOME=$APP_HOME/.config \
       setsid "$APP_BIN" "$@" >>"$APP_HOME/app-stdout.log" 2>&1 & echo $! > "$pidfile")
    for _ in $(seq 40); do [[ -n $(win) ]] && break; sleep 0.25; done
    if [[ -z $(win) ]]; then echo "The app did not open a window; last lines of its output:"; tail -5 "$APP_HOME/app-stdout.log"; exit 1; fi
    xdotool windowmove "$(win)" 0 0 windowsize "$(win)" 1440 900 2>/dev/null || true
    settle 2
    echo "The app is open."
    ;;
  shot)
    name=${2:?name}; mkdir -p "$SHOTS"
    import -window root "$SHOTS/$name.png"
    echo "$SHOTS/$name.png"
    ;;
  click)  xdotool mousemove "$2" "$3" click 1; settle ;;
  rclick) xdotool mousemove "$2" "$3" click 3; settle ;;
  dclick) xdotool mousemove "$2" "$3" click --repeat 2 --delay 80 1; settle ;;
  move)   xdotool mousemove "$2" "$3"; settle 1.2 ;;
  drag)   xdotool mousemove "$2" "$3" mousedown 1 mousemove --sync "$(( ($2+$4)/2 ))" "$(( ($3+$5)/2 ))" mousemove "$4" "$5" mouseup 1; settle ;;
  scroll)
    btn=5; [[ $4 == up ]] && btn=4
    xdotool mousemove "$2" "$3" click --repeat "${5:-1}" --delay 50 "$btn"; settle ;;
  type)   shift; xdotool type --delay 30 -- "$*"; settle ;;
  key)    shift; xdotool key -- "$@"; settle ;;
  wait)   sleep "${2:?seconds}" ;;
  size)   xdotool windowsize "$(win)" "$2" "$3"; settle 1 ;;
  quit)
    w=$(win); [[ -n $w ]] && xdotool windowclose "$w" 2>/dev/null || xdotool key --window "$w" ctrl+q 2>/dev/null || true
    sleep 2
    if [[ -f $pidfile ]] && kill -0 "$(cat "$pidfile")" 2>/dev/null; then echo "The app is still open."; else rm -f "$pidfile"; echo "The app closed."; fi
    ;;
  stop)
    [[ -f $pidfile ]] && kill "$(cat "$pidfile")" 2>/dev/null || true
    rm -f "$pidfile"; echo "The app was stopped."
    ;;
  *) sed -n '2,25p' "$0"; exit 2 ;;
esac
