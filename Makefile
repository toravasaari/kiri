.PHONY: check fmt fmt-check clippy test release

# The definition of done; CI runs the same target.
check: fmt-check clippy test

fmt:
	cargo fmt

fmt-check:
	cargo fmt --check

clippy:
	cargo clippy --all-targets -- -D warnings

test:
	cargo test

# Optimised binary in target/release/kiri.
release:
	cargo build --release
