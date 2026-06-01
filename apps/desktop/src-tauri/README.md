# Tauri Boundary

Reserved for future Tauri v2 Rust application code.

Tauri commands should be narrow typed request/response boundaries. They should
delegate domain behavior to `crates/screen-context`, `crates/safety`, and
`crates/prompt` instead of duplicating logic in command handlers.

No filesystem, shell, network, MCP, or browser automation capability is exposed
in Phase 0-A.
