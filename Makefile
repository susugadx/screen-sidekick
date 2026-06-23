RUST_TOOLCHAIN := 1.96.0

.PHONY: doctor setup fmt fmt-check clippy test check ci-check extension-typecheck extension-build extension-test extension-manifest-check local-setup-test desktop-bridge-test desktop-check desktop-ui-check desktop-dev codex-schema-refresh codex-schema-check codex-schema-metadata

doctor:
	@command -v rustup >/dev/null || { echo "rustup not found. Install Rust from https://rustup.rs/"; exit 1; }
	@command -v cargo >/dev/null || { echo "cargo not found. Run make setup after installing rustup."; exit 1; }
	@cargo --version
	@rustc --version
	@rustfmt --version
	@cargo clippy --version
	@command -v node >/dev/null && node --version || echo "node not found; extension checks are unavailable"
	@command -v npm >/dev/null && npm --version || echo "npm not found; extension checks are unavailable"

setup:
	rustup toolchain install $(RUST_TOOLCHAIN) --profile minimal --component rustfmt --component clippy

fmt:
	cargo fmt --all

fmt-check:
	cargo fmt --all -- --check

clippy:
	cargo clippy --workspace --all-targets -- -D warnings

test:
	cargo test --workspace

extension-typecheck:
	npm --prefix apps/extension run typecheck

extension-build:
	npm --prefix apps/extension run build

extension-test:
	npm --prefix apps/extension test

extension-manifest-check:
	node apps/extension/check-manifest.mjs

local-setup-test:
	npm run test:local-setup

desktop-bridge-test:
	cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --no-default-features

desktop-check:
	cargo check --manifest-path apps/desktop/src-tauri/Cargo.toml

desktop-ui-check:
	cargo check --manifest-path apps/desktop/ui/Cargo.toml --target wasm32-unknown-unknown

desktop-dev:
	cd apps/desktop/src-tauri && cargo tauri dev

codex-schema-refresh:
	./crates/codex-client/schema/refresh.sh

codex-schema-check:
	./crates/codex-client/schema/check.sh

codex-schema-metadata:
	@cat crates/codex-client/schema/metadata.json

check: fmt-check clippy test extension-typecheck extension-test extension-manifest-check local-setup-test desktop-bridge-test

ci-check: check desktop-check desktop-ui-check
