#!/usr/bin/env bash
# Reload the dev build: the daemon restarted (detached, logging to
# $XDG_RUNTIME_DIR/cabind-dev.log), and the panel's buttons restarted
# without tripping cosmic-session's backoff.
#
# cosmic-session (launch-pad) restarts a dead process after 2^restarts * random(0..10) ms
# and never resets that counter. But when one of cosmic-panel / cosmic-notifications
# dies, the OTHER is stopped and started as a brand-new process (counter 0), at the
# same instant. So alternate: kill the one that was NOT killed last time.
#
# `reload-dev.sh daemon` restarts just the daemon.
set -euo pipefail
here=$(dirname "$(realpath "$0")")
log=${XDG_RUNTIME_DIR:-/tmp}/cabind-dev.log

# The installed daemon would hold the bus name: the dev one takes over.
systemctl --user stop cabind.service 2>/dev/null || true
pkill -f 'target/release/cabind$' || true
sleep 0.5
# Under the user manager, so it runs with the session's environment as the
# installed service does, not the shell's (an editor's may pick the dGPU).
systemctl --user stop cabind-dev 2>/dev/null || true
systemd-run --user --unit=cabind-dev --collect --quiet -p StandardOutput=file:"$log" -p StandardError=file:"$log" "$here/run-dev.sh" cabind
echo "daemon started (log: $log)"
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
    if [ -n "$panel" ] && pgrep -P "$panel" -f "target/release/cabin-applet --kind $kind\$" >/dev/null; then up=$((up + 1)); fi
  done
  if [ "$up" = 4 ]; then
    echo "buttons running under panel $panel"; exit 0
  fi
done
echo "buttons not all up; check: journalctl --user -b | grep -i cabin" >&2
exit 1
