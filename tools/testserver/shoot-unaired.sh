#!/usr/bin/env bash
# P-TV-7: start the throwaway unaired-episode server (unaired.sh), render tools/scripts/unaired/*.txt into <shots-dir>, remove it.
#   tools/testserver/shoot-unaired.sh [shots-dir]
set -euo pipefail
D=$(cd "$(dirname "$0")" && pwd)
trap '"$D/unaired.sh" down' EXIT
"$D/unaired.sh" >/dev/null
"$D/../render-shots.sh" "${1:-$D/../../target/shots-unaired}" "$D"/../scripts/unaired/*.txt
