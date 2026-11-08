# Roadmap

kiri reads files to the user in the best and most understandable way the
user's terminal allows.

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
- Testing in the terminals kiri supports, at their versions. The goal is
  that kiri works in the latest minor version of each, such as Ghostty's;
  today each is tested by hand in one version.

### 3.3 Releases

- Semantic versioning (0.x until the command line settles), a changelog in
  Keep a Changelog format, and release binaries.
  - Releases made with `make patch-release` and its siblings, and Linux
    binaries (x86_64 and aarch64) built by GitHub when the tag is pushed.
  - Release binaries for macOS on arm64.
  - A test in CI that a branch added its changelog lines.
- Documented minimum versions for kitty and Rust.
- Release binaries without symbols (`strip = true` in the release profile),
  about a fifth smaller.
- Shell completions and a man page, generated from the command-line
  definition.
  `kiri --completions <shell>` writes the completion script for bash,
  zsh, fish, elvish, powershell and nushell. Some ideas for the rest: the
  shells Ion, Tcsh and Xonsh, which `clap_complete` does not cover (the
  young crate `carapace-rs-clap` might; not checked), and dynamic
  completion, where the shell asks kiri while completing, for values such
  as theme names.
- Third-party notices with the release binaries: the licences and notices of
  the libraries and data compiled in (`make notices`).

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
every build has: the full build for Mermaid's math as well, the minimal
build for formulas alone.

#### 4.2.1 Formula blocks (done)

- ` ```math ` blocks and `$$…$$`, as GitHub Markdown writes them, drawn
  as images, centred, at KaTeX's size of 1.21 em. A `$$…$$` inside a
  paragraph breaks it, as on GitHub. Empty `$$ $$` stays text.
- Formula text in the colour of the text around it, with a `[formula]`
  table in the theme for the text and the background, which default to
  the body text and the page.
- A formula that fails to draw stays as its source, with its error as
  `--render-errors` chooses, as for diagrams. `--no-images` and
  `--diagrams off` show formulas as code.
- RaTeX is in the minimal build too: it adds about 3 MB to its stripped
  binary (14.3 to 17.2 MB) and next to nothing to the full build.
- Images in alerts show the alert's tint behind them, not the page's; a
  formula, always transparent, made that plain.
- Still open: a cache for rendered images, which would pay for slow
  renderers such as `mmdc` across runs rather than for formulas. A
  formula inside `<sub>` or `<sup>` on a line of its own, such as
  `<sub>$$z$$</sub>`, shows the tags as text, because the paragraph is
  split at the formula.

#### 4.2.2 Inline formulas

- `$…$` and `` $`…`$ `` drawn inside running text at the size of the text
  around them, in headings too. (done)
- A formula that fits in the height of a row, or does when shrunk a
  little, is moved into the row. A taller one makes its line two rows
  high, with the line's text centred in it. (done)
- Formulas inside table cells, which stay text.
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

Needs G5.3.

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

Needs G7.3.2 for time and G7.3.3 for watching.

- Animated GIF, WebP and APNG (done): started by hand, one plays at a
  time, with a play button, a replay icon and a pill on the status line.
- Video (WebM, MP4, AVI and others) through ffmpeg when it is installed.
- asciinema recordings (`.cast`).

#### 4.4.1 Animations inside a line of text

- An animation in the middle of a line of text, or in a table cell.
  It plays in the text layout with a few faults, and nothing tells the
  reader that it can play; in reading mode it shows only its first frame.

### 4.5 Links

- Relative links become `file://` URLs, so kitty's `open-actions.conf` can
  open Markdown targets in kiri.
- A Markdown target opens in a kitty overlay window over the document
  (`kitty @ launch --type=overlay`), so closing it returns the reader to
  where they were.
- Links to local files or sections that do not exist are drawn in a
  colour of their own, so broken links stand out.
- A setting that turns links on or off whatever the terminal seems to
  support, such as `KIRI_HYPERLINKS`, for terminals and multiplexers
  where kiri guesses wrong. Inside tmux only tmux's answer counts (5.2.4).

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

Its open items need G7.3.1.

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

Needs G1.10.2, G7.2.3 and G7.2.4.

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

Following a link is done with G7.2.4.

- Choosing a link with the keyboard and following it; a Markdown target
  opens in an overlay window, as in 4.5. Choosing is done, and so is
  copying the address with Enter: Tab and Shift+Tab select a link on the
  screen in the text as in reading mode (4.15.5), in reverse video, and
  a link in a heading drawn as an image cannot be selected. Following it
  is open.
- Enter on a link selected with Tab in reading mode (4.15.5) opens it in
  the browser, as a click on the link does in kitty, instead of copying
  its URL. kiri would need a program to open it with, such as
  `xdg-open` from a setting.

#### 4.6.7 Copying

Needs G1.10.2.

- Code blocks can be copied to the clipboard (OSC 52): in the pager with a
  key, and in print mode through a link beside the block that kitty's
  `open-actions.conf` hands to kiri.
- Copying can give the Markdown source of a part or its text as shown;
  a setting chooses which.

#### 4.6.8 A sticky file header

- With several files (4.9.2), a file's header stays at the top of the
  pager while its file scrolls, as a sticky header does on a web page,
  and spans the width of the status line.

#### 4.6.9 Follow mode

Its open items, and watching the config files, need G7.3.3.

- Follow, for a lone document read from a file, such as one Claude is
  working on in the next window: it reloads on every change without a
  key press and keeps the view where the reader left it. `F` in the
  pager, `--follow on|off`, `-f` (`--follow=on`, moved from `--format`),
  `-F` (`--follow=off`), `KIRI_FOLLOW` and `follow` in the configuration
  file; following pages the file however short it is. (done)
- Auto-follow, which also jumps to a change, and a setting says which:
  the first, the last or the one that takes the most rows. `--follow
  auto`, `--fa`, `--follow-jump` and `follow_jump`; `F` goes from off to
  on to auto. The change it went to shows in a lighter background that
  fades out. (done)
- Following can be switched from the menu (4.14) too, where the value
  can be set more finely.
- Following standard input, such as `tail -f log | kiri`, as well as a
  file.
- Ctrl-C stops a reload that draws a slow diagram: replaced by loading
  in the background, where the pager stays responsive and `q` stops the
  programs. (replaced)
- Images and diagrams of a reload load in the background: the text shows
  at once; a changed image shows its old one dimmed, a new or reread one
  its code or alt text, each with a thin moving band in the blank line
  below; `[rendering: 2/5]` on the status line; no jump when an image
  comes; a newer change stops the loads it no longer wants, and `q`
  stops the programs. (done)
- The images in view load first, then the ones nearest to it, chosen
  again as the reader scrolls. Image files no longer wait behind slow
  diagrams for a program. (done)
- The initial load in the background too, behind a setting, as moving
  content may annoy some readers: `initial_load = "background"`,
  `--initial-load`, `KIRI_INITIAL_LOAD`, where the pager surely opens.
  Waiting, Space or Enter opens the pager at once. Failures are told on
  standard error once the pager closes. Print mode waits as today. (done)
- A grey box at the old image's size was tried as the placeholder and
  dropped for the block's own content. It may come back where it earns
  its place, such as a page-long code block whose diagram is short.
- Warnings about front matter in the pager, after a reload and on the
  first load: a red line above the status line for five seconds, and
  `⚠ 1` on the status line while the document has them. (done)
- A search keeps its place after a reload: the match it was on, rather
  than the first match from the top of the view.
- Images that did not change are not sent to the terminal again
  (G2.2), and images that changed or went are deleted from it, as are
  all of kiri's images when the pager quits. (done)
- Noticing at once when an image file that a document shows changes.
  Today an image file is read again only when the document is reloaded,
  so a diagram exported again beside a followed document shows only
  after the document itself changes. Watching the local image files too
  would show it at once.

#### 4.6.10 Changes as a diff

Needs G1.8.2 and G1.10.3.

- Changes shown as a diff, added text green and removed text red, fading
  out slowly: after a reload, and against a git revision.

#### 4.6.11 The target of a link

- The target of the link under the mouse or the one chosen with Tab is
  shown, for example on the status line.

### 4.7 Other input formats

#### 4.7.1 Formats

Needs G2.6.2 and G2.6.3.

- reStructuredText, Org, AsciiDoc, possibly through pandoc.
- Jupyter notebooks (`.ipynb`): Markdown cells, code cells with syntax
  highlighting, and their outputs, images included.
- CSV and TSV files, shown as a table. A file of more than 100 000 cells
  shows its first rows and a note of how many were left out. (done)
- `--format text`: any file shown as it is, in a code block highlighted
  by its extension, such as a Markdown or Mermaid file's source; plain
  output, such as into a pipe, gets the file as `cat` gives it. (done)
- Files told as text by their name: only a Markdown extension makes a
  file Markdown. Any other text file is shown as text, highlighted when
  the highlighter knows its extension or its whole name (`config.yaml`,
  `Makefile`, `.gitignore`) and plain otherwise (`notes.txt`, `LICENSE`),
  without the code block's band, padding and language label. Standard
  input and a directory's `README` without an extension stay Markdown,
  and standard input that is JSON, JSON Lines, a diff, a script with a
  shebang, XML or HTML is shown as text; so is a file whose name tells
  nothing, for highlighting. `--format` still decides. (done)
- Left out of the content check on purpose: YAML and TOML, which cannot be
  told from Markdown (`---` starts front matter, and `key: value` or
  `[section]` lines are ordinary text).
- A clearer message for a binary file that is not an image than "neither
  Markdown nor an image kiri knows", for example naming what its first
  bytes look like.
- `--text raw` writes a file's content unchanged, on a terminal too;
  perhaps `--raw` becomes its short form, which today works only with
  `--doctor`.

#### 4.7.2 Large files

Needs G1.10.2 and G1.11.2.

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

#### 4.7.3 Better look for files shown as text

Tentative ideas, to be chosen from when the part is planned. A file shown
as text, such as YAML, JSON or source code, could look more like `bat`:

- line numbers in a dim gutter, a wrapped line's continuation without a
  number;
- a thin vertical rule between the gutter and the code;
- a header with the file name, perhaps its language and size, between
  horizontal rules; with several files, the file header kiri already has;
- the full width of the window, without the document margins.

Left from 4.7.1:

- In a run that also has a Markdown file, such as `kiri README.md
  config.yaml`, reading mode is on for every file, so the file shown as
  text gets a code block's band again. Not settled: keep such files out
  of reading mode, or let reading mode draw them bare.
- Colour codes in a file shown as text, such as a build log's, show as
  replacement characters; they could be passed through, as `less -R`
  does.
- A long line breaks inside a word, as code blocks do.

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
  keys a document may set is still open.

#### 4.8.3 Listing the values

- A `--list-` family that shows what a value can be, as `--list-emojis`
  does: `--list-formats` for `--format`, `--list-languages` for code
  block languages, `--list-themes` for named themes (4.11),
  `--list-code-themes` for the `code_theme_dark` and `code_theme_light`
  keys, which `docs/theme.md` lists by hand.

#### 4.8.4 Where warnings and debug output go

- Warnings and debug output kept in a log file or the systemd journal
  instead of standard error, which the pager holds until it quits; a debug
  level that tells each program kiri runs and how long it takes.

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

#### 4.9.3 One file and several alike

One file is shown as the special case of several (G6.5.1), and a few
differences between the two were kept as they were. They are to be made
alike:
- A printed image alone tells the settings warnings, such as a wrong
  `KIRI_PAGING`, as a document does; now it drops them.
- `--reading` in plain output tells that it does nothing for text
  written as it is, alone as among several; now only among several.
- A named pipe reads its images from the current directory, alone as
  among several; among several it now reads them from the pipe's
  directory.
- `auto` paging treats every file the same: images and diagrams are paged
  when they are longer than the window, as documents are. (done)
- An error opening a file comes after the settings warnings, alone as
  among several; alone it now comes before them.

### 4.10 More Markdown

GitHub's Markdown first, then what is common elsewhere.

#### 4.10.1 Links within the document

Needs G1.10.2 and G7.2.4.

- Heading anchors as GitHub makes them, so a link such as `[see](#install)`
  leads to its heading; the pager jumps there (4.6).
- Footnotes (`[^1]`), listed at the end of the document as in a browser,
  with links from each reference to its note and back.
- Opening a document at a section from the command line, such as
  `kiri spec.md#3-2`, for a review that refers to `§3.2` of a spec.
  Needs the heading anchors above and anchor jumps in the pager (4.6.5).

#### 4.10.2 HTML tags

The HTML that READMEs use, drawn as a browser draws it instead of shown as
muted source. The parts are cut by what they share: inline tags and
entities are text, the next part reads a block that is one element, and
the rest need more of the layout.

##### 4.10.2.1 Inline tags

- Small HTML tags: `<kbd>` drawn as inline code, `<br>` as a line break
  and `<mark>` in the theme's `mark` colours. (done)
- `<b>`, `<strong>`, `<i>`, `<em>`, `<s>`, `<del>` and `<code>` in running
  text. (done)
- `<u>` and `<a href>` around text.
- Tags inside a `<summary>`, such as `<b>`, `<i>` and `<code>`, which
  GitHub READMEs often write. (done)

##### 4.10.2.2 Entities in HTML (done)

- Named entities in HTML, such as `&mdash;` and `&copy;`, shown as the
  characters they stand for (— and ©) in the tags kiri draws, such as
  `<summary>`, headings and `<pre>`. A table of the common names, not the
  whole HTML5 set.

##### 4.10.2.3 Blocks that are one element (done)

- Headings written as HTML, `<h1>` to `<h6>`, drawn as the document's
  headings, left-aligned whatever their `align` (centring is 4.10.2.5).
- A `<pre><code>` block, highlighted by its `lang`, drawn as a code block.

##### 4.10.2.4 `<details>`

Opening and closing `<details>` needs G1.11.2 and G7.3.4.

- `<details>` and `<summary>` are shown open, the summary in bold after a
  triangle, also nested and inside lists and quotes. (done)
- The pager can close and open a `<details>`.

##### 4.10.2.5 Centring and links

- Links written as HTML: a centred row of `<a>` tags, often around
  `<strong>`, in a `<p align="center">` below a README's title, as many
  GitHub READMEs and Hugging Face model cards have it. Such a block stays
  source today.
- Centred headings and paragraphs, such as `<h1 align="center">` and
  `<p align="center">` around text, as README titles often are. A heading
  is drawn left-aligned today (4.10.2.3); a paragraph stays source.

##### 4.10.2.6 HTML lists and tables

- HTML lists, `<ul>`, `<ol>` and `<li>`, drawn as Markdown lists, also
  inside `<details>`, with `<input type="checkbox">` as a task list box.
- An HTML `<table>`, with `<thead>`, `<th>` and `<td align>`, drawn as a
  Markdown table, with images and other inline HTML in its cells.

#### 4.10.3 Fractional sizes

Text drawn smaller or larger with the text sizing protocol's fractional
scale, built once for all of these:

- `<sub>` and `<sup>` at two thirds of the size around them, a sup at the
  top of the rows and a sub at the bottom; Unicode's script characters
  (x², H₂O) without text sizing when every character has one, else plain
  text. (done)
- Heading sizes with fractions. A theme writes `scale = 1.5`; kiri draws
  the nearest size it can and `--doctor` shows it. The defaults go from
  2 1/2 for `h1` down by quarters to 1 for `h6`, all bold, without
  underlines. (done)
- Footnote references, drawn smaller and raised: with footnotes, 4.10.1.

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

#### 4.10.7 Definition lists

- A term on a line of its own with its definitions on the lines below,
  each after a `:`, as Markdown Extra and Pandoc write them: the term in
  bold and its definitions indented under it, as a browser draws a `<dl>`.

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
- A way to force the light or dark side of kiri's own theme, as
  `--light` and `--dark` did before they became names of themes, such
  as `--dark --theme -`, where `-` names the built-in theme.

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
- Images inside running text. (done)

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
- Tab and Shift+Tab stop at every image, not only at animated ones (4.4),
  as a start for opening one.

#### 4.12.8 Remote images

Needs G7.3.3 and G5.3.

- Images from URLs, as many other viewers show them. `https:` URLs and
  the local machine, also over `http:`, are fetched by default; the local
  network, such as a home network or a Docker network, also over `http:`,
  only when the configuration file allows it, for all of it or for named
  hosts. A setting and a command-line option choose the sources, or none.
  Browsers draw the same line: a page from the web needs the reader's
  permission to reach the local network.
- Fetching an image tells its server that the document was opened, and an
  image on the local network lets a document send requests to devices
  there. The fetching itself is kept safe: an address is checked after its
  name is resolved, the ports browsers block (the Fetch standard's list,
  such as SMTP's 25) are not fetched from, the `--max-image-size` limit
  and a timeout apply to each fetch, a few fetches run at a time, no
  cookies or referrer are sent, and a cache keeps a document opened again
  from fetching its images again.
- Output that is not a terminal shows no images, so nothing is fetched
  for it.

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

Needs G7.2.2 and G7.2.4; the help comes with G7.2.3, and the theme among the options needs G1.8.1 and G2.5.1 too.

A menu in the pager, from which its keys and settings can be found.

#### 4.14.1 Keys by kind

Done with G7.2.3, which needs G7.2.2.

- The help panel in sections, such as moving, searching and several
  files, so it stays the same whatever is shown and the keys of one kind
  can be looked up under their own heading. Today it lists the keys for
  several files only when several are shown.
- Note: with several files the help panel is 23 rows high, so in a
  24-row window inside tmux, whose status line takes a row, its last
  row and bottom border are cut off.

#### 4.14.2 Themes on the fly

Needs G1.8.1, G2.5.1 and G2.5.2.

- Changing the theme (4.11) from the menu while the document is shown.

#### 4.14.3 Remote images from the menu

Needs G7.3.3 and G5.3.

- Allowing remote images from the local network (4.12.8) from the menu,
  for the document shown or for good, as a browser asks for it.

### 4.15 Reading mode (done)

Text in a proportional font, drawn as images, so that a document reads
the way GitHub shows it in a browser, a core feature of kiri. It is the
default where the terminal draws images; `--no-reading` turns it off, and
`M` in the pager toggles it. Tables and text files given alone stay in the
text layout unless reading mode is named. Selection and copying are given
up in this mode. The layout is kiri's own, on parley, with the fonts
resvg already loads.

#### 4.15.1 Text (done)

- Paragraphs, headings, lists, quotes, inline code and links in the
  system's sans-serif font at 0.8 times the cell height, with a line
  height of 1.5, in a column of 42 em, drawn as images through Unicode
  placeholders, in print mode and a screen at a time in the pager.
- GitHub's spacing: margins between blocks collapse, list items are
  half a line apart, and a list marker sits on its item's first
  baseline. Colour emoji and a fallback font for each script.
- Inline code without padding spaces, on a background lower than the
  line, breaking only at spaces.
- The page is opaque in the page's colour when the theme gives one, so
  text is blended the same way everywhere, and transparent otherwise, so
  that a translucent window or a background image shows through it. `B`
  in the pager turns the background on and off.

#### 4.15.2 Tables and code (done)

- An area 1.5 times as wide as the text, centred in the window. Text
  and every other block keep the column; tables and code blocks start
  at its left edge and may reach across the area.
- A table is as wide as its content up to that area, and its cells
  wrap as a browser wraps them. The theme's `table_style` draws a
  rounded frame, as the text layout does, or GitHub's grid.

#### 4.15.3 Sideways scrolling (done)

- A table or code block wider than the area wraps, or with
  `--wide scroll` or `W` in the pager scrolls sideways on its own, with
  a fade on each edge that has more and a thin scroll bar.
- One of them is selected: the topmost on screen, or the one a click or
  a search selects. Its scroll bar is brighter. ← → and `H` `L` scroll
  it, and a sideways wheel scrolls the one under the pointer.

#### 4.15.4 Images, diagrams, formulas and other blocks (done)

- Images, diagrams and formulas inside the layout, block and inline,
  from the renderers kiri already has; `$…$` formulas in text style on
  the line's baseline. Scaled pictures are cached between layouts.
- Alerts with GitHub's icons and colours, front matter as a table with
  tables in its cells, and `<details>` in the proportional font too.

#### 4.15.5 Search and links (done)

- `/` marks the matches in the page, in the theme's search colours, or
  black on yellow and orange where the theme leaves them to kiri. A
  match in a band scrolls it into view.
- Links are OSC 8 hyperlinks over the placeholders, which kitty opens.
  Tab and Shift+Tab select a link on the screen, and the status line
  tells where it goes; a link in a band scrolled out of view is reached
  too, and the band scrolls to it. Enter copies the URL of the selected
  link to the clipboard with OSC 52, also in tmux, and the status line
  says so; without a link selected on the screen it scrolls a line as
  before.

### 4.16 Audio

Needs G7.3.2 for time and G7.3.3 for watching.

- An audio file (MP3, Ogg, FLAC, WAV and others) shown as a document:
  its title, artist, length, format, chapters and cover art, read with
  ffprobe when it is installed. It reads the same in the text layout.
- A waveform of the whole file drawn as an image, with the playing
  position marked on it.
- Playback through mpv when it is installed, started, paused and moved
  from the pager.
- Lyrics or a transcript from an `.lrc` file of the same name shown
  beside it: the line being played is marked and kept in view, and Enter
  on a line moves playback there.
- A live visualizer while it plays, such as spectrum bars or a level
  meter, drawn with block characters in the text layout and in pixels
  where the terminal shows images.

## Phase 5 – Beyond kitty

kiri outside a plain kitty window: other terminals, kitty inside tmux, and
headings where kitty's text sizing is missing. Each terminal part names
the terminals it is tested in.

### 5.1 Terminal support

#### 5.1.1 Ghostty and WezTerm

- kitty's graphics protocol in other terminals that speak it: which of
  them have Unicode placeholders, and what the probe finds there.
- Done for Ghostty: it has the placeholders but no text sizing, so images,
  diagrams and formulas draw there, also inside tmux, and headings are
  images. kiri takes Ghostty 1.0 and later; tested with 1.1.3. The
  render-check skill renders in Ghostty with `--terminal ghostty`.
  WezTerm is still to do.
- Found in Ghostty, done: colour schemes that make bright black the
  background colour, such as Bright Lights and Solarized Dark, hid the
  built-in theme's quote bars, rules, list markers and table borders, and
  the pager's status line. With the built-in theme kiri now lightens that
  grey until it shows.

#### 5.1.2 Sixel (done for xterm and foot)

- Sixel images, as xterm, foot, Konsole and others draw them (done): kiri
  turns sixel on in every terminal whose device attributes list it, or by
  `--mode sixel`. Tested in xterm (`-ti vt340`) and foot; the other
  terminals are not measured. The terminal layer chooses the image
  protocol, `draws_images` and `max_image_cells` no longer count kitty's
  placeholder diacritics, and a sixel image has rows in the pager, reading
  mode, running text and print mode.
- xterm keeps only 16 sixel graphics when it redraws its own window, so
  kiri redraws the whole screen on focus-in, resize and Ctrl+L. In print
  mode the older images of a long document can disappear after such a
  redraw.
- `--doctor` lists the image protocols probed (done) and the sixel limits.
- Not drawn on sixel yet: loading bands (B23).
- Still open: `--doctor`'s "Plays animations" row for a terminal that draws
  sixel and also has kitty graphics without placeholders (as WezTerm may),
  animation playback on sixel, sixel behind tmux (under 5.2), the
  terminal's own scrolling, and measuring other terminals. The 32-bit image
  id, so that the slice and picture caches rarely collide, is done.

#### 5.1.3 iTerm2's protocol

- Images through iTerm2's protocol, in iTerm2 and the terminals that
  follow it.

#### 5.1.4 Block characters

- Images as coloured Unicode block characters where no image protocol
  works.

### 5.2 tmux

kitty inside tmux, as in a jailbee container where Claude runs in tmux and
a spec is read in another tmux window. tmux keeps kitty's text sizing out,
so large headings come with 5.3.

#### 5.2.1 Passthrough (done)

- kiri finds out that it runs inside tmux, and wraps kitty's graphics
  sequences so that tmux passes them on: `--passthrough detect|none|tmux`,
  `KIRI_PASSTHROUGH` and `passthrough` in the configuration file, as
  `kitten icat` has it. With `detect`, the default, kiri turns on tmux's
  `allow-passthrough` for its pane, as `kitten icat` does.
- The probe asks kitty through tmux, so `--doctor` shows what kitty can
  do. Today tmux shows the probe's graphics query as the pane's title.

#### 5.2.2 Images in tmux (done)

- Images, diagrams and formulas in tmux, in print mode and in the pager.
- tmux passes images on only while kiri's pane is visible, so the pager
  sends them again when the pane comes back, as a browser tab does.
- Every tmux pane in a kitty window shares kitty's image store, so each
  kiri run uses image ids of its own there; one quitting then leaves the
  others' images alone.
- Done: the pager learns that its pane is shown again from tmux's focus
  events, which need `set -g focus-events on` in tmux.conf (`--doctor`
  shows it), and sends again the images it drew meanwhile. Ctrl+L
  redraws the screen and sends all its images again. After printing,
  and after each pager frame, kiri asks kitty whether it holds the
  images and sends again those tmux dropped (B7).

#### 5.2.3 The mouse (done)

- The pager handles the mouse itself, so the wheel scrolls it inside tmux
  too. Without it, kitty turns the wheel into arrow keys in the pager's
  screen, but tmux with `mouse on` keeps the wheel from the pager: tmux
  3.6 drops it, and older tmux may go into its copy mode.
- Capturing the mouse stops plain text selection in kitty, so it is a
  lasting choice, in the configuration file or on the command line, never
  one that changes by itself.
- Done: the wheel scrolls three lines a notch, and clicks do nothing.
  `--mouse` and `--no-mouse`, `KIRI_MOUSE` and `mouse` in the
  configuration file choose; on unless set. Shift and a drag still
  select text in kitty.

#### 5.2.4 Links in tmux (done)

- Links as OSC 8 hyperlinks inside tmux too. Today kiri tells from the
  environment alone whether links work there, and inside tmux that says
  tmux, so links show as plain text in kitty and Ghostty alike.
- tmux passes links on only when its `terminal-features` give the outer
  terminal `hyperlinks`. kiri asks tmux whether it does, draws links when
  it does, and `--doctor` tells what to add to tmux.conf when it does not.
- Done: inside tmux only tmux's answer decides. tmux 3.6 gives kitty,
  Ghostty and WezTerm no `hyperlinks` by itself, so they need
  `set -as terminal-features ',xterm-kitty:hyperlinks'` (or
  `xterm-ghostty`) in tmux.conf, and a client that attaches after it.

#### 5.2.5 Sixel in tmux

- Sixel behind tmux is weak today: with `--mode sixel` kiri writes raw
  sixel to tmux, and the top of large headings and of images is cut off.
  The flag stays as it is, as it is not a fault of the program when the
  user asks for it. How tmux draws sixel, and whether kiri should wrap or
  skip it there, is looked at in a session of its own.

#### 5.2.6 Animations in tmux (kitty)

### 5.3 Headings without text sizing

Large headings where kitty's text sizing does not work, such as in tmux
or in other terminals, from the best to the plainest.

#### 5.3.1 Headings as images

- Headings drawn as images at the sizes text sizing would give them, in
  the terminal's own font, which kiri asks the terminal for, as
  [mdfried](inspiration.md#other-terminal-markdown-readers) does, or else
  a sans font found on the system.
- Done: headings are images where images are drawn but text sizing is
  not, in the system's sans-serif font; `--headings auto|sized|images|plain`
  chooses, and `--no-images` turns them off. A long heading wraps at its
  words into lines of the one image. Inline code and keys look as in the
  text, and emphasis is in italics. Still to do: the terminal's own font.
- Emoji in colour: today they come out in one colour, the heading's, and
  flags as boxed letters, as resvg draws no colour emoji fonts.
- Right-to-left scripts, such as Arabic or Hebrew, in the right order:
  today each word is drawn right, but the words are placed from left to
  right.
- Strikethrough, `<mark>` and the colour of links, which are plain
  heading text in the images today.
- Faster drawing: each word is parsed as an SVG of its own to measure it,
  and the space is measured again for every heading, so a document with
  a hundred headings parses hundreds of SVGs before it shows. Measuring a
  heading's words in one SVG, and the space once per level, would be
  faster.
- Whether `--no-images` should turn off headings drawn as images too.
  Today it does, but the flag is about a document's images and formulas,
  which one may want hidden while headings stay large.

#### 5.3.2 chafa

- Where images do not work either, headings in block characters through
  chafa.

#### 5.3.3 figlet

- Headings in large letters in the manner of figlet.

### 5.4 HTML backend

Needs G1.10.1, G6.3.2, G7.3.1 and G7.3.2.

- An HTML backend and `kiri serve` for reading in a browser with live
  reload.

### 5.5 Pixel rendering

- A headless browser screenshot (`--browser`). A native layout and
  rasterising engine comes with reading mode (4.15).

### 5.6 Editing support

Needs G1.10.1 for byte ranges and G7.3.1 for handing the terminal over.

- An editor and kiri side by side in kitty with live reload, and a key to
  open the editor at the current position.

### 5.7 Plugins

Needs G5.3, G7.2.2, G7.3.1 and G7.3.3.

Some ideas:

- kiri and a plugin could talk through a data protocol: events go to the
  plugin, and the plugin answers with commands, highlights, changes to
  the status line, images and the like.
- Plugins could be free of language: support for a language, such as Lua
  or Python, would come from a language plugin.
- Lua might be built in.
- A plugin might be added with something like `kiri plugin add <git URL>`
  and kept as a copy locked to a commit's SHA.
- Plugins might not depend on each other, only on a protocol version and
  on capabilities.
- Candidates: an audio visualizer (4.16), a minimap (the document drawn
  narrow beside it, with the part in view marked), the document styles
  of other sites, such as GitLab or Read the Docs, and others.

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

#### G1.3 A size for each part of a line (done)

- Each span of a line has its own size, and the line is as many rows high
  as its tallest span. Wrapping measures spans by their size. Fractional
  sizes (4.10.3) are its first use.
- Text at a fraction is written packed: `d` cells of text in `n` cells per
  escape sequence, so half-size `abc` takes two cells. A fraction's
  numerator is at most 7, the widest one escape sequence can be. Shorter
  text spans in a taller line sit on its first row.

#### G1.4 Images in a line (done)

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
  place; auto-follow uses it to find what changed, and the diff view will
  use it to show it. Nothing uses the source lines yet.

#### G1.7 Lines know their nested block

Covered by G1.10.1.

- The layout marks the blocks inside lists, quotes and alerts as it
  marks those inside `<details>`, so each laid-out line knows the
  innermost block it comes from. Today a change inside a list counts as
  a change of the whole list: auto-follow (4.6.9) goes to the list's
  start, and `most` counts all of its rows.

#### G1.8 One look and layers

- G1.8.1 (done) One look, resolved from the theme in one place and read by both
  layouts, instead of decisions spread over the theme, the text layout
  and reading mode; spacing as a table for each medium, with reading
  mode's GitHub values as the pixel table. Carries out the decisions on
  the differences G1.9.1 finds.
- G1.8.2 (done) Highlights as layers: one list of layers with a paint
  vocabulary and an order, which each layout paints in its own way, for
  the colouring after a reload, search matches, the selected link and
  the selected band. First tests for the cases that meet: a match on a
  coloured line, a match and the selected link on one page, Esc with
  both, and which message the status line shows.

#### G1.9 Layouts side by side

- G1.9.1 (done) A test that lays out documents in both layouts and compares
  what each decided per block, with a list of the differences that are
  meant. Seven differences nobody chose come first: list bullets from
  the third level, the heading rule, heading sizes, numbered task lists,
  dimmed quotes, table rows and table style, and `mark`, `sub` and `sup`.
- G1.9.2 (done) Reading mode's layout split into modules by block type, as the
  text layout's is (G1.1), with every snapshot byte for byte the same.
- G1.9.3 The grid builders and the sharing out of widths written once,
  for cells and pixels alike.
- G1.9.4 One document driver: both layouts lay out whole documents side
  by side, instead of the text layout handing blocks to reading mode.

#### G1.10 Positions and identity

- G1.10.1 Each node of the document gets a stable id, a stable hash and
  its byte range in the source, for blocks and spans.
- G1.10.2 Positions in the document for search, links, colouring and
  changes, the same in both layouts; reading mode's pages get stable ids;
  search runs once over the document's text.
- G1.10.3 Comparing two versions (G1.6) by the nodes' hashes.

#### G1.11 The layout's queries

- G1.11.1 One contract for the lines of both layouts, with a block's
  decorations (bars, frames) on the lines' lead and trail.
- G1.11.2 The layout answers a fixed set of queries: its rows for a
  range, its height, where a range of the document is, what is at a
  cell, its text, and the images it needs. The image plan moves under
  the last of these. With it, a test that lazy layout gives the same as
  laying out everything, before 4.7.2.
- G1.11.3 Layout in pieces by block, with reading mode's tiles cut along
  them.

### G2 Images

#### G2.1 Images from blocks (done)

- Images that come from a block rather than a URL, before formulas and
  diagrams are worked on side by side. `Images` keys images by a URL or a
  block, its language and source, and Mermaid diagrams (4.3.1) are the
  first images from blocks.

#### G2.2 An image encoder (done)

- What has been sent to the terminal, and under which image id, lives in
  `present/encoder.rs`, which the pager keeps for as long as it runs. A
  resize or a reload sends only the images the terminal does not hold at
  that size, and images that a new layout no longer shows, or that
  changed, are deleted from the terminal, as are all of them when the
  pager quits. Animation frames (4.4) and the image protocols of 5.1
  build on it.

#### G2.3 Images kitty drops

- kitty keeps about 320 MiB of images per screen and drops the oldest
  past that. Since the pager does not send an image again, a dropped one
  stays blank until its pixels or size change. It takes a document with
  many large images, such as twenty full-size screenshots on a large
  window. The encoder could count the bytes it holds and, past a budget,
  delete images out of view and send them again when they come back.

#### G2.4 One send for images with the same pixels

Needs G2.6.2.

- Two images with the same pixels share an id, yet each is sent, so that
  print mode writes what it always has. Sending a shared id once saves
  the bytes, also when a reload changes both, but it changes the print
  snapshots where pixels repeat.

#### G2.5 Image keys

- G2.5.1 Image keys that name every input, a test first for each: the
  diagram's page colour and a formula's colours, the heading's look, and
  the code theme and fonts in reading mode's page key. Done except the
  fonts, which come with G2.5.2, and the size a diagram is reduced to,
  which is left out of the key because a resize would drop every diagram
  until the wanted keys reload them.
- G2.5.2 One font database, loaded once by the platform and given to
  what draws, with its fingerprint in the keys that depend on it.

#### G2.6 Embeds and renditions

- G2.6.1 (done) An image block as an embed that offers more than one
  rendition, with every snapshot byte for byte the same.
- G2.6.2 Image keys become rendition keys.
- G2.6.3 Mermaid as an embed.
- G2.6.4 (done) Embeds in a line: images and formulas inside text.

#### G2.7 Image ids in more terminals

These are checks, not new features. The 32-bit image id, with its high byte
in a third placeholder diacritic, was tried in kitty, Ghostty and kitty
under tmux. Still to try, with an id whose high byte is not 0:

- Zellij 0.45 and iTerm2, which may draw the placeholders or not.
- WezTerm and Rio, once their Unicode placeholder support is released.
- A terminal that draws the placeholders and ignores the third diacritic
  would show a wrong image or none; none is known, and kiri keeps no
  fallback for it until one turns up.

### G3 Terminal

#### G3.1 One query for the window size (done)

- One query for the window size in cells and pixels, as fitting the
  window needs its height. `term.rs` used to ask for the width and the
  cell size separately.

#### G3.2 Raw mode for keys (done)

- Raw mode, which lived inside the probe in `term.rs` until the pager
  (4.6.1), made usable for reading keys.

#### G3.3 The image protocol as a capability (done)

- `Capabilities.graphics` names the image protocol instead of a yes or
  no. The probe already keeps the terminal's answers, and `--doctor`
  shows its name and version; the device attributes, where sixel support
  shows, are read too. Behind tmux, sixel counts only when tmux's
  answer and the outer terminal's both list it.

#### G3.4 The terminal layer (done)

- G3.4.1 (done) A terminal layer that does no I/O: what is shown as a scene,
  turned into rows and written to any writer, a frame at a time.
- G3.4.2 (done) A ledger of the images the terminal holds, kept from its
  answers.
- G3.4.3 (done) Only the rows that changed: the presenter keeps what the screen
  shows and writes the rows that differ, a line taller than one row whole.

### G4 Build

#### G4.1 Binary size

- How much each feature and dependency adds to a stripped binary, and
  smaller builds from it, such as RaTeX as a feature of its own for the
  minimal build (4.2.1).

#### G4.2 Ready for CI

- `rust-version` in `Cargo.toml` and a pinned toolchain, before CI (3.2).
  The licence check (`deny.toml`, `make licences`) is done: it fails when a
  dependency brings a licence kiri's MIT licence cannot ship with, such as the
  GPL.

### G5 Settings

#### G5.1 config.rs split (done)

- `config.rs` split into environment, file and resolving, with the
  leftovers of the settings refactor: `ConfigToml` and `Config`, and
  `Choices` and `Settings`, copy each other.

#### G5.2 Names and levels of settings (done)

- Internal names match the keys, such as `diagram_panel` for
  `background_panel`, and environment values are read the same way
  everywhere: `KIRI_NO_IMAGES` takes a wrong value without a warning.
- Each setting declared once, with the module that reads it, what a
  change of it needs (paint only, a new layout, the images again, the
  display, the next start) and its medium (cells, pixels, both), instead
  of being repeated in seven places.
- Some settings exist only as a flag (`--width`) or only in the file
  (`search_case`). General settings, which matter without the pager too
  (`-P`), get a flag of their own and belong on every level. Settings of
  how the pager works, such as `search_case`, need no flag: they change
  in the file and later in the menu (4.14).

#### G5.3 Trust in one place

- G5.3.1 One policy for what kiri may read and run, starting with the ban
  on remote images moved out of the image URL code with no visible
  change. Done at the start of its first user, 4.3.3 or 4.12.8.

### G6 Code

#### G6.1 Smaller cleanups

- The parser drops Markdown elements it does not know without a word.
- One light or dark page type for diagrams and themes.

#### G6.2 Shorter help texts (done)

- `kiri -h` gives each option one short line; `kiri --help` shows the
  details below it.

#### G6.3 Purity by types

- G6.3.1 (done) The decisions kiri makes at start, taken out of `main.rs` into a
  module of their own with unit tests; the answers that need I/O come in
  as values.
- G6.3.2 What layout gives is `Send + Sync`: no `Rc`, caches owned by
  the caller, a pixel type of kiri's own, and a snapshot of the public
  surface.
- G6.3.3 Clippy's list of banned methods as a guard for the parts that
  do no I/O, once G7.3.1 and G7.3.2 are done.

#### G6.4 Map, move and guards

- G6.4.1 (done) A placement map in `docs/architecture.md`: four parts, each
  module's place, and where a new thing goes; module docs at the top of
  the large files; rules on placement in `AGENTS.md`.
- G6.4.2 (done) The modules that belong wholly to one part moved into it, with
  paths and nothing else changed.
- G6.4.3 (done) Guards in `make check`: Clippy's limit on function length, line
  caps for the large files, and a test of which part may refer to which.
  With them, the pseudo-terminal tests wait on a condition instead of a
  fixed time.
- G6.4.4 (done) Loading and the file reload moved out of `main.rs` into a
  module of their own, the seed of the work runner, with no change in
  behaviour.

#### G6.5 One route

- G6.5.1 (done) One file shown as the special case of several, so that
  `main.rs`'s pairs of functions for one file and for several become
  one; the differences between the two are tested first and kept.

#### G6.6 Names that look alike

- Types and functions that share a name, such as `Loading` in
  `typeset::style`, in `images` and in the reading mode's `page/picture.rs`,
  and `layout::loading` beside the test helpers of the same name, are
  renamed or placed so that nobody mixes them up. The aim is less
  confusion, not that every name is unique: a few duplicates will stay
  where they are clear from the module.

### G7 Pager

#### G7.1 Every warning in the pager

Needs G7.2.1.

- The warning mark on the status line and the messages above it, which
  front matter warnings get in 4.6.9, for every warning: a diagram that
  does not draw, an image that cannot be read, from the first load and
  from each reload, with its line where it is known. A setting turns
  them off.
- Messages that go one at a time may become tiring in the long run; how
  many show, and for how long, to be looked at again.
- With the menu (4.14), a key such as `e` opens the warnings in a tab of
  their own.

#### G7.2 The pager's vocabulary

- G7.2.1 (done) One type for warnings, with where each came from, instead of
  the five ways warnings travel now.
- G7.2.2 (done) Commands and keys as data, each with its topic; the help panel
  built from them; Esc as one message that what is open answers.
- G7.2.3 The help as a document in a panel that scrolls, by topic,
  opening at the topic of what is in view (4.14.1).
- G7.2.4 Typed targets in the pager, a link one of them, with following
  a link (4.6.6) and a link's title.

#### G7.3 Control

- G7.3.1 (done) The pager's loop as messages and effects: one step from a
  message to the effects it asks for, and the loop waits, steps and
  draws.
- G7.3.2 A simulated terminal for the pager's tests, with a virtual
  clock and files in memory, instead of most pseudo-terminal tests;
  with it, a fixed test theme and real terminals' probe answers as
  fixtures. Done but for moving the tests: 7 of the pager's 85
  pseudo-terminal tests run in the simulated frontend, and the rest need
  `main.rs`'s wiring, so they move with G7.3.3.
- G7.3.3 A session that holds the sources, their versions and the views;
  the images it wants and what it watches replace the background loads'
  six entries and follow mode's polling, with one watcher for the
  document, its images, the config and the theme. Done but for what
  needs the later items: the session, the work runner (`src/load/queue.rs`),
  the wanted keys, the frontends in `src/frontend/` and one watcher for the
  followed file are in. The config, the theme and the image files are not
  watched yet (4.6.9).
- G7.3.4 The pager's core rules as named functions with tests: finding a
  place, bringing it into view, the status line's place and the stack of
  panels.
- G7.3.5 A screen model for the simulated terminal that knows kitty:
  the byte stream turned into a grid with image placements and scaled
  text.

## Uncategorized (extras)

Ideas not yet placed in a part, from small polish to features; a line or
two is enough. Also called extras: an idea "for the extras" goes here.
Each time a part of the roadmap is finished, the list is
sorted: an item moves to a part or to Bugs, or is dropped, and done items
go.

- How task list boxes look, as a setting. kiri draws ☐ and ☑, but
  Ghostty's built-in font has only ☐, so ☑ comes from another font,
  larger than ☐. ☐ and ☒ are the same size in Ghostty and kitty, but some
  fonts lack ☒.
- A Prisma grammar for code blocks, which two-face does not have. Little
  asked for: weigh it before adding a grammar to the binary.
- A side-by-side view: the Markdown source beside the rendered document,
  each part across from the other, so the source and what it becomes can
  be compared.
- Finer spacing between the words of a heading at a fraction. A space
  can only grow by whole cells of the heading's scale, so a short heading
  such as "Level three" at 1 3/4 has one space about twice as wide as
  normal. A space at the normal size would halve the step, but leaves an
  empty cell in the rows below it, under backgrounds and marks too. A
  heading allowed to start a few pixels from the margin would get even
  spaces for less.
- Faster wrapping of text at a fraction. The wrapper packs the whole line
  again for each word, and packing chooses all its groups together, so
  the time grows with the square of the line: a heading takes 1 to 4 ms
  in a release build, and one with a 276-byte URL at half size 124 ms.
  Packing only the last words again would be enough, as groups further
  back no longer change.
- A menu on the right mouse button, as tmux has one: choices for the
  place clicked, such as copying a code block or following a link. Inside
  tmux, how it goes with tmux's own right-click menu.
- A minimap beside the document in the pager: the whole document drawn
  too small to read, with the part in view marked on it, so the reader
  sees where they are and remembers places by their shape. Search
  matches, headings and the changes that following finds could be marked
  on it too. An image where the terminal draws images, braille
  characters elsewhere, as code-minimap draws them. Needs G1.11.2
  and G7.3.3.
- Documentation sites read without a build step, as Read the Docs shows
  them: a directory of Markdown files, or one described by an
  `mkdocs.yml`, read with its navigation beside the page in the pager,
  or served to a browser by `kiri serve` (5.4). It would build on
  several files (4.9.2), heading anchors (4.10.1), the table of contents
  (4.6.5) and resolving links across a vault (4.13). Needs G7.2.3,
  G7.2.4 and G7.3.3.
- The current search match fading in reading mode as it does in the
  text layout. The marks are drawn into the page's images, so each step
  of the fade would draw and send the page again.
- A faster first `M` on a document opened as text: reading mode's layout
  could be made ready in the background while the text is shown.
- A setting that keeps tables and code blocks in reading mode to the
  column's width instead of the wider area (4.15.2).
- A small animation while a page, an image or anything else loads: the
  `kiri` badge at the left end of the status line moves its capital
  letter along the name, `Kiri`, `kIri`, `kiRi`, `kirI`.
- Books without DRM. Some ideas: EPUB with its chapters as the table of
  contents, the reading place kept for each book and footnotes in a
  panel; DjVu as scanned pages, perhaps drawn by `ddjvu` with its text
  layer for search, as PDF (4.12.2) shows pages as images; Kindle
  formats through Calibre's `ebook-convert`. Some EPUB 3 books, such as
  children's books, carry sound.
- Choosing the language of a code block when its name could mean several,
  as a file extension that more than one language uses: `pro` is Prolog,
  QMake and IDL, and kiri highlights it as the most used one. Some ideas:
  the pager offers the others for the block in view, by a key or from the
  menu (4.14), and the choice is kept for the document.
- A `<details>` written in one HTML block with more than one element in
  it, such as two `<pre>` blocks, or a heading followed by text. Only a
  body of exactly one element is read as an image, a heading or a code
  block; any other body is its text as one paragraph. The body could be
  split into its top-level elements, each read as an HTML block of its own
  is (4.10.2).
- A document's front matter steering how kiri shows it, such as the
  layout, text or reading mode (4.15), since reading mode is on by
  default and some documents describe one look only. It builds on the
  presentation settings in front matter of 4.8.2, whose keys are still
  open. Some ideas: a key in the front matter names the layout the
  document is meant to be read in; the configuration file lists which
  keys a document may steer, a list of allowed control commands that is
  empty unless the reader fills it in, so a document cannot change how
  kiri behaves on its own; and what the reader gives on the command line
  or in the environment still wins, as 4.8.2 says. A first guess at each
  document's own setting, from the examples:
  `README.md` text or reading; `examples/README.md` text;
  `S01-text.md`, `S02-lists-and-quotes.md`, `S03a-tables.md`,
  `S05-alerts.md`, `S06a-images.md` and `S08-formulas.md` text, as they
  describe the text layout; `S09-reading-mode.md` reading; `S04a-code.md`
  and `S07a-diagrams.md` either. The key's name and where else it would
  help, such as a heading's size or the diagram panel, are open.
- Sentences in the examples that describe only the text layout and may
  not hold in reading mode, which is the default now (found when
  reading mode became the default, not yet changed). `S01-text.md`:
  the heading levels "each level after it a quarter smaller" and "bold
  and blue, without an underline"; the headings "drawn at a fraction of
  the cells they take" with the words kept whole and "starts a few
  pixels to the right"; "With kitty's text sizing a `<sub>` or `<sup>`";
  the marked text "in your terminal's own palette colours" and "in
  reverse video without colours". `S02-lists-and-quotes.md`: bullets,
  numbers and quote bars "grey, also where the terminal's colour scheme
  makes its grey the background". `S03a-tables.md`: "wider than the
  default text width of 100 cells", a cell "Scaled with OSC 66 in kitty"
  and the selected link "in reverse video". `S06a-images.md`: sizes in
  columns, rows and cells ("about 32 columns wide and 12 rows high",
  "not enlarged"), where reading mode keeps an image at its natural size
  in CSS pixels, shrunk to the column. `S08-formulas.md`: a formula "in
  the middle of the text's width", "a little larger than the text around
  it" and "four rows high". `S04a-code.md` and `S07a-diagrams.md` have
  nothing clearly wrong. Some ideas: each says which layout it
  describes, perhaps through the front matter above, and the other
  layout gets its own sentence where it differs.
- The status line: rethink how it is laid out and what it tells (the
  animation pill, the selected picture and link, the key hints). Tried
  with the animations (4.4): the pill first before the position and the
  key hints, and then last at the right edge; the selected picture or link
  after the file's name, in the colour of its frame, with no symbol; and a
  drop order for a narrow window (the pill's time, half its bar, the
  start of the file's name, the hints and the position, the selected
  picture or link, the pill), with the search term never giving way to the
  pill or the selection. The line has become crowded, and the pieces fight
  for room, and the key hints are left out while the pill is on the line;
  a click on the pill, which the line has no hit test for, would
  want one.
- kiri as the renderer of a Claude Code pane. A prototype mod shows the
  files Claude touched in a pane beside the chat, the chosen one drawn as
  PNG strips in the pane, by a small Python renderer for now. kiri could
  take its place. Some ideas, most needed first: an output that writes the
  document as PNG strips of at most a given number of rows, for a given
  width in cells and cell size, into a cache folder, and lists them as
  JSON, without asking a terminal anything; the same content as text where
  a pane draws no images (`--format text`); drawing again only the blocks
  that changed; a map beside the strips of links, headings and source lines
  by row, for clickable links and a table of contents; changed paragraphs
  marked with a bar at the edge; a mode that stays running and takes
  requests; `kiri --doctor --json` for the cell size; and a small source
  of facts for the pane (kiri's version, the latest release, the last
  `make check`).
- Many large images in the pager: it holds every decoded image, so a long
  list of photos takes gigabytes (24 photos of 4845 by 2980 pixels take
  about 1.4 GB). Read and drop them as the pages scroll, or keep them at
  the size they are shown.
- The last-column rule of sixel images in text (`reaches_last` in
  `src/present/output.rs`) compares with the layout's width, which is
  smaller than the window with `--width`. A sixel slice that ends at the
  layout's last column may then lose the row's trailing fill; nobody has
  seen it, so it is a suspicion, not a bug.
- Two things `--doctor` shows that nobody has explained: kitty on an
  800x600 virtual display reports a window of 284 by 70 cells (2556 by 1400
  pixels), and foot reports a window of 0 by 0 pixels. They may be right
  for those setups, or a fault in how kiri reads the window size.

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

#### B3 The examples fit test draws no diagrams (fixed)

The test that the example documents fit the window at every width,
`the_visual_check_documents_fit_every_mode`, loads no diagram renderer,
so it never draws a diagram. A drawn diagram, or the red error below one
that fails, could reach past the window's edge without the test noticing.

Fixed: the test draws Mermaid with a stand-in wider than any window it
tries, and the diagram that fails to draw gets a reason long enough to
wrap. Both fit; nothing had reached past the edge.

#### B4 Gaps in a heading of wide characters

A heading at a fractional size packs its text in groups of whole cells,
and a wide character never crosses the end of a group. Wide characters
such as CJK then end a group early and the group rounds up, so a heading
like `### 日本語の見出し 🇫🇮` at 1 3/4 shows gaps between its groups. Minor.

#### B5 Tags around a formula block show as source

A formula block inside an inline HTML tag, such as `<sub>$$z$$</sub>`,
is drawn as a formula, but the tags around it show as source: each
becomes an HTML block of its own, as the wrapper tags in B2 do.

#### B6 A gap inside a word of a heading at a fractional size (fixed)

Fixed: kitty draws each group a few pixels narrower than its cells, and
kiri now chooses and aligns the groups of a text together, so that the
spare pixels fall between words and spread over them. A word longer than
two groups still has a gap inside, and a heading that starts with a word
longer than one group starts a few pixels to the right.

A level-three heading at 1 3/4, "Level three with `code` and
*emphasis*", showed as "Level th ree with" in a render-check at 50
columns: a gap inside "three", where one group of packed cells ends and
the next begins. B4 has gaps like this around wide characters; here the
word is plain ASCII. The maintainer saw the gaps in a real kitty window
too.

#### B7 Images lost inside tmux in a long document (fixed)

Fixed in 5.2.2: kiri asks kitty afterwards whether it holds each image
and sends again the ones it does not.

`kiri examples/S06a-images.md` in tmux, in the maintainer's kitty
attached to a jailbee container, left three of the five images under
"PNG variants" as empty space of the right height. kitty answered
`ENOENT` for their ids, so their data never reached it, and
`tmux refresh-client` did not bring them back. The same section on its
own drew whole. tmux drops output for a client that cannot keep up, and
passthrough data in the dropped part is lost; that is the likely cause.
Resending images, as 5.2.2 plans, or checking after printing that kitty
has them, would fix it.

#### B8 Uneven word spacing in headings drawn as images

A heading drawn as an image (5.3.1) starts each word on a whole cell, so
that its lines can be cut between words. A gap is then a space plus up to
one cell, and at the smaller levels the difference shows, as in
"Level  three with code  and". The image is drawn before layout knows
where the lines break, so the fix has to work for every place a line
may end.

#### B9 Late answers from kitty typed into the shell

Not seen yet; found in review. After printing through tmux, kiri waits
half a second for kitty to say whether it holds the images (B7). An
answer that comes later, as over a slow connection, reaches the shell
once kiri has exited, and shows there as text such as
`^[_Gi=…;OK^[\`. Waiting longer when part of the answers came, or
reading away what arrives just before exiting, would fix it.

#### B10 Text at a fraction cut too early (fixed)

Found in review. `cut` in `src/typeset/wrap.rs` measures text one
grapheme at a time, and at a fraction each grapheme rounds up to a whole
cell, so a heading or a sup cut to fit takes fewer letters than fit.
Measuring the text kept, as `width` does, would fix it.

Fixed: `cut` measures the text it keeps as a whole. Only the status
line, the warnings and the help panel cut text so far, and none of it is
at a fraction, so nothing changes on the screen today.

#### B11 Rows of reading mode broken in a container's tmux (fixed)

Fixed: inside tmux, kiri asks kitty for its cell size through
passthrough and takes kitty's answer over the window size. The
container's terminal does not tell its size in pixels, so tmux gave the
pane 16 × 32 pixels a cell where kitty's were 10 × 21. kiri drew each
part of a page for the wrong cells, and kitty, fitting the part into its
cells, left gaps above and below it; where two parts met, rows were cut.
Neither the tmux version nor `RGB` played a part. `--mode kitty` asks
the terminal nothing, so inside such a tmux it still takes tmux's guess.

In the maintainer's kitty, reading mode shows a few broken rows in
tmux inside a jailbee container (tmux 3.6): the lower part of a line of
text is cut, the rest of the row starts a new line, and its text is
gone, as near "fifths of a row's height" in S09. Ctrl+L does not mend
it. It does not happen in the host's tmux (3.8), nor in the container
without tmux, and could not be reproduced in kitty on a virtual
display with either tmux. That tmux client's features were
`bpaste,ccolour,clipboard,cstyle,focus,title`, without `RGB`, which
the image ids in the placeholders' 24-bit colour depend on. A row on
the edge of two 30-row parts of a page is another suspect.

#### B13 Wrong cell size after a font change in a container's tmux (fixed)

Fixed: when the window changes size, the pager asks the terminal for its
cell size with `CSI 16 t`, through passthrough inside tmux, and lays the
page out again if kitty's answer differs from the cells it has. Where
the window tells real pixels outside tmux, as kitty on the host does, it
asks nothing.

Right after kitty's font size changed, tmux often still answered its own
guess of 16 × 32 pixels a cell, when the pager asked tmux for its
client's cell size, in 5 of 8 tries on a virtual display. The pager kept
the cells it had or took tmux's guess, so the page stayed at its old
scale with the bottom of the window empty, and a key did not mend it.
Outside tmux, in a container whose window tells no pixels, the pager
never followed a font change at all. The terminal itself always knows
its cells, and its answer comes among the keys. The pager lays the page
out for the cells it has at once and again when the answer differs, so a
frame for the old cells may show for a moment; waiting for the answer
first would delay every resize by a terminal that never answers.

#### B14 Two touching links to one address split at a line break (fixed)

In the pager's text layout, two links to one address that touch, with a
style change between them, such as `[aaaa](u)**[bbbb](u)**`, are one
link for Tab on one line but two when a line break falls between them.
Without the style change they stay one. Reading mode keeps them one. A
mark that the layout kept for touching parts across a break would mend
it.

Fixed: a link that touches one to the same address reuses that link's
`Rc`, so the pager joins the two across a break as it joins a link that
wraps.

#### B15 Tab stops at a link without an address (fixed)

`[text]()` is a Tab stop in the pager, in both layouts, though the
terminal shows no link there: an empty address in OSC 8 closes a link.
Enter already scrolls past it instead of copying, and Tab could skip it
too.

Fixed: Tab and Shift+Tab skip a link without an address in both
layouts.

#### B16 A bar in a table cell's text splits a wrapped link

To keep a link wrapped inside a table cell one link, the pager's text
layout finds the cells by their borders, `│` and `▎`. A bar in a cell's
own text before the link, such as a bold `│`, counts as a border too,
and the link splits in two there. Mending it needs the layout to mark
its borders.

Fixed already: a bar in the cell beside it, such as `` `│` ``, no
longer splits the link, as the pager compares the column of the last
border instead of counting borders.

#### B17 Headings, quotes and task lists look different in the two layouts

The text layout and reading mode decide some styles on their own, and
the parity test (G1.9.1) lists them as drifts. The aim is to bring the
two together, moving either one; reading mode may also keep styles of
its own, which is not decided yet.
- The heading rule: reading mode draws it under levels 1 and 2 always
  and ignores the theme's `rule`; the text layout draws none with the
  built-in theme. The text layout could move toward reading mode.
- Heading sizes: reading mode has GitHub's sizes and ignores a theme's;
  the text layout's are larger. Its sizes could move closer to reading
  mode's.
- A numbered task list: reading mode drops the number, as GitHub does;
  the text layout keeps it. Kept for now.
- Quote text: reading mode dims it, as GitHub does; the text layout
  keeps the body colour. Kept for now.

#### B18 Table rows in reading mode

Reading mode knows only stripes, on or off: the text layout's `lines`,
`indent` and `underline` styles of `table_rows` all become stripes
there. `indent` and `underline` are about the text layout's wrapped
lines and can stay its own; `lines` could draw lines between the rows
in reading mode too. `table_style` works only in reading mode.

#### B19 Two gaps in catching signals before the pager opens (fixed)

Fixed: the guard catches the signals before the wait opens its reader of
keys, and hands a signal it noted on to the pager's screen, which then
ends kiri by it once the terminal is restored.

Both are older than the session work and were found in its final review.

- When the reader skips the wait for the loads with Space or Enter, the
  signal guard stays held until the pager's screen takes over. The pager
  lays the whole document out in between. A SIGINT, SIGTERM or SIGHUP that
  comes in then is noted, nobody reads it, and it is thrown away: the pager
  opens and kiri ends with status 0 after `q`. Mending it: in the closure
  that runs when the screen has caught the signals (`opening` in
  `src/frontend/pager.rs`), raise the signal the guard noted before
  dropping the guard.
- `wait_or_skip` switches the terminal to non-canonical mode without echo
  before the guard catches the signals. A signal in the few microseconds
  between ends kiri by its default action and leaves the terminal in that
  mode. Mending it: catch first, then open the key reader, and release the
  guard where there is no key reader.

#### B20 Code blocks in a language GitHub knows by another name are not highlighted (fixed)

kiri looks a fence's language up among syntect's bundled grammars by name
or file extension (`find_syntax_by_token` in
`src/typeset/layout/highlight.rs`). A name that is neither stays plain
code, though GitHub highlights it. Seen: `shell`, the common name for
shell commands in READMEs; `sh` and `bash` work. Other names to check
the same way are `zsh`, `console`, `shellsession`, and languages
syntect's set may lack altogether, such as TypeScript and TOML. Mending
it: an alias table from GitHub's names to the bundled grammars, and a
test that the names the docs and examples use all highlight.

Fixed: kiri loads two-face's syntax set, which has 213 grammars, among
them TOML and TypeScript, instead of syntect's bundled one. A fence's
language is looked up first in a table of the names GitHub knows, made
from GitHub Linguist's `languages.yml`, then by syntect's own lookup,
then in the table's file extensions. `shell`, `typescript`, `csharp`,
`jsonc` and the file extensions such as `mjs` find their grammar there. A
Linguist language gets a grammar only when the grammar's scope or name is
the language's own, or when kiri says which grammar fits it, as for
`shell` and Fortran; a grammar that lists one of the language's file
extensions is not enough, so Prolog, COBOL and Smalltalk stay plain, as do VBA and Mojo, whose names are also
extensions of Vim script and XML. The table never points to a hidden
grammar, though syntect's own lookup may still find one by its name, such
as `javadoc`. A file extension that several languages share goes to the
most used of them, as a list in the generator decides, so `es` is
JavaScript; where that language has no grammar, as for `spec` (RPM Spec)
and `pro` (Prolog), the extension stays plain. A fence written with the
language's name, such as `erlang`, gets that language's own grammar.
kiri adds `console`, `shellsession`, `shell-session` and
`shellscript` (commands with their output, shown with the Bash grammar),
`containerfile`, `jsonl`, `dotenv`, `txt`, `proto3`, `h` (C, where syntect
alone gives Objective-C) and `fs` (F#). `pwsh` is not among them, as
two-face has no PowerShell grammar. The table is generated by an ignored
test, and a test checks that every language the docs and examples fence
is highlighted. The full build grew by about 0.6 MB and the minimal build
by the same.

#### B21 `jobs_stopped_while_they_wait_never_start` fails now and then under load

`load::queue::tests::jobs_stopped_while_they_wait_never_start` in
`src/load/queue.rs` fails in a full `make check` when the machine is
busy, and passes alone. Seen twice, on the branch that moved the merman
version to `Cargo.lock` and updated the dependencies (2026-11-03), at a
load of about 6 to 7, in the run without the default features:

```
assertion `left == right` failed
  left: ["a", "b", "c"]
 right: ["a", "c"]
```

Job `b` was halted while it waited and started anyway. The test alone
ran 300 times and its module 20 times without a failure, and the
branch changed nothing in `src/load/`, so it is older than that branch.
It may be a real race in `Loads::halt`, where a gated job that is just
being started wins over the halt, and not only the test. Mending it:
find out whether `halt` can lose that race; if it can, close it in the
queue, and if not, make the test wait for the state it asserts instead
of for time.

#### B22 Under tmux the pager loses its heading images after a window is opened and closed

In tmux, open a second window with `C-b c` and close it with `exit`: the
pager sometimes comes back without its heading images. It is not every
time: on 2026-11-07 master lost them in 3 of 6 runs, so the terminal-layer
work did not bring it in. Ctrl-L brought the images back in every run.

kiri writes nothing between the two steps, and kitty still holds the image
data and the placements, so the cells tmux redraws are sometimes not
shown by kitty. A guess at a fix: when tmux reports focus back, redraw the
whole screen even if nothing was lost, without resending the images. It
needs trying on a real desktop, as a virtual display could not confirm
that tmux sends the focus event.

#### B23 No loading bands on sixel

While an image loads, kitty shows a moving band in its place. On sixel
there is none: the status line tells the progress, as it does under tmux.
The band was left out of 5.1.2 because it moves every tick, and xterm keeps
only 16 sixel graphics: a slice drawn again takes a new place, and only
scrolling off frees one, so a band would push the page's own images out
the next time xterm redraws the window. That is reasoned from the spike's
measurements, not tried. The first thing to do is to try a band in xterm
and in foot, and see whether it flashes, is slow, or loses images. Whether
animation playback (4.4) helps depends on how it redraws a frame, which
needs the same measurement. The rule is one function,
`Capabilities::places_bands`, so drawing bands on sixel is a change to it
and to the writer.

When it is solved, the pager's tests choose by the rule `places_bands`
and not by a test switch for the terminal (`on_sixel`): two byte-level
checks in `src/pager/tests/loading.rs` still pick the terminal that way.

### Upstream bugs

Bugs in projects kiri uses. kiri works around most of them until they
are fixed there; an entry says when it does not.

#### kitty on a virtual display leaves images undrawn

kitty 0.47.1 on Xvfb, drawing with llvmpipe, often leaves the images of
a large first frame undrawn: the placeholders are on the screen, kitty
answers that it holds the images, and they show as soon as something
wakes kitty, such as `kitty @ ls` or `kitty @ get-text`. A write to the
terminal does not wake it. It happens without tmux and with
`kitten icat` too, so it is kitty's, and it has not been seen in the
maintainer's own kitty. Reading mode in the pager, whose first frame sends
two large images, came up blank in more than half of its starts.

This was B12, a blank pager after a resize under load, after a font
change and at start in tmux, all seen on a virtual display. kiri does
not work around it. The render-check skill waits for the pager by
asking kitty for its text, which wakes kitty, so its screenshots are
not blank.

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
