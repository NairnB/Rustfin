#!/usr/bin/env bash
# Public source export: an allow-listed snapshot of the committed tree, then the public leak gate.
#   tools/public-export.sh <out-dir> [git-ref, default HEAD]
# Nothing is exported unless it is on the allow-list below. Exit 1 if the gate finds anything.
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
OUT=$1 REF=${2:-HEAD}
ALLOW=(core desktop tools/bench tools/package tools/scripts tools/testserver tools/render-shots.sh tools/public-export.sh
       tv/README.md Cargo.toml Cargo.lock rustfmt.toml .gitignore LICENSE README.md BUILDING.md AGENTS.md docs-public)
mkdir -p "$OUT"
if [ -n "$(find "$OUT" -mindepth 1 -maxdepth 1 -print -quit)" ]; then echo "out dir must be empty: $OUT" >&2; exit 2; fi
cd "$ROOT"
present=()
for p in "${ALLOW[@]}"; do
  if git cat-file -e "$REF:$p" 2>/dev/null; then present+=("$p"); else echo "note: $p not in $REF (skipped)"; fi
done
git archive --format=tar "$REF" -- "${present[@]}" | tar -x -C "$OUT"
echo "exported $(find "$OUT" -type f | wc -l) files from $(git rev-parse --short "$REF")"
"$ROOT/tools/package/leak-gate.sh" "$OUT" public
( cd "$OUT" && find . -type f | sort | sed 's#^\./##' | awk -F/ '{d=$0; sub(/\/[^/]*$/,"",d); if (NF==1) d="."; c[d]++} END {for (k in c) printf "%4d  %s/\n", c[k], k}' | sort -k2 )
