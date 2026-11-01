# jailbee configuration

This directory configures [jailbee](https://jailbee.gisgro.io), a tool that
runs a development environment in an isolated Incus container, one per git
branch. Kiri uses it for coding agents that work on a branch without access
to the rest of the machine.

With jailbee installed and set up, `jailbee new feature/topic master` creates
a container with the Rust toolchain, kitty and the other build tools that
`config.yaml` and `install.d/` ask for, and `jailbee shell` opens a shell in
it. The container may reach crates.io and the rustup servers only, not
GitHub. Commits come back to the host with `jailbee git fetch`, and
`jailbee destroy` removes the container. You do not need jailbee to build or
develop kiri; `make check` works on any machine with a Rust toolchain.
