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
  J -X POST "http://$NAME:8096/Library/VirtualFolders?name=Shows&collectionType=tvshows&paths=%2Fmedia%2FShows&refreshLibrary=true" -H "$A" -d '{"LibraryOptions":{"EnableRealtimeMonitor":false}}'
  J -X POST "http://$NAME:8096/System/Configuration" -H "$A" -d "$(J http://$NAME:8096/System/Configuration -H "$A" | sed 's/"QuickConnectAvailable":false/"QuickConnectAvailable":true/; s/"MinResumeDurationSeconds":[0-9]*/"MinResumeDurationSeconds":1/')" || true
  sleep 20
fi
echo "http://$NAME:8096"
