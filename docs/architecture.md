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
  a CommonMark + GitHub Flavored Markdown syntax tree.
- **Document model:** kiri's own tree of blocks and inline spans. It holds
  what the document means (heading level 2, emphasis, image with alt text),
  not how any terminal draws it. Other input formats only need a converter
  into this model.
- **Backend:** turns the model into output for one target, given the target's
  capabilities and width. Backends never look at Markdown syntax.

Keeping the document model between the parser and the backends is the main
design decision. It is what lets a browser view, pixel rendering or other
terminals arrive later as new backends instead of rewrites.

There is one terminal backend, not one per terminal. It lays the model out
as styled lines of a fixed width, then writes those as bytes. What it may
use comes from the capabilities it is given: plain text for pipes, colours
and attributes for any modern terminal, and kitty's protocols on top where
they work.

## Terminal capabilities

`src/term.rs` is the only place that talks to the terminal to find out what
it can do: size in cells and pixels (`TIOCGWINSZ`), whether the text sizing
and graphics protocols work, and the background colour. 24-bit colour,
hyperlinks and underline styles follow from either protocol working.
Without them, 24-bit colour can still come from `COLORTERM` and hyperlinks
from `TERM_PROGRAM` or `VTE_VERSION`; underline styles come only from the
probe. `NO_COLOR` turns colours off in every mode. The result is a plain
value passed to the renderer, so tests can render for any terminal without
having one.

## Terminal features used

| Feature | Escape sequence | Written by | Used for |
|---|---|---|---|
| Text sizing | OSC 66 (kitty 0.40+) | `src/kitty/text.rs` | Headings in a larger font |
| Graphics | APC `_G` | `src/kitty/` (later) | Images, formulas, diagrams |
| Unicode placeholders | U+10EEEE with graphics | `src/kitty/` (later) | Images that scroll with text, work in pagers and tmux |
| Protocol queries | OSC 66 tests, APC `_G` query | `src/kitty/query.rs` | Telling whether kitty's protocols work |
| Hyperlinks | OSC 8 | `src/render/output.rs` | Clickable links |
| Text attributes and colours | SGR | `src/render/output.rs` | Bold, italic, colours |
| Styled underlines | SGR 4:x | `src/render/output.rs` | Heading underline styles |
| Cursor movement, erase | CSI C, CSI K | `src/render/output.rs` | Backgrounds beside scaled headings and across the width |
| Generic queries | CSI 6n, OSC 11, CSI c | `src/term.rs` | Cursor position, background colour, end of the answers |

Sequences that only kitty's protocols define live under `src/kitty/`:
text sizing, graphics, Unicode placeholders and the queries that detect
them; `src/term.rs` sends those along with the generic queries it spells
out itself. OSC 8 hyperlinks, SGR attributes, underline styles and cursor
movement are general terminal features that `src/render/output.rs` writes
itself; for scaled text it calls `src/kitty/text.rs`.
Underline styles began in kitty but are now common; kiri uses them only
when the probe finds kitty's protocols or `--mode kitty` asks for them.

## Modules

```
src/main.rs                command line (clap)
src/lib.rs                 library root
src/model.rs               document model
src/parse.rs               comrak tree -> document model
src/term.rs                capability detection, the only terminal queries
src/theme.rs               colours and sizes, theme files
src/color.rs               colours and underline styles for theme and render
src/render/mod.rs          document model -> terminal output
src/render/layout.rs       document model -> styled lines of a fixed width
src/render/wrap.rs         word wrapping for styled text
src/render/table.rs        tables with box-drawing characters, or stacked
src/render/style.rs        styled text between layout and output
src/render/output.rs       styled lines -> bytes (SGR, OSC 8, CSI C/K, OSC 66)
src/render/highlight.rs    syntax highlighting for code blocks (syntect)
src/kitty/mod.rs           kitty-specific escape sequences
src/kitty/text.rs          text sizing protocol (OSC 66)
src/kitty/query.rs         queries that detect kitty's protocols
```

## Choices

- **Rust:** one fast binary with no runtime, and mature crates for each part:
  comrak (Markdown), syntect (syntax highlighting), image and resvg (raster
  and SVG images), typst (formulas), crossterm (pager input).
- **Text first:** the alternatives were rendering the whole document as an
  image, through a headless browser or a custom layout engine. Both look
  closer to a browser but lose text selection, search and the terminal font,
  and the browser route adds a heavy dependency and seconds of start-up. They
  remain possible later as backends.
