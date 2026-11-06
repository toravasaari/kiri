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
  and in the default build also BMP, ICO, TIFF, QOI, PNM and TGA.
- Animated GIF, WebP and APNG images that you start yourself: a click, or Tab
  and `p`, plays one, and one plays at a time. A stopped one shows its first
  frame with a play button, a finished one its last with a replay icon, and a
  pill on the status line shows what plays and how far it has come. How many
  times an animation plays is the `animation_plays` setting. They play in kitty
  and Ghostty, and show their first frame in tmux, when printing and where the
  terminal cannot show images.
- Tab and Shift+Tab choose an animation as well as a link, walk through the
  whole document and bring the chosen one into view, in the middle when it was
  out of it.
- Mermaid diagrams and LaTeX formulas.
- CSV and TSV files as tables. Other files are shown as text, highlighted when
  kiri knows the language: only a Markdown extension makes a file Markdown,
  and `--format text` shows a Markdown file as its source. Standard input that
  is JSON, a diff or a script is shown as text too.
- Reading mode.
- A pager for long documents, with search, several files, reload (`r`),
  following a file changes and links chosen with the keyboard. Anything that
  is longer than the window is paged, images and diagrams too: `kiri *.png`
  opens the pager when the images together take more than a screen.
- Themes with `--theme`, `--light` and `--dark`, and `NO_COLOR`.
- A configuration file and environment variables, `--doctor` for what kiri
  finds out about the terminal, and shell completions for bash, zsh, fish,
  elvish, PowerShell and nushell.
- Plain text instead of escape sequences when the output is a pipe.
