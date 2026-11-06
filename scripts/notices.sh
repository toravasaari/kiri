#!/usr/bin/env bash
# Makes the third-party notices that a release archive carries:
#
#   make notices
#
# The result is target/notices/THIRD-PARTY-NOTICES.md. It holds the licences
# of the Rust libraries, which `cargo about` finds (about.toml), the syntax
# definitions' licences, and the hand-written parts in notices/. The work is
# done by crates/kiri-tr-notices, which also stops when a library is missing
# a licence text, so that a notice cannot go missing unseen. See its README.
#
# Needs cargo-about: cargo install --locked --features cli cargo-about

set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

command -v cargo-about > /dev/null || {
    echo "notices: cargo-about is not installed (cargo install --locked --features cli cargo-about)" >&2
    exit 1
}

out_dir=${CARGO_TARGET_DIR:-target}/notices
mkdir -p "$out_dir"

cargo about generate --locked --format json > "$out_dir/about.json"
cargo metadata --format-version 1 --locked > "$out_dir/metadata.json"
cargo run --quiet --locked -p kiri-tr-notices -- \
    "$out_dir/about.json" "$out_dir/metadata.json" > "$out_dir/THIRD-PARTY-NOTICES.md.new"
mv "$out_dir/THIRD-PARTY-NOTICES.md.new" "$out_dir/THIRD-PARTY-NOTICES.md"

echo "notices: $out_dir/THIRD-PARTY-NOTICES.md"
