# Changelog

The notable changes of each release of kiri. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the version
numbers follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html):
kiri stays at 0.x until its command line settles.

## [Unreleased]

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

[Unreleased]: https://github.com/toravasaari/kiri/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/toravasaari/kiri/releases/tag/v0.1.0
