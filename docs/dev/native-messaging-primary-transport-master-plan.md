# Native Messaging Primary Transport Master Plan

Status: implementation status ledger / remaining plan
Last refreshed for Windows Chrome -> WSL Sidekick hybrid support.

この文書は Screen Sidekick の Native Messaging primary transport の内部計画書である。
初回実装前の handoff plan ではなく、current tree の実装済み状態を固定する ledger と、残タスクの plan として扱う。

commit / push / PR / `@codex review` は、ユーザーの明示指示がある場合だけ行う。

## 0. Purpose

Screen Sidekick の Chrome / Edge extension 体験を、loopback URL / pairing token を手入力する dev bridge から、Native Messaging を primary transport とする構成へ移行する。

現在の primary path は次である。

```text
Linux / macOS Chrome / Edge extension side panel
  -> Chrome Native Messaging host
  -> in-process Rust Sidekick runtime
  -> Codex app-server
  -> extension chat UI

Windows Chrome / Edge extension side panel
  -> Windows Chrome Native Messaging host
  -> wsl.exe
  -> WSL screen-sidekick-daemon --stdio-status
  -> daemon WebSocket ws://127.0.0.1:<port>/v0/ws
  -> Codex app-server in WSL
  -> extension chat UI
```

Linux / macOS の通常 Native Messaging host は in-process Sidekick runtime を起動する。
Windows の通常 Native Messaging host は WSL auto-start config を読み、WSL 側 daemon を起動する。
`SCREEN_SIDEKICK_DAEMON_WS_URL` と `SCREEN_SIDEKICK_DAEMON_TOKEN` が両方設定された場合だけ、既存 daemon sidecar へ WebSocket 接続する。

既存 WebSocket / loopback daemon path は、dev fallback と emergency recovery 用に残す。

## 1. Current State / Implemented Preconditions

current tree で実装済みの前提:

- Root `Cargo.toml` は `crates/sidekick-native-host` を workspace member に含む。
- `apps/extension/manifest.json` は `nativeMessaging` permission を持つ。
- `crates/sidekick-native-host` は Chrome Native Messaging framing、host lifecycle、Linux / macOS in-process runtime、optional sidecar bridge、Windows WSL auto-start config を持つ。
- `crates/sidekick-native-host/manifest/com.screen_sidekick.host.json.template` は Native Messaging host manifest template を持つ。
- `crates/sidekick-daemon` は `screen-sidekick-daemon --stdio-status` binary を持ち、status JSON を stdout に 1 行出して daemon を維持できる。
- `scripts/native-host-dev.mjs` は user-level dev manifest の generate / install / uninstall / locations と Windows WSL auto-start config generation を持つ。
- `crates/sidekick-daemon/src/protocol/connection.rs` は `ProtocolConnection` として WebSocket / Native host 共通の request dispatch、initialize gating、notification visibility、native-host / sidecar-owned active turn cleanup policy を持つ。
- `crates/sidekick-daemon/src/protocol.rs` は WebSocket read/write loop に薄く戻り、method handling は reusable protocol connection に寄っている。
- `apps/extension/src/sidekick_protocol/core.ts` は pending request、serialization、parse、notification dispatch、timeout policy の shared client core を持つ。
- `apps/extension/src/sidekick_protocol/native_client.ts` は `chrome.runtime.connectNative()` adapter と `com.screen_sidekick.host` の native settings を持つ。
- `apps/extension/src/sidekick_protocol/websocket_client.ts` は explicit WebSocket fallback adapter を持つ。
- `apps/extension/src/side_panel_protocol_connection.ts` は Native Messaging を preferred path とし、fallback daemon settings がある場合だけ WebSocket に fallback する。
- `README.md`、`apps/extension/README.md`、`apps/desktop/README.md`、`docs/development.md` は Native Messaging primary path と WebSocket fallback を説明している。

今回変えない前提:

- Sidekick は executor ではない。
- Browser click / type / submit / automation は実装しない。
- repo editing、MCP execution、Computer Use、approval / sandbox 代替は実装しない。
- safety / redaction / prompt / capture policy を TypeScript に移さない。
- raw DOM、cookies、localStorage、sessionStorage、hidden inputs、password values、tokens、raw screenshots を永続化しない。
- default CI は installed native host、real Chrome / Edge、local Codex login を要求しない。

## 2. Implemented Source Findings

最新ソースから確認した事実:

- `crates/sidekick-protocol/src/lib.rs` が JSON-RPC 2.0 shape、method names、notification names、error codes、protocol DTO の source of truth である。
- `crates/sidekick-daemon/src/protocol/handlers/*` が status / session / context / turn handler を持ち、transport read/write から分離されている。
- `ProtocolConnectionAuth::PairingToken` は WebSocket path、`ProtocolConnectionAuth::NativeHost` は Native Messaging path の initialize contract を表す。
- Native host path では pairing token を extension に要求せず、extension id / origin は initialize params と caller origin として扱う。
- `run_from_environment()` は sidecar env が両方ある場合だけ sidecar mode に入り、Windows では config-driven WSL auto-start、Linux / macOS では default in-process runtime を起動する。
- Windows で sidecar env も WSL config もない場合、WSL config が不正な場合、WSL startup/status が失敗した場合、または reported WSL daemon WebSocket が初回 daemon response 前に接続 / protocol failure になった場合、native host は最初の Native Messaging request に setup-required error を返し、Windows in-process runtime や保存済み WebSocket 設定へ fallback しない。
- WSL daemon status parser は `ws://127.0.0.1:<port>/v0/ws` だけを受け入れ、query / fragment / localhost / non-ws を拒否する。
- `read_native_message()` / `write_native_message()` は native-endian `u32` length-prefixed UTF-8 JSON frame を扱い、incoming / outgoing size limit を持つ。
- `NativeHostError` / frame error response は raw payload、pairing token、page text を error message に含めない方針で実装されている。
- Extension shared core は `initialize`、`session/create`、`session/subscribe`、`session/get`、`context/attach_browser`、`message/send` を transport-independent に送る。
- Native adapter は `native://com.screen_sidekick.host` / `native-messaging` を internal settings sentinel として使い、通常 UI に pairing token を要求しない。
- WebSocket fallback は `http://127.0.0.1:<port>` または `ws://127.0.0.1:<port>` から `/v0/ws` / `/v0/capture` を組み立てる。Browser direct/fallback WebSocket の transient disconnect は active turn を fail せず、reconnect 後の `session/get` で active turn を復旧する。
- Dev manifest script は Chrome、Chrome for Testing、Chromium、Edge の user-level install locations を扱い、Windows は HKCU registry と `%APPDATA%\Screen Sidekick\native-host-config.json`、Linux / macOS は browser-specific `NativeMessagingHosts` path を使う。

## 3. Global Contracts

### Rust-first ownership

- Rust が protocol contract、session state、Codex integration、safety boundary、daemon/runtime orchestration を持つ。
- TypeScript は Chrome API、DOM capture、side panel rendering、transport adapter を持つ。
- TypeScript は safety policy、protocol source of truth、session semantics、Codex state owner にならない。

### Transport independence

- Sidekick protocol は WebSocket 固有ではなく、transport-independent な request / response / notification contract として扱う。
- Native Messaging と WebSocket は同じ protocol methods / notifications / error codes を使う。
- WebSocket transport は dev fallback として残すが、product primary path は Native Messaging とする。
- request id、pending request、notification fanout、timeout policy、message size policy は shared client core / protocol connection の責務として維持する。

### Native Messaging constraints

- Chrome Native Messaging host は stdin / stdout で length-prefixed JSON をやり取りする。
- stdout は protocol output 専用にする。debug / logs は stderr に出す。
- `screen-sidekick-daemon --stdio-status` の stdout は起動 status JSON 1 行だけにし、logs は stderr に出す。
- WSL auto-start の `--stdio-status` daemon は Native Messaging port ごとに起動されるため、startup 時に shared store の active turns を global recovery しない。stale turn recovery は通常 daemon / singleton owner の責務に残す。ただし、native-host sidecar-owned marker 付き WebSocket connection が開始した active turn は、その sidecar WebSocket disconnect で failed にする。
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
- Windows WSL config は native host が schema / mode / distro / Linux path / daemon binary を validate する。
- `wsl.exe` は shell 文字列ではなく argv で起動する。

### Product boundary

- Native Messaging host は Sidekick runtime への primary transport であり、Codex agent loop の再実装ではない。
- Codex auth / readiness は引き続き local `codex app-server` owner に寄せる。
- desktop app は status / setup / debug / fallback UI に降格できるが、daemon / session / Codex owner を失わない。

## 4. Non-goals

今回の ledger 更新と remaining tranche でやらないこと:

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
- desktop tray/status-only 化。

## 5. Responsibility Boundaries

```text
Protocol DTO source of truth:
  crates/sidekick-protocol

Protocol execution / method handlers:
  crates/sidekick-daemon/src/protocol/connection.rs
  crates/sidekick-daemon/src/protocol/handlers/*

Codex app-server integration:
  crates/codex-client

Session / message / attachment / turn persistence:
  crates/session

Raw browser capture normalization / safety / prompt:
  crates/capture-pipeline, crates/safety, crates/prompt

Chrome UI / DOM capture / extension rendering:
  apps/extension

Extension transport adapters:
  apps/extension/src/sidekick_protocol/core.ts
  apps/extension/src/sidekick_protocol/native_client.ts
  apps/extension/src/sidekick_protocol/websocket_client.ts

Native Messaging host runtime:
  crates/sidekick-native-host

WSL daemon status startup:
  crates/sidekick-daemon

Dev host manifest generation:
  scripts/native-host-dev.mjs

Desktop status / debug / fallback UI:
  apps/desktop/src-tauri, apps/desktop/ui
```

## 6. Implementation Status Ledger

| Area | Status | Current owner / evidence |
| --- | --- | --- |
| Phase 0: extension transport core split | Done | `apps/extension/src/sidekick_protocol/core.ts`, `native_client.ts`, `websocket_client.ts` |
| Phase 1: Rust native host crate / framing | Done | `crates/sidekick-native-host`, `read_native_message()`, `write_native_message()` |
| Phase 2: daemon protocol core reuse | Done | `crates/sidekick-daemon/src/protocol/connection.rs`, `handlers/*` |
| Phase 3: extension native transport | Done | `manifest.json` `nativeMessaging`, `NativeMessagingSidekickClient`, preferred native connection |
| Phase 4: dev manifest / docs | Done | `scripts/native-host-dev.mjs`, manifest template, README / development docs |
| Phase 5: native Ask path + fallback behavior + port-close cleanup | Done | native default Ask tests, native host streaming tests, owned active turn cleanup |
| Windows Chrome -> WSL hybrid runtime | Done | native host config parser / WSL argv builder / daemon `--stdio-status` |
| Final-A automated impact audit | Done for implemented code path | error sanitization tests, port-close tests, fallback tests, prior implementation verification |
| Final-B automated refactor pass | Done for implemented code path | client core split, daemon protocol split, focused test/harness updates |
| Windows Chrome / Edge manual smoke | Pending | requires Windows browser install state, Windows host exe, and user-level host manifest/config |
| Release / distribution readiness | Pending | extension ID, installer, signing, release manifest, CD remain undecided |

The previous implementation record treats the `bc024c9` code path and the immediately preceding `make ci-check` pass as the older automated baseline.
The Windows WSL hybrid tranche requires focused Rust, script, extension, and diff checks before reporting complete.

## 7. Implemented Test Surface

Automated coverage present in current tree:

- Native host framing:
  - valid UTF-8 JSON frame roundtrip
  - exact length-prefixed bytes
  - invalid UTF-8 rejection
  - oversized length rejection
  - partial length rejection
- Native host integration:
  - in-process host initializes without pairing token
  - in-process host streams `message/send` notifications
  - in-process host fails owned active turns when the native port closes
  - sidecar host disconnect fails the daemon-owned active turn through sidecar-owned WebSocket disconnect cleanup
  - browser direct/fallback WebSocket disconnect preserves a running active turn for reconnect + `session/get` recovery
  - default in-process host does not recover unrelated shared active turns
  - malformed frame errors do not include payload / token-like values
  - sidecar URL validation accepts only explicit loopback `/v0/ws`
  - sidecar initialize token injection is limited to initialize messages
- Windows WSL hybrid:
  - config parser accepts valid `wsl_auto` config and rejects missing / invalid distro / invalid Linux path fields
  - non-Windows hosts ignore `SCREEN_SIDEKICK_NATIVE_HOST_CONFIG` and keep the in-process runtime path
  - Windows runtime selection prefers sidecar env before WSL config
  - Windows runtime selection rejects missing config without starting in-process runtime
  - Windows WSL startup/status and reported WebSocket pre-initialize failures return structured setup-required before fallback
  - WSL command builder uses argv and does not shell-concatenate config values
  - WSL daemon status parser accepts only `ws://127.0.0.1:<port>/v0/ws` without query / fragment
- Daemon stdio status:
  - `--stdio-status` writes one `DaemonStatus` JSON line and stays alive until stdin closes
  - `--stdio-status` does not recover existing active turns owned by another live daemon
  - status stdout has no logs after the required status line
- Extension protocol:
  - native connect sends initialize without pairing token
  - native host unavailable maps to setup error
  - native disconnect reports sanitized `connection_lost` once
  - WebSocket behavior remains covered by existing protocol tests
- Side panel caller behavior:
  - default Ask path uses Native Messaging without daemon settings
  - partial fallback daemon URL / token input does not block Native Messaging
  - existing transcript / turn lifecycle tests remain owned by side panel harness

Default checks that should remain available:

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

## 8. Remaining Work / Next Tranche

### A. Windows Chrome -> WSL Manual Smoke

Manual smoke is still pending and must not be reported as complete until run against Windows Chrome / Edge.

Smoke path:

```text
1. Build or provide a Windows native host exe:
   screen-sidekick-native-host.exe

2. Build the WSL daemon binary:
   cargo build -p screen-sidekick-sidekick-daemon --bin screen-sidekick-daemon

3. Build extension:
   npm --prefix apps/extension run build

4. Load unpacked extension from apps/extension in Windows Chrome or Edge.

5. Copy the unpacked extension ID from chrome://extensions or edge://extensions.

6. Install dev Native Messaging host manifest and WSL config for that exact ID:
   node scripts/native-host-dev.mjs install --browser chrome --extension-id <id> --host-path <Windows exe> --wsl-distro <distro> --wsl-workdir <repo> --wsl-daemon-binary <daemon>

7. Open the side panel without opening the desktop window or WSL Chrome.

8. Ask Codex about the current page and confirm:
   - native host connects
   - WSL daemon starts and reports status
   - initialize succeeds without pairing token input
   - session/create and context/attach_browser work
   - message/send starts a turn
   - turn/delta and terminal notification update the transcript

9. Break or uninstall the manifest/config and confirm:
   - setup-required / host-not-installed state is user-actionable and does not
     silently fall back to saved WebSocket settings
   - raw request payload, token-like page text, and local secrets are not shown

10. Optionally launch WebSocket fallback and confirm:
   - explicit loopback daemon settings still connect
   - fallback does not become the default product path
```

Smoke report should record:

- browser and version
- extension ID source
- native host binary path
- WSL distro / workdir / daemon binary path
- manifest target path
- config target path
- whether desktop window was opened
- Ask result
- broken-manifest behavior
- WebSocket fallback result, if tested

WSL Chrome smoke remains useful as an implementation/debug aid, but it is no longer the primary end-to-end target for this tranche.

### B. Release / Distribution Readiness

Release work remains undecided and should be handled as a separate tranche:

- Fix distribution extension ID for Chrome Web Store / Edge Add-ons.
- Decide release manifest strategy for dev ID vs store ID.
- Build installer / package flow for per-user native host manifest registration.
- Handle signing / notarization / Windows installer behavior.
- Decide CD owner and CI secrets boundary.
- Add release smoke matrix for Chrome, Chrome for Testing, Chromium, Edge, Linux, macOS, Windows.
- Decide support wording for host manifest mismatch and upgrade / uninstall.

### C. Desktop Tray / Status-only Direction

Desktop tray/status-only work is outside this plan.
The current Native Messaging primary path allows the desktop window to become status / debug / fallback UI later, but that product decision should live in a separate plan.

### D. PR / Review / Publish Closeout

This plan does not include commit / push / PR / `@codex review`.
If the user explicitly asks to publish, first check for an existing PR, then push or reuse a branch, open/update PR, and request review according to the repository Git policy.

## 9. Mode / Policy / Defaults

Default product mode:

- Native Messaging primary transport.
- Runtime priority:
  1. explicit sidecar env if both `SCREEN_SIDEKICK_DAEMON_WS_URL` and `SCREEN_SIDEKICK_DAEMON_TOKEN` are set
  2. Windows WSL auto-start config
  3. Linux / macOS in-process runtime
  4. Windows setup-required error if no valid config is present

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
- Native Messaging protocol framing and Sidekick JSON-RPC method shapes remain compatible.
- Existing protocol version remains `sidekick.protocol.v0` unless wire shape changes require a version bump.

## 10. Docs / Config Surface

Current docs/config surface:

- `apps/extension/manifest.json`
- `apps/extension/README.md`
- `apps/desktop/README.md`
- `README.md`
- `docs/development.md`
- `crates/sidekick-native-host/manifest/com.screen_sidekick.host.json.template`
- `scripts/native-host-dev.mjs`
- `%APPDATA%\Screen Sidekick\native-host-config.json`
- `.github/workflows/ci.yml`

Docs should continue to explain:

- Native Messaging primary path.
- Windows Chrome + WSL daemon path.
- WebSocket fallback path.
- dev host manifest install / uninstall.
- WSL auto-start config generation / override path.
- extension ID mismatch failure.
- why desktop window is not required for normal extension Ask path.
- what still requires manual setup because installer/CD is out of scope.

## 11. Report / Status / Observability

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

## 12. Open Decisions

- Distribution extension ID is not fixed.
- Release installer / package signing / auto update / CD are not designed.
- Store release manifest generation and update strategy are not designed.
- Real Chrome / Edge manual smoke is not yet complete.
- Desktop tray/status-only direction is outside this plan.
- Whether to bump `SIDEKICK_PROTOCOL_VERSION` remains dependent on future wire-semantics changes, not on the current transport split alone.

## 13. Goal Handoff Policy

Use this file as the source of truth for Native Messaging status and remaining work.

If this plan and latest source structure conflict:

- preserve the global contracts and safety gates.
- adapt names / file layout to existing owner boundaries.
- do not weaken the non-executor boundary.
- do not move safety policy to TypeScript.
- do not make Native Messaging a full Codex harness rewrite.
- do not reopen Phase 0-5 implementation unless current-source evidence shows a regression.
- report any scope that must be deferred.

After context compaction / resume, re-read this file before continuing.

Do not commit or push unless explicitly requested.

## 14. Goal Prompt

```text
/goal Continue from docs/dev/native-messaging-primary-transport-master-plan.md as the source of truth for Native Messaging status and remaining work.

Do not re-implement Phases 0-5 or the Windows WSL hybrid path. Treat current tree code paths as implemented unless current-source evidence shows a regression. Preserve the Rust-first ownership, non-executor boundary, protocol/safety/session/Codex owners, Native Messaging primary mode, and WebSocket explicit fallback.

Primary next work is Windows Chrome/Edge manual smoke and release-readiness planning: provide the Windows native host exe, build the WSL daemon binary, build and load apps/extension in Windows Chrome/Edge, install the dev native host manifest and WSL config for the exact unpacked extension ID, verify Ask over Native Messaging without opening the desktop window or WSL Chrome, break/uninstall the manifest/config and verify setup-required behavior, optionally verify explicit WebSocket fallback, then report browser/version/manifest/config/result.

Keep distribution extension ID, installer/signing/CD, and desktop tray/status-only as separate decisions unless the user explicitly expands scope. If code changes become necessary, rerun the relevant focused checks and apply Final-A impact audit before reporting complete. Do not commit or push unless explicitly requested.
```

## 15. Validation For This Tranche

Run these checks after code or docs changes in this tranche:

```sh
cargo fmt --all
cargo test -p screen-sidekick-native-host
cargo test -p screen-sidekick-sidekick-daemon
cargo clippy -p screen-sidekick-native-host -p screen-sidekick-sidekick-daemon --all-targets -- -D warnings
npm --prefix apps/extension test
node apps/extension/check-manifest.mjs
node scripts/native-host-dev.mjs install --browser chrome --extension-id <32-char-id> --host-path <Windows exe> --wsl-distro <distro> --wsl-workdir <repo> --wsl-daemon-binary <daemon> --dry-run
git diff --check
```

Also scan this file for stale pre-implementation wording before completion.

## 16. References

- Chrome Native Messaging: `https://developer.chrome.com/docs/extensions/develop/concepts/native-messaging`
- Edge Native Messaging: `https://learn.microsoft.com/en-us/microsoft-edge/extensions/developer-guide/native-messaging`
- Existing chat-first plan: `docs/dev/chat-first-browser-mvp-master-plan.md`
- Development checks: `docs/development.md`
