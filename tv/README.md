# TV port (Samsung Tizen) — not built yet

What the later port needs, so the desktop work stays TV-ready:

- **Target:** ARM32 Linux (`armv7-unknown-linux-gnueabi`, glibc 2.28), OpenGL ES 2.0, ~1080p, remote only.
- **Shell:** a small C# Tizen app owns the window, the GLES2 context and the remote keys, and drives the Rust
  core through a C ABI (`cdylib`): `jf_init(gl_get_proc_address)`, `jf_frame(w, h, dt)`, `jf_key(code)`,
  `jf_player_event(...)`. The core never opens windows or talks to a browser.
- **Drawing:** the core draws with egui through `egui_glow`, which has a GLSL ES 1.00 (GLES2) shader path. The
  desktop app is tested on a GLES2 context too (`--gles2`), so a GLES3-only shortcut is caught early.
- **Video:** desktop uses libmpv behind the core's `Player` trait. On the TV the shell implements `Player` with
  Samsung's AVPlay and tells the core about position/end through the C ABI.
- **Input:** the core is remote-first (arrows, OK, Back). The shell maps Tizen key names to the core's `Key` enum.
- **Packaging:** signed `.tpk`, own package ID `local.rustfin.*`, installed alongside the stock Jellyfin app.
  Signing certs from `~/.config/secrets/` only.
