#!/usr/bin/env bash
# Throwaway Jellyfin 10.10.7 for tests. Fresh config, made-up user (tester / test-pass-1), generated library.
# Never point the tests at a real server.
#   up.sh        start (idempotent), prints the base URL inside the Docker network $JF_TEST_NET (default jf-test-net)
#                JF_TEST_PUBLISH=1 also publishes 127.0.0.1:8096 (+ 7359/udp) so tests can run on the host
#   up.sh down   remove the container and its config (media volume kept)
set -euo pipefail
IMG=jellyfin/jellyfin:10.10.7 NAME=jf-test NET=${JF_TEST_NET:-jf-test-net}
HERE=$(cd "$(dirname "$0")" && pwd)
if [ "${1:-}" = down ]; then docker rm -f $NAME >/dev/null 2>&1 || true; docker volume rm jf-test-config >/dev/null 2>&1 || true; exit 0; fi
docker network inspect "$NET" >/dev/null 2>&1 || docker network create "$NET" >/dev/null
docker volume create jf-media >/dev/null
docker run --rm -v jf-media:/media -v "$HERE/make-media.sh:/make-media.sh:ro" --entrypoint bash $IMG /make-media.sh
docker run --rm -v jf-media:/media -v "$HERE/make-bulk.sh:/make-bulk.sh:ro" --entrypoint bash $IMG /make-bulk.sh
docker run --rm -v jf-media:/media -v "$HERE/make-saga.sh:/make-saga.sh:ro" --entrypoint bash $IMG /make-saga.sh
EXTRAS=$(docker run --rm -v jf-media:/media -v "$HERE/make-extras.sh:/make-extras.sh:ro" --entrypoint bash $IMG /make-extras.sh)
if ! docker ps --format '{{.Names}}' | grep -qx $NAME; then
  docker rm -f $NAME >/dev/null 2>&1 || true
  PUB=(); [ "${JF_TEST_PUBLISH:-}" = 1 ] && PUB=(-p 127.0.0.1:8096:8096 -p 127.0.0.1:7359:7359/udp)
  docker run -d --name $NAME --network $NET "${PUB[@]}" --memory 4g -v jf-test-config:/config -v jf-media:/media:ro $IMG >/dev/null
fi
J() { docker run --rm --network $NET curlimages/curl:8.10.1 -s -H 'Content-Type: application/json' "$@"; }
for _ in $(seq 60); do J http://$NAME:8096/System/Info/Public >/dev/null 2>&1 && break; sleep 2; done
if J http://$NAME:8096/System/Info/Public | grep -q '"StartupWizardCompleted":false'; then
  J -X POST http://$NAME:8096/Startup/Configuration -d '{"UICulture":"en-GB","MetadataCountryCode":"GB","PreferredMetadataLanguage":"en"}'
  J http://$NAME:8096/Startup/User >/dev/null
  J -X POST http://$NAME:8096/Startup/User -d '{"Name":"tester","Password":"test-pass-1"}'
  J -X POST http://$NAME:8096/Startup/RemoteAccess -d '{"EnableRemoteAccess":true,"EnableAutomaticPortMapping":false}'
  J -X POST http://$NAME:8096/Startup/Complete
  AUTH='MediaBrowser Client="setup", Device="setup", DeviceId="setup", Version="1"'
  TOK=$(J -X POST http://$NAME:8096/Users/AuthenticateByName -H "Authorization: $AUTH" -d '{"Username":"tester","Pw":"test-pass-1"}' | sed -E 's/.*"AccessToken":"([^"]+)".*/\1/')
  A="Authorization: $AUTH, Token=\"$TOK\""
  J -X POST "http://$NAME:8096/Library/VirtualFolders?name=Movies&collectionType=movies&paths=%2Fmedia%2FMovies&refreshLibrary=false" -H "$A" -d '{"LibraryOptions":{"EnableRealtimeMonitor":false}}'
  J -X POST "http://$NAME:8096/Library/VirtualFolders?name=Archive&collectionType=movies&paths=%2Fmedia%2FArchive&refreshLibrary=false" -H "$A" -d '{"LibraryOptions":{"EnableRealtimeMonitor":false}}'
  J -X POST "http://$NAME:8096/Library/VirtualFolders?name=Mixed&paths=%2Fmedia%2FMixed&refreshLibrary=false" -H "$A" -d '{"LibraryOptions":{"EnableRealtimeMonitor":false}}'
  J -X POST "http://$NAME:8096/Users/New" -H "$A" -d '{"Name":"viewer","Password":"view-pass-1"}' >/dev/null   # non-admin user
  J -X POST "http://$NAME:8096/Library/VirtualFolders?name=Shows&collectionType=tvshows&paths=%2Fmedia%2FShows&refreshLibrary=true" -H "$A" -d '{"LibraryOptions":{"EnableRealtimeMonitor":false}}'
  J -X POST "http://$NAME:8096/System/Configuration" -H "$A" -d "$(J http://$NAME:8096/System/Configuration -H "$A" | sed 's/"QuickConnectAvailable":false/"QuickConnectAvailable":true/; s/"MinResumeDurationSeconds":[0-9]*/"MinResumeDurationSeconds":1/')" || true
  sleep 20
  # collection for the feature tests (made from the six "Star Drift - …" films; NFO <set> needs internet metadata)
  U=$(J http://$NAME:8096/Users/Me -H "$A" | grep -oE '"Id":"[0-9a-f]{32}"' | head -1 | cut -d'"' -f4)
  IDS=$(J "http://$NAME:8096/Items?userId=$U&Recursive=true&IncludeItemTypes=Movie&searchTerm=Star%20Drift%20-" -H "$A" \
        | grep -oE '"Name":"Star Drift - [^"]*","ServerId":"[0-9a-f]+","Id":"[0-9a-f]{32}"' | grep -oE '[0-9a-f]{32}"$' | tr -d '"' | paste -sd, -)
  J -X POST "http://$NAME:8096/Collections?Name=Star%20Drift%20Saga&Ids=$IDS" -H "$A" >/dev/null
  # trickplay tile sheets for the Movies library (RR4 / P-PLAY-21)
  "$HERE/trickplay.sh" >/dev/null
fi
# round-2 extras (multi-part film, chapters, a person with dates): chapter images on for Movies, then rescan if new
AUTH='MediaBrowser Client="setup", Device="setup", DeviceId="setup", Version="1"'
TOK=$(J -X POST http://$NAME:8096/Users/AuthenticateByName -H "Authorization: $AUTH" -d '{"Username":"tester","Pw":"test-pass-1"}' | sed -E 's/.*"AccessToken":"([^"]+)".*/\1/')
A="Authorization: $AUTH, Token=\"$TOK\""
VF=$(J http://$NAME:8096/Library/VirtualFolders -H "$A")
OPTS=$(echo "$VF" | grep -oP '"Name":"Movies".*?"LibraryOptions":\K\{.*?\}(?=,"ItemId")' | head -1)
if echo "$OPTS" | grep -q '"EnableChapterImageExtraction":false'; then
  ID=$(echo "$VF" | grep -oP '"Name":"Movies".*?"ItemId":"\K[0-9a-f]+' | head -1)
  OPTS=$(echo "$OPTS" | sed 's/"EnableChapterImageExtraction":false/"EnableChapterImageExtraction":true/; s/"ExtractChapterImagesDuringLibraryScan":false/"ExtractChapterImagesDuringLibraryScan":true/')
  J -X POST http://$NAME:8096/Library/VirtualFolders/LibraryOptions -H "$A" -d "{\"Id\":\"$ID\",\"LibraryOptions\":$OPTS}" >/dev/null || true
  EXTRAS=${EXTRAS:-chapters}
fi
if [ -n "$EXTRAS" ]; then
  J -X POST http://$NAME:8096/Library/Refresh -H "$A" >/dev/null
  sleep 30
  TID=$(J http://$NAME:8096/ScheduledTasks -H "$A" | grep -oP '"Key":"RefreshChapterImages","Id":"\K[0-9a-f]+' | head -1)
  [ -n "$TID" ] && { J -X POST "http://$NAME:8096/ScheduledTasks/Running/$TID" -H "$A" >/dev/null; sleep 20; }
fi
# P-PPL-1: Rowan Pike (a director in the saga films) gets born / died dates and a biography, once
PID=$(J "http://$NAME:8096/Persons?searchTerm=Rowan%20Pike" -H "$A" | grep -oE '"Id":"[0-9a-f]{32}"' | head -1 | grep -oE '[0-9a-f]{32}' || true)
if [ -n "$PID" ] && ! J "http://$NAME:8096/Items/$PID" -H "$A" | grep -q '"EndDate"'; then
  BODY=$(J "http://$NAME:8096/Items/$PID" -H "$A" | sed 's/^{/{"Overview":"A made-up director who spent forty years making quiet films about harbours and the people who leave them.","PremiereDate":"1950-03-02T00:00:00.0000000Z","EndDate":"2020-07-01T00:00:00.0000000Z",/; s/"LockData":false/"LockData":true/')
  J -X POST "http://$NAME:8096/Items/$PID" -H "$A" -d "$BODY" >/dev/null || true
fi
# P-USER-1: the profile-picture tests change a user's picture, so they use their own user, never viewer (parallel
# picture writes on one user broke every sign-in of that user until a restart, 11 Oct 2026).
J "http://$NAME:8096/Users" -H "$A" | grep -q '"Name":"picture"' ||
  J -X POST "http://$NAME:8096/Users/New" -H "$A" -d '{"Name":"picture","Password":"pic-pass-1"}' >/dev/null
# The playlists folder must never be empty: a library scan drops an empty /config/data/playlists from the server,
# and every later "create playlist" fails with 400 "parentFolder" until a restart (seen twice on 11 Oct 2026).
docker exec "$NAME" sh -c 'mkdir -p /config/data/playlists && touch /config/data/playlists/.keep'
echo "http://$NAME:8096"
