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
| Raw browser capture pipeline | `crates/capture-pipeline` | Owns `raw_browser_context.v0.1`, Rust normalization, safety review invocation, prompt generation invocation, and side panel response shape. |
| Sidekick protocol DTOs | `crates/sidekick-protocol` | Owns JSON-RPC method names, notifications, DTOs, error codes, limits, and protocol version. |
| Sidekick protocol execution | `crates/sidekick-daemon` | Owns initialize gating, auth mode, method dispatch, session visibility, capture attachment, and Codex turn streaming. |
| Native Messaging host | `crates/sidekick-native-host` | Owns Chrome/Edge Native Messaging framing, stdout/stderr separation, host lifecycle, and explicit sidecar fallback. |
| Loopback fallback bridge | `crates/sidekick-daemon`, `apps/desktop/src-tauri` | Owns `127.0.0.1` binding, bearer auth, extension-origin CORS, request body limit, and desktop status/debug commands. |
| Chrome/Edge APIs | `apps/extension` | Adapter entrypoints only. No domain rules, masking, or prompt generation. |
| Tauri desktop shell | `apps/desktop` | Leptos UI shows status/debug/fallback controls only. |

## Data Flow

```text
Chrome/Edge side panel adapter
  -> Native Messaging host
  -> sidekick-daemon shared protocol execution
  -> context/attach_browser raw_browser_context.v0.1
  -> crates/capture-pipeline normalization into RawScreenContext / ScreenContext v0.1
  -> crates/safety creates SanitizedScreenContext
  -> crates/session stores sanitized attachment context
  -> crates/codex-client streams Codex app-server turns
  -> side panel transcript
```

The TypeScript extension scaffold may call browser APIs and collect adapter
inputs such as selected text. It must not decide danger policy, masking policy,
prompt wording, or handoff semantics.

The WebSocket loopback transport uses the same protocol execution owner and is
kept as an explicit development fallback.

## Tauri Boundary

The desktop shell exposes narrow Tauri commands:

- `get_bridge_status` returns `bridge_status.v0.1` with URL, token, and status.

The fallback bridge exposes loopback-only HTTP/WebSocket endpoints. Transport
guards live in Rust bridge/daemon crates; capture normalization and output
generation live in `crates/capture-pipeline`.

References:

- Tauri v2 project structure: <https://v2.tauri.app/start/project-structure/>
- Tauri + Leptos guide: <https://v2.tauri.app/start/frontend/leptos/>
