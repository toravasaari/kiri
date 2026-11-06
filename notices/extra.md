What kiri takes from elsewhere without a library of its own, and what the
Linux binaries are linked with.

### GitHub Linguist

The names that a code fence may give a language are a table generated from
the `languages.yml` of [GitHub Linguist](https://github.com/github-linguist/linguist).

@include licenses/linguist.txt

### Octicons

The icons of GitHub's alerts in reading mode are the Octicons of GitHub
([primer/octicons](https://github.com/primer/octicons)), under the MIT License.
Their outlines were taken from the Octicons in the Nerd Fonts symbols font
that kitty bundles; the
[licence audit](https://github.com/ryanoasis/nerd-fonts/blob/-/license-audit.md)
of the Nerd Fonts project lists its Octicons as MIT.

@include licenses/octicons.txt

### Themes for code

The colours of highlighted code come from the default themes of
[syntect](https://crates.io/crates/syntect). `base16-ocean.dark`,
`base16-ocean.light`, `base16-eighties.dark` and `base16-mocha.dark` are
the Base16 colour schemes of Chris Kempson, as in
[spacegray](https://github.com/kkga/spacegray) by Sublime Text Packages.
`InspiredGitHub` is
[Seth Lopez's theme](https://github.com/sethlopezme/InspiredGitHub.tmtheme),
and `Solarized (dark)` and `Solarized (light)` are
[braver/Solarized](https://github.com/braver/Solarized), after the colours of
Ethan Schoonover.

The licence of spacegray:

@include licenses/spacegray.txt

The licence of Base16:

@include licenses/base16.txt

The licence of InspiredGitHub:

@include licenses/inspired-github.txt

The licence of braver/Solarized:

@include licenses/solarized.txt

### musl libc

The Linux binaries are linked statically with [musl](https://musl.libc.org/),
whose code is then part of them. This is musl's `COPYRIGHT` file, of version
1.2.5.

@include licenses/musl.txt

### The Rust standard library

Every kiri binary holds Rust's standard library, which is under the MIT
License or the Apache License 2.0, at one's choice. The MIT License:

@include licenses/rust.txt

The standard library in turn contains a few other libraries; their licences
are in `COPYRIGHT-library.html` of the Rust distribution.
