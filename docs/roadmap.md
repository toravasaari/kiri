# Roadmap

kiri grows in phases. Each phase leaves the project in a working state, and
review and refactoring are part of every phase rather than a phase of their
own. Later phases may be reordered or split as the work shows what matters.

## Phase 1 – Foundations

- README, this roadmap, [architecture](architecture.md), [AGENTS.md](../AGENTS.md).
- A walking skeleton: Cargo project, `make check` (rustfmt, clippy, tests),
  one snapshot test, and a jailbee configuration for isolated agent work.

## Phase 2 – Print mode (proof of concept)

`kiri file.md` renders the document to standard output and exits, like
`kitten icat`.

- Terminal capability detection: cell size in pixels, kitty graphics and text
  sizing support, with a plain ANSI fallback.
- Paragraphs with word wrapping, emphasis, inline code, block quotes, lists,
  thematic breaks.
- Headings drawn larger with the text sizing protocol (OSC 66).
- Fenced code blocks with syntax highlighting.
- Tables drawn with box-drawing characters.
- Links as OSC 8 hyperlinks.
- A theme file for colours and heading sizes.

## Phase 3 – Project hygiene

- GitHub repository, CI running `make check`, cargo-deny, Dependabot.
- Semantic versioning (0.x until the command line settles), a changelog in
  Keep a Changelog format, and release binaries.
- Documented minimum versions for kitty and Rust.
- A `render-check` skill that renders the fixture documents in kitty and
  captures a screenshot for review.
- License.

## Phase 4 – Rich content and the pager

- Images (PNG, JPEG, SVG) through the graphics protocol, placed with Unicode
  placeholders so they scroll with the text.
- Formulas (`$…$`, `$$…$$`) rendered to images through Typst's math engine.
- Mermaid diagrams through an external renderer when one is installed.
- A cache for rendered images.
- Links: relative links become `file://` URLs, so kitty's `open-actions.conf`
  can open Markdown targets in kiri.
- Interactive pager (`kiri -i`): scrolling, search, a table of contents,
  link selection, anchor jumps, back history, reload on file change.
- Pager status line with key hints and a `?` help panel.
- Other input formats (reStructuredText, Org, AsciiDoc), possibly through
  pandoc.

## Phase 5 – Beyond kitty

- Other terminals: sixel and iTerm2 images, plain styling for headings.
- An HTML backend and `kiri serve` for reading in a browser with live reload.
- Pixel rendering: a headless browser screenshot (`--browser`), and possibly
  a native layout and rasterising engine later.
- Editing support: an editor and kiri side by side in kitty with live reload,
  and a key to open the editor at the current position.
