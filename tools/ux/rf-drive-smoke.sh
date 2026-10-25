#!/usr/bin/env bash
# Smoke test for rf-drive: a desk and a tv session on the throwaway server (viewer), ~8 steps each — open a library,
# open a film, Back — checking every step gave an ok line and a JPEG, and that the TV refuses mouse commands.
#   tools/ux/rf-drive-smoke.sh   (run on the host: needs target/release/rustfin built, and jf-test; it starts its own build containers)
set -uo pipefail
# Optional private settings (RF_FLEET_HOME, RF_LOCKS, RF_RUNNER, ...): KEY=VALUE lines; variables already set win.
rf_env=${RF_ENV:-$HOME/.config/rf-fleet.env}
if [ -f "$rf_env" ]; then
  while IFS='=' read -r k v; do case $k in RF_*) [ -n "${!k+x}" ] || export "$k=$v";; esac; done < "$rf_env"
fi
RF=$(cd "$(dirname "$0")" && pwd)/rf-drive
SRV=http://jf-test:8096
tag=smoke-$$
fail=0
check() {  # check <session> <command…>: one step, must be ok with a JPEG
  local s=$1 out; shift
  out=$("$RF" "$s" "$@") || { echo "FAIL: $s $*: $out"; fail=1; return; }
  local jpg=${out##*screenshot: }
  case "$out" in *": ok — "*) ;; *) echo "FAIL: $s $*: $out"; fail=1; return;; esac
  [ "$(head -c2 "$jpg" | od -An -tx1 | tr -d ' ')" = ffd8 ] || { echo "FAIL: $s $*: $jpg is not a JPEG"; fail=1; return; }
  echo "$s: $* -> $(basename "$jpg") ($(stat -c %s "$jpg") bytes)"
}
# rig_has_cjk_font: the rig container must have a Japanese font, or libass draws Japanese subtitles as boxes.
n=$("${RF_RUNNER:-/bin/false}" -n "rfd-$tag-font" -- 'fc-list :lang=ja | wc -l' 2>/dev/null | tail -1)
case "$n" in ''|*[!0-9]*|0) echo "FAIL: rig_has_cjk_font: fc-list :lang=ja gave '${n:-nothing}'"; fail=1;; *) echo "rig_has_cjk_font: ok ($n fonts)";; esac
"$RF" start --mode desk --session "$tag-desk" --user viewer --pass view-pass-1 --server $SRV || fail=1
"$RF" start --mode tv --session "$tag-tv" --user viewer --pass view-pass-1 --server $SRV || fail=1
if [ $fail = 0 ]; then
  # Desk: sidebar (x y are pixels in the 1280-wide shot) → Movies → a film → Back → Back
  for c in "move 600 300" "click 70 253" "wait 500" "click 400 300" "scroll 0 -300" "key Back" "key Back" "key Down 2"; do
    check "$tag-desk" $c
  done
  # TV: remote keys only → Down into the bar, Right, Enter (a library), Enter (a film), Back, Back
  for c in "key Down" "key Right" "key Enter" "key Enter" "key Back" "key Back" "key Info" "wait 300"; do
    check "$tag-tv" $c
  done
  r=$("$RF" "$tag-tv" click 5 5); case "$r" in *"not available on a TV remote"*) echo "tv refuses click: ok";; *) echo "FAIL: tv took a click: $r"; fail=1;; esac
  r=$("$RF" "$tag-desk" dance); case "$r" in *"error unknown command"*) echo "unknown command: error line, app alive";; *) echo "FAIL: $r"; fail=1;; esac
  check "$tag-desk" wait 100
fi
"$RF" stop "$tag-desk"; "$RF" stop "$tag-tv"
F=${RF_FLEET_HOME:-$HOME/rf-fleet}
echo "shots: $F/$tag-desk/shots $F/$tag-tv/shots"
[ $fail = 0 ] && echo "rf-drive smoke: PASS" || echo "rf-drive smoke: FAIL"
exit $fail
