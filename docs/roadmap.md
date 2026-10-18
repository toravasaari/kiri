# Roadmap

kiri grows in phases. Each phase leaves the project in a working state, and
review and refactoring are part of every phase rather than a phase of their
own. Later phases may be reordered or split as the work shows what matters.

Larger phases are split into numbered parts, each marked done on its own.
The numbers name the parts rather than fix their order: images (4.1), for
example, come before the rest of phase 3.

## Phase 1 – Foundations (done)

- README, this roadmap, [architecture](architecture.md), [AGENTS.md](../AGENTS.md).
- A walking skeleton: Cargo project, `make check` (rustfmt, clippy, tests),
  one snapshot test, and a jailbee configuration for isolated agent work.

## Phase 2 – Print mode (done)

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

### 3.1 License

- Choose a license and add it to the repository.

### 3.2 Repository and CI

- GitHub repository, CI running `make check`, cargo-deny, Dependabot.

### 3.3 Releases

- Semantic versioning (0.x until the command line settles), a changelog in
  Keep a Changelog format, and release binaries.
- Documented minimum versions for kitty and Rust.

### 3.4 Render check

- A `render-check` skill that renders the fixture documents in kitty and
  captures a screenshot for review. Built together with images (4.1), which
  are the first output that needs a visual check.

## Phase 4 – Rich content and the pager

### 4.1 Images

- Local images through the graphics protocol, placed with Unicode
  placeholders so they scroll with the text.
- Formats: PNG, JPEG, GIF (first frame), WebP and SVG, plus the other
  formats the image decoders support cheaply.
- A paragraph that holds only an image is drawn as a block; images inside
  running text stay as alt text for now.
- Sizes follow the browser: CSS pixels, never upscaled, no wider than the
  text. Options for one image pixel per terminal pixel, and for fitting the
  window like `kitten icat`.
- `kiri image.png` shows an image on its own.
- HTML `<img>` tags with `src`, `alt`, `width` and `height`.
- Cargo features for a minimal build and a full build with every format.
- Groundwork: one query for the window size in cells and pixels. Fitting
  the window needs its height, and `term.rs` now asks for the width and
  the cell size separately.

### 4.2 Formulas

- `$…$` and `$$…$$` in LaTeX syntax, as GitHub Markdown writes them, and in
  Typst syntax, rendered to images through Typst's math engine.
- A cache for rendered images.
- Groundwork, before formulas and diagrams are worked on side by side:
  images that come from a block rather than a URL. `Images` maps URLs to
  images; formulas and diagrams need keys of their own and one cache.
- Groundwork: `render/layout.rs` split by block type (lists, quotes, code)
  before the next block type joins it, formulas or GitHub alert blocks,
  whichever comes first.

### 4.3 Diagrams

- Mermaid diagrams through an external renderer when one is installed.

### 4.4 Animation and video

- Animated GIF, WebP and APNG, played by kitty itself.
- Video (WebM, MP4, AVI and others) through ffmpeg when it is installed.
- Groundwork: what has been sent to the terminal, and under which image
  id, moves from `render/output.rs` behind an image encoder. Animation
  frames need that state, and the other image protocols of phase 5 fill
  it differently.

### 4.5 Links

- Relative links become `file://` URLs, so kitty's `open-actions.conf` can
  open Markdown targets in kiri.

### 4.6 Pager

- Interactive pager (`kiri -i`): scrolling, search, a table of contents,
  link selection, anchor jumps, back history, reload on file change.
- Pager status line with key hints and a `?` help panel.
- Groundwork: raw mode, now inside the probe in `term.rs`, usable for
  reading keys.

### 4.7 Other input formats

- reStructuredText, Org, AsciiDoc, possibly through pandoc.

## Phase 5 – Beyond kitty

- Other terminals: sixel and iTerm2 images, plain styling for headings.
- Groundwork: `Capabilities.graphics` names the image protocol instead of
  a yes or no, and the probe keeps the terminal's name, version and device
  attributes, where sixel support shows, instead of only yes-or-no
  answers.
- Images as coloured Unicode block characters where no image protocol
  works.
- Remote images, off by default because fetching one tells its server that
  the document was opened.
- An HTML backend and `kiri serve` for reading in a browser with live reload.
- Pixel rendering: a headless browser screenshot (`--browser`), and possibly
  a native layout and rasterising engine later.
- Editing support: an editor and kiri side by side in kitty with live reload,
  and a key to open the editor at the current position.

## Extras

Smaller polish that fits in whenever there is time, in any phase.

- A fenced code block with a language, such as `python`, shows the
  language name in its top-right corner, on a background colour of its own
  (a new theme key). With code padding on, the name sits on the top
  padding row; without it, on a line above the block.
- AVIF and JPEG XL images.
- An image inside a link can be clicked: the image's placeholder cells are
  wrapped in the link's OSC 8 hyperlink, as text is.
- Less image data over SSH. kitty only takes raw pixels or PNG, so
  compressed formats such as JPEG cannot be passed through as they are;
  this needs thought on what to send and when.
- Images inside table cells, which now show their alt text.
- Table columns share the width by their content, as in a browser: a
  table too wide for the terminal narrows each column in proportion to
  its text, instead of narrowing the widest column first. (done)
- The paper themes can be changed with `theme.light.toml` and
  `theme.dark.toml` in the configuration directory, which `--light` and
  `--dark` read on top of the built-in paper themes.
- GitHub alert blocks, quotes that start with `[!NOTE]`, `[!TIP]`,
  `[!IMPORTANT]`, `[!WARNING]` or `[!CAUTION]`, drawn with a colour and an
  icon of their own and a title such as "Note". The colours stay even in
  the paper themes, which are otherwise grey.

## Bugs

Known bugs, fixed in whatever phase is under way. None at the moment.
