# Changelog

The notable changes of each release of kiri. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the version
numbers follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html):
kiri stays at 0.x until its command line settles.

## [Unreleased]

### Added

- The HTML heads of README files: centred titles, logos and link rows, `<a>`
  links, and `<u>`, `<span>`, `<ins>` and `<strike>` in text. Two settings
  choose how much HTML is read and whether unread tags show.
- Footnotes, shown at the end of the document with links in both directions.
- Ctrl with the mouse wheel zooms in reading mode.
- With several files, a setting keeps the current file's name at the top of
  the pager as you scroll.

### Changed

- Headings in text mode are a little larger with the default themes.
- Horizontal rules are narrowed to the width of the text.
- The header above each of several files spans the whole window and has the
  status line's colour; the theme key `bar_background` sets both.
- HTML in Markdown is read more as a browser reads it: a plain `<div>` or `<p>`
  is its text, not source, and a heading inside a `<summary>` is read.

### Fixed

- Nested or stray tags in a paragraph no longer crash kiri or slow it down.

## [0.3.1] - 2026-11-23

This release is about speed: reading mode prepares the page far quicker and
eliminates any stutter almost completely. This is accomplished by a few small
improvements:

| Area | Old time (ms) | New time (ms)  | Method  |
|---|---|---|---|
| Rendering  | 27.6  | 7.0  | glyph cache and in-place conversion of the canvas  |
| Image id  | 6.1  | 2.4  | Id is computed 8 bytes at a time  |
| Compression and base64  | 32.1  | ~4  | changed to zlib-rs library, compression level 2 |
| **Total**  | **65.6**  | **~14**  |   |

Times are for one part of a page (about 1480 x 1290 px), numbers are measured
on a few-years-old laptop.

### Changed

- Images are prepared faster for a terminal. The environment variable
  `KIRI_IMAGE_COMPRESSION` can be used to tweak the compression level (0-9):
  1 is fast but sends more data, while 9 is slow but sends less data. The
  default is 2. And 0 does not compress at all.
- Image compression uses `zlib-rs` instead of `miniz_oxide`.
- Updated dependencies, among them comrak for Markdown parsing and parley and
  resvg for text and SVG.

## [0.3.0] - 2026-11-21

This release adds a table of contents, zoom in reading mode and support for
large files, tested up to 1 million lines.

### Changed

- Headings in the text layout are smaller and line up with the text. A theme
  can bring the old sizes back.
- Opaque images take a quarter less memory in the terminal.

### Added

- Table of contents in the pager, shown in a window of 110 columns or more:
  - `T` shows and hides it, and `t` moves the focus to it. The keyboard then
    moves through the headings, folds them and jumps to one.
  - The mouse works on it: click a heading, scroll, and drag the edge to
    change the width.
  - It is drawn in the theme's colours, and shows when it has the focus.
  - `--toc` sets whether it starts shown or hidden, and a setting can make it
    move the document along with its selection.
  - With several files open, it sets each file apart.
- Zoom in reading mode: its text and pictures larger or smaller, as a
  browser zooms a page, with `z` and `Z` in the pager or `--zoom` for good.
- Experimental: headings can start on whole terminal rows in reading mode, so
  that a jump to a heading lands on it exactly.
- Scrolling a page in Ghostty, foot and xterm moves the screen instead of
  drawing it again.
- A large heading that does not fit the rows left at the bottom of the screen
  is drawn smaller there, and grows as it scrolls up.
- A large log or text file opens quickly, highlighted too, and its colours
  come where you read, a moment after the text. Searching a large file is
  faster and takes less memory.
- The pager shows every row of a CSV or TSV file. A very long Markdown table
  is laid out with the widths its first rows ask for, until `:w` fits it to
  all of them.
- A followed log or CSV that grows reloads faster, and the colours of a
  followed log stay.
- `image_memory` sets how much image memory the pager keeps in the terminal,
  128 MB by default.

### Fixed

- Reading mode keeps the text at the top of the screen when the page is laid
  out again, as after a change of the window's size, instead of moving to
  other text in the same paragraph.
- A very tall image no longer takes more rows than the window leaves for it.
- Reading mode no longer crashes on a large highlighted file, such as a log
  of many thousand lines.
- Reading mode no longer cuts a long Markdown document off.
- With `--width`, the help panel is no longer narrowed to the text column.
- Reading mode no longer leaves parts blank after a long scroll, when the
  terminal has dropped their images.

## [0.2.2] - 2026-11-16

This release adds following links and small fixes.
Focus has been to refactor and improve the pager and the session.

### Added

- Navigation for links within the document and to other documents, also
  between several files opened at once:
  - Enter or a mouse click follows a link.
  - Backspace, Alt+← or a click on the status line's breadcrumbs goes back.
  - Alt+→ goes forward.
  - The mouse's back and forward buttons work too, if the terminal sends them.

### Changed

- The status line shows the title of the selected link when it has one.
- Search looks in the document's text, so it finds a term in a table cell, in
  the front matter and across the end of a line, with the same matches in both
  layouts, and does not find what kiri adds, such as list numbers, the kind of
  an alert, or the alt text of a picture that is drawn.
- Following on auto tints only the block that changed in a list, a quote or an
  alert, not the whole of it.
- The help is a document with a heading for each topic, which opens at the
  topic for what you are doing, scrolls in a small window, and closes with
  Esc, `?` or `q`; `q` no longer quits while it is open.

### Fixed

- A very long document opens in reading mode in a fraction of the time: 400 000
  lines in 15 seconds, not two minutes.
- kiri no longer hangs as it ends when the terminal has stopped reading, such
  as after Ctrl-S.
- An animation in reading mode plays in a large font or a wide window too.
- A code block that only moves down the page no longer makes reading mode draw
  it again, and a reload keeps its scroll and the selected link when something
  is added above them.
- A reload keeps the current search match when a match is added above it.
- A reload keeps the selected link selected, also when a block is added above
  it.
- The tint of a change stays when the window is resized or the layout changes
  with `M`, `W` or `B`, and a resize keeps the line at the top of the window.

## [0.2.1] - 2026-11-10

This fix release focuses on fixing small quirks and speeding up tmux
usage.

No macOS binaries in this release, but a few fixes for macOS users to allow experimentation. Installation via `cargo install --path .`.
Cargo can be installed e.g. via https://rustup.rs, linker via Xcode Command Line Tools (`xcode-select --install`).

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
- On macOS, kiri no longer waits for a key press before it shows the page.

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

[Unreleased]: https://github.com/toravasaari/kiri/compare/v0.3.1...HEAD
[0.3.1]: https://github.com/toravasaari/kiri/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/toravasaari/kiri/compare/v0.2.2...v0.3.0
[0.2.2]: https://github.com/toravasaari/kiri/compare/v0.2.1...v0.2.2
[0.2.1]: https://github.com/toravasaari/kiri/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/toravasaari/kiri/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/toravasaari/kiri/releases/tag/v0.1.0
