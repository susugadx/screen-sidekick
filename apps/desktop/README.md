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

## Development

Run the desktop app from the repository root with:

```sh
make desktop-dev
```

The direct Tauri command is supported from the repository root when the Tauri
config path is provided:

```sh
cargo tauri dev --config apps/desktop/src-tauri/tauri.conf.json
```

It is also supported from `apps/desktop/src-tauri` without extra arguments:

```sh
cd apps/desktop/src-tauri
cargo tauri dev
```

The Tauri hook starts Trunk from `apps/desktop/ui`, where `Trunk.toml` and the
Leptos `index.html` live.
