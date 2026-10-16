#!/bin/bash
# 55-rustup — install the Rust toolchain through rustup as the dev user.
# Env: CONTAINER_USER, JAILBEE_USER_HOME
# Installs: ${JAILBEE_USER_HOME}/.cargo/bin/{cargo,rustc,rustfmt,cargo-clippy}
#
# rustup rather than Ubuntu's cargo package: rust-toolchain.toml pins the
# channel and the rustfmt and clippy components that `make check` runs.
set -euo pipefail

echo "==> Installing rustup (stable, rustfmt, clippy) as ${CONTAINER_USER}"
runuser -l "${CONTAINER_USER}" -c \
    "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --component rustfmt,clippy"

test -x "${JAILBEE_USER_HOME}/.cargo/bin/cargo"
