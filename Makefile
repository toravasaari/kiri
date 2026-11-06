# The definition of done, and the target CI is to run. Every target covers
# all crates in the workspace.
.PHONY: check
check: fmt-check clippy test licences
	@$(MAKE) --no-print-directory sweep

# Deletes build artifacts in target/ that nothing has used for 7 days, then
# the oldest ones until target/ is under 100 GB; cargo never removes the old
# copies itself. Skipped in a container, which is destroyed anyway, and where
# cargo-sweep is not installed. A failing sweep only warns, so it cannot turn
# a passing `check` red.
.PHONY: sweep
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
.PHONY: build
build:
	cargo test --workspace --no-run
	cargo test --workspace --no-default-features --no-run
	cargo clippy --workspace --all-targets -- -D warnings
	cargo clippy --workspace --all-targets --no-default-features -- -D warnings

.PHONY: fmt
fmt:
	cargo fmt --all

.PHONY: fmt-check
fmt-check:
	cargo fmt --all --check

# Like `test`, also without the default features, so code only that
# build compiles is linted too.
.PHONY: clippy
clippy:
	cargo clippy --workspace --all-targets -- -D warnings
	cargo clippy --workspace --all-targets --no-default-features -- -D warnings

# Also without the default features, so a build with only some of them
# keeps working. Runs with cargo-nextest when it is installed, which runs
# each test in its own process and in one pool, and with cargo test otherwise.
.PHONY: test
test:
	@if command -v cargo-nextest >/dev/null; then \
		cargo nextest run --workspace \
			&& cargo nextest run --workspace --no-default-features; \
	else \
		echo "test: cargo-nextest not installed, using cargo test (cargo install --locked cargo-nextest)"; \
		cargo test --workspace \
			&& cargo test --workspace --no-default-features; \
	fi

# Fails when a dependency has a licence that kiri's MIT licence cannot ship
# with, as listed in deny.toml. Runs when cargo-deny is installed, and CI
# always runs it.
.PHONY: licences
licences:
	@if command -v cargo-deny >/dev/null; then \
		cargo deny check licenses; \
	else \
		echo "licences: cargo-deny not installed, skipped (cargo install --locked cargo-deny)"; \
	fi

# The third-party notices that a release archive carries, in
# target/notices/THIRD-PARTY-NOTICES.md. Needs cargo-about.
.PHONY: notices
notices:
	scripts/notices.sh

# Optimised binary in target/release/kiri.
.PHONY: build-release
build-release:
	cargo build --release

# Making a release is `scripts/release.sh`: it raises the version, dates the
# changelog, runs the checks, and commits and tags the result. It never
# pushes; pushing the tag is what starts the release workflow. `DRY_RUN=1`
# only says what would happen. `make release-version VERSION=0.1.0` tags a
# version by name, as for the first release, or a release candidate such as
# 0.2.0-rc.1; `make release-undo` takes back an unpushed release.
RELEASE_FLAGS = $(if $(DRY_RUN),--dry-run)

.PHONY: patch-release
patch-release:
	scripts/release.sh bump patch $(RELEASE_FLAGS)

.PHONY: minor-release
minor-release:
	scripts/release.sh bump minor $(RELEASE_FLAGS)

.PHONY: major-release
major-release:
	scripts/release.sh bump major $(RELEASE_FLAGS)

.PHONY: release-version
release-version:
	@test -n "$(VERSION)" || { echo "usage: make release-version VERSION=x.y.z"; exit 2; }
	scripts/release.sh version $(VERSION) $(RELEASE_FLAGS)

.PHONY: release-undo
release-undo:
	scripts/release.sh undo
