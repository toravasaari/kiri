# Architecture

## Goal

Show Markdown in a terminal as close to a browser rendering as the terminal
allows, without giving up what makes a terminal good: text stays text, so it
can be selected, searched and copied, and it uses the user's own font and
colours. Pictures are used only where text cannot do the job.

## Pipeline

```
Markdown ──parse──▶ document model ──▶ backend ──▶ output
                                         ├─ terminal  plain, ANSI or kitty
                                         ├─ HTML      (later)
                                         └─ pixels    (later)
```

- **Parse:** [comrak](https://crates.io/crates/comrak) turns the source into
  a CommonMark + GitHub Flavored Markdown syntax tree. kiri reads the HTML
  `<img>` tags in it itself, so an image means the same in the model
  whether Markdown or HTML wrote it.
- **Document model:** kiri's own tree of blocks and inline spans. It holds
  what the document means (heading level 2, emphasis, image with alt text),
  not how any terminal draws it. Other input formats only need a converter
  into this model.
- **Backend:** turns the model into output for one target, given the target's
  capabilities and width. Backends never look at Markdown syntax.

Keeping the document model between the parser and the backends is the main
design decision. It is what lets a browser view, pixel rendering or other
terminals arrive later as new backends instead of rewrites.

A file given on the command line need not be Markdown: `src/input.rs`
tells an image from a document by its extension and first bytes, and an
image skips the parser and is drawn on its own, as large as fits the
window.

The document can also come from standard input, which errors call
`<stdin>`, or from a directory, which stands for its README. Both are
resolved in `src/main.rs`, before `src/input.rs` looks at the bytes, so
an image piped in is shown on its own as well.

There is one terminal backend, not one per terminal. It lays the model out
as styled lines of a fixed width, then writes those as bytes. What it may
use comes from the capabilities it is given: plain text for pipes, colours
and attributes for any modern terminal, and kitty's protocols on top where
they work.

The pager shows the same styled lines on the terminal's alternate screen.
`src/pager/view.rs` chooses the lines that fit in the window, a scaled
line whole or not at all, and moves them; `src/pager/keys.rs` turns the
bytes the terminal sends into keys and keys into moves; `src/pager/mod.rs`
draws each frame, row by row through the backend's writer, and lays the
document out again when the window changes size. The terminal itself, the
alternate screen, raw mode and reading keys from the controlling terminal,
is opened in `src/term.rs`.

## Terminal capabilities

`src/term.rs` is the only place that talks to the terminal to find out what
it can do: size in cells and pixels (`TIOCGWINSZ`, or the cell size from
`CSI 16 t` where that has no pixels), whether the text sizing and
graphics protocols work, the background and palette colours, and the
terminal's name and version (XTVERSION). 24-bit colour, hyperlinks and underline
styles follow from either protocol working. Images need the graphics
protocol's Unicode placeholders, which no query reveals; kiri assumes them
when the graphics protocol works and the terminal is kitty 0.28 or later,
or when both protocols work, since text sizing came to kitty long after
placeholders. Elsewhere images are alt text.
Without them, 24-bit colour can still come from `COLORTERM` and hyperlinks
from `TERM_PROGRAM` or `VTE_VERSION`; underline styles come only from the
probe. `NO_COLOR` turns colours off in every mode. These variables come
in as part of `config::Environment`, read once at start-up with kiri's
own, so the rules are tested without the process environment. The result
is a plain value passed to the renderer, so tests can render for any
terminal without having one. kiri asks nothing when it runs in the
background of its terminal, where raw mode would get it stopped.

`kiri --doctor` reports what detection found and where each setting came
from. `src/term.rs` and `src/config.rs` hand it the detection, the
settings and the file paths; `src/doctor.rs` checks which files exist and
writes it all as a Markdown document, which kiri renders like any other or
writes as it is into a pipe.

## Terminal features used

| Feature | Escape sequence | Written by | Used for |
|---|---|---|---|
| Text sizing | OSC 66 (kitty 0.40+) | `src/kitty/text.rs` | Headings in a larger font |
| Graphics | APC `_G` | `src/kitty/graphics.rs` | Images, later formulas and diagrams |
| Unicode placeholders | U+10EEEE with graphics | `src/kitty/graphics.rs` | Images that scroll with text, work in pagers and tmux |
| Protocol queries | OSC 66 tests, APC `_G` query | `src/kitty/query.rs` | Telling whether kitty's protocols work |
| Hyperlinks | OSC 8 | `src/render/output.rs` | Clickable links |
| Text attributes and colours | SGR | `src/render/output.rs` | Bold, italic, colours |
| Styled underlines | SGR 4:x | `src/render/output.rs` | Heading underline styles |
| Cursor movement, erase | CSI C, CSI K, CSI H, CSI 2J | `src/render/output.rs`, `src/pager/mod.rs` | Backgrounds beside scaled headings and across the width; the pager's rows |
| Alternate screen, cursor | CSI ?1049h/l, CSI ?25l/h | `src/term.rs` | The pager's screen of its own |
| Synchronized output | CSI ?2026h/l | `src/pager/mod.rs` | Pager frames without flicker |
| Cell size query | CSI 16 t | `src/kitty/query.rs` | The cell size in pixels where the window size has none |
| Generic queries | CSI 6n, OSC 11, OSC 4, CSI > q, CSI c | `src/term.rs` | Cursor position, background and palette colours, terminal version, end of the answers |

Sequences that only kitty's protocols define live under `src/kitty/`:
text sizing, graphics, Unicode placeholders and the queries that detect
them; `src/term.rs` sends those along with the generic queries it spells
out itself. OSC 8 hyperlinks, SGR attributes, underline styles and cursor
movement are general terminal features that `src/render/output.rs` writes
itself; for scaled text it calls `src/kitty/text.rs`, and for images
`src/kitty/graphics.rs`.
Underline styles began in kitty but are now common; kiri uses them only
when the probe finds kitty's protocols or `--mode kitty` asks for them.

## Modules

```
src/main.rs                command line (clap)
src/lib.rs                 library root
src/input.rs               whether a file is a document or an image
src/model.rs               document model
src/parse/mod.rs           comrak tree -> document model
src/parse/html.rs          HTML <img> tags and their attributes
src/term.rs                capability detection, the only terminal queries,
                           the pager's screen and keys
src/config.rs              settings: options, environment, configuration file
src/doctor.rs              the --doctor report as Markdown
src/theme.rs               colours and sizes, theme files
src/images/mod.rs          loading and decoding the images a document shows
src/images/url.rs          data URLs, file URLs and paths, read up to a limit
src/images/svg.rs          SVG images and the fonts for their text
src/color.rs               colours and underline styles for theme and render
src/render/mod.rs          document model -> terminal output
src/render/layout/mod.rs   document model -> styled lines of a fixed width
src/render/layout/code.rs  code blocks and their language label
src/render/layout/list.rs  bullet, numbered and task lists
src/render/layout/quote.rs block quotes
src/render/wrap.rs         word wrapping for styled text
src/render/table.rs        tables with box-drawing characters, or stacked
src/render/image.rs        image sizes in pixels and terminal cells
src/render/style.rs        styled text between layout and output
src/render/output.rs       styled lines -> bytes (SGR, OSC 8, CSI C/K, OSC 66)
src/render/highlight.rs    syntax highlighting for code blocks (syntect)
src/pager/mod.rs           the pager: frames and the loop of keys
src/pager/view.rs          the lines a window shows, and scrolling
src/pager/keys.rs          keys from the terminal and what they do
src/kitty/mod.rs           kitty-specific escape sequences
src/kitty/text.rs          text sizing protocol (OSC 66)
src/kitty/graphics.rs      graphics protocol with Unicode placeholders
src/kitty/query.rs         queries that detect kitty's protocols
crates/kiri-image-utils    raster image decoding and scaling, a crate of its
                           own in the workspace
```

Decoding raster images and scaling them live in their own crate, a path
dependency of kiri, because generic code is compiled in the crate that
calls it: the BMP, TGA and QOI decoders of the image crate are generic, and
so is most of fast_image_resize. As a separate crate it can be optimised in
debug builds, where it decodes and scales a large photo in tenths of a
second at most instead of seconds, while kiri's own code stays easy to
debug.

## Choices

- **Rust:** one fast binary with no runtime, and mature crates for each part:
  comrak (Markdown), syntect (syntax highlighting), image and resvg (raster
  and SVG images), typst (formulas), rustix (terminal modes and pager input).
- **Text first:** the alternatives were rendering the whole document as an
  image, through a headless browser or a custom layout engine. Both look
  closer to a browser but lose text selection, search and the terminal font,
  and the browser route adds a heavy dependency and seconds of start-up. They
  remain possible later as backends.
