# TV port (Samsung Tizen) — trial builds

Two ways to put the shared core on a Samsung TV. Both were built and measured on the Samsung TV emulator; the
numbers and the choice live in the job's trial notes, not here. Both install **alongside** the stock Jellyfin app
under their own ids — never replace or uninstall it.

| | `tv/native` (chosen) | `tv/web` (fallback) |
|---|---|---|
| What | the core as `librustfin.so` (C ABI) + a small C# app (API 9 = Tizen 6.5) | the core built to WebAssembly in a Tizen web app |
| Package | `.tpk`, id `local.rustfin.native` | `.wgt`, widget `http://local.rustfin/tv`, package `RustfinTv1` |
| Drawing | egui on GLES 2 in EFL's `elm_glview` via its `Evas_GL_API` table (API 9 has no .NET GL view) | egui on WebGL 1 (same shaders) |
| Video | `Tizen.Multimedia.Player` on the video plane | AVPlay (`webapis.avplay`) on the video plane |
| Network, jobs | ureq + worker threads, as desktop | synchronous XHR (`core/src/webhttp.rs`); jobs run between frames (`tasks::web_pump`) |
| Per-TV build | yes: x86_64 (emulator) / armv7 (TV) | no: one `.wasm` for every TV |

Both draw transparent while the player screen is up, so the video plane shows through under the controls.

## Build

- **native:** `tv/native/build.sh [rust target]` (default `x86_64-unknown-linux-gnu`, the emulator; the TV is
  `armv7-unknown-linux-gnueabi`, linked in `tv/native/arm.Dockerfile` against glibc 2.28 so it loads on Tizen 6.5),
  then `tv/native/dotnet.sh` (a .NET 8 SDK container, `Tizen.NET.Sdk`) →
  `tv/native/shell/bin/Release/tizen90/local.rustfin.native-0.1.0.tpk`.
- **web:** `tv/web/build.sh` (needs the `wasm32-unknown-unknown` target and `wasm-bindgen-cli` at the lockfile's
  `wasm-bindgen` version), then `tv/web/package.sh <out>` with the Tizen CLI and an active security profile.
- Trial runs: a `trial.json` (`{server, user, pass, report}`) in `tv/web/www/` or `tv/native/shell/res/` signs in
  first so the app opens on Home and posts its numbers to `report`. Never committed.

## Signing

A real Samsung TV only installs packages signed with a **Samsung** author + distributor certificate whose
distributor certificate lists the TV's device ID (made with Samsung's certificate extension and a Samsung account).
Certificates and their passwords stay outside the repo.
