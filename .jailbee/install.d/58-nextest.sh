#!/bin/bash
# 58-nextest — install cargo-nextest as the dev user, for `make test`.
# Installs: ${JAILBEE_USER_HOME}/.cargo/bin/cargo-nextest
#
# Built from source: nextest's prebuilt binaries come from GitHub, which the
# strict network does not reach, while crates.io does. Needs the cargo that
# 55-rustup installs.
set -euo pipefail

echo "==> Installing cargo-nextest as ${CONTAINER_USER}"
runuser -l "${CONTAINER_USER}" -c \
    "${JAILBEE_USER_HOME}/.cargo/bin/cargo install --locked cargo-nextest"

test -x "${JAILBEE_USER_HOME}/.cargo/bin/cargo-nextest"
