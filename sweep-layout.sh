#!/usr/bin/env bash
# gtunes responsive-layout content-minimum sweep.
#
# Two measurement passes:
#   A. measure each tier's real content minimum, read out of the running app
#      (the class is pinned with GTUNES_LAYOUT_FORCE_CLASS because a floating
#      window cannot shrink into a smaller tier by itself: GTK refuses to go
#      below the content it is currently showing).
#   B. copy the thresholds out of src/ui/layout.rs and classify every realistic
#      window size, then assert its tier's measured content minimum fits on BOTH
#      axes. Any size that does not fit is reported FAIL.
#   C. an A/B of the bug itself on one real window size: the old Tall
#      assignment overflows it (the app prints "<-- CLIPS"), the new Short
#      assignment does not.
#
# Runs ISOLATED instances (own dbus-run-session, own XDG_DATA_HOME) so his live
# instance is never touched; the new pid is found by diffing pgrep.
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

# run one isolated instance; echo "<needs> <clips>" for the pinned class
run_instance() { # class [force_window]
  local class=$1 win=${2:-1896x1056} pid needs clips=""
  : >/tmp/gt.sweep.log
  GTUNES_LAYOUT_DEBUG=1 GTUNES_LAYOUT_FORCE_CLASS="$class" GTUNES_LAYOUT_FORCE_WINDOW="$win" \
    setsid -f dbus-run-session -- "$HOME/gtunes/target/release/gtunes" \
    >/tmp/gt.sweep.log 2>&1 </dev/null
  sleep 6
  pid=$(new_pid)
  [ -n "${pid:-}" ] && kill "$pid" 2>/dev/null
  sleep 1.5
  needs=$(grep -E "^layout: content needs" /tmp/gt.sweep.log | tail -1 | sed -E 's/layout: content needs ([0-9]+x[0-9]+).*/\1/')
  grep -q "<-- CLIPS" /tmp/gt.sweep.log && clips="CLIPS"
  echo "${needs:-?} ${clips}"
}

# ---- pass A: measured content minimum per tier --------------------------------
echo "=== A. measured content minimum per tier (window 1896x1056, class pinned) ==="
declare -A CM
for class in full/tall full/short full/tiny compact/tall compact/short compact/tiny \
             narrow/tall narrow/short narrow/tiny tiny/tall tiny/short tiny/tiny; do
  read -r needs _ < <(run_instance "$class")
  CM[$class]=$needs
  printf '%-14s %s\n' "$class" "$needs"
done

# ---- pass B: classify every realistic size, assert it fits --------------------
echo
echo "=== B. realistic-size sweep (thresholds copied from src/ui/layout.rs) ==="
echo "width:  Full>=900  Compact 780-899  Narrow 520-779  Tiny<520"
echo "height: Tall>=960  Short 480-959    Tiny<480"
echo
printf '%-11s  %-13s  %-11s  %s\n' size tier needs fits?
printf '%-11s  %-13s  %-11s  %s\n' ----------- ------------- ----------- -----
fail=0

classify() { # width height -> full/short etc.
  local w=$1 h=$2 wc hc
  if   (( w < 520 )); then wc=tiny
  elif (( w < 780 )); then wc=narrow
  elif (( w < 900 )); then wc=compact
  else wc=full; fi
  if   (( h < 480 )); then hc=tiny
  elif (( h < 960 )); then hc=short
  else hc=tall; fi
  echo "$wc/$hc"
}

check() {
  local w=$1 h=$2 class needs nw nh verdict
  class=$(classify "$w" "$h"); needs=${CM[$class]}
  nw=${needs%x*}; nh=${needs#*x}
  if (( nw <= w && nh <= h )); then verdict=PASS; else verdict=FAIL; fail=1; fi
  printf '%-11s  %-13s  %-11s  %s\n' "${w}x${h}" "$class" "$needs" "$verdict"
}

check 1896 1056   # fullscreen tile
check 941  1020   # half tile (his reference for "looks right")
check 961  494    # quarter (laptop)
check 941  503    # quarter (PC)
check 905  656    # the tile that used to clip
check 905  843    # the other clipping tile
check 466  503    # eighth tile (his smallest supported)
check 900  960    # exact Full/Tall boundary
check 899  960    # first Compact width, still Tall
check 899  959    # first Compact width, Short
check 519  960    # first Narrow width
check 519  503
check 466  480    # exact Short/Tiny height boundary

# ---- pass C: the bug, A/B on one real window --------------------------------
echo
echo "=== C. old-vs-new assignment on one real 905px window ==="
read -r needs_old clips_old < <(run_instance full/tall 905x656)
echo "old boundary (640): 905x656 -> Tall   -> needs ${needs_old}  ${clips_old:+(the app prints <-- CLIPS)}"
read -r needs_new clips_new < <(run_instance full/short 905x656)
echo "new boundary (960): 905x656 -> Short  -> needs ${needs_new}  ${clips_new:+(CLIPS)}"

hyprctl reload >/dev/null 2>&1
echo
echo "his instance(s) after: $(pgrep -x gtunes | tr '\n' ' ')"
echo "verdict: $([ $fail -eq 0 ] && echo ALL-FIT || echo CLIPPING-DETECTED)"
exit $fail
