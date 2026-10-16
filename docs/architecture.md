# Architecture

## Goal

Show Markdown in a terminal as close to a browser rendering as the terminal
allows, without giving up what makes a terminal good: text stays text, so it
can be selected, searched and copied, and it uses the user's own font and
colours. Pictures are used only where text cannot do the job.

## Pipeline

```
Markdown ──parse──▶ document model ──layout──▶ backend ──▶ output
                                                 ├─ kitty
                                                 ├─ plain ANSI
                                                 ├─ HTML        (later)
                                                 └─ pixels      (later)
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

## Terminal capabilities

`src/term.rs` is the only place that talks to the terminal to find out what
it can do: size in cells and pixels (`TIOCGWINSZ`), whether the text sizing
and graphics protocols work, and the background colour. 24-bit colour,
hyperlinks and underline styles follow from either protocol working. Without
them, 24-bit colour can still come from `COLORTERM` and hyperlinks from
`TERM_PROGRAM` or `VTE_VERSION`; underline styles come only from the probe.
`NO_COLOR` turns colours off in every mode. The result is a
plain value passed to the renderer, so tests can render for any terminal
without having one.

## kitty features used

| Feature | Protocol | Used for |
|---|---|---|
| Text sizing | OSC 66 (kitty 0.40+) | Headings in a larger font |
| Graphics | APC `_G` | Images, formulas, diagrams |
| Unicode placeholders | U+10EEEE with graphics | Images that scroll with text, work in pagers and tmux |
| Hyperlinks | OSC 8 | Clickable links |
| Styled underlines | SGR 4:x and 58 | Emphasis variants, link styling |

All escape sequences for these live under `src/kitty/`.

## Modules

```
src/main.rs                command line (clap)
src/lib.rs                 library root
src/model.rs               document model
src/parse.rs               comrak tree -> document model
src/term.rs                capability detection
src/theme.rs               colours and sizes, theme files
src/render/mod.rs          document model -> terminal output
src/render/layout.rs       document model -> styled lines of a fixed width
src/render/wrap.rs         word wrapping for styled text
src/render/table.rs        tables with box-drawing characters
src/render/style.rs        styled text between layout and output
src/render/output.rs       styled lines -> bytes (SGR, OSC 8, scaled text)
src/render/highlight.rs    syntax highlighting for code blocks (syntect)
src/kitty/mod.rs           kitty escape sequences
src/kitty/text.rs          text sizing protocol (OSC 66)
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
