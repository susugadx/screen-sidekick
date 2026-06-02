# Desktop App

This directory contains the Phase 0-B Tauri v2 + Leptos desktop shell.

The desktop app remains a local UI and bridge boundary around the Rust domain
crates. It displays bridge URL/token/status and starts the loopback capture
bridge. It does not capture the desktop, automate the browser, or send prompts
to Codex.

## Boundaries

- `src-tauri/` owns Tauri commands, loopback HTTP transport, auth, CORS, and
  body limits.
- `ui/` owns Leptos UI rendering and transient presentation state.
- Domain rules, safety policy, masking, prompt generation, and handoff package
  generation stay in Rust crates under `crates/`.

## Checks

```sh
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features
cargo check --manifest-path src-tauri/Cargo.toml
cargo check --manifest-path ui/Cargo.toml --target wasm32-unknown-unknown
```

The default Tauri check requires OS webview prerequisites. The UI check requires
the `wasm32-unknown-unknown` target.
