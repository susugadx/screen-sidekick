# Screen Sidekick

Screen Sidekick is a Rust-first local application for packaging visual context for
existing Codex workflows.

It is not a Codex replacement, browser automation agent, MCP runner, or local
repository editor. The app captures and normalizes screen context, previews
safety-sensitive information, and generates a handoff that a user can pass to
Codex.

## Phase 0-B Status

This repository contains the first vertical slice:

- Rust workspace for core domain crates.
- `RawScreenContext` / `ScreenContext v0.1` typed payloads.
- Safety rules for danger/redaction policy plus safety review primitives.
- Prompt preview generation from a `SanitizedScreenContext`.
- `capture-pipeline` for `raw_browser_context.v0.1` browser captures,
  sanitized ScreenContext JSON, safety summary, and prompt response generation.
- Tauri v2 desktop bridge status shell in `apps/desktop`.
- Chrome/Edge side panel adapter in `apps/extension`.

Phase 0-B uses a loopback HTTP bridge for local validation. It does not add
browser automation, MCP execution, Computer Use, local repository editing, or
automatic Codex submission.

## Repository Layout

```text
crates/screen-context  RawScreenContext / ScreenContext v0.1 types and serialization behavior
crates/safety-rules    pure danger detection and text/URL redaction policy
crates/safety          SafetyReview and SanitizedScreenContext owner
crates/prompt          Codex-ready prompt preview owner; consumes safety-reviewed context
crates/capture-pipeline raw browser capture DTO and sanitized side panel response owner
apps/desktop           Tauri + Leptos bridge status shell and loopback bridge transport
apps/extension         Chrome/Edge side panel adapter
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
See [docs/phase-0-b.md](docs/phase-0-b.md) for bridge and extension run notes.
