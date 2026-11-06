# Changelog

The notable changes of each release of kiri. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the version
numbers follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html):
kiri stays at 0.x until its command line settles.

## [Unreleased]

The first public release.

### Added

- Most of the Markdown features implemented for the kitty and Ghostty terminals,
  also inside tmux.
- Images through the kitty graphics protocol: PNG, JPEG, GIF, WebP and SVG,
  and in the default build also BMP, ICO, TIFF, QOI, PNM and TGA. An animated
  image shows its first frame.
- Mermaid diagrams and LaTeX formulas.
- CSV and TSV files as tables. Other files are shown as text, highlighted when
  kiri knows the language: only a Markdown extension makes a file Markdown,
  and `--format text` shows a Markdown file as its source. Standard input that
  is JSON, a diff or a script is shown as text too.
- Reading mode.
- A pager for long documents, with search, several files, reload (`r`),
  following a file changes and links chosen with the keyboard.
- Themes with `--theme`, `--light` and `--dark`, and `NO_COLOR`.
- A configuration file and environment variables, `--doctor` for what kiri
  finds out about the terminal, and shell completions for bash, zsh, fish,
  elvish, PowerShell and nushell.
- Plain text instead of escape sequences when the output is a pipe.
