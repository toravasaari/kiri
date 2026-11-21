# The definition of done, and the target CI is to run. Every target covers
# all crates in the workspace, except that `test-it`, `test-<area>`,
# `check-it` and `check-<area>` run the tests of the kiri-the-reader
# package only.
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

# The tests that take more than 5 s on an idle host. `check` runs them;
# `check-fast` and the checks and tests of one part leave them out. A test
# that grows slower than that goes on this list.
SLOW_TESTS = \
	typeset::locate::tests::what_is_at_a_cell_of_a_page_is_a_place_that_is_shown_on_that_row \
	typeset::reading::places::tests::what_is_under_a_glyph_of_a_text_on_a_page_is_the_place_it_shows \
	pager::tests::stretches::paging_through_a_csv_of_a_million_rows_makes_cells_of_the_rows_near_the_view_only \
	typeset::measured::a_layout_taken_back_into_a_measured_block_keeps_the_colours_of_the_whole_one \
	typeset::driver::tests::a_page_that_starts_inside_a_details_is_restarted_as_it_is_laid_out_whole \
	typeset::measured::grown::ten_thousand_lines_appended_lay_out_the_stretches_from_the_old_last_on \
	typeset::measured::grown::a_line_appended_to_a_log_of_100_000_lines_lays_out_its_last_stretch_alone \
	examples::the_visual_check_documents_fit_every_mode \
	typeset::locate::tests::what_is_at_the_start_of_each_grapheme_of_a_line_is_its_place_and_locate_finds_the_line \
	session::links::tests::the_stops_tab_visits_in_every_fixture_and_example_are_pinned \
	session::search::tests::both_engines_find_the_same_number_of_matches_in_every_fixture \
	typeset::reading::engine::tests::a_layout_in_reading_mode_made_in_steps_is_the_layout_made_whole \
	pager::tests::stretches::a_csv_of_a_million_rows_shows_its_first_frame_having_made_cells_of_few_of_them \
	guard_direction::non_test_code_uses_only_the_parts_above_it \
	typeset::locate::tests::line_of_is_the_smallest_line_of_the_stretches_after_the_place

# The slow tests as a cargo-nextest filter, and as cargo test arguments that
# skip them.
empty :=
space := $(empty) $(empty)
SLOW_FILTER = $(subst $(space), | ,$(strip $(foreach test,$(SLOW_TESTS),test(=$(test)))))
SLOW_SKIPS = $(foreach test,$(SLOW_TESTS),--skip $(test))
NO_NEXTEST = echo "$@: cargo-nextest not installed, using cargo test (cargo install --locked cargo-nextest)"

# Before each commit: the formatting, clippy and the tests of the default
# build, without the slow tests. `check`, before the merge, also runs the
# slow tests, and lints and tests the build without the default features.
.PHONY: check-fast
check-fast: fmt-check
	cargo clippy --workspace --all-targets -- -D warnings
	@if command -v cargo-nextest >/dev/null; then \
		cargo nextest run --workspace -E 'not ($(SLOW_FILTER))'; \
	else \
		$(NO_NEXTEST); \
		cargo test --workspace -- $(SLOW_SKIPS); \
	fi

# The tests in tests/it, without the slow tests. `test-it` runs only them;
# `check-it` is `check-fast` for them.
define test_it
@if command -v cargo-nextest >/dev/null; then \
	cargo nextest run --workspace \
		-E 'binary_id(kiri-the-reader::it) and not ($(SLOW_FILTER))'; \
else \
	$(NO_NEXTEST); \
	cargo test --test it -- $(SLOW_SKIPS); \
fi
endef

.PHONY: test-it
test-it: ; $(test_it)

.PHONY: check-it
check-it: fmt-check
	cargo clippy --workspace --all-targets -- -D warnings
	$(test_it)

# The tests of one part of kiri, without the slow tests: `make test-typeset`
# runs the tests of src/typeset, `make test-session` those of src/session.
# `make check-typeset` is `check-fast` for them. Every module at the top of
# src/ that has tests has its two targets written out below, so that the
# shell can list them with `make test-<Tab>`. A new module gets its two lines
# there. Without cargo-nextest, cargo test finds the name anywhere in a
# test's path, so more tests run, of other parts too.
define test_area
@if command -v cargo-nextest >/dev/null; then \
	cargo nextest run --workspace \
		-E 'package(kiri-the-reader) and kind(lib) and test(/^$(1)::/) and not ($(SLOW_FILTER))'; \
else \
	$(NO_NEXTEST); \
	cargo test --lib -- $(1):: $(SLOW_SKIPS); \
fi
endef

define check_area
cargo clippy --workspace --all-targets -- -D warnings
$(call test_area,$(1))
endef

.PHONY: test-config test-diagrams test-doctor test-frontend test-images \
	test-list test-load test-pager test-panics test-present test-render \
	test-session test-term test-testing test-typeset
test-config: ; $(call test_area,config)
test-diagrams: ; $(call test_area,diagrams)
test-doctor: ; $(call test_area,doctor)
test-frontend: ; $(call test_area,frontend)
test-images: ; $(call test_area,images)
test-list: ; $(call test_area,list)
test-load: ; $(call test_area,load)
test-pager: ; $(call test_area,pager)
test-panics: ; $(call test_area,panics)
test-present: ; $(call test_area,present)
test-render: ; $(call test_area,render)
test-session: ; $(call test_area,session)
test-term: ; $(call test_area,term)
test-testing: ; $(call test_area,testing)
test-typeset: ; $(call test_area,typeset)

.PHONY: check-config check-diagrams check-doctor check-frontend check-images \
	check-list check-load check-pager check-panics check-present check-render \
	check-session check-term check-testing check-typeset
check-config: fmt-check ; $(call check_area,config)
check-diagrams: fmt-check ; $(call check_area,diagrams)
check-doctor: fmt-check ; $(call check_area,doctor)
check-frontend: fmt-check ; $(call check_area,frontend)
check-images: fmt-check ; $(call check_area,images)
check-list: fmt-check ; $(call check_area,list)
check-load: fmt-check ; $(call check_area,load)
check-pager: fmt-check ; $(call check_area,pager)
check-panics: fmt-check ; $(call check_area,panics)
check-present: fmt-check ; $(call check_area,present)
check-render: fmt-check ; $(call check_area,render)
check-session: fmt-check ; $(call check_area,session)
check-term: fmt-check ; $(call check_area,term)
check-testing: fmt-check ; $(call check_area,testing)
check-typeset: fmt-check ; $(call check_area,typeset)

# The slow tests alone, in both builds.
.PHONY: test-slow
test-slow:
	@if command -v cargo-nextest >/dev/null; then \
		cargo nextest run --workspace -E '$(SLOW_FILTER)' \
			&& cargo nextest run --workspace --no-default-features -E '$(SLOW_FILTER)'; \
	else \
		$(NO_NEXTEST); \
		cargo test --workspace -- $(SLOW_TESTS) \
			&& cargo test --workspace --no-default-features -- $(SLOW_TESTS); \
	fi

# The property tests alone (the tests of the `props` modules and of
# tests/it/props_layout.rs), each with PROPTEST_CASES cases instead of the
# few `check` runs. A failing property prints the seed and the smallest case
# it found; the case is saved under proptest-regressions/ (or
# tests/proptest-regressions/) to be committed with its fix. Takes a few
# minutes; CI runs it monthly.
PROPTEST_CASES ?= 10000

.PHONY: props
props:
	@if command -v cargo-nextest >/dev/null; then \
		PROPTEST_CASES=$(PROPTEST_CASES) cargo nextest run --lib --test it props; \
	else \
		PROPTEST_CASES=$(PROPTEST_CASES) cargo test --lib --test it props; \
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
