# Screen Sidekick

Screen Sidekick is a Rust-first local application for packaging visual context for
existing Codex workflows.

It is not a Codex replacement, browser automation agent, MCP runner, or local
repository editor. The app captures and normalizes screen context, previews
safety-sensitive information, and generates a handoff that a user can pass to
Codex.

## Phase 0-A Status

This repository currently contains the initial scaffold only:

- Rust workspace for core domain crates.
- `RawScreenContext` / `ScreenContext v0.1` typed payloads.
- Safety rules for danger/redaction policy plus safety review primitives.
- Prompt preview generation from a `SanitizedScreenContext`.
- Placeholder desktop and browser extension adapter directories.

Phase 0-A intentionally does not add Tauri, Leptos, Trunk, npm build tooling, or
browser automation.

## Repository Layout

```text
crates/screen-context  RawScreenContext / ScreenContext v0.1 types and serialization behavior
crates/safety-rules    pure danger detection and text/URL redaction policy
crates/safety          SafetyReview and SanitizedScreenContext owner
crates/prompt          Codex-ready prompt preview owner; consumes safety-reviewed context
apps/desktop           future Tauri + Leptos application boundary
apps/extension         Chrome/Edge adapter scaffold
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

The extension scaffold has no package manager or TypeScript check in Phase 0-A.
