.PHONY: check build fmt fmt-check clippy test release

# The definition of done; CI runs the same target. Every target covers
# all crates in the workspace.
check: fmt-check clippy test

# Compiles everything `check` needs without running it, so a later
# `make check` starts warm.
build:
	cargo test --workspace --no-run
	cargo test --workspace --no-default-features --no-run
	cargo clippy --workspace --all-targets -- -D warnings

fmt:
	cargo fmt --all

fmt-check:
	cargo fmt --all --check

clippy:
	cargo clippy --workspace --all-targets -- -D warnings

# Also without the default features, so a build with only some of them
# keeps working.
test:
	cargo test --workspace
	cargo test --workspace --no-default-features

# Optimised binary in target/release/kiri.
release:
	cargo build --release
