#!/usr/bin/env bash
# F1 test states: set Drift Rebels' watched flags on the throwaway server (tester / test-pass-1) to match the
# live test in core/tests/show_plan.rs, so a render shows the right open season and Play label.
#   watch-state.sh <new|mid|newseason|all>
# new       : nothing watched            -> Play S1 E1, open season 1
# mid       : S1 + S2E1 watched          -> Play S2 E2, open season 2
# newseason : S1 + S2 watched            -> Play S3 E1, open season 3
# all       : everything watched         -> Play S1 E1, open season 3
set -euo pipefail
URL="${JF_TEST_URL:-http://jf-test:8096}"
STATE="${1:?usage: watch-state.sh <new|mid|newseason|all>}"
AUTH='MediaBrowser Client="watch-state", Device="watch-state", DeviceId="watch-state", Version="1"'

TOKEN=$(curl -s -X POST "$URL/Users/AuthenticateByName" -H 'Content-Type: application/json' -H "Authorization: $AUTH" \
  -d '{"Username":"tester","Pw":"test-pass-1"}' | jq -r '.AccessToken')
A="Authorization: $AUTH, Token=\"$TOKEN\""
UIDD=$(curl -s "$URL/Users/Me" -H "$A" | jq -r '.Id')

SID=$(curl -s "$URL/Items?userId=$UIDD&searchTerm=Drift%20Rebels&Recursive=true&IncludeItemTypes=Series&Limit=10" \
  -H "$A" | jq -r '.Items[] | select(.Type=="Series") | .Id' | head -n1)
[ -n "$SID" ] || { echo "watch-state: Drift Rebels not found" >&2; exit 1; }

EPS=$(curl -s "$URL/Shows/$SID/Episodes?userId=$UIDD&Fields=Overview" -H "$A")

eid() { echo "$EPS" | jq -r ".Items[] | select(.ParentIndexNumber==$1 and .IndexNumber==$2) | .Id"; }

setp() { # <season> <episode> <played:true|false>
  local id; id=$(eid "$1" "$2")
  [ -n "$id" ] || { echo "watch-state: S$1 E$2 not found" >&2; exit 1; }
  if [ "$3" = true ]; then
    curl -s -X POST "$URL/UserPlayedItems/$id?userId=$UIDD" -H "$A" >/dev/null
  else
    curl -s -X DELETE "$URL/UserPlayedItems/$id?userId=$UIDD" -H "$A" >/dev/null
  fi
}

# Start from everything unplayed, then mark the played episodes for the state.
for s in 1 2 3; do for e in 1 2; do setp "$s" "$e" false; done; done
case "$STATE" in
  new) ;;
  mid) setp 1 1 true; setp 1 2 true; setp 2 1 true ;;
  newseason) setp 1 1 true; setp 1 2 true; setp 2 1 true; setp 2 2 true ;;
  all) setp 1 1 true; setp 1 2 true; setp 2 1 true; setp 2 2 true; setp 3 1 true; setp 3 2 true ;;
  *) echo "unknown state: $STATE" >&2; exit 2 ;;
esac
echo "watch-state $STATE done"
