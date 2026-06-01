RUST_TOOLCHAIN := 1.96.0

.PHONY: doctor setup fmt fmt-check clippy test check extension-manifest-check

doctor:
	@command -v rustup >/dev/null || { echo "rustup not found. Install Rust from https://rustup.rs/"; exit 1; }
	@command -v cargo >/dev/null || { echo "cargo not found. Run make setup after installing rustup."; exit 1; }
	@cargo --version
	@rustc --version
	@rustfmt --version
	@cargo clippy --version
	@command -v node >/dev/null && node --version || echo "node not found; extension checks are limited in Phase 0-A"

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

extension-manifest-check:
	node apps/extension/check-manifest.mjs

check: fmt-check clippy test extension-manifest-check
