.PHONY: check fmt fmt-check clippy test release

# The definition of done; CI runs the same target. Every target covers
# all crates in the workspace.
check: fmt-check clippy test

fmt:
	cargo fmt --all

fmt-check:
	cargo fmt --all --check

clippy:
	cargo clippy --workspace --all-targets -- -D warnings

test:
	cargo test --workspace

# Optimised binary in target/release/kiri.
release:
	cargo build --release
