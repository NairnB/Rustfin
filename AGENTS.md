# Notes for coding agents

Read `README.md` for what Rustfin is and `BUILDING.md` for how to build and test it.

## Before you call a change done

1. `cargo fmt --all --check`
2. `cargo clippy --workspace --all-targets -- -D warnings`
3. `cargo test --workspace` (with the test server running and `JF_TEST_URL` set for the live tests; see BUILDING.md §5)
4. If you touched a screen: `tools/render-shots.sh` and look at the PNGs.

A failing check means not done: only report success if you saw every check pass after your last change.

## Rules

- **No personal data in commits**: no names, e-mail addresses, home-directory paths, private IP addresses, server
  addresses, device IDs, tokens or keys. Use `http://10.0.0.20:8096` and `a@example.test` in examples and tests.
- **Tests only against the throwaway server** from `tools/testserver/up.sh`, never against a real Jellyfin server.
- **Packages pass the leak gate** (`tools/package/leak-gate.sh`); the packaging scripts run it — don't bypass it.
- `core/` must stay free of window, player and browser code: it talks to the platform through the traits in
  `core/src/platform.rs` (the future TV port depends on this). Keep OpenGL use GLES2-compatible.
- Every colour, size, spacing and timing comes from `core/src/theme.rs`; screens use the parts in
  `core/src/ui/parts.rs` and never hard-code visual values.
- Never block the UI thread: network work goes through `Cx::spawn` (see `core/src/ui/connect.rs` for the pattern).
- Keep changes small and focused; match the surrounding style.
