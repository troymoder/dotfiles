#!/usr/bin/env bash
# The dev loop: build this checkout, then run its build everywhere. While
# $XDG_RUNTIME_DIR/cabin-dev links here, every installed cabin binary hands
# over to it first thing (cabin::hand_over_to_dev): the daemon, the panel
# buttons, the shortcuts (capture, the window switcher), D-Bus activation and
# gpg-agent's pinentry. Logging out ends it.
#
#   reload-dev.sh          build, restart the daemon and the panel buttons
#   reload-dev.sh daemon   build, restart only the daemon
#   reload-dev.sh off      back to the installed build
#
# The daemon runs as cabind.service either way: journalctl --user -u cabind -f
#
# cosmic-session (launch-pad) restarts a dead process after 2^restarts * random(0..10) ms
# and never resets that counter. But when one of cosmic-panel / cosmic-notifications
# dies, the OTHER is stopped and started as a brand-new process (counter 0), at the
# same instant. So alternate: kill the one that was NOT killed last time.
set -euo pipefail
here=$(dirname "$(realpath "$0")")
marker=${XDG_RUNTIME_DIR:?}/cabin-dev

if [ "${1:-}" = off ]; then
  rm -f "$marker"
else
  "$here/build-dev.sh"
  ln -sfn "$here" "$marker"
fi

systemctl --user restart cabind.service
echo "daemon restarted (journalctl --user -u cabind -f)"
[ "${1:-}" = daemon ] && exit 0

state=${XDG_CACHE_HOME:-$HOME/.cache}/cabin-reload-last
last=$(cat "$state" 2>/dev/null || true)
if [ "$last" = cosmic-panel ]; then victim=cosmic-notifications; else victim=cosmic-panel; fi
echo "killing $victim (last time: ${last:-none})"
pkill -f "^$victim\$"
echo "$victim" > "$state"
for _ in $(seq 1 15); do
  sleep 2
  panel=$(pgrep -f '^cosmic-panel$' | head -1 || true)
  up=0
  for kind in status clock system workspaces; do
    if [ -n "$panel" ] && pgrep -P "$panel" -f "cabin-applet(-wrapped)? --kind $kind\$" >/dev/null; then up=$((up + 1)); fi
  done
  if [ "$up" = 4 ]; then
    echo "buttons running under panel $panel"; exit 0
  fi
done
echo "buttons not all up; check: journalctl --user -b | grep -i cabin" >&2
exit 1
