# kiri-image-utils

Image code for [kiri](../../README.md) that debug builds optimise: two
functions that decode raster images into 8-bit RGBA through
[image](https://crates.io/crates/image), in the format the bytes name or in
a given one, and one that scales such pixels with a Catmull-Rom filter,
through
[fast_image_resize](https://crates.io/crates/fast_image_resize).

It is a crate of its own so that kiri's debug builds can optimise it.
Some of image's decoders and most of fast_image_resize are generic, so
their code is compiled in the crate that uses them. Here that is this
crate, which the workspace's `[profile.dev.package]` builds with
`opt-level = 3`, while kiri's own code stays easy to debug.
