#!/usr/bin/env bash
# Windows x86_64 zip: exe + libmpv-2.dll + licences + read-me. Linux cross-build (x86_64-pc-windows-gnu); see BUILDING.md.
#   tools/package/windows.sh <libmpv-2.dll> <out-dir> [preset-server]
# plain build: no server filled in.  preset build: <preset-server> (e.g. http://10.0.0.20:8096) pre-filled on the Connect screen.
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
DLL=$1 OUT=$2 SERVER=${3:-}
VER=$(grep -m1 '^version' "$ROOT/Cargo.toml" | cut -d'"' -f2)
NAME=rustfin-$VER-windows-x64${SERVER:+-preset}
cd "$ROOT"
# No build-machine paths in the binary; own target dir so dev builds keep their cache.
export RUSTFLAGS="--remap-path-prefix=${CARGO_HOME:-$HOME/.cargo}=/cargo --remap-path-prefix=$ROOT=/src --remap-path-prefix=$HOME=/build"
export CARGO_TARGET_DIR="$ROOT/target/release-pkg"
JF_DEFAULT_SERVER="$SERVER" cargo build --release -p jf-desktop --target x86_64-pc-windows-gnu -q
STAGE=$(mktemp -d)/$NAME; mkdir -p "$STAGE/LICENSES"
cp "$CARGO_TARGET_DIR/x86_64-pc-windows-gnu/release/rustfin.exe" "$STAGE/Rustfin.exe"
cp "$DLL" "$STAGE/libmpv-2.dll"
cp tools/package/LICENSES/* "$STAGE/LICENSES/"
cp tools/package/README-windows.txt "$STAGE/README.txt"
# Leak gate: a plain build carries nothing personal; a preset build only its server address.
PRESET_SERVER="$SERVER" tools/package/leak-gate.sh "$STAGE" "$([ -n "$SERVER" ] && echo preset || echo plain)"
mkdir -p "$OUT"; rm -f "$OUT/$NAME.zip"; ( cd "$(dirname "$STAGE")" && zip -qr "$OUT/$NAME.zip" "$NAME" )
ls -la "$OUT/$NAME.zip"
