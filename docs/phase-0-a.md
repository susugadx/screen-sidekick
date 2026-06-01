# Phase 0-A

Phase 0-A establishes repository boundaries before building a full application.

## In Scope

- Top-level Cargo workspace.
- Core Rust crates for screen context, safety rules, safety review, and prompt preview.
- Minimal tests for serialization, danger detection, masking, and prompt output.
- Placeholder desktop directory for a future Tauri + Leptos app.
- Placeholder Chrome/Edge extension entrypoints.
- Documentation for the non-executor boundary.

## Out of Scope

- Tauri, Leptos, Trunk, or npm dependency installation.
- Browser automation.
- Computer Use integration.
- MCP execution.
- Local repository editing.
- Automatic Codex submission.
- Always-on screen recording.

## Contracts Introduced

- `RawScreenContext` / `ScreenContext` carries `schema_version = "0.1"` and
  optional page metadata, selected text, screenshot metadata, visible buttons,
  and visible inputs.
- `screen-sidekick-safety-rules` owns danger detection and text/URL redaction
  policy.
- `SafetyReview` owns `SanitizedScreenContext` plus danger findings.
- Prompt generation consumes `SafetyReview` / `SanitizedScreenContext`, not a raw
  `ScreenContext`, so prompt-visible text, input values, and secret-bearing URL
  values are redacted before text preview generation.

## Checks

Use the repository checks once the toolchain is available:

```sh
make setup
make doctor
make check
```

No TypeScript package check exists in this phase because the extension is only a
scaffold and has no build tooling.
