.PHONY: check build fmt fmt-check clippy test release sweep

# The definition of done, and the target CI is to run. Every target covers
# all crates in the workspace.
check: fmt-check clippy test
	@$(MAKE) --no-print-directory sweep

# Deletes build artifacts in target/ that nothing has used for 7 days, then
# the oldest ones until target/ is under 100 GB; cargo never removes the old
# copies itself. Skipped in a container, which is destroyed anyway, and where
# cargo-sweep is not installed. A failing sweep only warns, so it cannot turn
# a passing `check` red.
sweep:
	@if command -v systemd-detect-virt >/dev/null \
			&& systemd-detect-virt --container --quiet; then \
		echo "sweep: skipped in a container"; \
	elif command -v cargo-sweep >/dev/null; then \
		cargo sweep --time 7 && cargo sweep --maxsize 100GB \
			|| echo "sweep: cargo sweep failed, ignored"; \
	else \
		echo "sweep: cargo-sweep not installed, skipped (cargo install cargo-sweep)"; \
	fi

# Compiles everything `check` needs without running it, so a later
# `make check` starts warm.
build:
	cargo test --workspace --no-run
	cargo test --workspace --no-default-features --no-run
	cargo clippy --workspace --all-targets -- -D warnings
	cargo clippy --workspace --all-targets --no-default-features -- -D warnings

fmt:
	cargo fmt --all

fmt-check:
	cargo fmt --all --check

# Like `test`, also without the default features, so code only that
# build compiles is linted too.
clippy:
	cargo clippy --workspace --all-targets -- -D warnings
	cargo clippy --workspace --all-targets --no-default-features -- -D warnings

# Also without the default features, so a build with only some of them
# keeps working. Runs with cargo-nextest when it is installed, which runs
# each test in its own process and in one pool, and with cargo test otherwise.
test:
	@if command -v cargo-nextest >/dev/null; then \
		cargo nextest run --workspace \
			&& cargo nextest run --workspace --no-default-features; \
	else \
		echo "test: cargo-nextest not installed, using cargo test (cargo install --locked cargo-nextest)"; \
		cargo test --workspace \
			&& cargo test --workspace --no-default-features; \
	fi

# Optimised binary in target/release/kiri.
release:
	cargo build --release
