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
  one for an image on its own is in 4.12.5.
- `kiri image.png` shows an image on its own, fitting the window like
  `kitten icat`.
- HTML `<img>` tags with `src`, `alt`, `width` and `height`.
- Cargo features for a minimal build and a full build with every format.
- Groundwork: one query for the window size in cells and pixels, as
  fitting the window needs its height. `term.rs` used to ask for the width
  and the cell size separately.

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

#### 4.3.1 Mermaid

- Mermaid diagrams through an external renderer when one is installed.

#### 4.3.2 Other diagram languages

- Other diagram languages that a renderer turns into an image, in the way
  Mermaid works, such as Graphviz (`dot`), PlantUML, D2 and Pikchr.

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

#### 4.6.1 Basic pager (done)

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
- Groundwork: raw mode, until then inside the probe in `term.rs`, made
  usable for reading keys.

#### 4.6.2 Status line (done)

- A status line with the file name, the position and key hints, and a
  `?` help panel.
- Keys and terminal answers that arrive split over two reads are
  decoded whole. Before, the second half of such a sequence was read as
  keys of its own, which a search prompt would have made worse.

#### 4.6.3 Search (done)

- Search with `/`, `n` and `N`; the status line shows the search term.
- Later: search with regular expressions, beside the plain text search.

#### 4.6.4 Reload

- Reload on file change, and a key that jumps to the first block that
  changed.
- A follow mode for a document another program is writing, such as one
  Claude is working on in the next window: it reloads on every change and
  shows the part that changed, without a key press.
- Changes shown as a diff, added text green and removed text red, fading
  out slowly: after a reload, and against a git revision.

#### 4.6.5 Moving around

- A table of contents, anchor jumps and back history.
- In print mode, without the pager: `--toc` puts a table of contents with
  links at the top of the document.
- Headings marked as shell prompts (OSC 133), so kitty's keys for jumping
  between prompts move between headings in print mode. Needs a try first:
  kitty may treat the marks as more than places to jump to.

#### 4.6.6 Link selection

- Choosing a link with the keyboard and following it; a Markdown target
  opens in an overlay window, as in 4.5.

### 4.7 Other input formats

#### 4.7.1 Formats

- reStructuredText, Org, AsciiDoc, possibly through pandoc.
- Jupyter notebooks (`.ipynb`): Markdown cells, code cells with syntax
  highlighting, and their outputs, images included.
- CSV and TSV files, shown as a table. A file of more than 100 000 cells
  shows its first rows and a note of how many were left out. (done)

#### 4.7.2 Large files

- Lazy layout, so that a large file, such as a CSV export, opens at once
  and takes little memory: only the source and where each line starts are
  kept, and the lines near the view are laid out when they are shown.
  Today every line is laid out up front, about 900 bytes for each table
  cell.
- Search over the source rather than the laid-out lines, which also finds
  a phrase broken over two lines.
- The position may be unknown until the end of the file has been read, as
  in `less`.
- With this in place, the cell limit for CSV and TSV goes or grows.

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

### 4.9 Input

Where documents come from, besides one file named on the command line.

#### 4.9.1 Standard input and directories (done)

Came before the pager (4.6), so that the pager was built for a piped
document from the start.

- `kiri -`, or no file name with a pipe on standard input, reads the
  document from standard input, such as the output of another command.
  Relative image paths are resolved against the current directory.
- `kiri some/dir` shows the directory's `README.md`, or else its
  `README`, as GitHub does.

#### 4.9.2 Several files

- Several files on the command line, as the shell expands `kiri *.md` or
  `kiri *.jpg`, shown one after another, each headed by its file name as
  `bat` does. Images are shown at full size; 4.12.3 adds a grid of them.
- In the pager, keys that move to the next and the previous file, as
  `:n` and `:p` do in `less`.

### 4.10 More Markdown

GitHub's Markdown first, then what is common elsewhere.

#### 4.10.1 Links within the document

- Heading anchors as GitHub makes them, so a link such as `[see](#install)`
  leads to its heading; the pager jumps there (4.6).
- Footnotes (`[^1]`), listed at the end of the document as in a browser,
  with links from each reference to its note and back.

#### 4.10.2 HTML tags

- `<details>` and `<summary>` are shown open, the summary in bold after a
  triangle, also nested and inside lists and quotes. (done)
- Small HTML tags: `<kbd>` drawn as inline code, `<br>` as a line break
  and `<mark>` in the theme's `mark` colours. (done)
- The pager can close and open a `<details>`.
- Tags inside a `<summary>`, such as `<b>`, `<i>` and `<code>`, which
  GitHub READMEs often write; they stay source today.
- Links written as HTML: a centred row of `<a>` tags, often around
  `<strong>`, in a `<p align="center">` below a README's title, as many
  GitHub READMEs and Hugging Face model cards have it. Such a block stays
  source today.

#### 4.10.3 Fractional sizes

Text drawn smaller or larger with the text sizing protocol's fractional
scale, built once for all of these:

- `<sub>` and `<sup>`, drawn smaller and lowered or raised.
- Footnote references, drawn smaller and raised.
- Heading scales with fractions, such as 1.5 for `h4`. Scales are whole
  numbers today, so `h4` to `h6` are either the size of body text or at
  least twice it.

#### 4.10.4 Front matter (done)

- Front matter at the very start of a Markdown file: `---` YAML, `+++`
  TOML, `;;;` JSON, or a language after the delimiter as GitLab writes it
  (`---php`). Shown as GitHub's table (keys in a header column, nested
  maps as tables three levels deep, lists of maps as stacked tables,
  deeper values as highlighted YAML), as GitLab's code block, hidden, or
  not recognised: `--front-matter`, `KIRI_FRONT_MATTER`, `front_matter`.
- Table rows told apart, for every table: `--table-rows zebra` (the
  default; every second row shaded from the page and text colours,
  nothing where that cannot show), `lines`, `indent`, `underline` or
  `off`, also as `KIRI_TABLE_ROWS` and `table_rows`.
- Code token colours made readable (WCAG 4.5:1) in every code theme.
- Later: its title and date heading the document.

#### 4.10.5 Emoji shortcodes (done)

- Emoji shortcodes such as `:rocket:`, as GitHub shows them.

### 4.11 More themes

- Built-in themes chosen by name, as btop offers them: `--theme nord`,
  `KIRI_THEME` or `theme` in the configuration file. The light and dark
  paper themes, greys with colour only in code blocks, are two of them;
  `--light` and `--dark` stay as short forms of `--theme light` and
  `--theme dark`.
- Themes live in `themes/` in the configuration directory, one file per
  name: `default.toml`, `dark.toml` and so on. How the built-in themes'
  files get there is part of this step.
- `--theme-file FILE`, `KIRI_THEME_FILE` or `theme-file` in the
  configuration file replaces today's `--theme FILE`. The file changes
  only the keys it gives on top of the named theme, so `theme = "nord"`
  and a small file of your own work together.

### 4.12 More images

#### 4.12.1 AVIF and JPEG XL

- AVIF and JPEG XL images.

#### 4.12.2 PDF

- PDF documents shown as images, as a quick look that saves opening a
  PDF reader rather than a replacement for one: `kiri file.pdf` and
  `![](figure.pdf)` in a document show the first page. A setting and a
  command-line option choose other pages: the first n pages, one page, a
  list of ranges such as `1-3,7,10-`, or all pages. How the pages are
  rendered is still open.

#### 4.12.3 Several images

The counterpart of several files (4.9.2) for images.

- Several images as a contact sheet: a grid of thumbnails with their file
  names beneath.

#### 4.12.4 Images in more places

- An image inside a link can be clicked: the image's placeholder cells are
  wrapped in the link's OSC 8 hyperlink, as text is.
- Images inside table cells, which now show their alt text.
- Images inside running text, which now show their alt text.

#### 4.12.5 An image on its own

- An image shown on its own, `kiri image.png`, can be shown at other sizes
  than the default fit to the window: one image pixel per terminal pixel
  even when that is larger than the window, its size in CSS pixels as in a
  document, or enlarged to fill the window.

#### 4.12.6 Images over SSH

- Less image data over SSH. kitty only takes raw pixels or PNG, so
  compressed formats such as JPEG cannot be passed through as they are;
  this needs thought on what to send and when.

### 4.13 Obsidian

Obsidian notes reach past single Markdown files, so this part may grow or
split once it is worked on.

- Wikilinks, `[[note]]` and `[[note|text]]`, as links to `note.md`, and
  embeds such as `![[image.png]]` as images.
- Links resolved as Obsidian resolves them, from anywhere in the vault
  rather than from the note's directory, so kiri needs to find the vault.
- Links to a heading or a block, `[[note#Heading]]` and `[[note#^id]]`.
- Callouts, which share the syntax of GitHub alert blocks, with
  Obsidian's other kinds and folding.
- Tags such as `#idea`.
- Properties, Obsidian's name for front matter (4.10.4), which is the
  groundwork for them.

## Phase 5 – Beyond kitty

- Other terminals: sixel and iTerm2 images, plain styling for headings.
- Groundwork: `Capabilities.graphics` names the image protocol instead of
  a yes or no. The probe already keeps the terminal's answers, and
  `--doctor` shows its name and version; the device attributes, where
  sixel support shows, are not read yet.
- `--doctor` lists the image protocols probed.
- Images as coloured Unicode block characters where no image protocol
  works.
- Remote images, off by default because fetching one tells its server that
  the document was opened.
- An HTML backend and `kiri serve` for reading in a browser with live reload.
- Pixel rendering: a headless browser screenshot (`--browser`), and possibly
  a native layout and rasterising engine later.
- Editing support: an editor and kiri side by side in kitty with live reload,
  and a key to open the editor at the current position.

## Uncategorized (extras)

Ideas not yet placed in a part, from small polish to features; a line or
two is enough. Also called extras: an idea "for the extras" goes here.
Each time a part of the roadmap is finished, the list is
sorted: an item moves to a part or to Bugs, or is dropped, and done items
go.

- Code blocks can be copied to the clipboard (OSC 52): in the pager with a
  key, and in print mode through a link beside the block that kitty's
  `open-actions.conf` hands to kiri.
- A `--list-` family that shows what a value can be: `--list-emojis` (a
  prototype, done); ideas: `--list-formats` for `--format`,
  `--list-languages` for code block languages, `--list-themes` for named
  themes (4.11), `--list-code-themes` for the `code_theme` keys, which
  `docs/theme.md` lists by hand.
- Front matter (4.10.4) in Forgejo's style: a table with one column per
  key, as GitHub used to draw it.
- Only the front matter of a document, without its body, as a value of
  the front matter setting or a flag of its own.
- Front matter tables: tell a list's items from a nested map, such as a
  muted `1.` line above each item's table or the number in its top
  border. Tried on `feature/front-matter`; the line survives stacking.

## Bugs

Known bugs, fixed in whatever phase is under way.

- An image shown on its own, `kiri image.png`, leaves an empty row below
  it in the maintainer's kitty, so the image is a row shorter than it
  could be and a two-line prompt scrolls its top row away. `kitten icat`
  does the same there. It does not happen in kitty on a virtual display,
  so the window and cell sizes the terminal reports are the first
  suspect. Once it is understood, the rows left free below the image may
  become a setting.
- An `<img>` set apart by blank lines inside a wrapper, such as
  `<p align="center">`, blank line, `<img>`, blank line, `</p>`, is drawn,
  but the wrapper's tags show as source: CommonMark makes three HTML
  blocks of it.
