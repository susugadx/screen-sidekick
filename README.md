# Screen Sidekick

Screen Sidekick is a Rust-first local application for packaging visual context for
existing Codex workflows.

It is not a Codex replacement, browser automation agent, MCP runner, or local
repository editor. The app captures and normalizes screen context, previews
safety-sensitive information, and generates a handoff that a user can pass to
Codex.

## Current Status

This repository contains the browser chat vertical slice:

- Rust workspace for core domain crates.
- `RawScreenContext` / `ScreenContext v0.1` typed payloads.
- Safety rules for danger/redaction policy plus safety review primitives.
- Prompt preview generation from a `SanitizedScreenContext`.
- `capture-pipeline` for `raw_browser_context.v0.1` browser captures,
  sanitized ScreenContext JSON, safety summary, and prompt response generation.
- `sidekick-daemon` for session state, sanitized attachment storage, and Codex
  app-server turn streaming.
- `sidekick-native-host` for the Chrome/Edge Native Messaging host.
- Chrome/Edge side panel adapter in `apps/extension`, using Native Messaging as
  the primary chat transport.
- Tauri v2 desktop bridge/status shell in `apps/desktop` for debug and fallback
  workflows.

The legacy loopback WebSocket / HTTP bridge remains available for development
fallback and debug capture. Screen Sidekick still does not add browser
automation, MCP execution, Computer Use, local repository editing, or automatic
Codex actions.

## Repository Layout

```text
crates/screen-context  RawScreenContext / ScreenContext v0.1 types and serialization behavior
crates/safety-rules    pure danger detection and text/URL redaction policy
crates/safety          SafetyReview and SanitizedScreenContext owner
crates/prompt          Codex-ready prompt preview owner; consumes safety-reviewed context
crates/capture-pipeline raw browser capture DTO and sanitized side panel response owner
crates/sidekick-daemon session, attachment, Codex app-server, and shared protocol execution
crates/sidekick-native-host Chrome/Edge Native Messaging framing and host lifecycle
apps/desktop           Tauri + Leptos status/debug shell and loopback fallback transport
apps/extension         Chrome/Edge side panel adapter and transport adapters
docs/                  architecture and non-executor boundary notes
```

## Local Checks

Install the Rust toolchain and verify the environment:

```sh
make setup
make doctor
make check
```

See [docs/development.md](docs/development.md) for the full setup notes.
See [apps/extension/README.md](apps/extension/README.md) for Native Messaging
development setup.
