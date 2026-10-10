#!/usr/bin/env bash
# Speed numbers (SPEC R-PERF-1): cold start, first poster, Home ready, 500-film scroll. Needs the test server
# (tools/testserver/up.sh) and Xvfb; see BUILDING.md.
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/../.." && pwd); cd "$ROOT"
cargo build --release -p jf-desktop -q
BIN=target/release/rustfin; DATA=$(mktemp -d)
X() { xvfb-run -a -s "-screen 0 1920x1080x24" "$BIN" --data-dir "$DATA" --size 1920x1080 --script "$1" --shots /tmp 2>/dev/null; }
X tools/bench/first-run.txt >/dev/null
echo "== cold start, empty image cache"; find "$DATA/cache" -mindepth 1 -delete; X tools/bench/cold-start.txt | grep -E "MARK|BENCH|MEM"
echo "== cold start, warm image cache"; X tools/bench/cold-start.txt | grep -E "MARK|BENCH|MEM"
rm -rf "$DATA"
echo "== sizes"; ls -l "$BIN" | awk '{print "linux binary bytes", $5}'
