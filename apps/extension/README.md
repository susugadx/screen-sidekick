# Browser Extension

This directory owns the Chrome/Edge side panel adapter.

The extension may own:

- Chrome extension entrypoints.
- Chrome API calls.
- DOM capture entrypoints.
- Screenshot and selected text capture entrypoints.
- Side panel UI wiring.

The extension must not own:

- `ScreenContext` schema policy.
- Danger detection.
- Secret or input masking.
- Prompt generation.
- Handoff execution.
- Browser automation.

## Transport

The side panel uses Chrome/Edge Native Messaging as the primary transport:

```text
side panel -> com.screen_sidekick.host -> Rust Sidekick runtime -> Codex app-server
```

The WebSocket daemon URL/token fields remain an explicit development fallback
and are also used by the legacy Debug Capture button. Normal Ask flow should not
require opening the desktop window or copying a pairing token.

Native Messaging is only called from the side panel extension page. Content
scripts do not call `chrome.runtime.connectNative()`.

## Native Host Development Setup

Build the native host:

```sh
cargo build -p screen-sidekick-native-host
```

Load this directory as an unpacked extension, then copy its extension ID from
`chrome://extensions` or `edge://extensions`. Install a user-level native host
manifest for that exact ID:

```sh
node ../../scripts/native-host-dev.mjs install \
  --browser chrome \
  --extension-id <32-character-extension-id>
```

Supported `--browser` values are `chrome`, `chrome-for-testing`, `chromium`, and
`edge`. The helper writes only user-level locations. It does not perform
system-wide writes, signing, packaging, or store distribution.

`allowed_origins` is always generated as an explicit
`chrome-extension://<extension-id>/` entry. Wildcards are not supported. An
unpacked development ID and a future release/store ID are different IDs and need
separate manifest entries or separate generated manifests.

For a dry run:

```sh
node ../../scripts/native-host-dev.mjs install \
  --browser chrome \
  --extension-id <32-character-extension-id> \
  --dry-run
```

Hybrid development fallback is available by launching the native host with both
environment variables set:

```sh
SCREEN_SIDEKICK_DAEMON_WS_URL=ws://127.0.0.1:<port>/v0/ws \
SCREEN_SIDEKICK_DAEMON_TOKEN=<pairing-token> \
target/debug/screen-sidekick-native-host
```

The host does not scan ports or discover token files.

## Checks

```sh
npm install
npm run typecheck
npm run build
npm test
node check-manifest.mjs
```

Build before loading the directory as an unpacked extension. The generated
`dist/` directory is intentionally ignored by git.
