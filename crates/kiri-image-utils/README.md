# kiri-image-utils

Image code for [kiri](../../README.md) that debug builds optimise: one
function that scales 8-bit RGBA pixels with a Catmull-Rom filter, through
[fast_image_resize](https://crates.io/crates/fast_image_resize).

It is a crate of its own so that kiri's debug builds can optimise it.
Most of fast_image_resize is generic, so its code is compiled in the crate
that uses it. Here that is this crate, which the workspace's
`[profile.dev.package]` builds with `opt-level = 3`, while kiri's own code
stays easy to debug.
