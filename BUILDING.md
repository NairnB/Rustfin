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

Docs screenshots come from the same tours: `tools/render-shots.sh target/docs tools/scripts/docs/docs-shots.txt`
(add more tours as `tools/scripts/docs/*.txt`).

### Driving the app step by step (rf-drive)

`tools/ux/rf-drive` runs the real app under Xvfb and takes one command at a time, answering each with a JPEG
screenshot (1280 px wide) — the way a first-time user would see it. Build the release binary first
(`cargo build --release -p jf-desktop`; `RFD_BIN` points it at another binary).

```sh
tools/ux/rf-drive start --mode desk --session try1 --user tester --pass test-pass-1 --server http://jf-test:8096
tools/ux/rf-drive try1 key Down 2        # one step: prints "step N: ok — screenshot: <path>"
tools/ux/rf-drive try1 click 640 360     # x y are pixels in the 1280-wide screenshot
tools/ux/rf-drive stop try1
```

- **desk** mode (1440×900): `click`, `move`, `scroll`, `drag`, `key <name> [n]`, `type <text>`, `wait <ms>`.
- **tv** mode (1920×1080, the TV layout): remote keys only — Up, Down, Left, Right, Enter, Back, Info, PlayPause —
  and `wait`.
- Sessions live in a folder per name (input pipe, one output line per step, `shots/NNN.jpg`, app log).
- Settings are environment variables, which `rf-drive`, `fleet-run`, `fleet-user.sh`, `dry-run.sh` and
  `rf-drive-smoke.sh` also read from an optional private file, `${RF_ENV:-$HOME/.config/rf-fleet.env}` (plain
  `KEY=VALUE` lines; a variable already set in the environment wins):
  - `RF_RUNNER` — **required to start a session**: the command that runs a command inside your build container (a
    headless box with Xvfb). It is called as `$RF_RUNNER -n <container-name> -- <command>`; a thin `docker run` wrapper
    does it.
  - `RF_FLEET_HOME` — where session folders live (default `$HOME/rf-fleet`).
  - `RF_LOCKS` — folder for the eight session-slot locks and the user-creation lock (default `$RF_FLEET_HOME/locks`).
  - `RF_PLAN_CMD` — optional; a command whose output is checked before each run so the fleet stops when a plan is
    nearly spent. Unset means no check.
  - `RF_CLAUDE_CONFIG` — optional; sets `CLAUDE_CONFIG_DIR` for the AI testers only when present.

### The fleet kit (first-time-user testing)

`tools/ux/` also holds a kit that lets an AI model try everyday tasks through rf-drive and checks the result on the
server, not by what the tester says:

- `tools/ux/tasks.yaml` — the tasks (plain-words goal, the server check, the shortest route for desk and TV). Ours names
  the titles on our own test server, so it isn't published: write your own for your server, in the same format
  (`tools/ux/tests/test_tasks.py` shows it).
- `tools/ux/fleet-run --round N [--tasks id,id] [--modes desk,tv]` — validates every route, runs one tester per task
  and mode with a fresh throwaway user, runs the server check (`tools/ux/check.py`), then writes `results.jsonl` and
  `summary.md`. `tools/ux/dry-run.sh` runs the whole loop with a fake driver and no model.
- It creates and deletes users and plays and marks things watched, so it needs a **throwaway Jellyfin server** with
  its own admin login (`JF_REAL_URL` points at it; see `tools/ux/jfapi.py`). Never aim it at a server you use.
- Its unit tests: `python3 -I -m unittest discover tools/ux/tests`.

## 6. Leak gate (before sharing any package)

```sh
tools/package/leak-gate.sh <staged package dir> plain|preset|public
PRESET_SERVER=http://10.0.0.20:8096 tools/package/leak-gate.sh dir preset
```

The packaging scripts call it automatically. It fails on personal names, home-directory paths, private network
addresses and secret-looking strings. Release builds also strip build-machine paths
(`--remap-path-prefix`, set inside the packaging scripts).

## 7. Samsung TV (Tizen)

Two packages share the Rust core; both install **next to** the stock Jellyfin app under their own ids and never
replace it. The native one is the main route; the web one is the fallback (`tv/README.md` has the comparison).

**You need:** Docker; the Rust targets in `tv/native` and `tv/web`; **Tizen Studio 5.6** CLI (6.1's packager is
broken); a Samsung TV (Tizen 6.5 or newer) in developer mode.

**Native `.tpk`** (C# EFL shell + `librustfin.so`):

```sh
tv/native/build.sh                          # x86_64 librustfin.so, for the TV emulator
tv/native/build.sh armv7-unknown-linux-gnueabi   # the TV itself; links inside tv/native/arm.Dockerfile (glibc 2.28)
tv/native/dotnet.sh                         # .NET 8 SDK container → tv/native/shell/bin/Release/tizen90/*.tpk
```

**Web `.wgt`** (the core as WebAssembly):

```sh
rustup target add wasm32-unknown-unknown    # and install wasm-bindgen-cli at the version in Cargo.lock
tv/web/build.sh
tv/web/package.sh <out dir>                 # needs `tizen` on PATH and an active security profile
```

**Signing.** A real TV only installs packages signed with a Samsung author **and** distributor certificate, and
the distributor certificate must list your TV's device ID. Make both with Tizen Studio's Certificate Manager and
your own Samsung account; keep them and their passwords outside the repository. Then register them and sign:

```sh
tizen security-profiles add -n myprofile -a <author.p12> -p <password> -d <distributor.p12> -dp <password>
tizen package -t tpk -s myprofile -- <path to the .tpk>        # re-signs in place
```

**Install.** On the TV, turn on developer mode (Apps → type 12345 → Developer mode on) and enter your computer's
address as the host PC, then restart the TV:

```sh
sdb connect <tv-address>:26101
tizen install -n <package>.tpk -t <tv-serial>     # `sdb devices` lists the serial
tizen run -p local.rustfin.native -t <tv-serial>
```

For the web build use the `.wgt` and `tizen run -p RustfinTv1.rustfin`. Signing in on the TV needs no typing: pick
**Use Quick Connect** and enter the 6-digit code under Settings → Quick Connect on a signed-in device (Quick
Connect must be enabled on the server).
