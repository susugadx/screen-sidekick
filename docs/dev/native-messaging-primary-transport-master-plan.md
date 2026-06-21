# Native Messaging Primary Transport Master Plan

Status: implementation master plan / decided

この文書は Codex Goal でまとめて実装するための内部実装仕様書である。
公開 docs ではなく、実装前の設計・壁打ち・handoff の source of truth として使う。

この文書は実装計画であり、実装そのものではない。
commit / push / PR はユーザーの明示指示がある場合だけ行う。

## 0. Purpose

Screen Sidekick の Chrome / Edge extension 体験を、loopback URL / pairing token を手入力する dev bridge から、Native Messaging を primary transport とする構成へ移行する。

狙う最終形は次である。

```text
Chrome extension
  -> Chrome Native Messaging host
  -> Rust Sidekick runtime
  -> Codex app-server
  -> extension chat UI
```

この計画の目的は、desktop window を毎回起動して URL / token をコピーする運用をやめ、extension から直接 local companion に接続して `Ask Codex` できる状態にすることである。

Goal で完了とみなす条件:

- Native Messaging host binary / crate が追加され、Chrome Native Messaging framing を Rust で扱える。
- extension は Native Messaging transport を primary とし、既存 WebSocket transport は dev fallback として残る。
- extension UI から `initialize`、`session/create`、`context/attach_browser`、`message/send`、streaming notifications が Native Messaging 経由で動く。
- 既存の Rust owner、session owner、Codex owner、safety boundary を崩さない。
- default test / CI は local Codex login や installed native host を要求しない。
- docs に setup / dev fallback / known limitations / smoke path が整理される。

## 1. Current State / Implemented Preconditions

現在の chat-first MVP は次の流れで動く。

```text
Chrome extension
  -> loopback WebSocket ws://127.0.0.1:<port>/v0/ws
  -> sidekick-daemon
  -> codex app-server over stdio
  -> extension chat UI
```

実装済みの前提:

- `crates/sidekick-protocol` が JSON-RPC 2.0 shape、method names、notification names、error codes、protocol DTO を持つ。
- `crates/sidekick-daemon` が loopback HTTP / WebSocket daemon、session orchestration、capture attachment、Codex turn lifecycle、streaming fanout を持つ。
- `crates/session` が SQLite session / message / attachment / turn state を持つ。
- `crates/codex-client` が Codex app-server child process と app-server protocol subset を持つ。
- `apps/extension` は Chrome API / DOM capture / side panel UI / WebSocket adapter を持つ。
- `apps/desktop/src-tauri` は Tauri shell と daemon startup、status response を持つ。
- `make ci-check` は local full gate として存在し、GitHub Actions の `CI` workflow で通る。

今回変えない前提:

- Sidekick は executor ではない。
- Browser click / type / submit / automation は実装しない。
- repo editing、MCP execution、Computer Use、approval / sandbox 代替は実装しない。
- safety / redaction / prompt / capture policy を TypeScript に移さない。
- raw DOM、cookies、localStorage、sessionStorage、hidden inputs、password values、tokens、raw screenshots を永続化しない。

## 2. Global Contracts

### Rust-first ownership

- Rust が protocol contract、session state、Codex integration、safety boundary、daemon/runtime orchestration を持つ。
- TypeScript は Chrome API、DOM capture、side panel rendering、transport adapter を持つ。
- TypeScript は safety policy、protocol source of truth、session semantics、Codex state owner にならない。

### Transport independence

- Sidekick protocol は WebSocket 固有ではなく、transport-independent な request / response / notification contract として扱う。
- Native Messaging と WebSocket は同じ protocol methods / notifications / error codes を使う。
- WebSocket transport は dev fallback として残すが、product primary path は Native Messaging とする。
- request id、pending request、notification fanout、timeout policy、message size policy は transport abstraction の責務として明確にする。

### Native Messaging constraints

- Chrome Native Messaging host は stdin / stdout で length-prefixed JSON をやり取りする。
- stdout は protocol output 専用にする。debug / logs は stderr に出す。
- Chrome host manifest の `allowed_origins` は extension ID を明示し、wildcard は使わない。
- extension manifest は `nativeMessaging` permission を持つ。
- host から Chrome への single message size limit を超える payload を出さない。
- streaming は小さい notification chunk として流し、大きい debug / context payload を単発 response に詰め込まない。

### Security boundary

- Native Messaging は local process boundary である。extension 由来 request は external input として扱う。
- host manifest の `allowed_origins` だけを信頼境界にしない。Rust runtime 側でも initialize / capability / protocol validation を行う。
- secret-bearing token / URL / raw page text / raw capture は logs / stderr / protocol error / UI debug に出さない。
- Native Messaging host は任意 extension から使えないように extension ID を固定する。
- dev / unpacked extension ID と distribution extension ID の扱いを明示し、wildcard で逃げない。

### Product boundary

- Native Messaging host は Sidekick runtime への primary transport であり、Codex agent loop の再実装ではない。
- Codex auth / readiness は引き続き local `codex app-server` owner に寄せる。
- desktop app は status / setup / debug / fallback UI に降格できるが、daemon / session / Codex owner を失わない。

## 3. Non-goals

今回やらないこと:

- Chrome Web Store / Edge Add-ons への公開。
- installer / package signing / auto update / CD。
- browser automation、click、type、submit。
- `repo_assisted` mode の有効化。
- `capture_current_context` の daemon-side implementation。
- local desktop screenshot / OCR / global hotkey。
- Codex Desktop direct integration。
- MCP / Computer Use integration。
- Sidekick 独自の login / cloud service。
- WebSocket / loopback daemon の即時削除。

## 4. Source Findings

最新ソースから確認した事実:

- `Cargo.toml` の root workspace members は `crates/screen-context`、`crates/safety-rules`、`crates/safety`、`crates/prompt`、`crates/capture-pipeline`、`crates/sidekick-protocol`、`crates/session`、`crates/codex-client`、`crates/sidekick-daemon` である。
- `apps/desktop/src-tauri` と `apps/desktop/ui` は root workspace から exclude されている。
- `apps/extension/manifest.json` は現在 `nativeMessaging` permission を持たない。
- `apps/extension/src/sidekick_protocol/client.ts` は `SidekickProtocolClient` として WebSocket 接続、pending request map、timeouts、JSON parse、notification dispatch、loopback URL normalization を同じ file で持つ。
- `apps/extension/src/sidekick_protocol/types.ts` は TypeScript 側の protocol mirror を持つ。
- `crates/sidekick-protocol/src/lib.rs` は `JSONRPC_VERSION`、`SIDEKICK_PROTOCOL_VERSION`、method / notification constants、DTO、error codes を持つ。
- `crates/sidekick-daemon/src/protocol.rs` は `websocket_loop` と `handle_ws_text` を持ち、WebSocket text を JSON-RPC request に parse して method handler へ dispatch する。
- `crates/sidekick-daemon/src/lib.rs` は `DaemonRuntime::start()` で token を生成し、loopback listener を bind し、`DaemonStatus` に `url`、`ws_url`、`token` を含める。
- `apps/desktop/src-tauri/src/lib.rs` は Tauri setup で `DaemonRuntime::start()` を呼び、`get_daemon_status` / `get_bridge_status` を expose する。
- `apps/extension/tests/sidekick_protocol.test.mjs` と `apps/extension/tests/support/protocol_harness.mjs` は WebSocket client behavior を test している。
- `crates/sidekick-daemon/tests/daemon.rs` は WebSocket 経由で daemon protocol、session、message send、turn lifecycle、recovery、legacy capture を広く test している。

外部仕様から確認した事実:

- Chrome Native Messaging は host manifest に name / path / type / allowed_origins を持つ。
- Chrome は host process と stdin / stdout で length-prefixed JSON をやり取りする。
- extension 側は `nativeMessaging` permission と `chrome.runtime.connectNative()` または `chrome.runtime.sendNativeMessage()` を使う。
- Native Messaging API は content script から直接使えず、extension page / service worker 経由で使う。

## 5. Responsibility Boundaries

```text
Protocol DTO source of truth:
  crates/sidekick-protocol

Protocol execution / method handlers:
  crates/sidekick-daemon

Codex app-server integration:
  crates/codex-client

Session / message / attachment / turn persistence:
  crates/session

Raw browser capture normalization / safety / prompt:
  crates/capture-pipeline, crates/safety, crates/prompt

Chrome UI / DOM capture / extension rendering:
  apps/extension

Extension transport adapters:
  apps/extension/src/sidekick_protocol/*

Native Messaging host runtime:
  new Rust crate / binary, likely crates/native-host or crates/sidekick-native-host

Desktop status / debug / fallback UI:
  apps/desktop/src-tauri, apps/desktop/ui
```

## 6. Implementation Priority

1. Transport abstraction foundation in extension and daemon protocol handler.
2. Rust Native Messaging framing and host binary.
3. Native Messaging extension client and manifest changes.
4. Runtime bridge from native host to existing daemon / protocol handlers.
5. End-to-end message/send streaming over Native Messaging.
6. Dev fallback / setup docs / manual smoke path.
7. Final-A impact audit.
8. Final-B comprehensive refactor including tests.

## 7. Phase 0: Transport Foundation

### Purpose

WebSocket 固有の client implementation と Sidekick protocol の上位 contract を分け、Native Messaging transport を追加できる土台を作る。

### Non-goals

- Native Messaging host の実装はこの phase では完了させない。
- UI の大幅な見た目変更はしない。
- protocol method / error code の意味を変えない。

### Current source findings

- `SidekickProtocolClient` は WebSocket object、pending request、timeouts、request serialization、notification dispatch、connect/initialize を一体で持つ。
- `SidekickRequestMethod` は TS union として client file 内にある。
- `buildDaemonWebSocketUrl` / `buildDaemonCaptureUrl` は loopback fallback に必要。

### Design contract

- 上位 UI が依存する interface を `SidekickClient` のような transport-independent interface に切る。
- WebSocket implementation は `WebSocketSidekickClient` などへ移す。
- request serialization / parse / pending request / notification dispatch は Native Messaging と WebSocket で重複させない。
- `message/send` timeout の semantics は維持する。side-effect RPC を generic timeout recovery と混ぜない。

### Safety gates

- 既存 extension tests が通るまで Native Messaging implementation へ進まない。
- `message/send` idempotency / timeout / connection lost behavior を変えた場合は、既存 retry-state tests を更新ではなく拡張で固定する。

### Tests

- WebSocket client existing tests remain passing.
- shared client core tests:
  - success response resolves pending request
  - failure response rejects pending request
  - notification dispatch works without pending request
  - invalid message fails pending and emits error notification
  - oversized incoming message fails safely
  - `message/send` timeout reports connection lost once

### Implementation owner candidates

- `apps/extension/src/sidekick_protocol/client.ts`
- `apps/extension/src/sidekick_protocol/types.ts`
- new files under `apps/extension/src/sidekick_protocol/`
- `apps/extension/tests/sidekick_protocol.test.mjs`
- `apps/extension/tests/support/protocol_harness.mjs`

### Open decisions

- Final TypeScript names for shared protocol core and transport adapters.

## 8. Phase 1: Native Messaging Host Crate / Binary

### Purpose

Chrome Native Messaging framing を扱う Rust host binary を追加する。

### Non-goals

- Installer / system-wide manifest install はこの phase では実装しない。
- Chrome Web Store distribution は扱わない。
- Codex / session / capture business logic を host crate に複製しない。

### Current source findings

- root workspace は Rust crates を管理しているが、Native Messaging host crate はまだない。
- `sidekick-daemon` には `DaemonRuntime::start_with_state` と protocol handler があるが、handler は WebSocket loop に閉じている。

### Design contract

- host crate は stdin / stdout framing、host lifecycle、transport boundary だけを持つ。
- protocol method handling は `sidekick-daemon` owner に残す。
- stdout に human-readable logs を出さない。logs は stderr。
- host process が Chrome に起動される前提で、current directory や environment に依存しすぎない。
- unsafe code は使わない。

### Behavior / output format

Native Messaging frame:

```text
u32 native-endian byte length
UTF-8 JSON payload
```

payload は Sidekick JSON-RPC request / response / notification とする。

### Safety gates

- maximum incoming message size を明示する。
- malformed frame / invalid UTF-8 / invalid JSON は protocol error として扱い、panic しない。
- stderr log に request payload、token、raw context、page text を出さない。
- host origin argument は記録する場合も raw extension URL を debug log に出さない。

### Tests

- frame encoder / decoder unit tests:
  - valid JSON frame roundtrip
  - partial frame read
  - invalid length / oversized frame rejection
  - invalid UTF-8 rejection
  - stdout writer writes exact frame bytes
- host protocol smoke with in-memory stdin/stdout:
  - initialize request returns response
  - notification can be emitted as frame
  - malformed frame returns safe error or exits with safe diagnostic

### Implementation owner candidates

- new `crates/sidekick-native-host/Cargo.toml`
- new `crates/sidekick-native-host/src/lib.rs`
- new `crates/sidekick-native-host/src/main.rs`
- `Cargo.toml` workspace members / dependencies
- `Makefile` focused checks if needed

### Open decisions

- Crate name: `screen-sidekick-native-host`.
- Binary name: `screen-sidekick-native-host`.

## 9. Phase 2: Daemon Protocol Core Reuse

### Purpose

`sidekick-daemon` の method handling を WebSocket loop から切り出し、WebSocket と Native Messaging host が同じ Rust protocol execution path を使えるようにする。

### Non-goals

- Session schema を変更しない。
- Codex client API を変えない。
- WebSocket behavior を削除しない。

### Current source findings

- `websocket_loop` は WebSocket receive、event broadcast receive、shutdown handling、subscription visibility、request handling を 1 function family で持つ。
- `handle_ws_text` は JSON parse、initialize gating、method dispatch、response serialization を行う。
- event broadcast は `broadcast::Sender<JsonRpcNotification>` で流れている。

### Design contract

- method dispatch / initialize gating / auth validation は reusable protocol session owner に切り出す。
- WebSocket-specific read/write/ping/close は WebSocket loop owner に残す。
- Native Messaging-specific frame read/write は native host owner に残す。
- subscribed session visibility は transport connection state として扱う。
- daemon state / store / codex / events source of truth は `sidekick-daemon` に残す。

### Safety gates

- unauthorized request before initialize remains rejected.
- bad token remains rejected with safe error.
- notifications are sent only after initialize and subscription visibility checks.
- lagged event handling must fail closed or force recovery as current WebSocket behavior does.

### Tests

- Existing `crates/sidekick-daemon/tests/daemon.rs` WebSocket tests remain passing.
- New protocol-core tests, if extracted:
  - initialize required before other methods
  - bad token rejected
  - session subscribe controls notification visibility
  - status/get and initialize readiness paths match WebSocket behavior
- Native host integration tests can use same fake codex client and session store.

### Implementation owner candidates

- `crates/sidekick-daemon/src/protocol.rs`
- `crates/sidekick-daemon/src/lib.rs`
- `crates/sidekick-daemon/tests/daemon.rs`
- new test support module if current daemon test file becomes too large

### Open decisions

- Whether protocol core lives in `protocol.rs` submodule or a new `connection.rs` / `transport.rs` module.

## 10. Phase 3: Extension Native Messaging Transport

### Purpose

extension から `chrome.runtime.connectNative()` を使い、Native Messaging transport で Sidekick protocol request / notification を流せるようにする。

### Non-goals

- content script から直接 Native Messaging を使わない。
- native host install automation はここでは実装しない。
- WebSocket fallback を消さない。

### Current source findings

- `manifest.json` permissions は `activeTab`、`scripting`、`sidePanel`、`storage`、`tabs` で、`nativeMessaging` は未追加。
- side panel entrypoint は `side_panel.html` + `dist/side_panel.js`。
- `background.ts` が service worker として存在する。

### Design contract

- Native Messaging API call は extension page / service worker owner に置く。
- Side panel UI は `SidekickClient` interface に依存し、transport details を知らない。
- Native port disconnect は `connection_lost` notification として上位へ出す。
- native host unavailable は setup-required / install-required error として UI に出す。
- native transport が primary。WebSocket settings form は dev fallback / advanced section に下げる。

### Behavior / output format

Primary connection flow:

```text
side panel opens
  -> NativeMessagingSidekickClient.connect()
  -> initialize
  -> status / readiness visible
  -> Ask Codex enabled if codex_readiness.available
```

Fallback flow:

```text
native host unavailable
  -> show actionable setup state
  -> optional dev fallback WebSocket settings
```

### Safety gates

- Native host name is constant and validated.
- no raw token is shown in normal UI for native path.
- no native host error includes raw request payload.
- extension ID / host unavailable / forbidden host errors are user-actionable and non-secret.
- service worker / side panel lifecycle does not duplicate `message/send`.

### Tests

- Native Messaging fake port tests:
  - connect sends initialize
  - initialize unavailable maps to install/setup state
  - success/failure/notification messages match WebSocket behavior
  - port disconnect reports connection_lost once
  - message/send timeout handling stays side-effect safe
- Side panel tests:
  - default path attempts native transport
  - fallback path can still use saved WebSocket settings
  - Ask disabled until transport initialized and Codex ready
  - host unavailable UI does not expose token/raw request

### Implementation owner candidates

- `apps/extension/manifest.json`
- `apps/extension/src/background.ts`
- `apps/extension/src/side_panel.ts`
- `apps/extension/src/side_panel_protocol_connection.ts`
- `apps/extension/src/sidekick_protocol/client.ts`
- new `apps/extension/src/sidekick_protocol/native_client.ts`
- new / updated test harness under `apps/extension/tests/support/`

### Open decisions

- Side panel calls `chrome.runtime.connectNative()` directly. Content scripts do not use Native Messaging.
- Native host name is `com.screen_sidekick.host`.

## 11. Phase 4: Host Manifest / Dev Setup

### Purpose

Native Messaging host manifest を dev 環境で登録・検証できるようにする。

### Non-goals

- Production installer / package signing / auto update は実装しない。
- Chrome Web Store extension ID をこの phase で固定しない場合は、Open decision として残す。

### Design contract

- host manifest は checked-in template と generated local file を分ける。
- dev install command は user-level manifest location を使う。
- generated local manifest は absolute path を持つ。
- unpacked extension ID の扱いを docs に明記する。
- `allowed_origins` は wildcard 禁止。

### Behavior / output format

Template example:

```json
{
  "name": "com.screen_sidekick.host",
  "description": "Screen Sidekick Native Messaging Host",
  "path": "/absolute/path/to/screen-sidekick-native-host",
  "type": "stdio",
  "allowed_origins": ["chrome-extension://<extension-id>/"]
}
```

### Safety gates

- install script must not overwrite unrelated native messaging host manifests.
- install script must print target path before writing.
- uninstall script removes only the matching manifest path.
- docs must explain extension ID mismatch failure.

### Tests

- manifest template JSON parse.
- generated manifest contains valid name / type / absolute path / allowed_origins.
- script dry-run output if scripts are added.

### Implementation owner candidates

- new `crates/sidekick-native-host/manifest/`
- new scripts under `scripts/` or `crates/sidekick-native-host/scripts/`
- `docs/development.md`
- `apps/extension/README.md`

### Open decisions

- Exact install helper location is `scripts/native-host-dev.mjs`.
- Dev manifest generation is script-owned; Makefile may expose convenience targets later, but default CI must not require installed native hosts.

## 12. Phase 5: End-to-End Native Ask Path

### Purpose

Native Messaging primary pathで、実際に browser context attach、message send、Codex stream 表示まで通す。

### Non-goals

- Browser action execution はしない。
- Debug capture legacy endpoint を削除しない。

### Design contract

- Raw browser capture still flows only through `context/attach_browser`.
- `message/send` continues to reference attachment IDs and idempotency key.
- Codex stream notifications remain protocol notifications.
- UI transcript source remains session / turn notifications, not raw Codex events.

### Safety gates

- raw browser text cannot bypass capture pipeline / safety review.
- duplicate Ask after reconnect cannot duplicate a turn if idempotency key is reused.
- native host restart / disconnect surfaces recovery state rather than silent transcript loss.
- page-originated secret fixtures do not appear raw in protocol errors or host stderr.

### Tests

- Fake native host smoke:
  - capture context attaches
  - message/send creates user message and starts turn
  - turn/delta streams to transcript
  - turn/completed finalizes transcript
- Rust native host integration:
  - initialize + session/create + attach + message/send with fake Codex
  - app-server failure persists failed turn
  - notification ordering matches existing WebSocket expectations
- Existing manual smoke path updated:
  - build extension
  - install dev native host manifest
  - load unpacked extension
  - open side panel
  - Ask without opening desktop window

### Implementation owner candidates

- `apps/extension/src/side_panel.ts`
- `apps/extension/src/side_panel_protocol_connection.ts`
- `apps/extension/tests/side_panel.test.mjs`
- `crates/sidekick-daemon/tests/daemon.rs`
- new `crates/sidekick-native-host/tests/`

### Open decisions

- Whether desktop app should also connect to daemon through same protocol core in this pass.

## 13. Mode / Policy / Defaults

Default product mode:

- Native Messaging primary transport.

Dev fallback mode:

- WebSocket / loopback daemon settings remain available for development and emergency recovery.
- Legacy `/v0/capture` remains debug / migration path unless a later cleanup plan removes it.

Connection priority:

```text
1. Native Messaging transport
2. WebSocket fallback if explicitly configured / enabled
3. Setup-required UI
```

Timeout policy:

- initialize: long enough for host and Codex readiness startup.
- control requests: bounded short timeout.
- `message/send`: side-effect aware timeout; do not treat as safe retry without recovery/idempotency checks.

Compatibility:

- Existing saved daemon settings should not break WebSocket fallback.
- Existing session DB remains compatible.
- Existing protocol version remains `sidekick.protocol.v0` unless wire shape changes require a version bump.

## 14. Config / Docs / Generated Metadata Surface

Files likely needing updates:

- `apps/extension/manifest.json`
- `apps/extension/README.md`
- `apps/desktop/README.md`
- `README.md`
- `docs/development.md`
- `docs/dev/chat-first-browser-mvp-master-plan.md` if it remains a source document
- new native host manifest template docs
- `Makefile` if adding focused native host checks or dev install helpers
- `.github/workflows/ci.yml` only if CI needs new build/test command

Docs must explain:

- Native Messaging primary path.
- WebSocket fallback path.
- dev host manifest install / uninstall.
- extension ID mismatch failure.
- why desktop window is not required for normal extension Ask path.
- what still requires manual setup because installer/CD is out of scope.

## 15. Report / Status / Observability

UI / status should distinguish:

- native host not installed / not found
- native host forbidden for this extension ID
- native host protocol error
- daemon/runtime startup failed
- Codex CLI not found
- Codex not logged in
- unsupported Codex version
- capture permission missing
- browser capture failed
- side-effect `message/send` connection loss / recovery required

Diagnostics must not include:

- raw page text
- raw capture JSON
- pairing token
- local secret-like URL query/fragment
- Codex raw app-server event JSON

## 16. Tests

Required test groups:

- Rust Native Messaging framing unit tests.
- Rust host integration tests with fake input/output.
- daemon protocol core tests preserving WebSocket behavior.
- extension shared protocol client tests.
- extension native transport fake-port tests.
- side panel caller tests for primary native path and WebSocket fallback.
- manifest/template validation tests.
- leak tests for token/raw page text in protocol errors and host stderr.
- manual smoke with real Chrome Native Messaging host.

Existing checks to keep:

```sh
npm --prefix apps/extension run typecheck
npm --prefix apps/extension test
node apps/extension/check-manifest.mjs
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --no-default-features
cargo check --manifest-path apps/desktop/src-tauri/Cargo.toml
cargo check --manifest-path apps/desktop/ui/Cargo.toml --target wasm32-unknown-unknown
make ci-check
git diff --check
```

Optional / environment-dependent:

```sh
make codex-schema-check
make desktop-dev
```

## 17. Verification Commands

Focused during implementation:

```sh
cargo test -p screen-sidekick-sidekick-protocol
cargo test -p screen-sidekick-sidekick-daemon
cargo test -p screen-sidekick-native-host
npm --prefix apps/extension run typecheck
npm --prefix apps/extension test
```

Before completion:

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --no-default-features
cargo check --manifest-path apps/desktop/src-tauri/Cargo.toml
cargo check --manifest-path apps/desktop/ui/Cargo.toml --target wasm32-unknown-unknown
npm --prefix apps/extension test
node apps/extension/check-manifest.mjs
make ci-check
git diff --check
```

Manual smoke:

```text
1. Build native host binary.
2. Generate / install dev Native Messaging host manifest for the loaded extension ID.
3. Build extension.
4. Load unpacked apps/extension in Chrome.
5. Open side panel.
6. Confirm native host connected without opening desktop window.
7. Ask Codex about current page.
8. Confirm turn/delta stream and final assistant message.
9. Stop host / break manifest and confirm setup-required UI.
10. Optionally enable WebSocket fallback and confirm old dev path still works.
```

## 18. Phase Final-A: Impact Audit / Review-hole Sweep

This phase is mandatory before completion.

Audit from output sinks backward:

- side panel transcript
- side panel status/errors
- Native Messaging stdout frames
- Native Messaging stderr logs
- WebSocket fallback messages
- daemon protocol responses
- daemon notifications
- session DB rows
- Codex app-server request / event mapping
- debug/export context
- legacy `/v0/capture`
- docs / dev install scripts

Questions:

- Can raw browser text reach Codex without safety review?
- Can raw browser text reach UI / stderr / protocol error as raw?
- Can pairing token appear in normal UI, logs, stderr, protocol error, or test snapshot?
- Can an unallowed extension connect to the host?
- Can dev unpacked extension ID accidentally become a wildcard trust policy?
- Can native host disconnect duplicate `message/send`?
- Can `message/send` timeout be retried without idempotency/recovery state?
- Can Codex raw app-server events reach extension UI?
- Can host framing parse failure panic?
- Can WebSocket fallback drift from Native Messaging primary behavior?
- Can default CI require installed native host or logged-in Codex?

Required counterexample tests:

- bad extension / forbidden origin host manifest path is documented and fails safely.
- token-like page title/button/input/selected text does not appear raw in protocol errors.
- host stderr does not contain raw request payload for malformed request tests.
- duplicate idempotency key returns existing turn / terminal replay behavior remains correct.
- unknown Codex event surfaces diagnostic, not raw JSON.
- app-server crash persists failed turn.
- raw capture is not persisted after `context/attach_browser`.

If any finding appears, fix it before moving to Final-B.

## 19. Phase Final-B: Mandatory Comprehensive Refactor Including Tests

This phase is mandatory, not optional.

Review production and test diff for:

- WebSocket-specific names leaking into transport-independent code.
- Native Messaging framing mixed with Sidekick method semantics.
- TypeScript transport adapter owning protocol policy.
- daemon protocol core mixed with transport read/write loops.
- duplicated request timeout / pending request logic.
- duplicated protocol method strings.
- oversized test harness files accumulating native and websocket fakes in one owner.
- generic helper hiding sensitive logging / redaction policy.
- docs saying Phase 0-B copy-first when product path is Native Messaging chat-first.

MUST refactor if:

- `apps/extension/src/sidekick_protocol/client.ts` remains a large mixed WebSocket/native/client-core file.
- daemon `protocol.rs` grows with Native Messaging read/write details.
- native host crate owns session / Codex / capture business logic.
- tests require installed Chrome native host for default CI.
- same test helper handles DOM capture, WebSocket fake, Native Messaging fake, and protocol assertions.

Exit checklist:

- production diff and test diff inventory completed.
- transport-independent core, WebSocket adapter, Native adapter, host framing, daemon method handling have clear owners.
- focused tests pass.
- broader `make ci-check` passes or environment-specific blocker is documented.
- remaining debt is reported by file/module, not as generic future work.

## 20. Implementer Freedom

The implementer may choose:

- final crate / module / type names.
- exact split between service worker and side panel for Native Messaging connection.
- exact helper names for frame reader/writer.
- exact location for dev install scripts.
- whether host manifest generation is a script or Makefile target.

The implementer must not change:

- Rust-first safety / protocol / session / Codex ownership.
- Sidekick non-executor boundary.
- no wildcard `allowed_origins`.
- no raw capture / token / page text in logs/errors.
- `message/send` side-effect timeout and idempotency recovery contract.
- default CI must not require installed native host or logged-in Codex.

## 21. Decisions / Remaining Open Items

- Cross-platform target is fixed: Linux, macOS, and Windows user-level install paths for Chrome, Chrome for Testing, Chromium, and Edge. System-wide writes are out of scope.
- Runtime mode is fixed as Hybrid: Native host starts an in-process Sidekick runtime by default. It connects to an existing daemon sidecar only when both `SCREEN_SIDEKICK_DAEMON_WS_URL` and `SCREEN_SIDEKICK_DAEMON_TOKEN` are set. It does not scan ports or discover token files.
- Distribution extension ID is not yet fixed. Dev unpacked ID handling must be documented separately from release distribution.
- Native host install / uninstall script location is `scripts/native-host-dev.mjs`.
- Whether desktop app will later become tray/status-only is outside this plan.
- Whether to bump `SIDEKICK_PROTOCOL_VERSION` depends on whether transport-independent wire semantics change.

## 22. Goal Handoff Policy

Use this file as the source of truth for implementation.

If this plan and latest source structure conflict:

- preserve the global contracts and safety gates.
- adapt names / file layout to existing owner boundaries.
- do not weaken the non-executor boundary.
- do not move safety policy to TypeScript.
- do not make Native Messaging a full Codex harness rewrite.
- report any scope that must be deferred.

After context compaction / resume, re-read this file before continuing.

Do not commit or push unless explicitly requested.

## 23. Goal Prompt

```text
/goal Implement docs/dev/native-messaging-primary-transport-master-plan.md end to end.

Use docs/dev/native-messaging-primary-transport-master-plan.md as the source of truth. Start with Phase 0 transport foundation, then implement the planned Native Messaging host, extension native transport, daemon protocol reuse, end-to-end Ask path, docs, and verification.

Final-A and Final-B are mandatory. After tests pass, run impact audit and mandatory post-implementation refactor; if tests, fixtures, fakes, table tests, or assertion helpers changed, also run test-boundary-refactor. Same-file repeated findings, large files, or generic helpers mixing semantic roles trigger a file/test split audit before strict review.

If the plan and latest source structure conflict, preserve the safety contracts, Rust-first ownership, non-executor boundary, and message/send idempotency/recovery contract. Re-read the plan after resume or context compaction. Do not commit or push unless explicitly requested.
```

## 24. References

- Chrome Native Messaging: `https://developer.chrome.com/docs/extensions/develop/concepts/native-messaging`
- Edge Native Messaging: `https://learn.microsoft.com/en-us/microsoft-edge/extensions/developer-guide/native-messaging`
- Existing chat-first plan: `docs/dev/chat-first-browser-mvp-master-plan.md`
- Development checks: `docs/development.md`
