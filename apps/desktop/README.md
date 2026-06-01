# Desktop App Scaffold

This directory is reserved for a future Tauri v2 + Leptos desktop application.

Phase 0-A does not add Tauri, Leptos, Trunk, or desktop capture dependencies.
The desktop app should remain a local UI and bridge boundary around the Rust
domain crates.

Future work:

- `src-tauri/` owns Tauri commands and local application integration.
- `ui/` owns Leptos UI rendering and transient presentation state.
- Domain rules, safety policy, masking, prompt generation, and handoff package
  generation stay in Rust crates under `crates/`.
