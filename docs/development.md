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
`NativeMessagingHosts/com.screen_sidekick.host.json` file. Windows install
requires the WSL auto-start options shown below so the registered host can start
the runtime it needs.

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

### Windows Chrome with WSL Sidekick

Windows Chrome can use a Windows native host executable that starts the Sidekick
daemon inside WSL. Build or provide the Windows host exe separately, and build
the WSL daemon binary from this repo:

```sh
cargo build -p screen-sidekick-sidekick-daemon --bin screen-sidekick-daemon
```

Generate the Windows manifest registration and WSL auto-start config:

```sh
node scripts/native-host-dev.mjs install \
  --browser chrome \
  --extension-id <32-character-extension-id> \
  --host-path 'C:\path\to\screen-sidekick-native-host.exe' \
  --wsl-distro Ubuntu-24.04 \
  --wsl-workdir /home/<user>/dev/projects/screen-sidekick \
  --wsl-daemon-binary /home/<user>/dev/projects/screen-sidekick/target/debug/screen-sidekick-daemon \
  --wsl-path /home/<user>/.nvm/versions/node/<version>/bin:/home/<user>/.cargo/bin:/usr/local/bin:/usr/bin:/bin
```

The config path defaults to
`%APPDATA%\Screen Sidekick\native-host-config.json`; override it with
`SCREEN_SIDEKICK_NATIVE_HOST_CONFIG` if needed. The native host validates the
config and starts WSL with argv, not a shell command string. `--wsl-path` is
optional, but recommended for Windows-launched WSL commands when `cargo`,
`npm`, or `codex` live under user-managed paths such as Cargo or nvm. It writes
native-host config schema v0.2, so setup checks that the configured
`--host-path` executable reports v0.2 support before writing the config. Rebuild
the Windows native host exe after updating this repository. The host and doctor
still read legacy v0.1 configs with `wsl_path` written by earlier local setup
builds. If the sidecar env vars below are not set and the Windows config is
missing or invalid, WSL startup/status reporting fails, or the host cannot
connect to the reported WSL daemon WebSocket before the first daemon response,
the host answers the first Native Messaging request with a structured
setup-required error instead of falling back to an in-process Windows runtime or
silent WebSocket fallback.
Each WSL auto-start daemon is tied to the native port that launched it, so it
does not run global interrupted-turn recovery on startup. A turn started by that
sidecar-owned WebSocket relay is still failed and cleared if that relay closes
before a terminal turn notification. Browser direct/fallback WebSocket reloads
do not use this sidecar marker; they preserve the running active turn for
reconnect and `session/get` recovery.

For loopback sidecar debugging, set both variables before launching the host:

```sh
SCREEN_SIDEKICK_DAEMON_WS_URL=ws://127.0.0.1:<port>/v0/ws \
SCREEN_SIDEKICK_DAEMON_TOKEN=<pairing-token> \
target/debug/screen-sidekick-native-host
```

If both variables are present, this explicit sidecar connection has priority on
all platforms. If either variable is missing, Linux and macOS hosts start the
in-process Sidekick runtime. Windows hosts use WSL auto-start config instead.
The host does not scan ports or read token files.

### Local Alpha Setup Commands

The root package exposes local setup wrappers for the Windows Chrome/Edge + WSL
development path:

```sh
npm run sidekick:install-local -- \
  --browser edge \
  --extension-id <32-character-extension-id> \
  --host-path 'C:\path\to\screen-sidekick-native-host.exe' \
  --wsl-workdir /home/<user>/dev/projects/screen-sidekick \
  --wsl-path /home/<user>/.nvm/versions/node/<version>/bin:/home/<user>/.cargo/bin:/usr/local/bin:/usr/bin:/bin

npm run sidekick:doctor-local -- --browser edge --extension-id <32-character-extension-id>
npm run sidekick:uninstall-local -- --browser edge
```

`install-local` builds the WSL daemon and extension, then delegates manifest,
registry, and WSL config generation to `scripts/native-host-dev.mjs`. It does
not build the Windows native host executable; provide that path explicitly or
through `SCREEN_SIDEKICK_WINDOWS_HOST_PATH`.

The setup wrappers accept these environment defaults:

```text
SCREEN_SIDEKICK_EXTENSION_ID
SCREEN_SIDEKICK_WINDOWS_HOST_PATH
SCREEN_SIDEKICK_WSL_DISTRO
SCREEN_SIDEKICK_WSL_WORKDIR
SCREEN_SIDEKICK_WSL_DAEMON_BINARY
SCREEN_SIDEKICK_WSL_PATH
```

When run from WSL/Linux, pass `--dry-run` to preview the Windows
HKCU/APPDATA writes. Actual Windows registry/config writes must run from
Windows. If the install command omits `--wsl-workdir`, set
`SCREEN_SIDEKICK_WSL_WORKDIR` first. `doctor-local --dry-run` validates resolved
options without spawning process checks. `SCREEN_SIDEKICK_WSL_PATH` uses the
same colon-separated absolute WSL path list as `--wsl-path`; include the active
nvm bin directory, Cargo bin directory, and system bin directories needed by the
daemon and Codex CLI. If this value is set, `install-local` verifies that the
configured Windows native host exe can read the v0.2 config. `doctor-local`
also verifies the registered host exe for installed v0.2 configs and legacy
v0.1 configs with `wsl_path`.
