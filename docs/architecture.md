# Architecture

Screen Sidekick keeps domain behavior in Rust and leaves browser or desktop
code as thin adapters.

## Responsibility Owners

| Area | Owner | Notes |
| --- | --- | --- |
| `RawScreenContext` / `ScreenContext v0.1` schema | `crates/screen-context` | Typed Rust structs own the raw normalized visual context contract and serialized shape. |
| Danger detection | `crates/safety-rules` | Detects destructive, sending, billing, permission, revoke/reset, and secret-related labels. |
| Text and URL redaction policy | `crates/safety-rules` | Pure policy for secret-like text and secret-bearing URL values. |
| `SanitizedScreenContext` | `crates/safety` | Only the safety crate can construct sanitized context from raw context. |
| Safety review mutation | `crates/safety` | Applies rules to raw context, masks input values, and produces `SafetyReview`. |
| `CodexPrompt` preview | `crates/prompt` | Builds preview text from a `SafetyReview` / `SanitizedScreenContext`; it does not send anything to Codex. |
| Chrome/Edge APIs | `apps/extension` | Adapter entrypoints only. No domain rules, masking, or prompt generation. |
| Tauri desktop shell | `apps/desktop` | Reserved for a future Tauri v2 + Leptos application. |

## Data Flow

```text
browser adapter / future desktop capture
  -> raw visual/browser context
  -> Rust normalization into RawScreenContext / ScreenContext v0.1
  -> crates/safety creates SanitizedScreenContext
  -> crates/prompt creates CodexPrompt from sanitized context
  -> user preview and explicit handoff
```

The TypeScript extension scaffold may call browser APIs and collect adapter
inputs such as selected text. It must not decide danger policy, masking policy,
prompt wording, or handoff semantics.

## Tauri Boundary

Phase 0-A reserves the standard `apps/desktop/src-tauri` Rust boundary and
`apps/desktop/ui` frontend boundary without adding dependencies. Future Tauri
commands should use typed request and response structures owned by Rust crates,
not ad-hoc frontend-only DTOs.

References:

- Tauri v2 project structure: <https://v2.tauri.app/start/project-structure/>
- Tauri + Leptos guide: <https://v2.tauri.app/start/frontend/leptos/>
