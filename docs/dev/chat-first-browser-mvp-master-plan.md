# Chat-First Browser MVP Master Plan

Status: implementation master plan / draft

この文書は Goal 実装者向けの source of truth である。
方向性と契約の根拠は `docs/codex-companion-direction.md` に置き、この文書では Phase 1 を実装可能な粒度へ分解する。

この文書は実装計画であり、実装そのものではない。
commit / push / PR はユーザーの明示指示がある場合だけ行う。

## Goal

Phase 1 の目的は、Screen Sidekick を copy-first の prompt/JSON preview から、Chrome side panel で Codex に直接聞ける chat-first MVP へ移行すること。

最初の成功体験:

```text
Chrome でページを見る
  -> Screen Sidekick side panel を開く
  -> 「このページ要約して」「このボタン何？」と聞く
  -> extension が current tab context を capture
  -> Rust が sanitize / package
  -> sidekick-daemon が codex app-server に turn を投げる
  -> Codex response stream が side panel chat に出る
```

## Non-Goals

Phase 1 で実装しないもの:

- browser click / type / submit
- autonomous browser agent loop
- local desktop screenshot / OCR / global hotkey
- Codex Desktop direct integration
- MCP tool registration
- Computer Use integration
- local repo editing inside Sidekick
- Sidekick 独自の approval / sandbox 代替
- raw DOM / raw screenshot の永続化
- TypeScript 側 domain logic の増加

Phase 1 では `screen_context_json` / `prompt_text` は debug/export へ降格する。
主 UX は `Ask Codex` と streaming chat である。

## Source Documents

- `docs/codex-companion-direction.md`
- `docs/non-executor-boundary.md`
- `AGENTS.md`
- `Cargo.toml`
- `Makefile`
- `apps/extension/src/side_panel.ts`
- `apps/extension/src/capture_contract.ts`
- `apps/desktop/src-tauri/src/bridge.rs`
- `crates/capture-pipeline/src/lib.rs`
- `crates/safety/src/lib.rs`
- `crates/prompt/src/lib.rs`

## Current Source Findings

Source-confirmed facts at plan creation time:

- Workspace crates currently include:
  - `crates/screen-context`
  - `crates/safety-rules`
  - `crates/safety`
  - `crates/prompt`
  - `crates/capture-pipeline`
- `apps/desktop/src-tauri` and `apps/desktop/ui` are excluded from the root workspace.
- Current bridge is HTTP-only:
  - Tauri starts a loopback bridge in `apps/desktop/src-tauri/src/bridge.rs`.
  - Current endpoint is `POST /v0/capture`.
  - It has bearer token auth, Chrome extension origin checks, and `MAX_CAPTURE_BODY_BYTES`.
- Current extension side panel is copy/preview oriented:
  - stores bridge URL/token in `chrome.storage.session`
  - captures current tab
  - posts to `/v0/capture`
  - renders `screen_context_json`, `prompt_text`, and safety summary
- Current capture pipeline:
  - accepts `RawBrowserContext`
  - normalizes to `RawScreenContext`
  - runs `review_screen_context`
  - builds `screen_context_json`
  - builds `prompt_text`
- Current safety boundary already has:
  - `SanitizedScreenContext`
  - `PromptSafeText`
  - `SanitizedUrl`
  - constrained screenshot metadata
- Current prompt crate only accepts `SanitizedScreenContext` and `PromptSafetyReview`.
- Local Codex reference at plan creation:
  - `codex-cli 0.138.0`
  - `codex app-server` supports `--listen stdio://` / `--stdio`
  - `codex app-server generate-json-schema --out <DIR>` exists
  - app-server and generated schema commands are marked experimental

If implementation starts in a later session, re-read these files and re-run the local Codex discovery commands before editing.

## Global Contracts

These apply to every phase.

### Rust-First Ownership

- Rust owns domain contracts, safety, protocol, session state, Codex integration, and daemon orchestration.
- TypeScript owns Chrome API calls, DOM capture, side panel rendering, and adapter-level WebSocket client code.
- TypeScript must not become the source of truth for protocol, safety, danger detection, session semantics, or Codex state.

### Sanitization Boundary

- Raw browser capture may enter only capture/normalize/safety paths.
- Codex context and UI debug/export must use `SanitizedScreenContext` or validated metadata.
- raw DOM, cookies, localStorage, sessionStorage, hidden input values, password values, bearer tokens, and raw screenshots must not be persisted or sent to Codex.
- page-originated text must be treated as untrusted context, not instructions.
- prompt/context packaging must not let page text create top-level instructions.

### Protocol Boundary

- Chrome/Tauri UI speaks Sidekick protocol only.
- UI must not receive raw Codex app-server events.
- `crates/sidekick-protocol` owns request/response/notification/error types.
- `crates/codex-client` owns app-server child process, protocol subset, schema compatibility, and Codex diagnostics.
- `crates/sidekick-daemon` owns translation from Sidekick protocol to session/capture/safety/Codex operations.

### Session Boundary

- `1 Sidekick session = 1 Codex thread` once the thread is created.
- Session may exist before Codex thread creation.
- `message/send` creates a persisted user message and pending turn before calling Codex.
- A failed Codex thread/turn must be visible as failed state, not silent loss.
- At most one active turn per session for Phase 1.
- `message/send` must be idempotent with a client-provided idempotency key.

### Build And OSS Stability

- Normal build/test must not require local `codex`.
- `codex app-server generate-json-schema` is an explicit developer command.
- Default tests must be offline and deterministic.
- Real Codex app-server smoke tests are optional/manual unless CI pins a Codex version.

## Target Crate Layout

Add these root workspace crates:

```text
crates/sidekick-protocol
crates/session
crates/codex-client
crates/sidekick-daemon
```

Expected dependency direction:

```text
sidekick-protocol
  - depends on serde/serde_json only where possible

session
  - depends on sidekick-protocol only if protocol-visible DTO reuse is truly needed
  - otherwise owns storage/domain records and maps at daemon boundary

codex-client
  - owns schema snapshot
  - may expose typed Codex events/errors
  - must not depend on sidekick-daemon

sidekick-daemon
  - depends on sidekick-protocol
  - depends on session
  - depends on codex-client
  - depends on capture-pipeline/safety/prompt as needed

apps/desktop/src-tauri
  - starts/supervises daemon or embeds daemon service
  - remains Tauri shell/launcher

apps/extension
  - imports generated protocol types if available
  - otherwise uses adapter-local types derived from Rust schema
```

Avoid cyclic dependencies.
Avoid putting daemon policy inside `apps/desktop/src-tauri`.

## Phase 0: Preflight And Source Alignment

Purpose:

Before implementation, make sure the Goal runner is not working from stale assumptions.

Tasks:

- Re-read this plan.
- Re-read `docs/codex-companion-direction.md`.
- Re-read `AGENTS.md`.
- Check `git status --short` and identify unrelated dirty changes.
- Re-run:

```text
codex --version
codex app-server --help
codex app-server generate-json-schema --help
```

- Re-read current owners:
  - `Cargo.toml`
  - `Makefile`
  - `crates/capture-pipeline/src/lib.rs`
  - `crates/safety/src/lib.rs`
  - `crates/prompt/src/lib.rs`
  - `apps/desktop/src-tauri/src/bridge.rs`
  - `apps/extension/src/side_panel.ts`
  - `apps/extension/src/capture_contract.ts`

Completion:

- Implementation owner map is still valid or updated.
- Any source drift is recorded before editing.

Stop conditions:

- local Codex app-server command is missing in a way that invalidates Phase 1
- current source has already implemented parts of this plan differently
- workspace layout changed enough that crate ownership must be reconsidered

## Phase 1: Workspace And Crate Scaffold

Purpose:

Create stable package boundaries before implementation logic accumulates in Tauri or extension files.

Tasks:

- Add `crates/sidekick-protocol`.
- Add `crates/session`.
- Add `crates/codex-client`.
- Add `crates/sidekick-daemon`.
- Register them in root `Cargo.toml`.
- Add workspace dependencies only as needed.
- Keep initial crates compiling with minimal public API.

Initial package responsibilities:

- `sidekick-protocol`: JSON-RPC envelope, protocol version, error code enum, method/notification DTOs.
- `session`: SQLite schema/migration owner and repository API.
- `codex-client`: app-server schema snapshot, stdio client, Codex event/error types.
- `sidekick-daemon`: WebSocket server, auth/origin checks, session/capture/Codex orchestration.

Tests:

```text
cargo test -p screen-sidekick-sidekick-protocol
cargo test -p screen-sidekick-session
cargo test -p screen-sidekick-codex-client
cargo test -p screen-sidekick-sidekick-daemon
```

Completion:

- All new crates compile.
- No production logic is hidden in generic `utils` modules.
- No app-server or SQLite behavior is faked through public test-only hooks.

## Phase 2: Sidekick Protocol Contract

Purpose:

Define the UI-daemon protocol before daemon and UI implementation diverge.

Tasks:

- Implement JSON-RPC 2.0 shaped envelope:
  - request
  - success response
  - error response
  - notification
- Implement protocol version:
  - start with `sidekick.protocol.v0`
- Implement core request/response types:
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
- Implement core notifications:
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
- Implement stable error codes from `docs/codex-companion-direction.md`.
- Add schema or generated TypeScript export path if feasible in this phase.

Important details:

- Error `message` is displayable.
- Error `code` is stable and drives UI behavior.
- Error `data` must not include raw page text, raw DOM, bearer token, or secret values.
- Unknown additive fields should be ignored where safe.
- raw app-server event JSON is not part of Sidekick protocol.

Tests:

- envelope serialization/deserialization
- notification serialization
- `initialize` version negotiation
- unknown method -> `method_not_found`
- unsupported version -> `unsupported_protocol_version`
- error redaction fixture with token/page text values

Completion:

- Protocol crate can be tested without daemon, Codex, Chrome, or Tauri.
- Protocol examples exist next to tests/schema.

## Phase 3: SQLite Session Storage

Purpose:

Build the persistent source of truth for sessions, messages, attachments, turns, Codex thread links, and idempotency.

Tasks:

- Add SQLite dependency choice in `crates/session`.
- Add migrations:
  - `schema_migrations`
  - `sessions`
  - `messages`
  - `attachments`
  - `turns`
  - `codex_thread_links`
  - `idempotency_keys`
- Add repository API:
  - create/list/get session
  - create user message
  - create attachment
  - create/update turn
  - link Codex thread
  - reserve/resolve idempotency key
  - recover active turn state
- Enable foreign keys.
- Use WAL where supported.
- Do not hold write transactions while waiting for Codex stream.

Invariants:

- at most one active turn per session
- retrying same idempotency key returns existing message/turn
- failed Codex thread creation leaves no fake thread id
- raw browser capture is not persisted
- raw DOM and raw screenshots are not persisted by default

Tests:

- migrations apply on empty DB
- foreign keys enforced
- active-turn uniqueness
- idempotency prevents duplicate message/turn
- failed thread creation leaves valid session
- reconnect lookup recovers active turn state
- attachment persists sanitized context only
- error/debug fields do not contain token/page secret fixtures

Completion:

- `crates/session` can be tested without daemon, Codex, Chrome, or Tauri.
- Session repository has no UI-specific behavior.

## Phase 4: Codex App-Server Schema Snapshot

Purpose:

Make app-server compatibility explicit without making normal builds depend on local Codex.

Tasks:

- Add schema directory:

```text
crates/codex-client/schema/
  metadata.json
  README.md
  app-server/
  examples/
```

- Add `make codex-schema-refresh`.
- Add `make codex-schema-check`.
- Add `make codex-schema-metadata`.
- Generate schema snapshot with:

```text
codex app-server generate-json-schema --out crates/codex-client/schema/app-server
```

- Record metadata:
  - Codex CLI version
  - generation command
  - generation timestamp
  - experimental flag
  - schema hash
- Add example fixtures for Phase 1 subset.

Rules:

- Do not run schema generation from `build.rs`.
- Do not run schema generation in default `cargo test`.
- If `--experimental` is required, record it explicitly and do not mix schemas.

Tests:

- schema JSON parses
- metadata hash matches committed schema
- example fixtures parse
- schema drift check works when explicitly invoked

Completion:

- Normal `cargo test --workspace` works without invoking `codex`.
- Schema drift is visible via explicit command.

## Phase 5: Codex Client

Purpose:

Create a narrow, testable Rust client for the app-server subset needed by Phase 1.

Tasks:

- Add app-server child process owner.
- Prefer stdio:

```text
codex app-server --stdio
```

- Implement runtime discovery:
  - find `codex` from PATH or configured path
  - `codex --version`
  - app-server command/capability probe
- Implement typed errors:
  - Codex CLI missing
  - app-server command unavailable
  - unsupported version/schema
  - not logged in
  - child process crashed
  - unknown event/method
  - turn failed/cancelled
- Implement Phase 1 handwritten request/response/event subset.
- Implement request id correlation.
- Implement stream event reader.
- Implement cancellation if supported by schema; otherwise return typed unsupported error.
- Add fake app-server stdio harness for deterministic tests.

Rules:

- Do not expose raw app-server JSON to UI.
- Unknown event kind becomes typed diagnostic, not panic.
- Do not auto-install Codex.
- Do not store Codex credentials.

Tests:

- missing binary -> typed diagnostic
- unsupported version -> typed diagnostic
- request id correlation
- known example events deserialize
- unknown event kind -> diagnostic
- fake stream produces typed turn delta/completed events
- app-server crash during turn -> failed turn event
- cancel maps to app-server cancel or unsupported error

Completion:

- `crates/codex-client` can be tested without real OpenAI network calls.
- Manual real app-server smoke test is documented but not required in default suite.

## Phase 6: Sidekick Daemon

Purpose:

Own the local WebSocket protocol, session orchestration, capture attachment, Codex turn lifecycle, and streaming fanout.

Tasks:

- Implement loopback WebSocket endpoint:

```text
ws://127.0.0.1:<port>/v0/ws
```

- Keep `/healthz` and `/readyz` HTTP endpoints.
- Implement local token/pairing credential.
- Implement Chrome extension origin/extension id checks.
- Implement `initialize`.
- Implement `session/create`, `session/list`, `session/get`, `session/subscribe`, `session/unsubscribe`.
- Implement `context/attach_browser`:
  - validate size/schema
  - normalize raw browser capture
  - run safety review
  - persist sanitized attachment
  - emit `context/attached`
- Implement `message/send`:
  - validate session
  - reserve idempotency key
  - persist user message
  - attach current context if requested
  - create pending turn
  - ensure Codex thread
  - start Codex turn
  - stream `turn/delta`
  - persist completed/failed/cancelled state
- Implement `turn/cancel`.
- Implement `status/get`.

Compatibility:

- Keep legacy `POST /v0/capture` temporarily for debug/migration if needed.
- New chat UI must not be built on `POST /v0/capture`.
- Bridge URL/token should move out of primary user UI.

Tests:

- fake Chrome client over WebSocket
- fake Tauri/service client
- fake Codex client stream through daemon
- reconnect during active turn -> `session/get` recovers state
- two clients subscribed to same session receive consistent turn state
- `context/attach_browser` never persists raw capture
- duplicate `message/send` idempotency key does not create duplicate turn
- error response does not include raw page text/token fixtures

Completion:

- Daemon can run without Tauri window.
- Daemon owns orchestration, not desktop UI.
- Protocol-level tests pass without real Chrome or real Codex network.

## Phase 7: Desktop/Tauri Integration

Purpose:

Let the local desktop app start/supervise the daemon for Phase 1 without turning Tauri into the daemon owner.

Tasks:

- Integrate `sidekick-daemon` startup into `apps/desktop/src-tauri`.
- Keep Tauri command surface minimal:
  - daemon status
  - connection info for extension
  - diagnostics
- Avoid putting session/protocol/Codex logic into Tauri command handlers.
- Keep current bridge status UI only as transitional/debug.
- Update desktop README for new run path if commands change.

Tests:

- `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --no-default-features`
- `cargo check --manifest-path apps/desktop/src-tauri/Cargo.toml`

Completion:

- Tauri can start the daemon.
- Desktop UI does not become the long-term owner of daemon logic.

## Phase 8: Chrome Extension Chat UI

Purpose:

Replace copy-first side panel with Ask Codex chat-first side panel.

Tasks:

- Add WebSocket client to side panel adapter.
- Add `initialize` handshake.
- Add session create/select flow for MVP.
- Add chat transcript rendering.
- Add question input and `Ask Codex` main action.
- On ask:
  - capture current tab context
  - call `context/attach_browser` or use combined daemon flow if implemented
  - call `message/send`
  - render `turn/delta`
  - render completed/failed state
- Move `screen_context_json` / `prompt_text` to debug/export area.
- Keep permission request handling for current tab/site access.
- Keep screenshot metadata optional; invalid metadata should be dropped by Rust boundary.
- Add clear errors for:
  - daemon unavailable
  - unauthorized/pairing failure
  - site permission missing
  - Codex unavailable/not logged in
  - context too large
  - turn already running

Rules:

- TypeScript must not duplicate domain protocol rules beyond adapter behavior.
- TypeScript must not implement safety masking.
- Raw capture should be sent only to daemon attach endpoint.
- Do not persist bridge token in a way broader than needed.

Tests:

- protocol message construction
- reconnect behavior
- turn delta rendering
- permission missing display
- daemon unavailable display
- existing capture_contract tests remain passing
- existing dom_capture tests remain passing

Commands:

```text
npm --prefix apps/extension run typecheck
npm --prefix apps/extension test
node apps/extension/check-manifest.mjs
```

Completion:

- User can ask Codex from side panel without copy/paste.
- Debug prompt/JSON remains available but is not primary UI.

## Phase 9: End-To-End Smoke Path

Purpose:

Verify the complete local path with controlled fakes first, then optional real Codex.

Fake smoke path:

```text
extension-like client
  -> sidekick-daemon WebSocket
  -> session repository
  -> fake codex-client stream
  -> turn/delta notifications
```

Manual real smoke path:

```text
make desktop-dev
Chrome side panel
Ask Codex about current page
real codex app-server over stdio
stream response in side panel
```

Manual real smoke is allowed to be documented as environment-dependent.
Default CI must not require logged-in Codex.

## Verification Commands

Focused checks during implementation:

```text
cargo fmt --all
cargo test -p screen-sidekick-sidekick-protocol
cargo test -p screen-sidekick-session
cargo test -p screen-sidekick-codex-client
cargo test -p screen-sidekick-sidekick-daemon
npm --prefix apps/extension run typecheck
npm --prefix apps/extension test
node apps/extension/check-manifest.mjs
```

Broader checks before completion:

```text
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --no-default-features
cargo check --manifest-path apps/desktop/src-tauri/Cargo.toml
cargo check --manifest-path apps/desktop/ui/Cargo.toml --target wasm32-unknown-unknown
make check
git diff --check
```

Optional/manual:

```text
make codex-schema-check
make desktop-dev
```

If `make desktop-dev` or desktop checks fail due to native Tauri/WebKit/GTK/pkg-config prerequisites, report that as environment-specific only after focused Rust/extension checks pass.

## Phase Final-A: Impact Audit

This phase is mandatory before completion.

Audit from output sinks backward:

- side panel chat transcript
- protocol notifications
- protocol error responses
- session DB rows
- debug/export context
- debug/export prompt
- legacy `/v0/capture` if still present
- logs/diagnostics

Questions:

- Can raw browser text reach Codex without safety review?
- Can raw browser text reach UI debug/export as raw?
- Can raw app-server event JSON reach UI?
- Can error response include token/page secret/raw DOM?
- Can reconnect duplicate a `message/send`?
- Can failed Codex turn disappear from session state?
- Can two UI clients diverge in state?
- Can TypeScript become source of truth for domain protocol?
- Can default build/test require local Codex?

Required counterexample tests:

- token-like page title/button/input/selected text does not appear raw in protocol errors
- duplicate idempotency key returns existing turn
- unknown Codex event surfaces diagnostic, not raw JSON
- app-server crash persists failed turn
- raw capture is not persisted after `context/attach_browser`

If any finding appears, fix it before moving to Final-B.

## Phase Final-B: Comprehensive Refactor Gate

This phase is mandatory before completion.

Review production and test diff for:

- giant daemon modules
- protocol DTOs mixed with storage models without clear mapping
- app-server compatibility logic outside `crates/codex-client`
- SQLite schema logic outside `crates/session`
- capture/safety policy in TypeScript
- generic helpers hiding domain policy
- duplicated error code strings
- duplicated protocol method strings
- flaky sleep-based stream tests
- test fixtures that copy production policy incorrectly

Refactor requirements:

- centralize method/error code definitions in `crates/sidekick-protocol`
- keep app-server event mapping in `crates/codex-client`
- keep WebSocket fanout in `crates/sidekick-daemon`
- keep DB transition rules in `crates/session`
- split tests if a single test file becomes a dumping ground

If behavior risk is found during Final-B, stop refactor and return to Final-A.

Completion report must include:

- owner map of final crates/modules
- tests run
- checks not run and why
- remaining concrete debt by file/module
- whether commit/push was not performed

## Open Decisions For Implementation

These are not blockers, but implementation must choose and record the choice:

- SQLite crate choice and async/sync repository boundary.
- WebSocket crate choice for daemon tests.
- Whether protocol TypeScript types are generated in Phase 1 or manually mirrored with tests.
- Exact app-server Phase 1 method/event subset after schema snapshot is generated.
- Exact daemon profile directory for SQLite DB and local pairing token.
- How much of legacy `/v0/capture` remains after chat UI lands.

Do not resolve these by adding broad fallback wrappers.
Choose the owner, add tests, and document the decision in the implementation report.

## Goal Handoff Prompt

Use this short prompt when handing implementation to `/goal`:

```text
docs/dev/chat-first-browser-mvp-master-plan.md を source of truth として、Phase 1 Chat-First Browser MVP を実装してください。

目的は Chrome side panel から copy/paste なしで Codex に current page context 付き質問を送り、Codex app-server over stdio の streamed response を side panel chat に表示することです。

commit / push / PR は行わないでください。
作業前に AGENTS.md、docs/codex-companion-direction.md、この master plan、現在の source を再読してください。
新規 owner は crates/sidekick-protocol、crates/session、crates/codex-client、crates/sidekick-daemon に分け、Tauri と extension に domain logic を寄せないでください。
通常 build/test が local codex を要求しない構成にしてください。
raw browser capture / raw DOM / raw screenshot / secrets は persist せず、Codex/UI/debug/export には sanitized context または validated metadata だけを渡してください。

各 phase の focused tests を追加し、最後に Phase Final-A impact audit と Phase Final-B comprehensive refactor gate を必ず実施してください。
完了報告では、変更 owner、実行した検証、未実行チェック、残 debt、commit/push していないことを報告してください。
```
