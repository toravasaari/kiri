#!/usr/bin/env bash
# Makes and checks kiri releases. Run it through make:
#
#   make patch-release | minor-release | major-release
#   make release-version VERSION=0.1.0
#   make release-undo
#
# Add DRY_RUN=1 to a make command to see what it would do. Directly:
#
#   scripts/release.sh bump patch|minor|major [--dry-run]
#   scripts/release.sh version X.Y.Z[-suffix] [--dry-run]
#   scripts/release.sh undo
#   scripts/release.sh verify vX.Y.Z    what the release workflow checks
#   scripts/release.sh notes vX.Y.Z     the changelog text of a version
#
# A release raises the version in Cargo.toml and Cargo.lock, turns the
# [Unreleased] part of CHANGELOG.md into the new version, runs the checks
# on that tree, and commits and tags it. Nothing is pushed: after your own
# tests, push the commit and the tag together:
#
#   git push --atomic origin master vX.Y.Z
#
# If a step fails, the three files go back as they were and nothing is
# committed or tagged.

set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

SEMVER='^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$'

die() {
    echo "release: $*" >&2
    exit 1
}

say() {
    echo "release: $*"
}

# The version of the kiri package; the workspace's other packages have
# versions of their own.
cargo_version() {
    awk '/^\[/ { in_package = ($0 == "[package]") }
         in_package && /^version = "/ { gsub(/^version = "|"$/, ""); print; exit }' Cargo.toml
}

repo_url() {
    sed -n 's/^repository = "\(.*\)"$/\1/p' Cargo.toml | head -n 1
}

# The text under "## [$1]" in CHANGELOG.md, without its blank edges. $1 is
# Unreleased or a version.
changelog_section() {
    awk -v heading="## [$1]" '
        /^## \[/ {
            if (found) { exit }
            if (index($0, heading) == 1) { found = 1; next }
        }
        found && /^\[[^]]+\]: / { exit }
        found { print }
    ' CHANGELOG.md | awk '
        NF { started = 1 }
        started { lines[++n] = $0 }
        END {
            while (n > 0 && lines[n] ~ /^[[:space:]]*$/) { n-- }
            for (i = 1; i <= n; i++) { print lines[i] }
        }
    '
}

changelog_has_content() {
    [ -n "$(changelog_section "$1")" ]
}

# The version after raising the part named by $2 of the version $1.
bumped() {
    local current=$1 part=$2 major minor patch
    [[ $current =~ ^([0-9]+)\.([0-9]+)\.([0-9]+)$ ]] \
        || die "cannot raise $current: only a version of the form x.y.z can be raised"
    major=${BASH_REMATCH[1]}
    minor=${BASH_REMATCH[2]}
    patch=${BASH_REMATCH[3]}
    case $part in
        patch) echo "$major.$minor.$((patch + 1))" ;;
        minor) echo "$major.$((minor + 1)).0" ;;
        major) echo "$((major + 1)).0.0" ;;
        *) die "unknown part '$part': use patch, minor or major" ;;
    esac
}

# Is the base (x.y.z) of version $1 lower than that of version $2?
base_lower() {
    local a=${1%%-*} b=${2%%-*}
    [ "$a" != "$b" ] && [ "$(printf '%s\n%s\n' "$a" "$b" | sort -V | head -n 1)" = "$a" ]
}

set_cargo_version() {
    local current=$1 new=$2
    awk -v current="$current" -v new="$new" '
        /^\[/ { in_package = ($0 == "[package]") }
        in_package && !done && $0 == "version = \"" current "\"" {
            print "version = \"" new "\""
            done = 1
            next
        }
        { print }
    ' Cargo.toml > Cargo.toml.new
    mv Cargo.toml.new Cargo.toml
    # Any cargo command that resolves the workspace brings Cargo.lock along.
    cargo metadata --format-version 1 --offline > /dev/null
    local changed
    changed=$(git diff -U0 Cargo.lock | grep -c '^[+-][^+-]' || true)
    [ "$changed" -eq 2 ] \
        || die "Cargo.lock changed in $changed lines instead of 2; check 'git diff Cargo.lock'"
}

# Turns [Unreleased] into the version $1 of date $2, with a new empty
# [Unreleased] above it, and keeps the links at the end of the file: $3 is
# the previous tag, if there is one.
stamp_changelog() {
    local version=$1 date=$2 previous=$3 url
    url=$(repo_url)
    [ -n "$url" ] || die "Cargo.toml has no repository"
    local unreleased_link="[Unreleased]: $url/compare/v$version...HEAD"
    local version_link
    if [ -n "$previous" ]; then
        version_link="[$version]: $url/compare/$previous...v$version"
    else
        version_link="[$version]: $url/releases/tag/v$version"
    fi
    awk -v version="$version" -v date="$date" \
        -v unreleased_link="$unreleased_link" -v version_link="$version_link" '
        /^\[Unreleased\]: / { next }
        /^## \[Unreleased\]/ && !stamped {
            print
            print ""
            print "## [" version "] - " date
            stamped = 1
            next
        }
        /^\[[^]]+\]: / && !linked {
            print unreleased_link
            print version_link
            linked = 1
        }
        { print }
        END {
            if (!linked) {
                print ""
                print unreleased_link
                print version_link
            }
        }
    ' CHANGELOG.md > CHANGELOG.md.new
    mv CHANGELOG.md.new CHANGELOG.md
}

PROBLEMS=()

problem() {
    PROBLEMS+=("$1")
}

# Everything that can be known before a file is touched.
preflight() {
    local version=$1 current=$2
    [ "$(git branch --show-current)" = master ] || problem "not on master"
    [ -z "$(git status --porcelain)" ] \
        || problem "the working tree is not clean (commit or stash first)"
    if git rev-parse -q --verify "refs/tags/v$version" > /dev/null; then
        problem "the tag v$version exists already"
    fi
    if git fetch --quiet origin 2> /dev/null; then
        local behind
        behind=$(git rev-list --count HEAD..origin/master)
        [ "$behind" -eq 0 ] || problem "origin/master has $behind commits that master does not"
    else
        problem "git fetch origin failed, so origin/master is not known"
    fi
    if ! changelog_has_content Unreleased; then
        problem "CHANGELOG.md has nothing under [Unreleased]"
    fi
    if grep -q "^## \[$version\]" CHANGELOG.md; then
        problem "CHANGELOG.md has a section for $version already"
    fi
    if [ "$version" != "$current" ] && base_lower "${version}" "${current}"; then
        problem "$version is lower than the current version $current"
    fi
    local tool
    for tool in cargo-about cargo-deny; do
        command -v "$tool" > /dev/null \
            || problem "$tool is not installed (cargo install --locked $tool; cargo-about also needs --features cli)"
    done
    make -n notices > /dev/null 2>&1 \
        || problem "make has no 'notices' target yet (3.3 third-party notices)"
}

release_binary() {
    echo "${CARGO_TARGET_DIR:-target}/release/kiri"
}

run_checks() {
    local version=$1
    say "make check"
    make check
    say "make notices"
    make notices
    say "cargo deny check licenses"
    cargo deny check licenses
    say "cargo deny check advisories (a warning only)"
    cargo deny check advisories \
        || say "WARNING: advisories were found; read them before pushing the tag"
    say "make build-release"
    make build-release
    local binary
    binary=$(release_binary)
    say "trying $binary"
    [ "$("$binary" --version)" = "kiri $version" ] \
        || die "$binary --version does not say 'kiri $version'"
    "$binary" README.md > /dev/null || die "$binary could not show README.md"
    "$binary" --format text Cargo.toml > /dev/null \
        || die "$binary could not show Cargo.toml as text"
}

restore_files() {
    git checkout -- Cargo.toml Cargo.lock CHANGELOG.md
    rm -f Cargo.toml.new CHANGELOG.md.new
}

release() {
    local new=$1 dry_run=$2 current previous
    [[ $new =~ $SEMVER ]] || die "'$new' is not a version such as 1.2.3 or 1.2.3-rc.1"
    current=$(cargo_version)
    [ -n "$current" ] || die "no version found in Cargo.toml"
    previous=$(git describe --tags --abbrev=0 --match 'v*' 2> /dev/null || true)

    preflight "$new" "$current"

    say "releasing $new (now $current; previous tag: ${previous:-none})"
    if [ "$dry_run" = yes ]; then
        if [ "$new" != "$current" ]; then
            say "would raise the version in Cargo.toml and Cargo.lock to $new"
        else
            say "would keep the version $current"
        fi
        say "would date CHANGELOG.md's [Unreleased] as $new, $(date +%F)"
        say "would run make check, make notices, cargo deny, make build-release and try the binary"
        say "would commit 'chore(release): $new' and tag v$new"
        say "this text would be the tag message and the text of the release:"
        changelog_section Unreleased | sed 's/^/    /'
        if [ "${#PROBLEMS[@]}" -gt 0 ]; then
            say "but it would stop:"
            printf '    - %s\n' "${PROBLEMS[@]}"
            exit 1
        fi
        exit 0
    fi
    if [ "${#PROBLEMS[@]}" -gt 0 ]; then
        say "cannot release:"
        printf '    - %s\n' "${PROBLEMS[@]}" >&2
        exit 1
    fi

    trap restore_files EXIT
    if [ "$new" != "$current" ]; then
        set_cargo_version "$current" "$new"
    fi
    stamp_changelog "$new" "$(date +%F)" "$previous"
    run_checks "$new"

    git add Cargo.toml Cargo.lock CHANGELOG.md
    git commit --quiet -m "chore(release): $new"
    trap - EXIT
    local message
    message=$(mktemp)
    {
        echo "kiri $new"
        echo
        changelog_section "$new"
    } > "$message"
    git tag -a "v$new" -F "$message"
    rm -f "$message"

    say "committed and tagged v$new; nothing is pushed."
    say "Try it (kiri examples/), then push the commit and the tag together:"
    say "    git push --atomic origin master v$new"
    say "To take it back before pushing: make release-undo"
}

undo() {
    local tag subject
    tag=$(git describe --tags --exact-match HEAD 2> /dev/null || true)
    subject=$(git log -1 --format=%s)
    [[ $tag == v* ]] && [[ $subject == "chore(release): "* ]] \
        || die "HEAD is not a release commit with its tag, so nothing is undone"
    [ -z "$(git status --porcelain)" ] || die "the working tree is not clean"
    local status=0
    git ls-remote --exit-code --tags origin "refs/tags/$tag" > /dev/null 2>&1 || status=$?
    case $status in
        0) die "origin has $tag already: a pushed release is not undone" ;;
        2) ;; # origin answered, and has no such tag
        *) die "cannot ask origin whether it has $tag, so nothing is undone" ;;
    esac
    git fetch --quiet origin 2> /dev/null || die "git fetch origin failed"
    if git merge-base --is-ancestor HEAD origin/master; then
        die "the release commit is on origin/master already"
    fi
    git tag -d "$tag" > /dev/null
    git reset --hard --quiet HEAD~1
    say "took back $tag and its commit"
}

# What the release workflow checks about a tag, with the history it has.
verify() {
    local tag=$1 version
    [[ $tag == v* ]] || die "'$tag' is not a version tag"
    version=${tag#v}
    [[ $version =~ $SEMVER ]] || die "'$tag' is not a version tag such as v1.2.3"
    git rev-parse -q --verify "refs/tags/$tag" > /dev/null || die "there is no tag $tag"
    [ "$(cargo_version)" = "$version" ] \
        || die "the tag is $tag but Cargo.toml says $(cargo_version)"
    changelog_has_content "$version" || die "CHANGELOG.md has no text for $version"
    git rev-parse -q --verify refs/remotes/origin/master > /dev/null \
        || die "origin/master is not known: fetch the whole history"
    git merge-base --is-ancestor "$tag^{commit}" origin/master \
        || die "the commit of $tag is not on master"
    say "$tag matches Cargo.toml and CHANGELOG.md, and is on master"
}

notes() {
    local tag=$1 version
    version=${tag#v}
    changelog_has_content "$version" || die "CHANGELOG.md has no text for $version"
    changelog_section "$version"
}

dry_run_flag() {
    case ${1:-} in
        "") echo no ;;
        --dry-run) echo yes ;;
        *) die "unknown option '$1'" ;;
    esac
}

main() {
    local command=${1:-} new dry_run
    case $command in
        bump)
            [ $# -ge 2 ] || die "usage: release.sh bump patch|minor|major [--dry-run]"
            # Separate statements, so that a failure stops the script.
            new=$(bumped "$(cargo_version)" "$2")
            dry_run=$(dry_run_flag "${3:-}")
            release "$new" "$dry_run"
            ;;
        version)
            [ $# -ge 2 ] || die "usage: release.sh version X.Y.Z [--dry-run]"
            dry_run=$(dry_run_flag "${3:-}")
            release "$2" "$dry_run"
            ;;
        undo) undo ;;
        verify)
            [ $# -eq 2 ] || die "usage: release.sh verify vX.Y.Z"
            verify "$2"
            ;;
        notes)
            [ $# -eq 2 ] || die "usage: release.sh notes vX.Y.Z"
            notes "$2"
            ;;
        *) die "usage: release.sh bump|version|undo|verify|notes ... (see the top of the file)" ;;
    esac
}

main "$@"
