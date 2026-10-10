#!/usr/bin/env bash
# macOS arm64 .app: Rustfin.app with libmpv + its dylibs bundled in Contents/Frameworks, ad-hoc signed.
# Runs ON a Mac (needs Homebrew mpv + dylibbundler, rustup); see BUILDING.md.
#   tools/package/mac.sh <out-dir> [preset-server]
# plain build: no server filled in.  preset build: <preset-server> (e.g. http://10.0.0.20:8096) pre-filled on the Connect screen.
set -euo pipefail
export PATH="$HOME/.cargo/bin:/opt/homebrew/bin:$PATH"
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
OUT=$1 SERVER=${2:-}
VER=$(grep -m1 '^version' "$ROOT/Cargo.toml" | cut -d'"' -f2)
cd "$ROOT"
# No build-machine paths in the binary; own target dir so dev builds keep their cache.
export RUSTFLAGS="--remap-path-prefix=${CARGO_HOME:-$HOME/.cargo}=/cargo --remap-path-prefix=$ROOT=/src --remap-path-prefix=$HOME=/build"
export CARGO_TARGET_DIR="$ROOT/target/release-pkg"
JF_DEFAULT_SERVER="$SERVER" cargo build --release -p jf-desktop -q
APP="$OUT/Rustfin.app"; rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Frameworks" "$APP/Contents/Resources/LICENSES"
cp "$CARGO_TARGET_DIR/release/rustfin" "$APP/Contents/MacOS/rustfin"
cp "$(brew --prefix mpv)/lib/libmpv.2.dylib" "$APP/Contents/Frameworks/"
# copy libmpv's own dylibs in and point them at @executable_path/../Frameworks
dylibbundler -of -b -x "$APP/Contents/Frameworks/libmpv.2.dylib" -d "$APP/Contents/Frameworks/" \
  -p @executable_path/../Frameworks/ >/dev/null
install_name_tool -id @executable_path/../Frameworks/libmpv.2.dylib "$APP/Contents/Frameworks/libmpv.2.dylib"
# dylibbundler can add the same LC_RPATH twice; macOS refuses to load such a dylib ("duplicate LC_RPATH")
for f in "$APP"/Contents/Frameworks/*.dylib; do
  otool -l "$f" | awk '/LC_RPATH/{r=1} r&&/ path /{print $2; r=0}' | sort | uniq -d | while read -r rp; do
    while [ "$(otool -l "$f" | awk '/LC_RPATH/{r=1} r&&/ path /{print $2; r=0}' | grep -cxF "$rp")" -gt 1 ]; do
      install_name_tool -delete_rpath "$rp" "$f" 2>/dev/null; done
  done
done
cp tools/package/LICENSES/* "$APP/Contents/Resources/LICENSES/"
cat > "$APP/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>Rustfin</string>
  <key>CFBundleDisplayName</key><string>Rustfin</string>
  <key>CFBundleIdentifier</key><string>local.rustfin.app</string>
  <key>CFBundleExecutable</key><string>rustfin</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$VER</string>
  <key>CFBundleVersion</key><string>$VER</string>
  <key>LSMinimumSystemVersion</key><string>13.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSLocalNetworkUsageDescription</key><string>Finds Jellyfin servers on your network.</string>
</dict></plist>
EOF
# Leak gate: a plain build carries nothing personal; a preset build only its server address.
PRESET_SERVER="$SERVER" tools/package/leak-gate.sh "$APP" "$([ -n "$SERVER" ] && echo preset || echo plain)"
codesign --force --deep -s - "$APP" 2>/dev/null
codesign --verify --deep "$APP" && echo "built $APP ($(du -sh "$APP" | cut -f1))"
