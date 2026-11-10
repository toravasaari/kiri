# Changelog

The notable changes of each release of kiri. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the version
numbers follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html):
kiri stays at 0.x until its command line settles.

## [Unreleased]

### Changed

- Animation support for tmux w/ kitty.

### Fixed

- Scrolling in reading mode inside tmux no longer stutters or loses images.
- Ghostty on Wayland no longer closes while kiri sends images through tmux.
- An animated image with many frames or a broken one no longer hangs kiri.
- A table with superscript or subscript text no longer crashes in a narrow
  window.
- Ctrl-C works while kiri closes background programs after the pager.
- Starting a second animation no longer makes the play button of the first one
  flicker.
- An animation that plays again no longer shows the pictures beside it as they
  were when it first played.
- `p` starts the animation of the pill again after the window has been
  resized.

### Security

- Warnings no longer pass control characters from a document on to the
  terminal.
- YAML front matter whose aliases multiply no longer takes all the memory.

## [0.2.0] - 2026-11-08

### Added

- `--doctor` tells which image protocols the terminal answered to, sixel among them.
- Sixel terminal support for images, formulas and diagrams, with up to 24-bit
  colours, in the pager and in printed output. Tested with foot and xterm
  (`-ti vt340`).
- HTML tags: `<b>`, `<i>`, `<s>` and `<code>` in text, `<h1>`-`<h6>` and `<pre>` code blocks.

### Changed

- When only part of the screen changes, such as the status line or an
  animation, the pager rewrites only those rows.
- Kitty image ids are 32 bits instead of 24, so two images in one window
  rarely share an id.

### Fixed

- The help panel and the warning line no longer break apart over the lower
  half of a large heading.

## [0.1.0] - 2026-11-06

The first public release.

### Added

- Most of the Markdown features implemented for the kitty and Ghostty terminals,
  also inside tmux.
- Images through the kitty graphics protocol: PNG, JPEG, GIF, WebP and SVG,
  and in the default build also BMP, ICO, TIFF, QOI, PNM and TGA.
- Animated GIF, WebP and APNG images that play when you start them, with a
  click or with Tab and `p`. Tab and Shift+Tab move between animations and
  links.
- Mermaid diagrams and LaTeX formulas.
- CSV and TSV files as tables, and other files as text, highlighted when kiri
  knows the language.
- Reading mode, a browser-like view.
- A pager for long documents, with search, several files and following a file
  as it changes.
- Themes with `--theme`, `--light` and `--dark`, and `NO_COLOR`.
- A configuration file, environment variables, `--doctor` for a report on the
  terminal and the settings kiri uses, and shell completions for bash, zsh,
  fish, elvish, PowerShell and nushell.
- Plain text instead of escape sequences when the output is a pipe.

[Unreleased]: https://github.com/toravasaari/kiri/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/toravasaari/kiri/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/toravasaari/kiri/releases/tag/v0.1.0
