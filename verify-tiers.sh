#!/usr/bin/env bash
# Verify the layout tiers against real window sizes.
#
# Adds a RUNTIME-ONLY float rule for gTunes (via eval, never written to any
# config file), launches isolated instances at exact sizes so they open
# floating at that size, and prints what the app measured and decided. His own
# running instance is never focused, moved or killed. The rule is removed at the
# end and the removal is verified.
set -uo pipefail
cd "$HOME/gtunes" || exit 1
export XDG_RUNTIME_DIR=/run/user/$(id -u)
export WAYLAND_DISPLAY=wayland-1
export HYPRLAND_INSTANCE_SIGNATURE=$(ls "$XDG_RUNTIME_DIR/hypr/" 2>/dev/null | head -1)
export DBUS_SESSION_BUS_ADDRESS=unix:path=$XDG_RUNTIME_DIR/bus
export XDG_DATA_HOME=/tmp/gtune-test/share
mkdir -p /tmp/gtune-test/share
[ -d /tmp/gtune-test/share/gtunes ] || cp -a "$HOME/.local/share/gtunes" /tmp/gtune-test/share/ 2>/dev/null || true

BEFORE=" $(pgrep -x gtunes | tr '\n' ' ') "

new_pid() {
  for p in $(pgrep -x gtunes); do
    case "$BEFORE" in *" $p "*) ;; *) echo "$p"; return ;; esac
  done
}

launch() {
  local size=$1
  : >/tmp/gt.tier.log
  GTUNES_LAYOUT_DEBUG=1 GTUNES_LAYOUT_FORCE_WINDOW="$size" \
    setsid -f dbus-run-session -- "$HOME/gtunes/target/release/gtunes" \
    >/tmp/gt.tier.log 2>&1 </dev/null
  sleep 6
}

report() {
  local pid size=$1
  pid=$(new_pid)
  [ -z "${pid:-}" ] && { echo "  (instance did not start)"; return; }
  python3 "$HOME/gtunes/gtwin.py" "$pid" | tail -1 | sed 's/^/  /'
  grep -E "^layout:" /tmp/gt.tier.log | tail -3 | sed 's/^/  /'
  kill "$pid" 2>/dev/null
  sleep 1.5
}

echo "his instance(s): $BEFORE"
echo
echo "=== add the runtime float rule (memory only) ==="
hyprctl eval 'hl.window_rule({ match = { class = "dev.fivves.gTunes" }, float = true })' 2>&1 | tail -1

for size in 1896x1056 961x494 941x503 466x503 466x400; do
  launch "$size"
  echo "=== requested $size ==="
  report "$size"
  echo
done

echo "=== remove the rule: reload, then check a new window is tiled again ==="
hyprctl reload 2>&1 | tail -1
launch 500x400
report 500x400

echo
echo "his instance(s) after: $(pgrep -x gtunes | tr '\n' ' ')"
