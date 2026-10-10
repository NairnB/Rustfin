#!/usr/bin/env bash
# 500 tiny generated films for the speed test (500-item grid). Runs inside the jellyfin image like make-media.sh.
set -euo pipefail
FF=/usr/lib/jellyfin-ffmpeg/ffmpeg
M=/media/Archive
[ -f "$M/.done" ] && exit 0
mkdir -p "$M"
one() {
  local n=$1 t; t=$(printf 'Archive Film %03d' "$n"); local d="$M/$t ($((1950 + n % 70)))"
  mkdir -p "$d"
  $FF -loglevel error -y -f lavfi -i "testsrc2=size=320x180:rate=10:duration=2,hue=h=$((n * 7 % 360))" -c:v libx264 -preset ultrafast -pix_fmt yuv420p "$d/$t.mkv"
  $FF -loglevel error -y -f lavfi -i "gradients=size=300x450:seed=$n,hue=h=$((n * 7 % 360))" -frames:v 1 "$d/poster.jpg"
  printf '<?xml version="1.0" encoding="utf-8" standalone="yes"?>\n<movie><title>%s</title><year>%d</year><plot>Generated film for the speed test.</plot><lockdata>true</lockdata></movie>\n' "$t" $((1950 + n % 70)) > "$d/$t.nfo"
}
export -f one; export FF M
seq 1 500 | xargs -P 8 -I{} bash -c 'one {}'
touch "$M/.done"
