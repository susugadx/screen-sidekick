# Screen Sidekick

Screen Sidekick is a Rust-first local screen assistant for Codex users.

It is not a Codex replacement, browser automation agent, MCP runner, or local
repository editor. The app helps a user ask about the browser page or local
desktop screen they are looking at, then bridges that screen context into Codex
developer workflows when deeper repo work is needed.

## Current Status

This repository contains the browser chat vertical slice:

- Rust workspace for core domain crates.
- `RawScreenContext` / `ScreenContext v0.1` typed payloads.
- Safety rules for danger/redaction policy plus safety review primitives.
- Prompt preview generation from a `SanitizedScreenContext`.
- `capture-pipeline` for `raw_browser_context.v0.1` browser captures,
  sanitized ScreenContext JSON, safety summary, and prompt response generation.
- `sidekick-daemon` for session state, sanitized attachment storage, and Codex
  app-server turn streaming.
- `sidekick-native-host` for the Chrome/Edge Native Messaging host.
- Chrome/Edge side panel adapter in `apps/extension`, using Native Messaging as
  the primary chat transport.
- Tauri v2 desktop bridge/status shell in `apps/desktop` for debug and fallback
  workflows.

The legacy loopback WebSocket / HTTP bridge remains available for development
fallback and debug capture. Screen Sidekick can grow toward confirmed
browser/desktop actions, but it still must not perform browser automation, MCP
execution, Computer Use, local repository editing, or automatic Codex actions
without an explicit user confirmation boundary and a separate owner.

## Repository Layout

```text
crates/screen-context  RawScreenContext / ScreenContext v0.1 types and serialization behavior
crates/safety-rules    pure danger detection and text/URL redaction policy
crates/safety          SafetyReview and SanitizedScreenContext owner
crates/prompt          Codex-ready prompt preview owner; consumes safety-reviewed context
crates/capture-pipeline raw browser capture DTO and sanitized side panel response owner
crates/sidekick-daemon session, attachment, Codex app-server, and shared protocol execution
crates/sidekick-native-host Chrome/Edge Native Messaging framing and host lifecycle
apps/desktop           Tauri + Leptos status/debug shell and loopback fallback transport
apps/extension         Chrome/Edge side panel adapter and transport adapters
docs/                  architecture and non-executor boundary notes
```

## Local Checks

Install the Rust toolchain and verify the environment:

```sh
make setup
make doctor
make check
```

See [docs/development.md](docs/development.md) for the full setup notes.
See [apps/extension/README.md](apps/extension/README.md) for Native Messaging
development setup.

## Product Direction

The product center is assistant-first:

```text
Browser page or local desktop screen
  -> summon Screen Sidekick
  -> ask "what is this?", "what should I do next?", or "is this safe?"
  -> get an explanation, next-step guidance, and risk notes
  -> bridge to Codex / repo work only when the task needs local development context
```

The next product tranche should improve the assistant answer style before adding
automation: explain the meaning of the current screen, avoid reading text back
verbatim, suggest the next step, and flag risky actions.

See [docs/codex-companion-direction.md](docs/codex-companion-direction.md) for
the assistant-first direction.

## Next Setup Work

The next setup tranche is a local alpha installer, not store publication. This
supports the assistant direction by making the local Native Messaging / WSL /
Codex path repeatable on the developer machine.

Goal: make this developer machine recoverable with one setup command before
adding more local invocation features. That command should build/copy the
Windows native host, build the WSL daemon and extension, register the
browser-specific Native Messaging manifest for the exact extension ID, write the
WSL auto-start config, run a setup doctor/smoke check, and provide uninstall.

Local setup commands:

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

The commands also accept environment defaults:
`SCREEN_SIDEKICK_EXTENSION_ID`, `SCREEN_SIDEKICK_WINDOWS_HOST_PATH`,
`SCREEN_SIDEKICK_WSL_DISTRO`, `SCREEN_SIDEKICK_WSL_WORKDIR`, and
`SCREEN_SIDEKICK_WSL_DAEMON_BINARY`, and `SCREEN_SIDEKICK_WSL_PATH`. The WSL
PATH value is a colon-separated list of absolute WSL paths used for
Windows-launched non-interactive WSL commands, including build, doctor, and
native-host daemon startup. Because `--wsl-path` writes native-host config
schema v0.2, setup verifies that the `--host-path` executable reports v0.2
support; rebuild the Windows native host exe after updating this repository.
Doctor also verifies the registered host exe for installed v0.2 configs and
legacy v0.1 configs with `wsl_path`. The host still reads legacy v0.1 configs
with `wsl_path` written by earlier local setup builds. If `--wsl-workdir` is omitted, set
`SCREEN_SIDEKICK_WSL_WORKDIR` first. From WSL/Linux, use `--dry-run` for the
Windows registry/config step; actual HKCU/APPDATA writes must run from Windows.

Store publication, code signing, and a formal Windows installer are later
distribution work. Keep them separate until the local setup path is repeatable.
