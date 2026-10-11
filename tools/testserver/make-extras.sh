#!/usr/bin/env bash
# Round-2 test data (made-up titles only), idempotent. Runs inside the jellyfin image like make-media.sh, with the
# media volume on /media:
#   docker run --rm -v jf-media:/media -v ./make-extras.sh:/make-extras.sh:ro --entrypoint bash IMG /make-extras.sh
# - "Harbour Crossing (2016)": a two-part film (part1 + part2 files)   → P-ITEM-16
# - "Three Rooms (2015)": a 30 s film with three chapters             → P-ITEM-17
# (The person "Rowan Pike" with birth / death dates + biography is set through the API in up.sh: Jellyfin 10.10
#  does not read a person.nfo for an existing person.)                  → P-PPL-1
# Prints EXTRAS_NEW when it made something, so up.sh knows to rescan.
set -euo pipefail
FF=/usr/lib/jellyfin-ffmpeg/ffmpeg
M=/media/Movies
[ -f "$M/.extras-done" ] && exit 0
x() { printf '<?xml version="1.0" encoding="utf-8" standalone="yes"?>\n%s\n' "$2" > "$1"; }
clip() { # <out.mkv> <seconds> <hue> [extra ffmpeg input args…]
  local out=$1 d=$2 hue=$3; shift 3
  $FF -loglevel error -y -f lavfi -i "testsrc2=size=1280x720:rate=25:duration=$d,hue=h=$hue" \
    -f lavfi -i "sine=frequency=440:duration=$d" "$@" -c:v libx264 -preset veryfast -crf 30 -pix_fmt yuv420p -c:a aac -b:a 64k "$out"
}
art() { $FF -loglevel error -y -f lavfi -i "gradients=size=$2:seed=$3,hue=h=$3" -frames:v 1 "$1"; }

H="$M/Harbour Crossing (2016)"; mkdir -p "$H"
clip "$H/Harbour Crossing - part1.mkv" 3 20
clip "$H/Harbour Crossing - part2.mkv" 3 200
art "$H/poster.jpg" 600x900 20; art "$H/backdrop.jpg" 1920x1080 20
x "$H/Harbour Crossing.nfo" "<movie><title>Harbour Crossing</title><year>2016</year><plot>A made-up two-part film for the parts tests.</plot><lockdata>true</lockdata></movie>"

R="$M/Three Rooms (2015)"; mkdir -p "$R"
cat > /tmp/three-rooms.ffmeta <<'META'
;FFMETADATA1
[CHAPTER]
TIMEBASE=1/1000
START=0
END=10000
title=The Kitchen
[CHAPTER]
TIMEBASE=1/1000
START=10000
END=20000
title=The Attic
[CHAPTER]
TIMEBASE=1/1000
START=20000
END=30000
title=The Cellar
META
clip "$R/Three Rooms.mkv" 30 120 -i /tmp/three-rooms.ffmeta -map 0:v -map 1:a -map_metadata 2 -map_chapters 2
art "$R/poster.jpg" 600x900 120; art "$R/backdrop.jpg" 1920x1080 120
x "$R/Three Rooms.nfo" "<movie><title>Three Rooms</title><year>2015</year><plot>A made-up film with three chapters.</plot><lockdata>true</lockdata></movie>"

touch "$M/.extras-done"
echo EXTRAS_NEW
