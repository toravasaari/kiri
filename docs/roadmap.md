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
- Release binaries without symbols (`strip = true` in the release profile),
  about a fifth smaller.
- Shell completions and a man page, generated from the command-line
  definition.

### 3.4 Render check (done)

- A `render-check` skill that renders the fixture documents in kitty and
  captures a screenshot for review. Built together with images (4.1), which
  are the first output that needs a visual check.

## Phase 4 – Rich content and the pager

### 4.1 Images (done)

- Local images through the graphics protocol, placed with Unicode
  placeholders so they scroll with the text.
- Formats: PNG, JPEG, GIF (first frame), WebP and SVG, plus the other
  formats the image decoders support cheaply: BMP, ICO, TIFF, QOI, PNM and
  TGA.
- A paragraph that holds only an image is drawn as a block; images inside
  running text stay as alt text for now.
- Sizes follow the browser: CSS pixels, never upscaled, no wider than the
  text. Options for other sizes wait until the defaults have been tried;
  one for an image on its own is in Extras.
- `kiri image.png` shows an image on its own, fitting the window like
  `kitten icat`.
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
  whichever comes first. (done)

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
- A Markdown target opens in a kitty overlay window over the document
  (`kitty @ launch --type=overlay`), so closing it returns the reader to
  where they were.

### 4.6 Pager

An interactive pager, so that a document can be read on a screen of its
own and leaves the shell's scrollback as it was, as `bat` does through
`less`. It comes in parts, so that the basic pager can be used early.

#### 4.6.1 Basic pager

- The document on the terminal's alternate screen; quitting returns to
  the prompt as it was.
- `--paging=always|auto|never`, with `-p` for `always` and `-P` for
  `never`, the `KIRI_PAGING` environment variable (`1` is `always`, `0`
  is `never`) and a `paging` key in the configuration file. `auto`, the
  default, pages a document longer than the window when the output is a
  terminal, as `bat` does; a shorter document is printed as before. A
  document from standard input and the `--doctor` report are paged only
  with `always`.
  `--doctor` shows the choice and where it comes from.
- Scrolling by line and by page, to the start and to the end, with the
  keys of `less` for now, and with the mouse wheel. The keys are
  provisional until they have been tried in daily use.
- Large headings scroll in and out whole, as the text sizing protocol
  draws them only from their top row.
- Keys come from the terminal rather than standard input, as in `less`,
  so a piped document can be paged too.
- The document is laid out again when the window changes size.
- Groundwork: raw mode, now inside the probe in `term.rs`, usable for
  reading keys.

#### 4.6.2 Search and status line

- Search with `/`, `n` and `N`.
- A status line with the position and key hints, and a `?` help panel.
- Keys and terminal answers that arrive split over two reads are
  decoded whole. Now the second half of such a sequence is read as keys
  of its own, which a search prompt would make worse.

#### 4.6.3 Reload

- Reload on file change, and a key that jumps to the first block that
  changed.
- A follow mode for a document another program is writing, such as one
  Claude is working on in the next window: it reloads on every change and
  shows the part that changed, without a key press.

#### 4.6.4 Moving around

- A table of contents, anchor jumps and back history.

#### 4.6.5 Link selection

- Choosing a link with the keyboard and following it; a Markdown target
  opens in an overlay window, as in 4.5.

### 4.7 Other input formats

- reStructuredText, Org, AsciiDoc, possibly through pandoc.
- Jupyter notebooks (`.ipynb`): Markdown cells, code cells with syntax
  highlighting, and their outputs, images included.
- CSV and TSV files, shown as a table.

### 4.8 Diagnostics (done)

- `kiri --doctor` shows what kiri found and what it uses: the terminal's
  answers (cell and window size, graphics, text sizing, colours), each
  setting with where it came from (default, configuration file,
  environment variable or command-line flag), the configuration file
  paths and the image formats this build decodes.
- The external tools later parts need (Typst, a Mermaid renderer, ffmpeg,
  pandoc) join the report with the part that uses each, saying whether it
  was found.
- The report is a Markdown document that kiri renders itself, so it also
  shows how headings, tables and images look in this terminal. Piped to a
  file it stays plain Markdown, ready to paste into a bug report.

### 4.9 Standard input and directories (done)

Comes before the pager (4.6), so that the pager is built for a piped
document from the start.

- `kiri -`, or no file name with a pipe on standard input, reads the
  document from standard input, such as the output of another command.
  Relative image paths are resolved against the current directory.
- `kiri some/dir` shows the directory's `README.md`, or else its
  `README`, as GitHub does.
- The pager (4.6.1) reads its keys from the terminal rather than from
  standard input, as `less` does, so a piped document can be paged too.

### 4.10 More Markdown

GitHub's Markdown first, then what is common elsewhere.

- Footnotes (`[^1]`), listed at the end of the document as in a browser,
  with links from each reference to its note and back.
- YAML front matter, as in Jekyll, Hugo and Obsidian files, is not shown
  as text; its title and date may head the document.
- `<details>` and `<summary>`: open in print mode, with the summary marked
  by a triangle; the pager can open and close them.
- Small HTML tags: `<kbd>`, `<br>`, `<sub>`, `<sup>` and `<mark>`.
  Subscripts, superscripts and footnote references are drawn smaller,
  lowered or raised, with the text sizing protocol's fractional scale.
- Emoji shortcodes such as `:rocket:`, as GitHub shows them.
- Heading anchors as GitHub makes them, so a link such as `[see](#install)`
  leads to its heading; the pager jumps there (4.6).
- A base for Obsidian notes: wikilinks, `[[note]]` and `[[note|text]]`,
  as links to `note.md`, and embeds such as `![[image.png]]` as images.
  Obsidian's callouts share the syntax of GitHub alert blocks (Extras).

## Phase 5 – Beyond kitty

- Other terminals: sixel and iTerm2 images, plain styling for headings.
- Groundwork: `Capabilities.graphics` names the image protocol instead of
  a yes or no, and the probe keeps the terminal's name, version and device
  attributes, where sixel support shows, instead of only yes-or-no
  answers.
- `--doctor` lists the image protocols probed and the terminal's name and
  version.
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
  language name in its top-right corner, on the top padding row in a
  colour between the code's text and background. Without code padding,
  as with `--no-margins`, the name is left out. (done)
- AVIF and JPEG XL images.
- PDF documents shown as images, as a quick look that saves opening a
  PDF reader rather than a replacement for one: `kiri file.pdf` and
  `![](figure.pdf)` in a document show the first page. A setting and a
  command-line option choose other pages: the first n pages, one page, a
  list of ranges such as `1-3,7,10-`, or all pages. How the pages are
  rendered is still open.
- An image inside a link can be clicked: the image's placeholder cells are
  wrapped in the link's OSC 8 hyperlink, as text is.
- Less image data over SSH. kitty only takes raw pixels or PNG, so
  compressed formats such as JPEG cannot be passed through as they are;
  this needs thought on what to send and when.
- Images inside table cells, which now show their alt text.
- An `<img>` set apart by blank lines inside a wrapper, such as
  `<p align="center">`, blank line, `<img>`, blank line, `</p>`, is drawn,
  but the wrapper's tags show as source: CommonMark makes three HTML
  blocks of it.
- An image shown on its own, `kiri image.png`, can be shown at other sizes
  than the default fit to the window: one image pixel per terminal pixel
  even when that is larger than the window, its size in CSS pixels as in a
  document, or enlarged to fill the window.
- Code blocks can be copied to the clipboard (OSC 52): in the pager with a
  key, and in print mode through a link beside the block that kitty's
  `open-actions.conf` hands to kiri.
- Changes shown as a diff, added text green and removed text red, fading
  out slowly: in the pager after a reload, and against a git revision.
- Table columns share the width by their content, as in a browser: a
  table too wide for the terminal narrows each column in proportion to
  its text, instead of narrowing the widest column first. (done)
- The paper themes can be changed with `theme.light.toml` and
  `theme.dark.toml` in the configuration directory, which `--light` and
  `--dark` read on top of the built-in paper themes.
- GitHub alert blocks, quotes that start with `[!NOTE]`, `[!TIP]`,
  `[!IMPORTANT]`, `[!WARNING]` or `[!CAUTION]`, drawn with a colour and an
  icon of their own and a title such as "Note". The colours stay even in
  the paper themes, which are otherwise grey. (done)
- Headings marked as shell prompts (OSC 133), so kitty's keys for jumping
  between prompts move between headings in print mode, without the pager.
  Needs a try first: kitty may treat the marks as more than places to
  jump to.
- Heading scales with fractions, such as 1.5 for `h4`, drawn with the text
  sizing protocol's fractional scale. Scales are whole numbers today, so
  `h4` to `h6` are either the size of body text or at least twice it.
- `--toc` puts a table of contents with links at the top of the document
  in print mode.

## Bugs

Known bugs, fixed in whatever phase is under way.

- An image shown on its own, `kiri image.png`, leaves an empty row below
  it in the maintainer's kitty, so the image is a row shorter than it
  could be and a two-line prompt scrolls its top row away. `kitten icat`
  does the same there. It does not happen in kitty on a virtual display,
  so the window and cell sizes the terminal reports are the first
  suspect. Once it is understood, the rows left free below the image may
  become a setting.
