# Roadmap

kiri grows in phases. Each phase leaves the project in a working state, and
review and refactoring are part of every phase rather than a phase of their
own. Later phases may be reordered or split as the work shows what matters.

Larger phases are split into numbered parts, each marked done on its own.
The numbers name the parts rather than fix their order: images (4.1), for
example, come before the rest of phase 3.

Groundwork that several parts need, such as a change inside the layout
that a feature waits for, is listed in its own section after the phases,
numbered G1, G2 and so on by area, so that a part can name what it needs.
Refactoring and small fixes that are not bugs go there too.

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
- Third-party notices with the release binaries for the dependencies
  compiled in, among them merman's ELK layout (EPL-2.0), the KaTeX font
  data of its math (OFL-1.1), and the CSS crates it brings, cssparser,
  cssparser-macros, selectors and dtoa-short (MPL-2.0), whose notice
  also tells where their source is.

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

### 4.2 Formulas

Formulas in LaTeX syntax, drawn through RaTeX, a Rust port of KaTeX that
the full build already has for Mermaid's math.

#### 4.2.1 Formula blocks

- ` ```math ` blocks and `$$…$$`, as GitHub Markdown writes them, drawn
  as images. A `$$…$$` inside a paragraph breaks it, as on GitHub.
- Formula text in the colour of the text around it, with theme keys of
  its own for the text and the background that default to body text.
- A formula that fails to draw stays as its source, with its error as
  `--render-errors` chooses, as for diagrams.
- Still open: whether the minimal build gets RaTeX as a feature of its
  own; it adds about 5 MB.
- A cache for rendered images.

#### 4.2.2 Inline formulas

Needs G1.2 to G1.4.

- `$…$` and `` $`…`$ `` drawn inside running text at the size of the text
  around them, in headings too.
- A formula that fits in the height of a row is moved into the row. A
  taller one makes its line two rows high, with the line's text centred
  in it.
- Still open: how an error shows. Numbered arrows below the line, and a
  count of errors on the pager's status line with a key to show them,
  looked workable in a first try.

#### 4.2.3 Typst

- Typst blocks drawn through a renderer from the configuration file
  (4.3.3). Without one, a ` ```typst ` block stays highlighted code.

#### 4.2.4 Formula syntax from front matter

- A document's front matter chooses whether `$…$` is LaTeX or Typst, when
  the configuration file allows it, as with presentation settings in
  front matter (Uncategorized).

### 4.3 Diagrams

#### 4.3.1 Mermaid (done)

- Mermaid code blocks drawn as diagrams, as GitHub draws them, with
  Mermaid's `default` theme on a light page and `dark` on a dark one
  unless a diagram chooses its own. A diagram in colours for the other
  page lies on a panel of its own background, unless `background_panel`
  is off.
- The full build draws them itself through merman: every diagram type,
  the Cytoscape and ELK layouts, and math. It lays diagrams out with
  dagre, as GitHub's Mermaid 11 does, unless a diagram asks for another
  layout; merman follows Mermaid 12, whose default is ELK. The minimal
  build uses mermaid-cli's `mmdc`, found on `PATH` or where npm, nvm and
  Homebrew install it, unless `auto_detect` in `[diagrams.mermaid]` is
  off, and shows the PNG that its browser draws at three pixels to the
  CSS pixel.
- An external command for a language in the configuration file,
  `[diagrams.mermaid]`, with `{input}`, `{output}`, `{theme}`,
  `{config}` and `{scale}`; it may write SVG or a raster image, and one
  that takes `{scale}` writes a PNG at that scale, or at a smaller one
  for a diagram that would be too large to load. The Mermaid
  configuration it gets as `{config}` has the built-in renderer's look
  and layout, and each diagram at its own width. It
  replaces the built-in renderer, and one that fails does not fall back
  to it. Its time limit, 20 s unless `timeout` says otherwise, and
  Ctrl-C, Ctrl-\\, SIGTERM and SIGHUP stop it and what it started.
- `--diagrams on|off`, `KIRI_DIAGRAMS` and `enabled` in the configuration
  file's `[diagrams]` table; `--doctor` shows the switch, the time
  limit and the panels with their origins, and each language's
  renderers.
- A diagram that does not draw stays code, with the reason in red below
  it and a warning naming its line; `--render-errors inline|stderr|both`,
  `KIRI_RENDER_ERRORS` and `render_errors` in the configuration file
  choose one of them or both, as they will for formulas. Drawing that
  takes longer than half a second shows how far it has got.
- `kiri file.mmd` (or `.mermaid`, or `--format mermaid`): a Mermaid file
  drawn on its own, fit to the window as an image file is; its source as
  text when it is not drawn, and an error when it fails to draw.
- merman's SVG fixed where resvg would lose part of it: sequence-diagram
  notes, clipped class-diagram labels and text past a diagram's edge,
  each pinned with a test.
- Later: a disk cache for diagrams across runs, mainly for `mmdc`.
- Later: in the pager, showing the document at once and filling the
  diagrams in as they are drawn.

#### 4.3.2 Other diagram languages

- Other diagram languages that a renderer turns into an image, in the way
  Mermaid works, such as Graphviz (`dot`), PlantUML, D2 and Pikchr.

#### 4.3.3 Renderers from the configuration file

- A code block in any language drawn by a program that the configuration
  file names: a way out when kiri has no support of its own for a rare
  format. Each renderer is a table under `[diagrams]`, and the table's
  name is its key:

  ```toml
  [diagrams.graphviz]
  name = "Graphviz"
  languages = ["dot", "graphviz"]
  command = ["dot", "-Tsvg"]
  safe = true
  ```

  `name` is the renderer's name in messages and `languages` the info
  strings it draws; both default to the table's name. A language in two
  tables is an error.
- A command without `{input}` reads the source from standard input, and
  one without `{output}` writes the image to standard output, as filters
  such as `dot -Tsvg` do.
- A renderer marked `safe` draws in any document. Any other draws only
  in a document the reader trusts, as a program such as PlantUML or
  LaTeX can read files or run commands that the document asks for. The
  reader chooses one of:
  - not trusting the document,
  - trusting this version of it, by its SHA-256, so the trust moves with
    the file,
  - trusting every version of the file at its path,
  - trusting the files in its directory.
- A block that is not drawn for want of trust stays as code, with a line
  below it saying that drawing it was blocked.
- `-X` (`--allow-dangerous-execution` or a similar name) lets every
  renderer draw for that run alone. It has no environment variable and
  stores no trust.
- The documentation shows how to add a renderer, with base64 as the
  example: `command = ["base64", "-d"]` draws an image embedded in a
  block as base64.
- Question: where trust is stored, and how the pager asks for it.
- Question: whether the rule on trust applies to Mermaid's commands too,
  `mmdc` and those in `[diagrams.mermaid]`, which today draw in any
  document.

#### 4.3.4 Diagrams to a file

- Writing a drawn diagram to a file, SVG or PNG, instead of the
  terminal, such as for a review or a web page, without finding the
  Mermaid program by hand. `--format` names the input, so this needs an
  option of its own.

### 4.4 Animation and video

Needs G2.2.

- Animated GIF, WebP and APNG, played by kitty itself.
- Video (WebM, MP4, AVI and others) through ffmpeg when it is installed.

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

- Reload with `r`, for a lone document read from a file. The view
  stays at the block that was at the top, found by comparing the two
  versions (G1.6), so a block added above it does not move it; a view at
  the end goes to the new end. It loads only the images and diagrams it
  does not have yet: diagrams are kept by their source unless they failed
  to draw, and an image file is loaded again when its modification time
  changed. (done)
- After a reload that worked, the status line's `reloaded` stays for
  four seconds and fades out over three; other messages stay until the
  next key. (done)
- A `:` command that jumps to the block a reload changed first.

#### 4.6.5 Moving around

- A table of contents, anchor jumps and back history.
- In print mode, without the pager: `--toc` puts a table of contents with
  links at the top of the document.
- Headings marked as shell prompts (OSC 133), so kitty's keys for jumping
  between prompts move between headings in print mode. Needs a try first:
  kitty may treat the marks as more than places to jump to.
- More `:` commands in the pager, as `less` and vim have them: `:x` for
  the first file, `:q` to quit, `:` and a number for a line or a file,
  and a jump to a heading, such as `:§3.2`.

#### 4.6.6 Link selection

- Choosing a link with the keyboard and following it; a Markdown target
  opens in an overlay window, as in 4.5.

#### 4.6.7 Copying code blocks

- Code blocks can be copied to the clipboard (OSC 52): in the pager with a
  key, and in print mode through a link beside the block that kitty's
  `open-actions.conf` hands to kiri.

#### 4.6.8 A sticky file header

- With several files (4.9.2), a file's header stays at the top of the
  pager while its file scrolls, as a sticky header does on a web page,
  and spans the width of the status line.

#### 4.6.9 Follow mode

- Follow, for a lone document read from a file, such as one Claude is
  working on in the next window: it reloads on every change without a
  key press and keeps the view where the reader left it. `F` in the
  pager, `--follow on|off`, `-f` (`--follow=on`, moved from `--format`),
  `-F` (`--follow=off`), `KIRI_FOLLOW` and `follow` in the configuration
  file; following pages the file however short it is. (done)
- Auto-follow, which also jumps to a change, and a setting says which:
  the first, the last or the one that changed most. `--follow auto` and
  `--fa`. Following can be switched from the menu (4.14) too, where the
  value can be set more finely.
- Following standard input, such as `tail -f log | kiri`, as well as a
  file.
- Ctrl-C stops a reload that draws a slow diagram; today the terminal
  is raw then, and only the time limit ends the wait.
- Images and diagrams load in the background: the text of a reload shows
  at once, with a note where an image is still being drawn, and a newer
  change stops the load of an older one. Today a reload waits for every
  changed diagram, which with a slow `mmdc` holds the pager for seconds.
- Warnings about front matter after a reload, on the status line. The
  first load prints them before the pager opens; a reload drops them
  today.
- Images that did not change are not sent to the terminal again
  (G2.2), and images that changed or went are deleted from it, as are
  all of kiri's images when the pager quits.

#### 4.6.10 Changes as a diff

- Changes shown as a diff, added text green and removed text red, fading
  out slowly: after a reload, and against a git revision.

### 4.7 Other input formats

#### 4.7.1 Formats

- reStructuredText, Org, AsciiDoc, possibly through pandoc.
- Jupyter notebooks (`.ipynb`): Markdown cells, code cells with syntax
  highlighting, and their outputs, images included.
- CSV and TSV files, shown as a table. A file of more than 100 000 cells
  shows its first rows and a note of how many were left out. (done)
- `--format text`: any file shown as it is, in a code block highlighted
  by its extension, such as a Markdown or Mermaid file's source; plain
  output, such as into a pipe, gets the file as `cat` gives it. (done)
- `--text raw` writes a file's content unchanged, on a terminal too;
  perhaps `--raw` becomes its short form, which today works only with
  `--doctor`.

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
- Image files shown with several files (4.9.2) are read in full before
  the pager opens, about 48 MB for a 12-megapixel photo. Reduce each to
  the largest size it can be shown at, as drawn diagrams are.

### 4.8 Diagnostics

#### 4.8.1 The doctor report (done)

- `kiri --doctor` shows what kiri found and what it uses: the terminal's
  answers (cell and window size, graphics, text sizing, colours), each
  setting with where it came from (default, configuration file,
  environment variable or command-line flag), the configuration file
  paths and the image formats this build decodes.
- The external tools later parts need (Typst, ffmpeg, pandoc) join the
  report with the part that uses each, saying whether it was found, as
  the Mermaid renderers did with 4.3.1.
- The report is a Markdown document that kiri renders itself, so it also
  shows how headings, tables and images look in this terminal. Piped to a
  file it stays plain Markdown, ready to paste into a bug report.

#### 4.8.2 Settings

- The report names each setting's key in the configuration file, so it
  also tells how to change the setting.
- `--print-config` writes the settings in effect as a configuration file,
  a start for one's own.
- `--config KEY=VALUE` (named so rather than `--option`) overrides any
  configuration key on the command line, as cargo's `--config` does,
  such as `diagrams.timeout=60`; with it, a way to list the keys, and a
  `[diagrams.mermaid.config]` table passed to Mermaid as it is, which
  4.3.2's languages could have too.
- kiri creates a default configuration file, and perhaps settings edited
  from the UI.
- Presentation settings in a document's front matter, such as its theme
  or heading sizes, read only when the configuration file turns this on.
  Front matter would then override the configuration file, and the
  environment and the command line would override front matter. Which
  keys a document may set is still open. Needs G5.2.

#### 4.8.3 Listing the values

- A `--list-` family that shows what a value can be, as `--list-emojis`
  does: `--list-formats` for `--format`, `--list-languages` for code
  block languages, `--list-themes` for named themes (4.11),
  `--list-code-themes` for the `code_theme_dark` and `code_theme_light`
  keys, which `docs/theme.md` lists by hand.

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

#### 4.9.2 Several files (done)

- Several files on the command line, as the shell expands `kiri *.md` or
  `kiri *.jpg`, shown one after another in the order given, each under a
  header that names it, as `bat` does. Each is shown as it would be on its
  own, so an image is fitted to the window together with its header;
  4.12.3 adds a grid of them. A file that cannot be shown is reported
  on standard error at its place among the others, as `bat` does, the
  others are still shown, and kiri exits with an error. In the pager the
  file keeps its place in the stream, under its header with the reason.
- The header is a band of its own colour across the text, with a bar
  down its left edge and the name in bold, so that nothing in a document
  looks like it. Plain text names each file as `head` does, such as
  `==> docs/spec.md <==`.
- Files that are all images or diagram files go to the pager only with
  `-p`, as a piped document does. With a document among them, the paging
  setting applies to all of them.
- In the pager the files are one stream, headers and all, so a search
  finds a term in any of them. The status line names the file at the top
  of the view and its place among them, such as `b.md 2/3`, and `:n` and
  `:p` move to the next and the previous file, as in `less`; Shift+PgDn
  and Shift+PgUp do the same. Some terminals keep those keys for their
  own scrollback: Ghostty binds
  `shift+page_up=scroll_page_up` by default. There `:n` and `:p` still
  work, or `keybind = shift+page_up=unbind` and
  `keybind = shift+page_down=unbind` in Ghostty's configuration hand the
  keys to kiri.
- `:n` at the last file tells `no next file` on the status line, and
  `:p` at the start of the first `no previous file`, until the next key.
  A single document has no other file, so there `:` does nothing.

### 4.10 More Markdown

GitHub's Markdown first, then what is common elsewhere.

#### 4.10.1 Links within the document

- Heading anchors as GitHub makes them, so a link such as `[see](#install)`
  leads to its heading; the pager jumps there (4.6).
- Footnotes (`[^1]`), listed at the end of the document as in a browser,
  with links from each reference to its note and back.
- Opening a document at a section from the command line, such as
  `kiri spec.md#3-2`, for a review that refers to `§3.2` of a spec.
  Needs the heading anchors above and anchor jumps in the pager (4.6.5).

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
- Centred headings and paragraphs, such as `<h1 align="center">` and
  `<p align="center">` around text, as README titles often are. They stay
  source today.

#### 4.10.3 Fractional sizes

Needs G1.2 and G1.3.

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

#### 4.10.6 Only the front matter

- `--front-matter-only` shows a document's front matter (4.10.4) without
  its body.

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
- Images inside running text, which now show their alt text. Needs G1.4.

#### 4.12.5 An image on its own

- An image shown on its own, `kiri image.png`, can be shown at other sizes
  than the default fit to the window: one image pixel per terminal pixel
  even when that is larger than the window, its size in CSS pixels as in a
  document, or enlarged to fill the window.

#### 4.12.6 Images over SSH

- Less image data over SSH. kitty only takes raw pixels or PNG, so
  compressed formats such as JPEG cannot be passed through as they are;
  this needs thought on what to send and when.

#### 4.12.7 Zooming

- Zooming images and diagrams in the pager, or opening one in a view of
  its own. A diagram drawn by an external program as PNG is drawn again
  at a larger scale when it is zoomed past the one it has. A diagram
  too large to load as PNG even at one pixel to the CSS pixel, or one
  to be zoomed further than a PNG allows, may be drawn as SVG instead,
  behind an option of its own; PNG stays the normal path, as the more
  reliable one.

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

### 4.14 Menu

A menu in the pager, from which its keys and settings can be found.

#### 4.14.1 Keys by kind

- The help panel in sections, such as moving, searching and several
  files, so it stays the same whatever is shown and the keys of one kind
  can be looked up under their own heading. Today it lists the keys for
  several files only when several are shown.

#### 4.14.2 Themes on the fly

- Changing the theme (4.11) from the menu while the document is shown.

## Phase 5 – Beyond kitty

- Other terminals: sixel and iTerm2 images, plain styling for headings.
  Needs G3.3.
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

## Groundwork

What the parts build on, by area: changes inside the code that a feature
needs before it can start, refactoring, and small fixes that are not bugs.
A part that needs one names it, such as "needs G1.3".

### G1 Layout

#### G1.1 Layout by block type (done)

- `render/layout.rs` split by block type (lists, quotes, code) before the
  next block type joins it, formulas or GitHub alert blocks, whichever
  comes first.

#### G1.2 Text size as a type (done)

- A text size of its own type instead of a whole number: a scale, a
  fraction of it and a vertical alignment, as the text sizing protocol
  takes them. Text is drawn as it is today.

#### G1.3 A size for each part of a line

- Each span of a line has its own size, and the line is as many rows high
  as its tallest span. Wrapping measures spans by their size. Fractional
  sizes (4.10.3) are its first use.

#### G1.4 Images in a line

- A span can be the cells of an image instead of text: which image, how
  many columns and rows it covers, and the row its baseline is on. The
  wrapper keeps it whole, and its alt text or source is what search
  finds. `ImageKey::Block` is renamed for its wider use. Images in running
  text (4.12.4) are its first use, inline formulas (4.2.2) the next.

#### G1.5 Lines know their block (done)

- Each laid-out line records the block it belongs to, for a table of
  contents, heading anchors, going back, keeping the place on reload or
  resize, and opening a `<details>` block in the pager. Finding the files
  of a stream, and keeping the top block in place on a resize and on a
  reload, use it already; the others are still to come.

#### G1.6 Comparing two documents (done)

- Two versions of a document compared block by block: which blocks
  stayed, which changed, and which were added or removed. Each block
  also knows the line of the source it starts on, so a reader can later
  open the source there in an editor or point to it, such as `plan.md`
  line 42. A reload (4.6.4) uses the comparison to keep the reader's
  place; the follow mode will use it to find what changed, and the diff
  view to show it. Nothing uses the source lines yet.

### G2 Images

#### G2.1 Images from blocks (done)

- Images that come from a block rather than a URL, before formulas and
  diagrams are worked on side by side. `Images` keys images by a URL or a
  block, its language and source, and Mermaid diagrams (4.3.1) are the
  first images from blocks.

#### G2.2 An image encoder

- What has been sent to the terminal, and under which image id, moves
  from `render/output.rs` behind an image encoder. Animation frames (4.4)
  need that state, and the other image protocols of phase 5 fill it
  differently.
- The encoder lives longer than one layout, so the pager does not send
  every image again after a resize or a reload, and it knows which ids
  to delete. Today each layout makes a new writer, which sends again all
  images it shows, and kiri never deletes an image.

### G3 Terminal

#### G3.1 One query for the window size (done)

- One query for the window size in cells and pixels, as fitting the
  window needs its height. `term.rs` used to ask for the width and the
  cell size separately.

#### G3.2 Raw mode for keys (done)

- Raw mode, which lived inside the probe in `term.rs` until the pager
  (4.6.1), made usable for reading keys.

#### G3.3 The image protocol as a capability

- `Capabilities.graphics` names the image protocol instead of a yes or
  no. The probe already keeps the terminal's answers, and `--doctor`
  shows its name and version; the device attributes, where sixel support
  shows, are not read yet.

### G4 Build

#### G4.1 Binary size

- How much each feature and dependency adds to a stripped binary, and
  smaller builds from it, such as RaTeX as a feature of its own for the
  minimal build (4.2.1).

#### G4.2 Ready for CI

- `rust-version` in `Cargo.toml`, a `deny.toml` for licences, and a pinned
  toolchain, before CI (3.2).

### G5 Settings

#### G5.1 config.rs split

- `config.rs` split into environment, file and resolving, with the
  leftovers of the settings refactor: `ConfigToml` and `Config`, and
  `Choices` and `Settings`, copy each other.

#### G5.2 Names and levels of settings

- Internal names match the keys, such as `diagram_panel` for
  `background_panel`, and environment values are read the same way
  everywhere: `KIRI_NO_IMAGES` takes a wrong value without a warning.
- Some settings exist only as a flag (`--width`) or only in the file
  (`search_case`); decide which belong on every level.

### G6 Code

#### G6.1 Smaller cleanups

- The parser drops Markdown elements it does not know without a word.
- One light or dark page type for diagrams and themes.
- Diagram errors as a type of their own rather than strings, as images
  have.
- Status line key hints written once with the keys they name.

#### G6.2 Shorter help texts

- `kiri --help` explains many options in several sentences, such as
  `[FILE]...` in five lines; one short line each would do, with the
  details in the documentation.

## Uncategorized (extras)

Ideas not yet placed in a part, from small polish to features; a line or
two is enough. Also called extras: an idea "for the extras" goes here.
Each time a part of the roadmap is finished, the list is
sorted: an item moves to a part or to Bugs, or is dropped, and done items
go.

- Find out which command shortcuts other pagers and editors offer, such
  as the `:` commands of `less` and vim, for `docs/inspiration.md`.
- A side-by-side view: the Markdown source beside the rendered document,
  each part across from the other, so the source and what it becomes can
  be compared.

## Bugs

Known bugs, fixed in whatever phase is under way. Each has a number, B1,
B2 and so on, that is not used again once the bug is fixed.

#### B1 An image on its own is a row short in kitty

An image shown on its own, `kiri image.png`, leaves an empty row below it
in the maintainer's kitty, so the image is a row shorter than it could be
and a two-line prompt scrolls its top row away. `kitten icat` does the
same there. It does not happen in kitty on a virtual display, so the
window and cell sizes the terminal reports are the first suspect. Once it
is understood, the rows left free below the image may become a setting.

#### B2 Tags of an HTML wrapper show as source

An `<img>` set apart by blank lines inside a wrapper, such as
`<p align="center">`, blank line, `<img>`, blank line, `</p>`, is drawn,
but the wrapper's tags show as source: CommonMark makes three HTML
blocks of it. Markdown inside a `<div align="center">` set apart the same
way is drawn too, and the `<div>` tags show as source.

#### B3 The examples fit test draws no diagrams

The test that the example documents fit the window at every width,
`the_visual_check_documents_fit_every_mode`, loads no diagram renderer,
so it never draws a diagram. A drawn diagram, or the red error below one
that fails, could reach past the window's edge without the test noticing.

### Upstream bugs

Bugs in projects kiri uses, which kiri works around until they are fixed
there.

#### resvg and the SVG that mmdc writes

mmdc's SVG drawn through resvg differs from the same SVG in a browser in
two known ways, so the external Mermaid path asks mmdc for PNG instead.
A fix upstream would let kiri take SVG from mmdc again, which also drops
the PNG's size limits and the retry at a smaller scale.

- UML arrowheads that should be hollow are filled: aggregation looks like
  composition, inheritance and realization like plain arrows. Mermaid
  writes the rule as `#id [id$="-extensionStart"], #id .extension
  { fill: transparent }`. simplecss 0.2.2, resvg's CSS parser, knows the
  attribute operators `=`, `~=` and `|=` but not `$=`, `^=` or `*=`, and
  on the first selector it cannot parse it skips the rest of the list, so
  the `.extension` rule is lost too and the marker takes the root's fill.
  Removing the `[id$=…]` selectors from the SVG draws the arrowheads
  hollow. merman writes `[id="…"]`, which is why the built-in renderer
  is not affected.
- A Gantt milestone is a square, not a diamond. Mermaid turns it with the
  CSS property `transform: rotate(45deg) scale(0.8,0.8)`; resvg reads
  only SVG's unitless `rotate(45)`, so it drops the whole transform.
  Writing `rotate(45)` in the SVG draws the diamond.

Fix: patch simplecss for the CSS 3 attribute operators, and svgtypes or
usvg for angle units in a CSS transform, upstream. Until they are
released in resvg, kiri could instead rewrite mmdc's style text before
drawing it; that is fragile, as Mermaid may change its CSS.
