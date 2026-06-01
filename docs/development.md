# Development Environment

This project uses Rust as the source of truth for domain logic. The browser
extension and future desktop UI are adapters around the Rust crates.

## Required Tools

- Rust through `rustup`
- `rustfmt` and `clippy`
- Node.js for the Phase 0-A manifest JSON check

No npm package, Tauri dependency, Leptos dependency, or Trunk dependency is
installed in Phase 0-A.

## Setup

Install Rust with `rustup`, then install the repository toolchain:

```sh
source "$HOME/.cargo/env"
make setup
make doctor
```

The repository includes `rust-toolchain.toml`, so `cargo`, `rustfmt`, and
`clippy` should use the pinned Rust toolchain when run from this
directory.

If `cargo` is not found in a newly opened shell, run `source "$HOME/.cargo/env"`
or restart the terminal.

## Checks

Run all checks:

```sh
make check
```

Or run individual checks:

```sh
make fmt
make clippy
make test
make extension-manifest-check
```

## Future Tauri Setup

Tauri, Leptos, Trunk, and OS-level Tauri prerequisites are intentionally deferred
until the desktop app is introduced. Adding them now would expand Phase 0-A
beyond the current Rust-first scaffold.
