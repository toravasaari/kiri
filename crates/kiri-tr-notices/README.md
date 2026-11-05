# kiri-tr-notices

A program for [kiri](../../README.md)'s third-party notices, not part of the
`kiri` binary. `make notices` (`scripts/notices.sh`) runs it to put together
the `THIRD-PARTY-NOTICES.md` that every release archive carries:

- the licences of the Rust libraries, from `cargo about`, with the libraries
  whose licence files it cannot read, or that carry notices beyond their
  licence, given in `notices/by-hand.toml`;
- the licences of the syntax definitions that kiri highlights code with,
  which [two-face](https://crates.io/crates/two-face) keeps as data that only
  a function of its own reads, which is why a program is needed;
- the hand-written parts in `notices/`: `header.md`, `extra.md` and the
  licence texts in `notices/licenses/`.

It stops with an error when a library is left with the placeholder text
that `cargo about` puts in for a missing licence file, and when
`notices/by-hand.toml` names a library kiri does not use any more, so that the
file cannot go stale unseen.
