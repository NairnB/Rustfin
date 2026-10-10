#!/usr/bin/env bash
# Round-1 feature test data (made-up titles only). Runs inside the jellyfin image like make-media.sh:
# a "Mixed" library with a space-saga film set (collection + story-order films), a documentary, a holiday special
# tagged rf-section:Specials, people with credits, and a show with 3 seasons + specials (F1 season rules).
set -euo pipefail
FF=/usr/lib/jellyfin-ffmpeg/ffmpeg
M=/media/Mixed
[ -f "$M/.done" ] && exit 0
mkdir -p "$M"
clip() { $FF -loglevel error -y -f lavfi -i "testsrc2=size=640x360:rate=10:duration=$2,hue=h=$3" -f lavfi -i "sine=frequency=500:duration=$2" -c:v libx264 -preset ultrafast -pix_fmt yuv420p -c:a aac -b:a 48k "$1"; }
art() { $FF -loglevel error -y -f lavfi -i "gradients=size=$2:seed=$3,hue=h=$3" -frames:v 1 "$1"; }
x() { printf '<?xml version="1.0" encoding="utf-8" standalone="yes"?>\n%s\n' "$2" > "$1"; }
ACT='<actor><name>Ada Quill</name><role>Captain Vela</role><type>Actor</type></actor>'
DIR='<director>Rowan Pike</director>'
film() { # <title> <year> <hue> <extra nfo xml>
  local d="$M/$1 ($2)"; mkdir -p "$d"
  clip "$d/$1.mkv" 3 "$3"; art "$d/poster.jpg" 300x450 "$3"; art "$d/backdrop.jpg" 1280x720 "$3"
  x "$d/$1.nfo" "<movie><title>$1</title><year>$2</year><plot>A made-up film for the feature tests.</plot>$4<lockdata>true</lockdata></movie>"
}
SET='<set><name>Star Drift Saga</name></set>'
film "Star Drift - Ember Rising" 1999 10 "$SET$ACT$DIR"
film "Star Drift - Shadow Accord" 2002 40 "$SET$ACT"
film "Star Drift - The Long Night" 2005 70 "$SET$DIR"
film "Star Drift - Beacon" 1977 100 "$SET$ACT"
film "Star Drift - Iron Tide" 1980 130 "$SET"
film "Star Drift - Last Harbour" 1983 160 "$SET"
film "Making Star Drift" 2010 190 "<genre>Documentary</genre>"
film "Star Drift Holiday Special" 1978 220 "<tag>rf-section:Specials</tag>"
S="$M/Drift Rebels (2014)"; mkdir -p "$S"; art "$S/poster.jpg" 300x450 250; art "$S/backdrop.jpg" 1280x720 250
x "$S/tvshow.nfo" "<tvshow><title>Drift Rebels</title><year>2014</year><plot>A made-up show for the season rules.</plot><lockdata>true</lockdata></tvshow>"
for s in 0 1 2 3; do
  n=$([ $s = 0 ] && echo 1 || echo 2); mkdir -p "$S/Season 0$s"
  for e in $(seq 1 $n); do
    f="$S/Season 0$s/Drift Rebels S0${s}E0${e}"
    clip "$f.mkv" 3 $((s*60+e*15))
    x "$f.nfo" "<episodedetails><title>$([ $s = 0 ] && echo Special || echo Chapter) $e</title><season>$s</season><episode>$e</episode><plot>Season $s, episode $e of a made-up show.</plot><lockdata>true</lockdata></episodedetails>"
  done
done
touch "$M/.done"
