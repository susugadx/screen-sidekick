# Development Environment

This project uses Rust as the source of truth for domain logic. The browser
extension and future desktop UI are adapters around the Rust crates.

## Required Tools

- Rust through `rustup`
- `rustfmt` and `clippy`
- Node.js and npm for the extension TypeScript build

The Tauri app uses system webview dependencies. On Linux, install the Tauri v2
system prerequisites for your distribution before running the default desktop
check. This environment may report missing `pkg-config` or DBus/WebKit packages;
do not install them from automation unless explicitly requested.

The Leptos UI build requires `trunk` and the `wasm32-unknown-unknown` Rust
target.

## Setup

Install Rust with `rustup`, then install the repository toolchain:

```sh
source "$HOME/.cargo/env"
make setup
make doctor
npm --prefix apps/extension install
```

The repository includes `rust-toolchain.toml`, so `cargo`, `rustfmt`, and
`clippy` should use the pinned Rust toolchain when run from this
directory.

If `cargo` is not found in a newly opened shell, run `source "$HOME/.cargo/env"`
or restart the terminal.

## Checks

Run the local quick gate:

```sh
make check
```

Run the full CI gate:

```sh
make ci-check
```

Or run individual checks:

```sh
make fmt
make clippy
make test
make extension-typecheck
make extension-build
make extension-test
make extension-manifest-check
make desktop-bridge-test
make desktop-check
make desktop-ui-check
```

Native Messaging host focused checks:

```sh
cargo test -p screen-sidekick-native-host
cargo build -p screen-sidekick-native-host
node scripts/native-host-dev.mjs install --browser chrome --extension-id <32-character-extension-id> --dry-run
```

The desktop bridge handler tests avoid Tauri system webview dependencies:

```sh
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --no-default-features
```

The full Tauri check uses the default desktop feature and requires OS
prerequisites:

```sh
cargo check --manifest-path apps/desktop/src-tauri/Cargo.toml
```

The Leptos UI check requires Trunk or a wasm target check:

```sh
rustup target add wasm32-unknown-unknown
cargo check --manifest-path apps/desktop/ui/Cargo.toml --target wasm32-unknown-unknown
trunk build apps/desktop/ui/index.html --release --dist apps/desktop/ui/dist
```

## CI

GitHub Actions runs the `CI` workflow for pull requests and pushes to `main`.
The required branch protection checks should be the normal `CI` workflow jobs:

- `check`
- `whitespace`

The `check` job runs `make ci-check`, which includes the local quick gate plus
the desktop Tauri check and the desktop UI wasm check. The `whitespace` job runs
`git diff --check` against the pull request or push diff.

The default CI path does not require an installed Chrome/Edge native host,
launching Chrome, or a logged-in Codex CLI. Native Messaging and Codex app-server
behavior is covered by fake-port, framing, daemon protocol, and fake Codex
tests. Manual browser smoke is still required before release packaging.

Codex app-server schema drift is intentionally separate from the required PR
gate. Use the manual `Codex Schema` workflow, or run the explicit developer
command locally:

```sh
make codex-schema-check
```

## Native Messaging Development

Build the host:

```sh
cargo build -p screen-sidekick-native-host
```

Load `apps/extension` as an unpacked extension after running
`npm --prefix apps/extension run build`, then copy the unpacked extension ID
from the browser extension page.

Install a user-level host manifest for that exact ID:

```sh
node scripts/native-host-dev.mjs install \
  --browser chrome \
  --extension-id <32-character-extension-id>
```

The helper supports `chrome`, `chrome-for-testing`, `chromium`, and `edge`.
It writes user-level locations only. On Windows it writes an HKCU registry entry
with `reg.exe`; on Linux and macOS it writes the browser-specific
`NativeMessagingHosts/com.screen_sidekick.host.json` file.

The generated manifest contains one explicit allowed origin:

```json
{
  "name": "com.screen_sidekick.host",
  "type": "stdio",
  "allowed_origins": ["chrome-extension://<extension-id>/"]
}
```

Wildcards are not valid. If the unpacked extension ID changes, regenerate the
manifest. A future store/release extension ID will need its own explicit entry.

For loopback sidecar debugging, set both variables before launching the host:

```sh
SCREEN_SIDEKICK_DAEMON_WS_URL=ws://127.0.0.1:<port>/v0/ws \
SCREEN_SIDEKICK_DAEMON_TOKEN=<pairing-token> \
target/debug/screen-sidekick-native-host
```

If either variable is missing, the native host starts the in-process Sidekick
runtime instead. It does not scan ports or read token files.
