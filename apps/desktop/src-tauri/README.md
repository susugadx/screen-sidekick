# Tauri Boundary

This crate owns the local Tauri application boundary and Phase 0-B loopback
bridge transport.

Tauri commands should be narrow typed request/response boundaries. They should
delegate domain behavior to `crates/screen-context`, `crates/safety`, and
`crates/prompt` instead of duplicating logic in command handlers.

Exposed surfaces:

- `get_bridge_status` Tauri command.
- `POST /v0/capture` on `127.0.0.1` with bearer auth and extension-origin CORS.

No filesystem, shell, MCP, or browser automation capability is exposed.
