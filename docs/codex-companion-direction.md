# Codex Companion Direction Memo

Status: draft / chat-first direction memo

このメモは、Phase 0-B 実装後の利用検証と設計会話から出てきた方向転換を残すためのもの。
まだ全部は決定していない。特に Codex app-server / SDK、Codex Desktop 連携、browser action の実行境界は調査が必要。

## Working Goal

Screen Sidekick は、`prompt/JSON を作ってコピーする道具` ではなく、Codex を browser や local screen に召喚する chat-first companion にする。

短く言うと:

> Codex 版 Gemini / Claude in Chrome。
> ただし Chrome だけでなく、terminal、IDE、local desktop screen にも召喚できる。

ユーザー体験の中心はこれ:

```text
Chrome でページを見る
  -> Sidekick を開く
  -> 「このページ要約して」「このボタン何？」「このフォーム埋めて」
  -> Codex が答える

Zed / terminal / Windows 画面を見る
  -> hotkey で Sidekick を開く
  -> 「この画面なに？」「このエラー何？」「次どうする？」
  -> Codex が答える
```

`screen_context_json` や `prompt_text` は主 UI ではなく、裏側の artifact / debug view / export として扱う。

## Why This Changed

これまでの Phase 0-B は、実質この流れだった:

```text
画面を capture
  -> Rust で sanitize
  -> prompt / JSON を生成
  -> ユーザーが copy
  -> Codex に paste
```

これは安全境界と Rust core の検証としては意味があるが、普段使いの UX としては弱い。

本当に欲しい体験は:

```text
Sidekick を開く
  -> その場で Codex に聞く
  -> Codex が返す
  -> 必要なら browser action / repo work / docs / PR に進む
```

つまり、Handoff-first ではなく Chat-first。

## Product Positioning

Screen Sidekick is not:

- a Codex replacement
- a new agent harness
- a copy-only prompt generator
- a general desktop automation layer
- a full autonomous browser agent

Screen Sidekick is:

- a Codex companion UI
- a Chrome side panel for asking Codex about the current page
- a desktop summon UI for asking Codex about local screens
- a local-first screen/browser context provider
- a safety/session layer between screen context and Codex
- a lighter first step before using Computer Use / Chrome DevTools MCP

## Difference From The Previous Direction

The underlying parts are mostly the same. The product face changes.

| Area | Previous direction | New direction |
| --- | --- | --- |
| Main action | Copy prompt / create handoff | Ask Codex |
| Main UI | Context preview / JSON / prompt | Chat side panel |
| User mental model | Tool that prepares context for Codex | Codex is already in the sidebar |
| Browser UX | Capture page and copy | Ask about page directly |
| Local screen UX | Future handoff source | Hotkey summon and ask |
| Handoff | Primary flow | Secondary/export flow |
| Debug artifacts | Visible by default | Hidden behind debug/export |

The Rust safety/context pipeline still matters. It becomes the internal path that prepares screen context before Codex sees it.

## Target User

主対象は Codex を日常的に使う人。

- Codex CLI を使っている人。
- Codex Desktop を使っている人。
- Gemini / Claude in Chrome のような体験を Codex でも欲しい人。
- Browser、terminal、IDE、local app の画面を見ながら作業する人。
- Computer Use / Chrome 操作が重い、または単純な相談には過剰だと感じている人。
- OSS / local-first / 自分で制御できる companion を好む人。

競合は気にしすぎない。Gemini / Claude と似た UI でもよい。
価値は「Codex でそれができる」ことと、「Chrome だけでなく local screen にも召喚できる」こと。

## Primary UX

### Chrome Side Panel

Browser 用の主 UI。

Core commands:

- このページ要約して
- このボタン何？
- このフォーム埋めて
- 次どこ押す？
- 危険な操作ある？
- このエラー何？
- この画面を Codex 作業に渡して

Expected flow:

```text
User opens page
  -> Opens Screen Sidekick side panel
  -> Types a question or clicks a quick action
  -> Extension captures page context
  -> Rust core sanitizes / packages it
  -> Linked Codex answers in the side panel
```

### Desktop / Local Screen Summon

Chrome 外の画面用。

Core commands:

- この画面なに？
- このログどういう意味？
- この状態は成功？
- 次に何を確認する？
- この画面を今の Codex 作業に添付して

Expected flow:

```text
User sees terminal / IDE / local app
  -> Presses hotkey
  -> Sidekick captures active window or selected region
  -> OCR / app metadata / session context is attached
  -> Linked Codex answers
```

### Copy / Export

Copy は fallback / debug / sharing 用。

Use cases:

- Codex Desktop と直接連携できない時。
- 別チャットや issue に貼りたい時。
- Context artifact を保存したい時。
- Debug で prompt / JSON を確認したい時。

通常利用では `Copy prompt` ではなく `Ask Codex` を主ボタンにする。

## Component Roles

Decision:

> Both Chrome side panel and Tauri desktop app will have chat UI.
> The local daemon owns sessions, Codex app-server lifecycle, safety, and streaming.
> Each UI is a thin client for a shared Sidekick session.

```text
Chrome extension
  - browser side panel UI
  - browser page chat
  - current tab URL/title/DOM/selected text capture
  - optional site permission handling
  - future confirmed browser actions

Tauri / desktop app / local daemon
  - desktop/local screen chat
  - Codex connector
  - local screen capture / OCR
  - global hotkey
  - settings / auth / bridge
  - screen session storage
  - desktop companion UI

Rust core
  - RawScreenContext / SanitizedScreenContext
  - safety review
  - secret masking
  - danger detection
  - prompt/context packaging
  - session and handoff primitives

Codex
  - model / reasoning / thinking
  - agent loop
  - approvals / sandbox
  - shell execution
  - repo edits / tests / diff / PR work
```

Sidekick should not implement its own Codex harness. It should be a client/companion that sends screen context to Codex and displays Codex output.

The app must avoid one Codex app-server per UI. Both UI surfaces should talk to the same Sidekick daemon and the same app-server/session owner.

```text
Chrome side panel
  -> Sidekick daemon
  -> codex app-server

Tauri desktop UI
  -> same Sidekick daemon
  -> same codex app-server
```

This allows a user to ask about a browser page in Chrome, then attach a terminal or IDE screen from the desktop UI into the same Sidekick session.

## Rust Package Boundaries

Decision:

> Split daemon, session, protocol, and Codex client into dedicated crates.
> Keep `apps/desktop/src-tauri` as a Tauri shell / launcher rather than the owner of all daemon logic.

Target crate layout:

```text
crates/session
  - SQLite schema / migrations
  - SidekickSession / SidekickMessage / ScreenAttachment
  - repository API
  - retention policy hooks for future image attachments

crates/sidekick-protocol
  - JSON-RPC request / response / notification types
  - method names
  - protocol versioning
  - client-visible error codes
  - serde contracts shared by Chrome side panel and Tauri UI

crates/codex-client
  - codex app-server stdio process owner
  - app-server JSON-RPC client
  - thread/start, thread/resume, turn/start, turn/cancel wrappers
  - app-server event -> Sidekick event mapping
  - auth / login failure surface

crates/sidekick-daemon
  - WebSocket server for UI clients
  - local auth / pairing / origin checks
  - session orchestration
  - streaming fanout to connected clients
  - calls crates/session, crates/sidekick-protocol, crates/codex-client
  - calls crates/capture-pipeline and crates/safety for context preparation

apps/desktop/src-tauri
  - Tauri shell
  - starts / supervises sidekick-daemon
  - desktop chat UI commands and local screen capture entrypoints
  - no long-term domain policy owner
```

Why:

- Avoid turning `apps/desktop/src-tauri` into a giant daemon module.
- Keep protocol and session contracts reusable across Chrome, Tauri, CLI/MCP, and tests.
- Keep app-server process management testable without launching a desktop window.
- Preserve Rust-first ownership of domain behavior.
- Make future Codex Desktop / MCP / local screen integrations easier to add.

Naming can change later, but the ownership split should remain.

## Codex Integration Strategy

Decision:

> Use `codex app-server` over stdio as the primary Codex integration.
> Treat the generated app-server schema as the compatibility source of truth, but keep the Rust runtime client intentionally narrow.

The desired user experience is B-style:

```text
Screen Sidekick UI
  -> Screen Sidekick local daemon
  -> codex app-server over stdio
  -> Codex session
  -> streamed answer / progress / approvals back into Sidekick UI
```

This is the product direction because it supports a Gemini / Claude-like chat UI.

Primary path:

1. Codex app-server / SDK
   - Use `codex app-server` directly as the main integration.
   - The Sidekick daemon starts or connects to the app-server.
   - Prefer stdio first because it keeps the app-server private to the local daemon.
   - Avoid WebSocket as the default because the current docs describe that transport as experimental / unsupported.
   - Use generated app-server schemas to keep the protocol version aligned with the installed Codex version.

Secondary / fallback paths:

2. `codex exec` / `codex resume`
   - Easier first connector.
   - Good for prototype summon / one-shot ask.
   - Weaker for live interactive sessions.

3. MCP server
   - Useful for Codex CLI to pull screen context.
   - Not the product centerpiece if the main goal is Sidekick chat UI.
   - Still useful as an integration surface for CLI users.

4. Codex Desktop
   - Desired target if it exposes a usable external integration path.
   - If not available, Sidekick can still support copy/export fallback.

5. Codex SDK
   - Useful as reference implementation or spike material.
   - Not the first production path if it forces a Node/Python sidecar into the Rust-first local daemon.
   - Can still be used later if it proves simpler and stable enough for the chat UI.

The important shift:

```text
MCP tool only = Codex CLI remains the main UI.
Sidekick client = Sidekick becomes the Codex chat companion UI.
```

The current product goal is the second one.

First vertical slice:

```text
Ask Codex about current page
  -> Chrome side panel sends a user question
  -> extension captures current tab context
  -> Rust core sanitizes/packages the context
  -> local daemon starts/uses codex app-server over stdio
  -> daemon starts a Codex thread/turn
  -> streamed Codex response returns to side panel chat UI
```

Out of scope for the first vertical slice:

- browser click/type
- local desktop screenshot/OCR
- Codex Desktop direct integration
- MCP tool registration
- replacing Codex approvals/sandbox

## Codex App-Server Contract

Decision:

> Use the best long-term configuration, not the shortest prototype path.
> `crates/codex-client` owns the app-server child process, protocol compatibility, event mapping, and user-visible diagnostics.

Current local reference version while writing this memo:

```text
codex-cli 0.138.0
```

This is not a hard permanent pin, but the implementation must record which Codex CLI/schema version it was validated against.

### Transport And Process Model

Primary transport:

- `codex app-server --stdio`
- child process started by `crates/codex-client`
- stdio pipe private to `sidekick-daemon`
- no app-server listener exposed directly to Chrome or Tauri UI

Process ownership:

- one app-server process per Sidekick daemon profile
- lazy start on first Codex request is acceptable
- eager readiness check is acceptable after settings/auth screen exists
- no one-app-server-per-UI
- no browser extension launching Codex directly

Restart policy:

- app-server crash during idle: restart on next request
- app-server crash during active turn: fail the current turn, surface a diagnostic, then allow retry
- repeated crash loop: stop restarting and show an actionable error
- never silently replay a user message after a crash without user confirmation

Runtime discovery:

- find `codex` from `PATH` or explicit setting
- run `codex --version`
- run `codex app-server --help` or equivalent capability probe
- show clear UI diagnostics for missing CLI, unsupported version, or unavailable app-server command
- do not auto-install Codex

### Schema Strategy

Use generated schema as the compatibility source of truth:

```text
codex app-server generate-json-schema --out <schema-dir>
```

Repository policy:

- commit a schema snapshot for the supported Codex CLI version
- place it under `crates/codex-client/schema/` or equivalent owner-owned path
- include a small metadata file with:
  - Codex CLI version
  - generation command
  - generation date
  - whether `--experimental` was used
- do not run schema generation from `build.rs`
- do not require every OSS contributor's local `codex` version during normal build

Rust type policy:

- hand-write the minimal Rust request/response/event types used by Phase 1
- validate them against generated schema fixtures
- keep unknown app-server fields ignored where safe
- keep unknown event kinds visible as `Unknown` / diagnostic events, not panics
- do not expose raw app-server JSON directly to UI as the product contract

Why not generate all Rust types immediately:

- app-server is still marked experimental
- generated Rust bindings can make every upstream field churn a repo-wide break
- Phase 1 needs a narrow path: create/resume thread, send turn, stream events, cancel/fail
- schema snapshot + focused handwritten types gives compatibility evidence without coupling the whole app to unstable details

Dev/CI checks:

- add a manual or make target for schema refresh
- add a drift check that can compare current generated schema against the committed snapshot when `codex` is available
- CI should either pin a Codex version for this check or make the check explicitly opt-in
- normal Rust build/test must remain offline/reproducible

## Codex Schema Snapshot And Drift Checks

Decision:

> Store a committed app-server schema snapshot under `crates/codex-client`.
> Normal build/test must not execute the local `codex` binary.
> Schema refresh and drift detection are explicit developer commands.

Target layout:

```text
crates/codex-client/
  schema/
    metadata.json
    README.md
    app-server/
      *.schema.json
    examples/
      initialize.json
      thread_create.json
      turn_stream.jsonl
      turn_cancel.json
```

Exact generated filenames can follow `codex app-server generate-json-schema`.
The ownership boundary should remain `crates/codex-client/schema/`.

`metadata.json` should include:

```json
{
  "codex_cli_version": "codex-cli 0.138.0",
  "generated_at": "2026-06-11T00:00:00Z",
  "generation_command": "codex app-server generate-json-schema --out crates/codex-client/schema/app-server",
  "experimental": false,
  "schema_hash": "sha256:...",
  "notes": []
}
```

Generation policy:

- default snapshot should be generated without `--experimental`
- if Phase 1 requires experimental fields, record that explicitly in metadata
- do not mix experimental and non-experimental schemas in one ambiguous directory
- if both are needed, store them as separate named snapshots
- do not generate schema from `build.rs`
- do not generate schema automatically in default tests

Make targets:

```text
make codex-schema-refresh
make codex-schema-check
make codex-schema-metadata
```

Expected behavior:

- `codex-schema-refresh` intentionally regenerates the snapshot and metadata.
- `codex-schema-check` regenerates into a temp directory and compares against the committed snapshot.
- `codex-schema-check` fails when schema drift is detected and prints the observed Codex version.
- if `codex` is missing, `codex-schema-check` should fail only when explicitly requested; default CI can skip or run a pinned Codex job.
- `codex-schema-metadata` prints current committed schema metadata for bug reports.

Rust client policy:

- Phase 1 Rust types remain a handwritten subset.
- Each handwritten type must have fixture coverage against committed schema/examples.
- Raw app-server JSON may be logged only through redacted debug paths.
- Unknown app-server event kinds map to typed diagnostics, not panics.
- `crates/codex-client` owns app-server compatibility errors.
- `crates/sidekick-protocol` owns user-facing protocol errors.

Test policy:

- schema files must be parseable JSON
- metadata `schema_hash` must match committed schema files
- Phase 1 examples must deserialize into handwritten Rust types
- unknown fields in known messages are ignored where safe
- missing required fields fail with typed client errors
- schema drift test is separate from normal offline unit tests

Why:

- contributors may not have Codex installed
- contributors may have different Codex versions
- app-server is still experimental enough that schema drift must be visible
- Rust-first does not mean coupling the entire app to generated unstable bindings

### Session And Turn Mapping

Primary invariant:

```text
1 SidekickSession = 1 Codex thread
1 user message = 1 Codex turn
screen/browser context = attachment to the user message / turn
```

Thread lifecycle:

- create Codex thread lazily when the first message is sent
- persist `codex_thread_id` in `crates/session`
- resume existing Codex thread when a Sidekick session is reopened
- if thread creation fails, keep the Sidekick session but do not persist a fake thread id

Turn lifecycle:

- `message/send` creates a Sidekick user message first
- daemon attaches sanitized context
- `codex-client` starts a Codex turn
- streamed app-server events map to Sidekick notifications
- final assistant text is persisted as a Sidekick message
- turn cancellation must map to app-server cancel if supported

Event mapping owner:

```text
app-server raw event
  -> crates/codex-client typed event
  -> crates/sidekick-daemon orchestration event
  -> crates/sidekick-protocol UI notification
```

UI should receive Sidekick events, not app-server internals.

### Context Injection Rules

Screen/page context is untrusted content.

The daemon must pass context to Codex as context, not as system/developer instruction.

Rules:

- only `SanitizedScreenContext` / validated metadata can enter Codex prompt/context
- page text must not become top-level instructions
- context packaging is owned by Rust core / prompt/context package owner
- `crates/codex-client` transports prepared context; it does not sanitize raw browser data
- raw DOM, hidden input values, cookies, localStorage/sessionStorage, secrets, and masked input values must not be sent

Workspace binding:

- browser-only questions should not implicitly bind to a random repo/cwd
- a Sidekick session may optionally bind to a user-selected workspace
- repo/tool work requires an explicit workspace/mode selection
- if app-server requires a cwd, use a stable Sidekick-owned default only for ask-only mode and avoid implying repo access in UI

### Auth And Settings

Sidekick should use the user's existing Codex auth.

Rules:

- do not ask the user for an OpenAI API key in Sidekick for the primary app-server path
- do not store Codex credentials in Sidekick
- if Codex is not logged in, show a diagnostic and the command/user action needed to fix it
- if Codex auth expires mid-turn, fail the turn cleanly and allow retry after login

Settings needed for Phase 1:

- Codex binary path override
- supported/observed Codex version display
- app-server readiness status
- optional workspace binding
- debug export of schema/version diagnostics

### Approvals And Tool Execution

Sidekick must not replace Codex approvals/sandbox.

Policy:

- if app-server emits approval requests, Sidekick may display them
- Sidekick must not auto-approve
- user approval in Sidekick must be explicit and tied to the exact Codex request
- dangerous browser actions still use Sidekick's own browser-action confirmation model
- local repo edits, tests, shell commands, MCP execution, and Computer Use remain Codex-owned capabilities

Phase 1 can defer rich approval UI. If approvals are not implemented yet, the UI should show a clear limitation or route the user to continue in Codex rather than pretending the turn is fully interactive.

### Failure Modes To Treat As Product States

These are not crashes or mystery errors:

- Codex CLI not installed
- `codex` not in `PATH`
- app-server command missing
- unsupported app-server schema/version
- user not logged in
- app-server child process crashed
- app-server returned unknown event/method
- turn timed out or was cancelled
- model/provider error
- approval required but Sidekick approval UI is not implemented

Each should have a typed error in `crates/codex-client` and a stable client-visible error code in `crates/sidekick-protocol`.

### Tests Required For The First Implementation

Focused tests:

- app-server JSON-RPC request id correlation
- generated schema fixture can be loaded
- handwritten Phase 1 types deserialize known schema/example events
- unknown event kind is surfaced as diagnostic, not panic
- missing `codex` binary maps to a typed diagnostic
- unsupported version maps to a typed diagnostic
- app-server crash during turn fails the turn and does not persist a fake assistant response
- cancel request maps to app-server cancel or a known unsupported response

Integration tests:

- fake app-server process over stdio for deterministic stream tests
- no real OpenAI/Codex network calls in default test suite
- optional manual smoke test against real `codex app-server`

Verification commands should eventually include:

```text
cargo test -p screen-sidekick-codex-client
cargo test -p screen-sidekick-sidekick-daemon
make codex-schema-check
```

Exact package names can change, but the responsibilities should not.

## UI-Daemon Protocol

Decision:

> Replace the Phase 0 HTTP capture bridge with a local JSON-RPC-over-WebSocket protocol for UI clients.
> Keep HTTP only for health/debug endpoints.
> Treat this protocol as Screen Sidekick's public local contract, not a thin pass-through of Codex app-server internals.

Why:

- Chat-first UX needs streamed responses.
- Both Chrome side panel and Tauri desktop UI need to share the same sessions.
- Future approvals, cancel, browser actions, and local screen attachments are bidirectional event flows.
- The Codex app-server side is also JSON-RPC-like, so a JSON-RPC-shaped Sidekick protocol keeps the bridge conceptually aligned.
- The current HTTP `POST /v0/capture` path was useful for Phase 0 validation, but it is not the right long-term shape for chat UI.

Target shape:

```text
Chrome side panel
  -> ws://127.0.0.1:<port>/v0/ws
  -> Sidekick daemon
  -> codex app-server over stdio

Tauri desktop UI
  -> same Sidekick daemon protocol/service
  -> same sessions
  -> same codex app-server owner
```

Tauri may call Rust services directly where it is natural, but protocol and state should still be designed as if both UI surfaces are clients of the same daemon-owned session model.

Core RPC methods:

- `initialize`
- `session/create`
- `session/list`
- `session/get`
- `message/send`
- `context/attach_browser`
- `context/attach_screen`
- `turn/cancel`

Core notifications:

- `session/updated`
- `turn/started`
- `turn/delta`
- `turn/completed`
- `turn/failed`
- `approval/requested`
- `context/attached`
- `error`

Future browser action methods / notifications:

- `browser/list_interactables`
- `browser/action_proposed`
- `browser/action_approved`
- `browser/action_completed`

Transport rules:

- Bind only to `127.0.0.1`.
- Require bearer-style local token or equivalent pairing credential.
- Check allowed origins for browser extension clients.
- Keep per-message size limits.
- Keep session-scoped IDs.
- Do not expose the WebSocket listener on `0.0.0.0`.
- Leave `/healthz` and `/readyz` HTTP endpoints for local debugging.

Phase 0 compatibility:

- `POST /v0/capture` can remain temporarily for debug / migration.
- New chat UI should not build on `POST /v0/capture`.
- Long-term, capture is represented as `context/attach_browser` or `context/attach_screen`, followed by `message/send`.

## Sidekick JSON-RPC Protocol Contract

Decision:

> Define a versioned Screen Sidekick protocol over WebSocket.
> UI clients speak Sidekick protocol only.
> `sidekick-daemon` is the translation boundary between UI protocol, session state, safety/capture pipeline, and Codex app-server.

The protocol is not:

- Codex app-server protocol exposed to Chrome
- a generic MCP protocol
- a browser automation protocol
- a debug-only bridge

The protocol is:

- a local UI-to-daemon contract
- the shared API for Chrome side panel and Tauri desktop UI
- the streaming chat/event transport
- the session/context/turn lifecycle owner-facing surface
- the stable error/status surface users will see

### Transport

Primary transport:

```text
ws://127.0.0.1:<port>/v0/ws
```

Rules:

- JSON-RPC 2.0 shaped messages over WebSocket.
- One WebSocket connection represents one UI client instance.
- The daemon may serve multiple clients at once.
- The protocol version is negotiated during `initialize`.
- The daemon sends notifications for live changes.
- Client requests must include `id`.
- Daemon notifications must not include `id`.
- Request/response ordering must not be assumed.
- Large binary payloads are not sent inline in Phase 1.
- `/healthz` and `/readyz` remain plain HTTP.

Security:

- bind only to `127.0.0.1`
- require a local bearer/pairing token
- check browser extension origin / extension id
- reject unknown origins by default
- enforce max message size
- enforce max attachment size
- enforce idle and turn timeouts
- never expose listener on `0.0.0.0`
- do not put token in URL query string

### Envelope

Client request:

```json
{
  "jsonrpc": "2.0",
  "id": "req_01",
  "method": "message/send",
  "params": {}
}
```

Daemon success response:

```json
{
  "jsonrpc": "2.0",
  "id": "req_01",
  "result": {}
}
```

Daemon error response:

```json
{
  "jsonrpc": "2.0",
  "id": "req_01",
  "error": {
    "code": "session_not_found",
    "message": "Session was not found.",
    "data": {
      "retryable": false
    }
  }
}
```

Daemon notification:

```json
{
  "jsonrpc": "2.0",
  "method": "turn/delta",
  "params": {}
}
```

Rules:

- `message` is user-facing and stable enough for UI display.
- `code` is stable and used by UI behavior.
- `data` is typed per error code.
- unknown `data` fields are ignored by clients.
- request ids are scoped to one WebSocket connection.
- domain ids use explicit prefixes where useful: `sess_`, `msg_`, `att_`, `turn_`.

### Initialize

`initialize` is required before other methods.

Request includes:

- `client_kind`: `chrome_extension` | `tauri_desktop`
- `client_version`
- `protocol_version`
- `capabilities`
- optional `extension_id`
- optional `origin`

Response includes:

- accepted `protocol_version`
- daemon version
- protocol capabilities
- auth/session status
- codex readiness summary
- max message / attachment limits
- current user-facing warnings

Capabilities should be explicit, not inferred:

- `browser_context`
- `desktop_context`
- `chat_stream`
- `turn_cancel`
- `approval_ui`
- `browser_actions`
- `debug_export`

If the protocol version is unsupported, return `unsupported_protocol_version`.

### Core Methods

Phase 1 methods:

- `initialize`
- `session/create`
- `session/list`
- `session/get`
- `session/subscribe`
- `session/unsubscribe`
- `context/attach_browser`
- `message/send`
- `turn/cancel`
- `status/get`

Phase 2 methods:

- `context/attach_screen`
- `message/list`
- `attachment/list`
- `debug/export_context`
- `debug/export_prompt`
- `settings/get`
- `settings/update`

Browser action phase methods:

- `browser/list_interactables`
- `browser/action/preview`
- `browser/action/approve`
- `browser/action/reject`

Method ownership:

```text
session/*       -> crates/session + sidekick-daemon
context/*       -> capture-pipeline + safety + session
message/send    -> sidekick-daemon + codex-client + session
turn/*          -> codex-client + sidekick-daemon
status/*        -> sidekick-daemon + codex-client diagnostics
browser/action  -> extension adapter + sidekick-daemon safety gate
debug/*         -> daemon-owned export path
settings/*      -> daemon-owned local settings
```

### Notifications

Core notifications:

- `session/updated`
- `context/attached`
- `message/created`
- `turn/started`
- `turn/delta`
- `turn/completed`
- `turn/failed`
- `turn/cancelled`
- `status/changed`
- `error`

Future notifications:

- `approval/requested`
- `approval/resolved`
- `browser/action_proposed`
- `browser/action_completed`

Notification rules:

- notifications are session-scoped when related to a session
- clients receive only subscribed session events plus global status events
- `turn/delta` carries append-only text chunks or structured progress events
- `turn/completed` carries final metadata, not duplicated full history unless requested
- `turn/failed` carries stable error code and retryability
- raw Codex app-server events are not forwarded directly

### Session, Message, Attachment, Turn Models

Protocol-visible session:

```text
SessionSummary
  - id
  - title
  - created_at
  - updated_at
  - active_turn_id?
  - source_summary
  - codex_status
```

Protocol-visible message:

```text
Message
  - id
  - session_id
  - role: user | assistant | system_notice
  - created_at
  - text
  - attachment_ids
  - turn_id?
  - status: pending | streaming | completed | failed | cancelled
```

Protocol-visible attachment:

```text
Attachment
  - id
  - session_id
  - source_type: browser_tab | desktop_screen | manual_text
  - created_at
  - summary
  - safety_status
  - debug_available
```

Protocol-visible turn:

```text
Turn
  - id
  - session_id
  - user_message_id
  - assistant_message_id?
  - status: pending | running | completed | failed | cancelled
  - started_at
  - completed_at?
  - error?
```

UI protocol should expose summaries by default.
Full sanitized context / prompt debug export requires an explicit debug method.

### `context/attach_browser`

Purpose:

Attach current tab context to a Sidekick session.

Request includes:

- `session_id`
- `capture_id`
- raw browser capture payload from extension adapter
- `capture_reason`: `message_send` | `manual_attach` | `debug`
- optional `related_message_id`

Daemon behavior:

- validate capture size and schema
- normalize raw capture
- run safety review
- create sanitized context
- persist attachment summary and sanitized context
- return `attachment_id`
- emit `context/attached`

Rules:

- raw browser capture must not be persisted
- raw browser capture must not be sent to Codex
- screenshot metadata string fields must be validated/constrained before persistence or prompt use
- capture failure should be field-level where safe; for example invalid screenshot string metadata can be dropped
- permission failures should map to typed protocol errors

### `message/send`

Purpose:

Send a user question to the linked Codex thread.

Request includes:

- `session_id`
- `text`
- optional `attachment_ids`
- optional `capture_current_context`: boolean
- optional `workspace_binding`
- optional `mode`: `ask_only` | `repo_assisted`

Daemon behavior:

1. create and persist user message
2. attach current context when requested
3. build Codex context package from sanitized attachments
4. ensure Codex thread exists
5. start Codex turn
6. stream events as notifications
7. persist final assistant message or failure state

Rules:

- if a turn is already running for the session, return `turn_already_running`
- `ask_only` must not imply repo/tool execution
- `repo_assisted` requires explicit workspace binding
- if approval UI is required but unavailable, return or emit `approval_ui_not_supported`
- never silently drop user text after persisting it; failed turn state must be visible

### `turn/cancel`

Purpose:

Cancel an active turn.

Request includes:

- `session_id`
- `turn_id`

Behavior:

- map to app-server cancel when supported
- mark local turn as cancelling/cancelled
- emit `turn/cancelled`
- if app-server cancel is unsupported, return `turn_cancel_unsupported`

### Error Codes

Stable protocol error codes:

- `unauthorized`
- `forbidden_origin`
- `unsupported_protocol_version`
- `invalid_request`
- `invalid_params`
- `method_not_found`
- `payload_too_large`
- `rate_limited`
- `session_not_found`
- `message_not_found`
- `attachment_not_found`
- `turn_not_found`
- `turn_already_running`
- `turn_cancel_unsupported`
- `context_too_large`
- `context_rejected`
- `browser_permission_missing`
- `browser_capture_failed`
- `safety_review_failed`
- `codex_not_found`
- `codex_not_logged_in`
- `codex_app_server_unavailable`
- `unsupported_codex_version`
- `codex_turn_failed`
- `approval_required`
- `approval_ui_not_supported`
- `workspace_required`
- `workspace_not_found`
- `internal_error`

Error data should include where useful:

- `retryable`
- `user_action`
- `details_debug_id`
- `required_permission`
- `supported_versions`
- `observed_version`
- `max_size_bytes`

Do not put raw page text, secrets, raw DOM, or bearer tokens in error messages or error data.

### Reconnect And Idempotency

WebSocket reconnect is normal.

Rules:

- clients can call `session/subscribe` after reconnect
- daemon sends current session state after subscribe
- active turn state is recoverable from `session/get`
- `message/send` should accept a client-generated idempotency key
- if the same idempotency key is retried after connection loss, daemon returns the existing message/turn rather than creating a duplicate
- notifications may be missed during disconnect; clients recover by fetching session state
- the daemon is the source of truth for persisted messages and turns

### Versioning

Protocol versioning:

- start at `sidekick.protocol.v0`
- breaking changes require a new protocol version
- additive fields are allowed
- unknown fields are ignored by receivers unless explicitly forbidden
- unknown methods return `method_not_found`
- unknown notification kinds are ignored by clients but may be shown in debug logs

Schema policy:

- keep protocol types in `crates/sidekick-protocol`
- commit JSON schema or generated TypeScript types for extension/Tauri UI consumption
- do not let TypeScript define independent domain protocol types
- TypeScript may import generated types or keep adapter-local lightweight types derived from Rust source
- protocol examples should live next to schema/tests

### Tests Required For The First Implementation

Focused tests:

- request/response envelope serialization
- notification serialization
- `initialize` version negotiation
- auth/origin rejection maps to stable error code
- `message/send` rejects duplicate active turn
- `message/send` idempotency retry does not create duplicate message
- `context/attach_browser` never persists raw capture
- `context/attach_browser` rejects or drops invalid metadata according to safety rules
- unknown notification/event from Codex does not leak raw app-server JSON to UI
- error responses never include raw page text or token values

Integration tests:

- fake Chrome client over WebSocket
- fake Tauri client over WebSocket or service boundary
- fake Codex app-server stream through daemon
- reconnect during active turn followed by `session/get`
- two clients subscribed to same session receive consistent turn state

Verification commands should eventually include:

```text
cargo test -p screen-sidekick-sidekick-protocol
cargo test -p screen-sidekick-sidekick-daemon
npm --prefix apps/extension test
```

Exact package names can change, but protocol ownership and tests should remain.

## Existing Browser Tools

Chrome DevTools MCP and Playwright MCP already exist.
Therefore, Screen Sidekick should not make "generic browser MCP replacement" its main value.

Use them as heavier tools when needed:

```text
Sidekick
  - fast current page context
  - chat-first page questions
  - lightweight candidate actions

Chrome DevTools MCP / Playwright MCP
  - deeper browser automation
  - console / network / performance
  - richer test/debug flows
```

Sidekick can route to those tools through Codex when the light context is not enough.

## Browser Action Strategy

Browser action is useful, but it should come after chat-first ask works.

Initial action goals:

- Fill this form
- Click this candidate
- Scroll
- Read result after action

Safety model:

- Extension enumerates interactable elements.
- Each candidate gets a temporary ID.
- Codex proposes candidate-based actions.
- Sidekick previews the exact action.
- User approves.
- Extension executes.

Avoid:

- Autonomous browser agent loop inside Sidekick.
- Free-form JS or selector execution from Codex.
- Click/type/submit without preview.
- Skipping confirmation for delete / publish / send / billing / permission / secret actions.

## Local Screen Strategy

Local screen is a differentiator from Chrome-only companions.

Primary value:

- Ask Codex about terminal / IDE / local app without attaching screenshots manually.
- Maintain screen session context.
- Let Codex decide whether command/repo investigation is needed.

Not the priority:

- General desktop automation.
- Reimplementing Computer Use.
- Clicking local app UI.

For developer workflows, Codex should usually prefer files, commands, APIs, and repo context over GUI manipulation.

## Screen Session

Decision:

> Use `1 Sidekick session = 1 Codex thread`.
> Persist sessions in SQLite from the start.
> Store sanitized context and metadata, not raw DOM or raw screenshots.

Screen Session should support chat-first usage.

It can remember:

- current screen
- previous screens
- current browser URL/title
- active app/window title
- OCR / extracted text
- DOM summary
- user questions
- Codex answers
- decisions made
- current repo/cwd/branch if known
- safety notes

Value:

- "さっきの続き。今この画面。次どうする？"
- Browser and local screens can belong to the same Codex conversation.
- The session can later become a Markdown runbook, issue note, or PR context.

Session model:

```text
SidekickSession
  - id
  - title
  - created_at
  - updated_at
  - codex_thread_id
  - current_turn_id?
  - source_summary

SidekickMessage
  - id
  - session_id
  - role: user | codex | system
  - created_at
  - text
  - turn_id?

ScreenAttachment
  - id
  - session_id
  - message_id?
  - source_type: browser_tab | desktop_screen | manual_text
  - created_at
  - sanitized_context_json
  - safety_review_json
  - source_metadata_json

CodexThreadLink
  - session_id
  - codex_thread_id
  - created_at
  - updated_at
```

Persistence:

- Use SQLite, owned by the local daemon.
- Keep session state available across Chrome side panel reloads and Tauri restarts.
- Fan out live turn events to all connected UI clients for the same session.
- Allow Chrome and Tauri to attach context to the same session.
- Do not store raw DOM.
- Do not store raw screenshots by default.
- If screenshot images are added later, store only explicit user-approved image attachments or local file references with retention rules.

Context attachment policy:

- Attach current browser/screen context per user message by default.
- Store attachments as session objects that messages can reference.
- Send only the relevant current attachment plus compact session summary to Codex where possible.
- Add summarization/compaction later when long sessions grow.

## SQLite Session Storage Contract

Decision:

> `crates/session` owns SQLite schema, migrations, repository APIs, and state transition invariants.
> SQLite is the source of truth for Sidekick sessions, persisted messages, attachments, turns, Codex thread links, and `message/send` idempotency.

SQLite is not just history storage.
It is required for:

- Chrome side panel reload recovery
- Tauri restart recovery
- WebSocket reconnect recovery
- multiple UI clients watching the same session
- active turn state
- Codex thread mapping
- idempotent `message/send`
- debug/export artifacts

### Ownership

```text
crates/session
  - migrations
  - schema tests
  - repository traits / concrete SQLite repository
  - transaction helpers
  - state transition validation
  - retention hooks

crates/sidekick-daemon
  - calls session repository
  - owns orchestration
  - owns live WebSocket fanout
  - does not define database schema
```

Tauri code and Chrome extension code must not access the database directly.

### Initial Tables

Required from the first persistent implementation:

```text
schema_migrations
sessions
messages
attachments
turns
codex_thread_links
idempotency_keys
```

Optional later tables:

```text
workspaces
session_summaries
attachment_blobs
settings
debug_exports
```

Do not add optional tables until their owner and retention policy are clear.

### Table Contracts

`sessions`:

- `id`
- `title`
- `created_at`
- `updated_at`
- `archived_at?`
- `active_turn_id?`
- `source_summary`
- `default_workspace_id?`

`messages`:

- `id`
- `session_id`
- `role`: `user` | `assistant` | `system_notice`
- `text`
- `status`: `pending` | `streaming` | `completed` | `failed` | `cancelled`
- `turn_id?`
- `created_at`
- `completed_at?`

`attachments`:

- `id`
- `session_id`
- `message_id?`
- `source_type`: `browser_tab` | `desktop_screen` | `manual_text`
- `created_at`
- `summary`
- `sanitized_context_json`
- `safety_review_json`
- `source_metadata_json`
- `debug_available`

`turns`:

- `id`
- `session_id`
- `user_message_id`
- `assistant_message_id?`
- `codex_thread_id?`
- `codex_turn_id?`
- `status`: `pending` | `running` | `completed` | `failed` | `cancelled`
- `error_code?`
- `error_debug_id?`
- `started_at`
- `completed_at?`

`codex_thread_links`:

- `session_id`
- `codex_thread_id`
- `codex_cli_version?`
- `codex_schema_hash?`
- `created_at`
- `updated_at`

`idempotency_keys`:

- `session_id`
- `method`
- `key`
- `request_hash`
- `message_id?`
- `turn_id?`
- `status`: `in_progress` | `completed` | `failed`
- `created_at`
- `expires_at`

### Invariants

- `1 Sidekick session = 1 Codex thread` once a Codex thread is created.
- A session can exist before a Codex thread exists.
- A failed thread creation must not persist a fake `codex_thread_id`.
- At most one active turn per session.
- `message/send` must be idempotent for the same `(session_id, method, key)`.
- Retrying the same idempotency key after reconnect returns the existing message/turn.
- A completed assistant message must belong to a completed turn.
- A failed turn must be visible in session state.
- Deleting or archiving a session must not leave orphan active turns.
- raw browser capture must not be persisted.
- raw DOM must not be persisted.
- raw screenshots must not be persisted by default.
- bearer tokens, cookies, localStorage/sessionStorage, hidden input values, and password values must not be persisted.

### Transaction Boundaries

`message/send` should use explicit transactions:

1. validate session and active turn state
2. reserve idempotency key
3. persist user message
4. persist requested attachments or attachment links
5. create pending turn
6. commit before calling Codex app-server

After app-server starts:

- update turn to `running`
- stream deltas through daemon fanout
- persist final assistant message and mark turn `completed`
- or persist failure/cancel state

Do not hold a SQLite write transaction while waiting for a Codex stream.

### Storage And Runtime Settings

SQLite settings:

- enable foreign keys
- use WAL mode where supported
- keep migrations deterministic
- avoid runtime schema mutation outside migrations
- store timestamps in UTC ISO-8601 or integer epoch consistently

Database location:

- owned by local daemon profile directory
- not inside the project repo by default
- configurable only through daemon settings

### Retention And Debug Export

Default persistence:

- sanitized context
- safety review
- source metadata
- message text
- assistant text
- turn status

Not persisted by default:

- raw DOM
- raw browser capture
- raw screenshot images
- tokens / secrets / cookies
- app-server raw events

Debug/export:

- explicit debug methods can export sanitized context or prompt
- debug export should include schema/version metadata
- debug export must not bypass safety boundary
- raw capture export is not a default feature

### Tests Required For The First Implementation

Focused tests:

- migrations apply on an empty database
- foreign keys are enforced
- one active turn per session is enforced
- `message/send` idempotency prevents duplicate messages/turns
- failed thread creation leaves session valid without fake thread id
- reconnect lookup can recover active turn state
- attachments persist sanitized context but not raw capture
- archived/deleted session cannot keep active turn
- error/debug fields do not contain raw page text or token values

Integration tests:

- fake daemon flow: create session -> attach browser context -> send message -> stream completion
- reconnect during active turn -> `session/get` recovers current state
- two subscribed clients observe the same persisted session state
- app-server crash during turn persists failed turn and no fake assistant completion

Verification commands should eventually include:

```text
cargo test -p screen-sidekick-session
cargo test -p screen-sidekick-sidekick-daemon
```

Exact package names can change, but `crates/session` remains the schema owner.

## Current MVP Reality

Phase 0-B currently has:

- Chrome extension current tab capture.
- Loopback bridge to Rust/Tauri.
- Rust RawScreenContext -> SanitizedScreenContext -> prompt / JSON.
- Safety review and secret masking.
- Extension display for `screen_context_json` and `prompt_text`.

This is useful as core infrastructure, but not the final UX.

Needed next:

- Hide bridge URL/token from the main product UI.
- Replace prompt copy as the main action with Ask Codex.
- Make Chrome side panel a chat UI.
- Add a Codex connector.
- Keep prompt/JSON as debug/export.

## Success Criteria

Old:

> 1週間使って、手動スクショ貼り付けに戻りたくないか？

Better:

> Chrome でも local screen でも、Sidekick を開けば Codex にその場で聞けるか？

Concrete:

- `このページ要約して` が copy/paste なしでできる。
- `このボタン何？` が current tab context つきで聞ける。
- `このフォーム埋めて` が confirmation-first で進められる。
- `この画面なに？` が local screen hotkey で聞ける。
- Codex の返答が Sidekick UI に返る。
- 必要なら repo/test/docs/PR 作業へ自然に進める。
- Computer Use / DevTools MCP を使う前に軽い相談ができる。

## Proposed Roadmap

### Phase 1: Chat-First Browser MVP

- Add the new Rust crate boundaries:
  - `crates/session`
  - `crates/sidekick-protocol`
  - `crates/codex-client`
  - `crates/sidekick-daemon`
- Commit a Codex app-server schema snapshot for the supported CLI version.
- Add explicit schema refresh/check metadata and make targets.
- Implement a narrow handwritten Rust app-server client backed by schema fixture tests.
- Define `crates/sidekick-protocol` request / response / notification / error schema.
- Add initial SQLite migrations for sessions/messages/attachments/turns/thread links/idempotency.
- Add WebSocket reconnect and `message/send` idempotency contract tests.
- Chrome side panel を chat UI にする。
- Main button を `Ask Codex` にする。
- Current tab context を自動添付する。
- `screen_context_json` / `prompt_text` は debug/export に移す。
- UI-daemon transport を JSON-RPC-over-WebSocket に置き換える。
- `codex app-server` stdio connector の最小実装を作る。
- Codex response stream を side panel chat UI に表示する。
- Session / stream API は、後で Tauri chat UI も同じ session に接続できる形で設計する。

### Phase 2: Local Codex Connector / Session

- Tauri/local daemon を app-server owner として整理する。
- Bridge URL/token 設定をユーザー向け UI から隠す。
- SQLite-backed Screen Session を Tauri UI / settings / debug export へ広げる。
- Chrome side panel と desktop app が同じ session を見られるようにする。
- Tauri desktop app に chat UI を追加し、local screen なしでも同じ Codex session に参加できるようにする。
- Legacy `POST /v0/capture` を debug / compatibility に降格する。
- `codex exec` / copy/export fallback を残す。

### Phase 3: Desktop Summon

- Global hotkey。
- Active window screenshot。
- OCR。
- App/window title。
- `Ask Codex about this screen`。

### Phase 4: Confirmed Browser Actions

- Interactable candidate IDs。
- Confirmed click。
- Confirmed type。
- Form fill plan。
- Dangerous action confirmation。

### Phase 5: Deeper Codex Integration

- Approval UI を本格化する。
- Resume / attach to existing Codex session。
- Codex Desktop integration if possible。
- Codex SDK は、app-server primary path を置き換える必要がある場合だけ再評価する。

### Phase 6: Export / Handoff Artifacts

- Markdown runbook。
- Issue / PR context。
- Repro package。
- Debug prompt / JSON export。

## Open Questions

- app-server protocol の具体的な method / event 名は、schema snapshot 作成時にどれを Phase 1 subset にするか。
- 起動中 Codex TUI session に external context を push できるか。
- Codex Desktop との接続口はあるか。
- Local OCR は Rust/Tauri 側で持つか、OS/external tool に寄せるか。
- Browser action は extension API で軽く持つか、既存 Chrome DevTools MCP に寄せるか。
- Screenshot image を Codex に添付するか、OCR/DOM/context text から始めるか。
- Long session の compaction / summary owner をどこに置くか。
- OSS として setup/auth/token をどう簡単にするか。

## Boundary Update Needed If Adopted

現在の `docs/non-executor-boundary.md` は、Sidekick が browser automation をしない前提で書かれている。

Chat-first 方向を採用するだけなら、境界は大きく変えなくてよい。

Browser actions を採用するなら、次のように改定する必要がある:

- Sidekick must not autonomously execute browser actions.
- Sidekick may provide user-approved browser operation tools to Codex.
- Click / type / submit must be candidate-based and previewed.
- Dangerous actions require explicit confirmation.
- Sidekick still must not edit local repos, run tests, or replace Codex approvals/sandbox.

この改定はまだ未実施。

## One-Line Pitch Candidates

- Codex 版 Gemini / Claude in Chrome。ローカル画面にも召喚できる。
- Ask Codex about any screen.
- ブラウザでもローカル画面でも、今見ているものを Codex に聞ける。
- スクショ貼り付けより楽で、Computer Use より軽い Codex companion。
- Codex に目をつける。ただし脳と手は Codex に任せる。
