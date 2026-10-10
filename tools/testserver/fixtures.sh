#!/usr/bin/env bash
# Re-capture core/tests/fixtures/*.json from the throwaway jf-test server (no tokens are saved).
set -euo pipefail
OUT=$(cd "$(dirname "$0")/../../core/tests/fixtures" && pwd)
A='MediaBrowser Client="fixtures", Device="fixtures", DeviceId="fixtures", Version="1"'
C() { docker run --rm --network ${JF_TEST_NET:-jf-test-net} curlimages/curl:8.10.1 -s -H 'Content-Type: application/json' "$@"; }
R=$(C -X POST http://jf-test:8096/Users/AuthenticateByName -H "Authorization: $A" -d '{"Username":"tester","Pw":"test-pass-1"}')
TOK=$(echo "$R" | python3 -c 'import json,sys;print(json.load(sys.stdin)["AccessToken"])')
U=$(echo "$R" | python3 -c 'import json,sys;print(json.load(sys.stdin)["User"]["Id"])')
G() { C -H "Authorization: $A, Token=\"$TOK\"" "$@"; }
B=http://jf-test:8096
G "$B/System/Info/Public" > "$OUT/info_public.json"
G "$B/UserViews?userId=$U" > "$OUT/views.json"
MV=$(python3 -c 'import json,sys;print([i["Id"] for i in json.load(open(sys.argv[1]))["Items"] if i["Name"]=="Movies"][0])' "$OUT/views.json")
G "$B/Items?userId=$U&ParentId=$MV&Recursive=true&IncludeItemTypes=Movie,Series&SortBy=SortName&Fields=Overview,ProductionYear,RunTimeTicks,UserData,PrimaryImageAspectRatio&Limit=3" > "$OUT/movies_page.json"
SER=$(G "$B/Items?userId=$U&Recursive=true&IncludeItemTypes=Series" | python3 -c 'import json,sys;print(json.load(sys.stdin)["Items"][0]["Id"])')
G "$B/Shows/$SER/Seasons?userId=$U" > "$OUT/seasons.json"
G "$B/Shows/$SER/Episodes?userId=$U&Fields=Overview,MediaStreams" > "$OUT/episodes.json"
EP=$(python3 -c 'import json,sys;print(json.load(open(sys.argv[1]))["Items"][0]["Id"])' "$OUT/episodes.json")
G "$B/Items/$EP?userId=$U" > "$OUT/episode_item.json"
G -X POST "$B/Items/$EP/PlaybackInfo?userId=$U" -d '{}' > "$OUT/playbackinfo.json"
C -X POST "$B/Sessions/Logout" -H "Authorization: $A, Token=\"$TOK\"" >/dev/null
! grep -l "$TOK" "$OUT"/*.json
echo "fixtures refreshed in $OUT"
