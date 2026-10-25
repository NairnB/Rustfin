#!/usr/bin/env bash
# P-TV-7: a SEPARATE throwaway Jellyfin (own name, own volumes) holding one series with an episode that has not aired
# yet (2099), so the "Missing episodes" option can be shot. Never touches jf-test / jf-real. Login: viewer / view-pass-1.
#   unaired.sh        start (idempotent), prints the base URL
#   unaired.sh down   remove the container and both volumes
set -euo pipefail
IMG=jellyfin/jellyfin:10.10.7 NAME=jf-tv7 NET=${JF_TEST_NET:-jf-test-net}
if [ "${1:-}" = down ]; then docker rm -f $NAME >/dev/null 2>&1 || true; docker volume rm jf-tv7-config jf-tv7-media >/dev/null 2>&1 || true; exit 0; fi
docker network inspect "$NET" >/dev/null 2>&1 || docker network create "$NET" >/dev/null
docker volume create jf-tv7-media >/dev/null
M=$(mktemp); trap 'rm -f "$M"' EXIT
cat > "$M" <<'IN'
set -euo pipefail
FF=/usr/lib/jellyfin-ffmpeg/ffmpeg; S="/media/Shows/Airs Later (2024)"
[ -f /media/.done ] && exit 0
mkdir -p "$S/Season 01"
nfo() { printf '<?xml version="1.0" encoding="utf-8" standalone="yes"?>\n%s\n' "$2" > "$1"; }
nfo "$S/tvshow.nfo" "<tvshow><title>Airs Later</title><year>2024</year><plot>A generated series with one episode that has not aired yet.</plot><lockdata>true</lockdata></tvshow>"
for e in 1 2 3; do
  $FF -loglevel error -y -f lavfi -i "testsrc2=size=1280x720:rate=25:duration=10,hue=h=$((e*60))" -f lavfi -i "sine=frequency=440:duration=10" \
    -c:v libx264 -preset veryfast -crf 30 -pix_fmt yuv420p -c:a aac -b:a 64k "$S/Season 01/Airs Later S01E0$e.mkv"
  D=""; [ $e = 3 ] && D="<aired>2099-01-01</aired><premiered>2099-01-01</premiered>"
  nfo "$S/Season 01/Airs Later S01E0$e.nfo" "<episodedetails><title>Episode $e</title><season>1</season><episode>$e</episode><plot>Generated test episode.</plot>$D<lockdata>true</lockdata></episodedetails>"
done
touch /media/.done
IN
docker run --rm -v jf-tv7-media:/media -v "$M":/m.sh:ro --entrypoint bash $IMG /m.sh
if ! docker ps --format '{{.Names}}' | grep -qx $NAME; then
  docker rm -f $NAME >/dev/null 2>&1 || true
  docker run -d --name $NAME --network $NET --memory 2g -v jf-tv7-config:/config -v jf-tv7-media:/media:ro $IMG >/dev/null
fi
J() { docker run --rm --network $NET curlimages/curl:8.10.1 -s -H 'Content-Type: application/json' "$@"; }
for _ in $(seq 60); do J http://$NAME:8096/System/Info/Public >/dev/null 2>&1 && break; sleep 2; done
if J http://$NAME:8096/System/Info/Public | grep -q '"StartupWizardCompleted":false'; then
  J -X POST http://$NAME:8096/Startup/Configuration -d '{"UICulture":"en-GB","MetadataCountryCode":"GB","PreferredMetadataLanguage":"en"}'
  J http://$NAME:8096/Startup/User >/dev/null
  J -X POST http://$NAME:8096/Startup/User -d '{"Name":"tester","Password":"test-pass-1"}'
  J -X POST http://$NAME:8096/Startup/Complete
  AUTH='MediaBrowser Client="setup", Device="setup", DeviceId="setup", Version="1"'
  TOK=$(J -X POST http://$NAME:8096/Users/AuthenticateByName -H "Authorization: $AUTH" -d '{"Username":"tester","Pw":"test-pass-1"}' | sed -E 's/.*"AccessToken":"([^"]+)".*/\1/')
  A="Authorization: $AUTH, Token=\"$TOK\""
  J -X POST "http://$NAME:8096/Users/New" -H "$A" -d '{"Name":"viewer","Password":"view-pass-1"}' >/dev/null
  J -X POST "http://$NAME:8096/Library/VirtualFolders?name=Shows&collectionType=tvshows&paths=%2Fmedia%2FShows&refreshLibrary=true" -H "$A" -d '{"LibraryOptions":{"EnableRealtimeMonitor":false}}'
  sleep 20
fi
echo "http://$NAME:8096"
