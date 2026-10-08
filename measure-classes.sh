#!/usr/bin/env bash
# Measure what each layout class needs, in a big window, without needing a window
# that is actually that size. Prints the content minimum per class.
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

hyprctl eval 'hl.window_rule({ match = { class = "dev.fivves.gTunes" }, float = true })' >/dev/null 2>&1

measure() {
  local class=$1
  : >/tmp/gt.cls.log
  GTUNES_LAYOUT_DEBUG=1 GTUNES_LAYOUT_FORCE_CLASS="$class" \
    GTUNES_LAYOUT_FORCE_WINDOW=1896x1056 \
    setsid -f dbus-run-session -- "$HOME/gtunes/target/release/gtunes" \
    >/tmp/gt.cls.log 2>&1 </dev/null
  sleep 6
  printf '%-18s ' "$class"
  grep -E "^layout: content needs" /tmp/gt.cls.log | tail -1 | sed 's/layout: content needs //'
  grep -E "^layout: rail=" /tmp/gt.cls.log | tail -1 | sed 's/^layout: /    decisions: /'
  pid=$(new_pid)
  [ -n "${pid:-}" ] && kill "$pid" 2>/dev/null
  sleep 1.5
}

echo "window is 1896x1056 for every measurement."
echo "his quarter tiles are 961x494 (laptop) and 941x503 (PC); his eighth is 466x503."
echo
for class in full/tall full/short compact/tall compact/short narrow/short tiny/short; do
  measure "$class"
done

echo
echo "=== remove the runtime rule ==="
hyprctl reload >/dev/null 2>&1
echo "reloaded"
echo
echo "his instance(s) after: $(pgrep -x gtunes | tr '\n' ' ')"
