#!/usr/bin/env bash
# Trickplay test data on the THROWAWAY server only: turn on trickplay extraction for the Movies library (not during
# the scan) and run the "Generate Trickplay Images" task once. Idempotent; safe to run again.
#   trickplay.sh        prints "trickplay ready" when a tile sheet is served for Amber Tide
set -euo pipefail
NAME=jf-test NET=${JF_TEST_NET:-jf-test-net}
J() { docker run --rm --network $NET curlimages/curl:8.10.1 -s -H 'Content-Type: application/json' "$@"; }
AUTH='MediaBrowser Client="trickplay", Device="trickplay", DeviceId="trickplay", Version="1"'
TOK=$(J -X POST http://$NAME:8096/Users/AuthenticateByName -H "Authorization: $AUTH" -d '{"Username":"tester","Pw":"test-pass-1"}' | sed -E 's/.*"AccessToken":"([^"]+)".*/\1/')
A="Authorization: $AUTH, Token=\"$TOK\""
U=http://$NAME:8096

# Movies library: enable extraction, not during the scan.
FOLDER=$(J "$U/Library/VirtualFolders" -H "$A" | jq -c '.[] | select(.Name=="Movies")')
ID=$(echo "$FOLDER" | jq -r '.ItemId')
OPTS=$(echo "$FOLDER" | jq -c '.LibraryOptions | .EnableTrickplayImageExtraction=true | .ExtractTrickplayImagesDuringLibraryScan=false')
J -X POST "$U/Library/VirtualFolders/LibraryOptions" -H "$A" -d "{\"Id\":\"$ID\",\"LibraryOptions\":$OPTS}" >/dev/null

# Run the scheduled task once and wait for it to finish.
TASK=$(J "$U/ScheduledTasks" -H "$A" | jq -r '.[] | select(.Name=="Generate Trickplay Images") | .Id')
J -X POST "$U/ScheduledTasks/Running/$TASK" -H "$A" >/dev/null
for _ in $(seq 120); do
  [ "$(J "$U/ScheduledTasks/$TASK" -H "$A" | jq -r '.State')" = Idle ] && break
  sleep 2
done

# Amber Tide must now serve a tile sheet.
MOVIE=$(J "$U/Items?Recursive=true&IncludeItemTypes=Movie&searchTerm=Amber%20Tide&Fields=Trickplay" -H "$A")
VID=$(echo "$MOVIE" | jq -r '.Items[0].Id')
W=$(echo "$MOVIE" | jq -r '.Items[0].Trickplay | to_entries[0].value | keys[0]')
CODE=$(docker run --rm --network $NET curlimages/curl:8.10.1 -s -o /dev/null -w '%{http_code}' -H "Authorization: $AUTH, Token=\"$TOK\"" "$U/Videos/$VID/Trickplay/$W/0.jpg")
[ "$CODE" = 200 ] && echo "trickplay ready" || { echo "trickplay sheet not served ($CODE)" >&2; exit 1; }
