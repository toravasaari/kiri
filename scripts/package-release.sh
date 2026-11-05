#!/usr/bin/env bash
# Packs a release build for one target into an archive and its checksum:
#
#   scripts/package-release.sh x86_64-unknown-linux-musl
#
# The build is target/<target>/release/kiri (cargo build --release --target
# <target>), and the third-party notices are those of `make notices`. The
# result is target/dist/kiri-the-reader-<target>.tar.gz, which holds a
# directory of the same name with the binary, the licence, the notices and
# the README, and target/dist/kiri-the-reader-<target>.tar.gz.sha256.
#
# The archive is named after the package, not the binary, so that
# cargo-binstall finds it (see [package.metadata.binstall] in Cargo.toml).
# It is made with GNU tar, so the same files give the same archive.

set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

[ $# -eq 1 ] || { echo "usage: package-release.sh TARGET" >&2; exit 2; }

target=$1
target_dir=${CARGO_TARGET_DIR:-target}
name=kiri-the-reader-$target
out=$target_dir/dist

binary=$target_dir/$target/release/kiri
notices=$target_dir/notices/THIRD-PARTY-NOTICES.md

for file in "$binary" "$notices" LICENSE README.md; do
    [ -f "$file" ] || { echo "package-release: $file is missing" >&2; exit 1; }
done

rm -rf "${out:?}/$name"
mkdir -p "$out/$name"
cp "$binary" "$out/$name/kiri"
cp "$notices" "$out/$name/THIRD-PARTY-NOTICES.md"
cp LICENSE README.md "$out/$name/"

# The time of the commit, so that the archive does not depend on the day.
epoch=$(git log -1 --format=%ct 2> /dev/null || date +%s)

(
    cd "$out"
    tar --sort=name --owner=0 --group=0 --numeric-owner --mtime="@$epoch" \
        -cf - "$name" | gzip -9n > "$name.tar.gz"
    sha256sum "$name.tar.gz" > "$name.tar.gz.sha256"
)

echo "package-release: $out/$name.tar.gz"
