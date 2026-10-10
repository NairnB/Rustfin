#!/usr/bin/env bash
# Generates the throwaway test library with Jellyfin's own ffmpeg (no downloaded media). Runs inside the jellyfin image:
#   docker run --rm -v jf-media:/media --entrypoint bash jellyfin/jellyfin:10.10.7 /make-media.sh
set -euo pipefail
FF=/usr/lib/jellyfin-ffmpeg/ffmpeg
M=/media
[ -f "$M/.done" ] && exit 0
mkdir -p "$M/Movies" "$M/Shows"
srt() { # <file> <label>
  printf '1\n00:00:01,000 --> 00:00:04,000\n%s line one\n\n2\n00:00:05,000 --> 00:00:09,000\n%s line two\n\n' "$2" "$2" > "$1"
}
vid() { # <out.mkv> <seconds> <hue> <title>
  local out=$1 d=$2 hue=$3 t=$4 tmp; tmp=$(mktemp -d)
  srt "$tmp/en.srt" "English"; srt "$tmp/de.srt" "Deutsch"
  $FF -loglevel error -y \
    -f lavfi -i "testsrc2=size=1280x720:rate=25:duration=$d,hue=h=$hue" \
    -f lavfi -i "sine=frequency=440:duration=$d" -f lavfi -i "sine=frequency=660:duration=$d" \
    -i "$tmp/en.srt" \
    -map 0:v -map 1:a -map 2:a -map 3:s \
    -c:v libx264 -preset veryfast -crf 30 -pix_fmt yuv420p -c:a aac -b:a 64k -ac 2 -c:s srt \
    -metadata title="$t" \
    -metadata:s:a:0 language=eng -metadata:s:a:0 title="English" \
    -metadata:s:a:1 language=fre -metadata:s:a:1 title="Francais" \
    -metadata:s:s:0 language=eng -metadata:s:s:0 title="English (embedded)" "$out"
  cp "$tmp/de.srt" "${out%.mkv}.de.srt"; rm -rf "$tmp"
}
nfo() { printf '<?xml version="1.0" encoding="utf-8" standalone="yes"?>\n%s\n' "$2" > "$1"; } # local metadata, locked: no internet lookups
backdrop() { # <out.jpg> <hue> : 16:9 generated art
  $FF -loglevel error -y -f lavfi -i "gradients=size=1920x1080:c0=0x0b132b:c1=0x3a506b:seed=$2,hue=h=$2" -frames:v 1 "$1"
}
poster() { # <out.jpg> <hue>
  $FF -loglevel error -y -f lavfi -i "gradients=size=600x900:c0=0x1d2b53:c1=0x7e2553:seed=$2,hue=h=$2" -frames:v 1 "$1"
}
i=0
for f in "Amber Tide (2019)" "Blue Harbour (2021)" "Copper Sky (2018)" "Distant Signal (2022)" "Echo Valley (2020)" "First Light (2023)"; do
  i=$((i+1)); mkdir -p "$M/Movies/$f"
  vid "$M/Movies/$f/$f.mkv" 40 $((i*55)) "${f% (*}"; poster "$M/Movies/$f/poster.jpg" $((i*55))
  backdrop "$M/Movies/$f/backdrop.jpg" $((i*55))
  y=${f##*(}; y=${y%)}
  nfo "$M/Movies/$f/$f.nfo" "<movie><title>${f% (*}</title><year>$y</year><plot>A short generated test film.</plot><lockdata>true</lockdata></movie>"
done
S="$M/Shows/Night Shift Stories (2024)"; mkdir -p "$S"; poster "$S/poster.jpg" 200
backdrop "$S/backdrop.jpg" 200
nfo "$S/tvshow.nfo" "<tvshow><title>Night Shift Stories</title><year>2024</year><plot>A generated test series.</plot><lockdata>true</lockdata></tvshow>"
for s in 1 2; do
  mkdir -p "$S/Season 0$s"
  for e in 1 2 3; do
    vid "$S/Season 0$s/Night Shift Stories S0${s}E0${e}.mkv" 20 $((s*90+e*20)) "Episode $e"
    nfo "$S/Season 0$s/Night Shift Stories S0${s}E0${e}.nfo" "<episodedetails><title>Episode $e</title><season>$s</season><episode>$e</episode><plot>Generated test episode.</plot><lockdata>true</lockdata></episodedetails>"
  done
done
touch "$M/.done"
