# Building Rustfin

Written for a person or a coding agent: clone this repository, follow the steps in order, and run the check at
the end of each step.
Every command runs from the repository root unless it says otherwise.

## 0. What you need everywhere

- **Rust 1.90 or newer** via [rustup](https://rustup.rs): `rustup default stable`. Check: `cargo --version`.
- **libmpv at run time** (not at build time: Rustfin loads it when the app starts). The app looks for
  `libmpv-2.dll` (Windows), `libmpv.2.dylib` (macOS) or `libmpv.so.2` (Linux) next to the executable or on the
  system library path. Without it the app runs but cannot play video, and says so.

## 1. Linux (development platform)

```sh
# Debian / Ubuntu packages (names may differ on other distributions)
sudo apt install build-essential pkg-config cmake clang libmpv-dev \
  libxkbcommon-dev libwayland-dev libx11-dev libx11-xcb-dev libxcursor-dev libxrandr-dev libxi-dev \
  libgl1-mesa-dri libegl-dev libgl-dev libfontconfig1-dev
cargo build --release -p jf-desktop
./target/release/rustfin            # opens the window; pick or type your server
```

Check: the window opens on "Connect to Jellyfin" and lists servers found on your network.

Useful flags: `--server http://10.0.0.20:8096` (pre-fill the address), `--data-dir DIR` (settings + image cache
somewhere else), `--size 1920x1080`, `--gles2` (force an OpenGL ES 2.0 context, as the TV will have).

## 2. Windows

### Option A — cross-build on Linux (the tested path)

```sh
sudo apt install mingw-w64 zip
rustup target add x86_64-pc-windows-gnu
```

Get a 64-bit libmpv: download `mpv-dev-x86_64-<date>-git-<hash>.7z` from
<https://github.com/shinchiro/mpv-winbuild-cmake/releases> and extract `libmpv-2.dll` from it (`7z x …`).

```sh
tools/package/windows.sh /path/to/libmpv-2.dll dist                          # plain build
tools/package/windows.sh /path/to/libmpv-2.dll dist http://10.0.0.20:8096    # preset build (address pre-filled)
```

This writes `dist/rustfin-<version>-windows-x64.zip` (or `…-preset.zip`) containing `Rustfin.exe`,
`libmpv-2.dll`, `README.txt` and `LICENSES/`. The script runs the leak gate (step 6) before zipping and stops if it fails.

### Option B — build on Windows itself (untested by us, expected to work)

Install Rust with the MSVC toolchain (rustup's default on Windows, needs the Visual Studio C++ Build Tools), then:

```powershell
cargo build --release -p jf-desktop
copy path\to\libmpv-2.dll target\release\
target\release\rustfin.exe
```

### Running it on Windows

Unzip, keep `Rustfin.exe` and `libmpv-2.dll` together, double-click `Rustfin.exe`. The build is not code-signed,
so the first start shows "Windows protected your PC": **More info → Run anyway**. Settings live in `%APPDATA%\Rustfin`.

## 3. macOS (Apple Silicon)

```sh
brew install mpv dylibbundler rustup
rustup-init -y            # or: rustup default stable
export PATH="$(brew --prefix rustup)/bin:$HOME/.cargo/bin:$PATH"   # Homebrew's rustup is keg-only
tools/package/mac.sh dist                          # plain build
tools/package/mac.sh dist http://10.0.0.20:8096    # preset build
```

This writes `dist/Rustfin.app` with libmpv and its libraries bundled in `Contents/Frameworks`, ad-hoc signed.
The leak gate runs before signing. First start: **right-click → Open** (the app is not notarised).
Settings live in `~/Library/Application Support/Rustfin`.

## 4. Preset server

A preset build has a server address pre-filled on the Connect screen; a plain build starts empty. The address
comes only from the packaging argument (or the `JF_DEFAULT_SERVER` environment variable at build time):

```sh
JF_DEFAULT_SERVER=http://10.0.0.20:8096 cargo build --release -p jf-desktop
```

## 5. Tests

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace                 # unit tests; live tests skip without JF_TEST_URL
```

### The throwaway test server (needs Docker)

Live tests run against a throwaway Jellyfin 10.10.7 with a generated library (6 short films with two audio tracks
and subtitles, a series with 2 seasons, and 500 tiny films for the speed test). **Never** point the tests at a real
server: they sign in, change watched state and report playback.

```sh
JF_TEST_PUBLISH=1 tools/testserver/up.sh     # starts container jf-test, also on 127.0.0.1:8096 (+ 7359/udp)
JF_TEST_URL=http://localhost:8096 cargo test --workspace -- --test-threads=1
tools/testserver/up.sh down                  # remove it again (the generated media volume is kept)
```

User `tester`, password `test-pass-1`. The Docker network is `jf-test-net` (override with `JF_TEST_NET`).
Check: all tests pass (about 60).

### Render tests and benchmarks (Linux, needs Xvfb + the test server)

```sh
sudo apt install xvfb xauth imagemagick
tools/render-shots.sh target/shots           # walks every screen from tools/scripts/*.txt, one PNG per screen
tools/bench/run.sh                           # cold start, first poster, Home ready, 500-film grid scroll, memory
```

The render scripts reach the server at `http://jf-test:8096`, so run them from a container on the test network, or
add `127.0.0.1 jf-test` to `/etc/hosts` when the server is published.

## 6. Leak gate (before sharing any package)

```sh
tools/package/leak-gate.sh <staged package dir> plain|preset|public
PRESET_SERVER=http://10.0.0.20:8096 tools/package/leak-gate.sh dir preset
```

The packaging scripts call it automatically. It fails on personal names, home-directory paths, private network
addresses and secret-looking strings. Release builds also strip build-machine paths
(`--remap-path-prefix`, set inside the packaging scripts).
