#!/usr/bin/env bash
# Dry run of the whole fleet loop with no app and no Haiku: 2 tasks x 2 modes, the fake driver (tests/fake_rf_drive.py)
# and the fake tester (a route replay). Proves: route validation, accounts (created and deleted), seeding, server
# checks, results.jsonl + summary.md. Run it inside the build container: $RF_RUNNER -- tools/ux/dry-run.sh
set -euo pipefail
# Optional private settings (RF_FLEET_HOME, RF_LOCKS, RF_RUNNER, ...): KEY=VALUE lines; variables already set win.
rf_env=${RF_ENV:-$HOME/.config/rf-fleet.env}
if [ -f "$rf_env" ]; then
  while IFS='=' read -r k v; do case $k in RF_*) [ -n "${!k+x}" ] || export "$k=$v";; esac; done < "$rf_env"
fi
HERE="$(cd "$(dirname "$0")" && pwd)"
RF_FLEET_HOME="$(mktemp -d "${RF_FLEET_HOME:-$HOME/rf-fleet}-dry.XXXXXX")"; export RF_FLEET_HOME
export FLEET_OUT="$RF_FLEET_HOME/out"
export RF_DRIVE="$HERE/tests/fake_rf_drive.py"
export TESTER_CMD="$HERE/fake-tester.sh"
"$HERE/fleet-run" --round 0 --tasks play-sintel,mark-watched,resume-film --modes desk,tv --repeat-on-signal
python3 -I - "$FLEET_OUT" "$HERE" <<'PY'
import json, sys
out, here = sys.argv[1:3]
sys.path.insert(0, here)
import jfapi
recs = [json.loads(l) for l in open(out + "/round-0/results.jsonl")]
bad = [r for r in recs if not r["check"]["pass"] or r.get("error")]
ok = json.load(open(out + "/routes-ok.json"))
assert len(recs) == 6, f"expected 6 runs, got {len(recs)}"
assert not bad, bad
assert len(ok) == 6 and all(v["ok"] for v in ok.values()), ok
assert all(r["steps"] > 0 and r["ratio"] for r in recs), recs
left = [u["Name"] for u in jfapi.Api.admin().get("/Users") if u["Name"].startswith("fleet-")]
assert not left, f"fleet users left behind: {left}"
print("dry run OK:", len(recs), "runs, routes ok, no fleet users left")
PY
cat "$FLEET_OUT/round-0/summary.md"
