#!/usr/bin/env bash
# Render test: release build, run the tour script under Xvfb (1920x1080) on a fresh data dir, check the shots.
#   tools/render-shots.sh [shots-dir] [script...]   (default: every tools/scripts/*.txt; needs Xvfb + the test server, see BUILDING.md)
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
OUT=${1:-$ROOT/target/shots}; shift || true
SCRIPTS=("$@"); [ ${#SCRIPTS[@]} -gt 0 ] || SCRIPTS=("$ROOT"/tools/scripts/*.txt)
mkdir -p "$OUT"; rm -f "$OUT"/*.png "$OUT"/*.log
cargo build --release -p jf-desktop -q
fail=0; want=""
for SCRIPT in "${SCRIPTS[@]}"; do
  n=$(basename "$SCRIPT" .txt); DATA=$(mktemp -d)
  echo "== $n"
  if ! timeout 300 xvfb-run -a -s "-screen 0 1920x1080x24" "$ROOT/target/release/rustfin" --data-dir "$DATA" \
      --size 1920x1080 --script "$SCRIPT" --shots "$OUT" 2>"$OUT/$n.stderr.log" | tee "$OUT/$n.log"; then
    echo "RENDER FAIL: script $n exited non-zero (see $OUT/$n.stderr.log)"; fail=1
  fi
  rm -rf "$DATA"
  want="$want $(grep -E '^shot ' "$SCRIPT" | awk '{print $2}')"
done
for s in $want; do
  f="$OUT/$s.png"
  if [ ! -f "$f" ]; then echo "RENDER FAIL: $s.png missing"; fail=1; continue; fi
  sd=$(convert "$f" -colorspace gray -format '%[fx:standard_deviation]' info:)
  if awk "BEGIN{exit !($sd < 0.01)}"; then echo "RENDER FAIL: $s.png is flat"; fail=1; else echo "render ok: $s ($sd)"; fi
done
exit $fail
