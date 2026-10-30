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
  a CommonMark + GitHub Flavored Markdown syntax tree. kiri reads a few
  HTML tags in it itself: `<img>`, so an image means the same in the model
  whether Markdown or HTML wrote it, and `<details>`, `<summary>`, `<kbd>`,
  `<mark>`, `<sub>`, `<sup>` and `<br>`. Front matter at the start of a
  Markdown file is split off in `src/parse/front_matter.rs` before comrak
  sees the text and read as YAML, TOML or JSON into the model's own value
  tree; `--front-matter off` leaves it to comrak. A formula in `$$…$$`
  becomes a code block in `math`, as a ` ```math ` block is, so that a
  backend that cannot draw it shows its source.
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
window. A CSV or TSV file, told by its extension or by `--format csv`
or `--format tsv`, goes through a parser of its own, `src/parse/csv.rs`,
into a document of one table, so the backends draw it as any other table.
A Mermaid file becomes a document of one Mermaid code block, whose
diagram is drawn as in any document and then shown on its own as an
image file is, or the document is shown when it is not drawn.
`--format text` skips the parser too: the file becomes a document of one
code block, in the language its extension names, so it is highlighted,
wrapped and paged as any code block is. Plain output, without styling,
gets the file as it is instead, from `src/main.rs`.

The document can also come from standard input, which errors call
`<stdin>`, or from a directory, which stands for its README. Both are
resolved in `src/main.rs`, before `src/input.rs` looks at the bytes, so
an image piped in is shown on its own as well.

Several files on the command line are shown one after another, each under
a header that names it. Each is read as it would be on its own, and a file
that cannot be read or shown is told about while the others are still
shown. In the document model they are `File`s, each a name and a document;
an image file, or a diagram file that is drawn, is a document of one
`Block::ImageFile`, whose pixels kiri reads itself and keys by the file's
name. The layout gives each file its header, and marks each line with the
block it comes from; the header has a block of its own. The documents'
images load together, each document's relative to its own directory, under
keys that carry the document's number among the files. Files that are each
shown on their own, as images are, go to the pager only when paging is
`always`; with a document among them, the files are paged as one document
is. The pager then shows them as one stream, headers and all, so a search
finds a term in any of them, and reads every image file before it opens;
when they are printed, an image file is read only as it is shown. An
image file or a diagram file given alone takes the same way to the pager
when paging is `always`, as one file among one: the pager shows it as a
document on its own, without a header, and has nothing to reload.

There is one terminal backend, not one per terminal. It lays the model out
as styled lines of a fixed width, then writes those as bytes. What it may
use comes from the capabilities it is given: plain text for pipes, colours
and attributes for any modern terminal, and kitty's protocols on top where
they work.

Each styled line also says which block of the document it comes from. The
layout keeps a table of the blocks, and a line holds only a small number
that points into it. The table says which file a block is in and where in
its document it is: its number among the top-level blocks, then its number
among the blocks inside a `<details>` block, and so on, so the second
block inside the first `<details>` has the place 0, 1. The summary of a
`<details>` block belongs to that block. A file's header has a block of
its own with no place in the document. The blank lines between blocks, and
between two files, belong to none. A number means something only for the
layout it came from, so a caller that keeps a place across a new layout,
as the pager does when the window is resized, compares the blocks
themselves.

A document also keeps the line of the source each of its blocks starts on,
in a table that is looked up by the same kind of place, and the layout
copies the line into its table of blocks. The table takes no part when two
documents are compared for equality, because the lines say where a block
was written and not what the document is. `src/compare.rs` compares an
old document with a new one: it finds the blocks that stayed as they were,
then pairs the blocks of the same kind that stand in the gaps between them
as changed, and calls the rest removed or added. When a block that changed
holds other blocks, such as a quote or a list, the blocks inside are
compared the same way. The result is told by place, so that a caller can
ask what became of a block of the new document or which blocks are gone.
The places inside lists have two numbers, the item and then the block in
it; the layout's table has to use them too once it marks those blocks.

The pager shows the same styled lines on the terminal's alternate screen.
`src/pager/view.rs` chooses the lines that fit in the window, a scaled
line whole or not at all, and moves them, and when the window changes size
it keeps the same block at the top; `src/pager/keys.rs` turns the bytes
the terminal sends into keys and keys into moves; `src/pager/prompt.rs`
keeps the search term as it is typed, and `src/pager/search.rs` finds it
in the laid-out lines and splits a line where a match starts and ends, so
the matches can be highlighted without a change to the writer;
`src/pager/fade.rs` mixes the colours a match fades through after a jump,
those of the status line's `reloaded` as it fades out, and those of the
warning line;
`src/pager/files.rs` finds where each of several files starts from the
blocks of the lines, so the status line names the file at the top of the
view and `:n` and `:p` move from one file's start to another's;
`src/pager/status.rs` and `src/pager/help.rs` make the status line and the
help panel as styled spans, and `src/pager/warnings.rs` the line of text
above the status line that tells the first warning; `src/pager/mod.rs` draws each frame, row by
row through the backend's writer, and lays the document out again when the
window changes size or when `r` reloads it. For that the pager is given a
`Reload`, as it is given a `Terminal` for keys: it asks the `Reload` for the
document and the images again, and keeps its place by the block at the
top: it compares the old document with the new one, and follows the block
at the top to the block that stayed or changed from it, so a block added
above the view does not move it. When the block at the top is gone, the
view goes to the next block that is still there. A view at the end of the
document goes to the new end, so a document that grew shows what was
added. Following on auto then moves the view to a change:
`src/pager/changes.rs` finds the runs of lines whose blocks changed or
were added, and the place of a removal with neither beside it, and chooses
one as the follow jump setting says, and `src/pager/tint.rs` gives the
lines of that change a lighter background for a moment, fading out
through `src/pager/fade.rs`. While the pager follows the
document, it asks the `Reload` on each poll for keys whether the document
changed, and reads it again once a change still looks the same 100 ms, a
poll's time, after it was first seen, so that a file caught half written
is not shown. The terminal itself, the alternate screen, raw mode and
reading keys from the controlling terminal, is opened in
`src/term/screen.rs`.

A reload shows the new text at once: the `Reload` keeps the images the
document still shows and starts the others loading in the background, in
the pager's `Loads`, which the pager polls beside the follow poll.
Meanwhile `src/pager/loading.rs` chooses what stands in for each image
from the comparison: a block that changed shows its old image dimmed,
and any other block, a new one or one that stayed while its image file
changed, shows as it does without its image, code or alt text. Below
each goes a thin band whose light part keeps running across it: in the
blank line after the block, or over the bottom of the block's last line
where none follows, so that it takes no row, and an image as large as
its placeholder moves nothing when it comes. The band is an image of its
own placed over the text; clearing the screen frees it, so each frame
sends it anew, as it is then, and the pager draws a frame each 50 ms
while a band shows, so that it moves. The status line says
`[rendering: 1/3]` in place of the name meanwhile. The images in view
load first, then the nearest: each band knows its image and the lines
that show it, and whenever the view moves `src/pager/loading.rs` orders
the images by how far those lines are from the view, a line below
before a line above, and tells `Loads`, which hands the waiting jobs out
in that order. Say the reader is at the end when a reload adds three
diagrams: the last one starts first. An image that comes
is laid out as after a resize, but the top line stays the same line, so
the view does not move, and the tint of auto-follow's change stays on
its blocks; auto-follow jumped already when the text came. A newer
reload stops the loads whose images the new document no longer shows,
and `q`, or a signal, stops the programs that draw. `src/main.rs` waits
for them to end once the pager has left its screen, with
`kiri: closing background programs…` on standard error while that takes
more than a second, but not for merman, which cannot stop. `Images` notes
which outcomes came in the background; once the pager has left its
screen, `src/main.rs` writes the warnings of those that failed and are
still shown, as it does before the pager opens for images loaded first.

With `initial_load = "background"`, the initial load goes the same way when
the pager surely opens. With paging on `auto`, `src/main.rs` tells that
by laying the documents out with a box of three rows,
`Placeholder::Rows`, for each image not loaded yet; a document that fits
loads its images first, as with `wait`. Otherwise it opens the screen and
then starts every image of the documents loading in the pager's `Loads`,
each shown as a new image of a reload is; the image and diagram files
among several files load before, as they have nothing else to show.
Should the screen not open, the images load before the documents are
printed.

The pager keeps one image encoder, `src/render/encoder.rs`, for as long
as it runs. An image the terminal holds is not sent again: after a
resize that leaves it at the same size, or a reload that leaves its
pixels as they were. An image that the lines of a new layout no longer
place, or that changed, is deleted from the terminal, and when the
pager quits, every image it sent is.

## Reading mode

`--reading`, experimental, shows the text as a browser does: in a
proportional font, drawn as images. `src/reading/` knows nothing about
terminals. It lays a run of text blocks (headings, paragraphs, lists,
quotes, thematic breaks, code blocks, highlighted as the text layout
highlights them, and tables, whose columns share the room as a browser
shares it) out with parley, in the fonts of the fontdb
database that SVG text uses, and draws them with tiny-skia, colour emoji
included. GitHub's sizes and spacing are its measure: 16 pixels of text in
rows of 20, a column of 42 times that, margins that collapse between
blocks. `src/render/reading.rs` turns such a run into a page: its rows are
image rows, as a large image's are, of parts 30 rows tall that the encoder
draws when a line first shows them, so the pager draws only the parts in
view. The rows keep their blocks, so the pager keeps its place across a
resize. Images, diagrams and formulas are in the page at their natural
size in CSS pixels, shrunk to the column, a formula in the middle of it;
the page gets them through `Pictures`, which `src/render/reading.rs`
writes over the loaded images, as it gets code's colours through
`Highlight`, so `src/reading/` knows nothing of the loader. A picture
that has not come reserves room, with a loading band over the last row of
it as in the text layout, and one a reload replaces shows the old
pixels dimmed; the page's id covers the pixels of its pictures, so a
picture that came is a new page. An image in the middle of text, which
the loader wants only when reading mode is asked for, as the others are
only ever their alt text, is a box in its line, which parley places on
the baseline and moves to the next line whole. So is a formula in the
middle of text, `Inline::Formula`, drawn by the formula renderer as
a diagram in text style at the text's size, which also tells how far
below its baseline it reaches, so that the box sits on the line's
baseline; until it has come it is shown as its source, as the other
layouts show it. An alert is a tint, a bar, a title after the
kind's icon, which is drawn from an embedded Octicon outline
(`src/reading/octicons.rs`), and the blocks inside it in the room left.
Front matter is a table of its keys and values in the column, with a
table in a cell for a value that has keys, or a code block when the
front matter shows as code. A `<details>` block is its summary after a
triangle and its blocks, each with a place of its own for the pager; raw
HTML is its source, and a note of kiri's own is muted text. What is left
as text between the pages, a file's header, a file's error message and an
image file shown on its own, starts at the area's left edge and is as wide as the text's column:
only code blocks and tables reach into the area. A code
block or table wider than the area wraps to fit it, or, with `--wide
scroll` in the pager, or after `W` there, becomes a band on whole rows of
its own, which keeps its lines and words whole and scrolls sideways under
a thin scroll bar, faded on each edge with more beyond it; a code band's
language stays in its top right corner. Its rows show an image of their
own, cut into slices of at most 30 rows as pages are, keyed by its slice,
its offset and whether it is selected. The pager keeps those by the
band's block in `src/pager/bands.rs`: the topmost band on the screen is
selected until a click, or a search that finds something in another,
selects it, ← and → or `H`
and `L` scroll it, and so does a sideways wheel, or the wheel with Shift,
over a band, and the pager then shows the same lines with the band's rows
keyed anew, without laying the document out again. Where the terminal
draws no images, kiri warns and shows the text as usual. In the pager,
`M` turns reading mode on and off and lays the document out again,
keeping the block at the top as a resize does.

A page is drawn opaque, in the page's colour, when that colour is known:
the theme's background or the one the terminal reported. Then all of its
text is blended by kiri over the same ground, shaded rows or not. Where
kiri would have to guess black or white, the page stays transparent and the
terminal blends it. With `background_opacity` below 1 an opaque page shows
as an opaque area, not as the translucent background.

Search and links in a page work from the positions the layout knows: it
records, in pixels and a piece for each line, where each link and each
place of a search term is, in the page's or a band's own coordinates.
`src/pager/search.rs` finds a term in the texts of a page, so a match is
on the row of its line of text, and a match in a band scrolls the band to
it and selects it. The marks are drawn into the page's image, a box in
the theme's match colours with the text drawn over it, and so is a box
round the selected link: what is drawn over a part is an overlay, named by
a hash of the marks and the link in that part alone, in the key of the
rows it is on, and kept by the encoder under its page and that hash, so
that the terminal keeps the part with it and the part without and moving
the current match draws only the parts it leaves and enters. A link is an
OSC 8 hyperlink round the placeholders it covers (`src/kitty/graphics.rs`),
which kitty opens as it does a link in text; in a band the cells move with
the scrolling.
`src/pager/links.rs` selects one of the links on the screen with Tab and
Shift+Tab, numbering them from the page's own list so that a link keeps its
id while a band scrolls, and the status line says where it goes. Enter
copies the selected link's URL with OSC 52 (`src/term/clipboard.rs`),
wrapped in tmux's passthrough where the images are.
`Reading` also holds a cache of the pictures scaled for the last layout,
by image, its pixels and size, so that a layout scales only what is new
and lets go of what it did not show.

## Diagrams

A diagram stays a code block in the document model, `Block::CodeBlock`
with its language, so the parser knows nothing about renderers and a
diagram that cannot be drawn is shown as the code it already is. Whether
it is drawn is decided when images are loaded. `src/images/mod.rs` keys
each image by an `ImageKey`, a URL or a block (its language, in lower
case, and its source), with the number of the document it is in, so
that documents shown together keep their own images, and collects the
code blocks whose language a renderer draws besides the lone images. It
hands all of a document's diagrams to the renderer at once, then
`src/images/drawn.rs` decodes each image it gets back by its content,
as it decodes an image file, so sizing, the width limit and redrawing
after a window resize work as for any image. An SVG is decoded as an
SVG file is; a raster image, such as the PNG of a browser's drawing,
becomes `Source::Dense`, pixels with their size in CSS pixels, so that
it is sized in CSS pixels as an SVG is. Its pixels are reduced there to
the most an image can be shown at, as many cells as one image covers
each way, so that a large diagram holds no more memory than showing
it needs; its size in CSS pixels stays. There it also grows an SVG
diagram whose text runs past its edges, and lays one drawn in colours
for the other page, light or dark, on a panel of its own background,
after the reduction, so the panel's copy is small too.

A heading is drawn as an image the same way, where the terminal draws
images but not large text, as kitty inside tmux: `ImageKey::Heading`
holds its level, its words, each marked with the inline code or key it
is in and whether it is emphasised, and the cell size it is drawn for,
and `src/images/heading.rs` draws the text through the SVG renderer in
the system's sans-serif font, emphasis in italics, and inline code and
keys in its monospace font on the background of inline code, at the size
and in the colours that `render::heading_look` resolves from the theme
and the terminal, as the text layout's inline code takes them, so
`src/images/` knows nothing of themes. Other inline styles are drawn as
the heading's text. Each word starts on a whole cell,
and `Source::Text` tells the columns each word covers, so layout wraps a
heading too long for the window as the sized one wraps: each line writes
the placeholders of the image's columns its words cover, which kitty's
placeholders allow from any column, and the image stays one whatever the
width. A heading whose image did not come is laid out as text, without a
warning.

Renderers live in `src/diagrams/`, outside `src/kitty/` and the parser.
A renderer implements the `Renderer` trait: whether it draws a language,
and a diagram's source and a `Theme`, for a light or a dark page, with
the colour of the page's text for a formula, turned into an image file, SVG or raster, and the page its colours are for. It
knows nothing about terminals, so an HTML backend could use the same
image.

The dependency between `src/diagrams/` and `src/images/` is not yet one
way. `External` takes the image loader's pixel limit `MAX_PIXELS`, its
unit `MB`, its `png_size`, `read_at_most` and the wording of a size
limit, `size`, and merman's tests decode its SVG with the loader. These
move to a module of their own when a second language, in
4.3.2, shows what the two share.

What kiri knows about each language is one entry of `LANGUAGES`, a
`Language` in `src/diagrams/language.rs`: plain data, such as its name,
its display name, the extensions of its files, its theme names and the
command kiri looks for, with functions where languages differ, such as
reading the page a diagram chooses from its source, finding its first
line for a warning, writing a configuration file for its program, and
its built-in renderer. Mermaid's entry and what it needs, its directives,
the settings kiri gives a diagram and mermaid-cli's command line, are in
`src/diagrams/mermaid/mod.rs`, compiled in every build; only the part
that runs merman is behind the `mermaid` feature. The rest of kiri asks
the table: input detection for diagram files, the configuration file's
`[diagrams.<language>]` tables, `--doctor`'s rows and the image loader's
panel colour.

- **Built-in:** `BuiltIn` draws a language with the renderer its entry
  has in this build: Mermaid through merman, in a build with the
  `mermaid` feature, and formulas through RaTeX, in every build
  (`src/diagrams/math.rs`). `src/diagrams/mermaid/merman.rs` fixes merman's SVG
  where resvg would lose part of it, and tests the fixes through merman
  itself. A fault that kiri works around elsewhere, such as text past a
  diagram's edge, has a test that fails once merman no longer has it.
- **External:** `External`, in `src/diagrams/external.rs`, runs the
  command the configuration file gives for a language, without a shell
  and with a time limit, several at once, with its files in a directory
  of its own in kiri's cache directory, which is also the program's
  `TMPDIR`. A command that takes `{scale}` writes a raster image, such
  as `mmdc`'s PNG, at that many pixels to the CSS pixel, and gets a
  Mermaid configuration that leaves labels in HTML, as a browser draws
  them; another is taken to write SVG, with labels as SVG text, which
  resvg draws. A PNG with more pixels than kiri loads, as its header
  tells before it is decoded, is drawn once more at the largest whole
  scale that fits. `src/diagrams/find.rs` finds a default command, `mmdc` for
  Mermaid, when nothing is configured, the configuration file does not
  turn the search off and the build has no renderer of its own for the
  language; `--doctor` reports the same choice.
- **Together:** `Renderers` draws each language with its external
  command when it has one and with the built-in renderer otherwise. An
  external command that fails does not fall back to the built-in one.

A reload, `r` in the pager, loads only the images the document does not
have yet: a diagram is kept by its key, which holds its source, so one
that did not change is not drawn again, unless it failed to draw, such as
for a time limit; and an image from a local file is loaded again when its
modification time, which `src/images/stamps.rs` keeps, is not the one it
was loaded at. Other images are kept as they are. What is loaded loads
in the background, `src/images/background.rs`, on threads of their own,
with at most four programs drawing at once, also when reloads overlap;
while four draw, an image file loads before the diagrams waiting for a
program. Each load has a stop flag of its own, which `External` looks at
besides the one for signals.

`src/images/pipeline.rs` builds the renderers from the settings and the
environment, given as values, and loads a document's images with them;
`src/main.rs` hands it `src/term/progress.rs`, which tells on the
terminal how far loading has got when it takes long, and another caller
may tell it elsewhere. Where the pager surely opens and standard error
is a terminal, the wait loads in the background instead, while
`src/term/keys.rs` reads keys with the terminal in a mode that keeps
Ctrl-C a signal; Space or Enter hands the loads still under way to the
pager, and the mode, with any key typed meanwhile, is gone before the
pager or the shell takes the terminal.

The programs run in process groups of their own, which Ctrl-C from the
terminal does not reach, nor a signal sent to kiri alone. So when the
document has a diagram that a program draws, `src/term/interrupt.rs`
catches SIGINT, SIGQUIT, SIGTERM and SIGHUP while its images load:
`External` stops the programs, SIGINT first so that `mmdc` can close its
browser, removes their files, and kiri then ends by the signal that came.
Without such a diagram, and once the images are loaded, the signals end
kiri as before, and the pager catches them itself, from before its
terminal is raw until it has been restored, or until its screen fails to
open, when they end kiri again while the document is printed. A signal
that was ignored when kiri started, as under `nohup`, stays ignored in
both. A new diagram language is a new entry in `LANGUAGES` in
`src/diagrams/language.rs`, in a module of its own as Mermaid's is, with
a default command, a built-in renderer or both; the rest follows from the
entry.

## Terminal capabilities

`src/term/` is the only place that talks to the terminal to find out what
it can do: size in cells and pixels (`TIOCGWINSZ`, or the cell size from
`CSI 16 t` where that has no pixels or inside tmux), whether the text sizing and
graphics protocols work, the background and palette colours, and the
terminal's name and version (XTVERSION). 24-bit colour, hyperlinks and underline
styles follow from either protocol working. Images need the graphics
protocol's Unicode placeholders, which no query reveals; kiri assumes them
when the graphics protocol works and the terminal is kitty 0.28 or later
or Ghostty 1.0 or later, or when both protocols work, since text sizing
came to kitty long after placeholders. Elsewhere images are alt text.
Without them, 24-bit colour can still come from `COLORTERM` and hyperlinks
from `TERM_PROGRAM` or `VTE_VERSION`, but inside tmux from tmux, below;
underline styles come only from the probe. `NO_COLOR` turns colours off in every
mode. These variables come in as part of `config::Environment`, read once
at start-up with kiri's own, so the rules are tested without the process
environment. The result is a plain value, `Capabilities` in
`src/caps.rs`, passed to the renderer, so tests can render for any
terminal without having one. kiri asks nothing when it runs in the
background of its terminal, where raw mode would get it stopped.

Inside tmux, the probe's graphics query goes wrapped in tmux's
passthrough, and only kitty's answer to it comes from kitty; tmux answers
the rest itself. Detection first asks tmux, with `tmux display-message`,
for the terminal outside, the pane's `allow-passthrough` and whether
tmux passes OSC 8 links on (the `hyperlinks` terminal feature), and turns
`allow-passthrough` on when it is `off`, as `kitten icat` does. Links
are then drawn exactly when tmux passes them, whatever the terminal
answers: `TERM_PROGRAM` and `VTE_VERSION` inherited from outside tmux say
nothing about tmux. The capabilities then say `tmux_passthrough`, and
the writer the output goes through (`term::output`, and the pager's
screen) wraps each graphics sequence on its way out, so the renderers
write the same bytes inside tmux as outside it.

tmux drops output for a client that cannot keep up, and the images in it
with it. Through tmux the encoder therefore notes the images it sends,
and after printing, `src/term/check.rs` asks the terminal about them
with a virtual placement each, which kitty and Ghostty answer with
`ENOENT` for an image they do not hold; the encoder sends those again, a
few times at most. The pager writes the same checks after each frame that
sent images, and the terminal's answers come among the keys.

`kiri --doctor` reports what detection found and where each setting came
from. `src/term/` and `src/config.rs` hand it the detection, the
settings and the file paths; `src/doctor.rs` checks which files exist and
writes it all as a Markdown document, which kiri renders like any other or
writes as it is into a pipe.

## Terminal features used

| Feature | Escape sequence | Written by | Used for |
|---|---|---|---|
| Text sizing | OSC 66 (kitty 0.40+) | `src/kitty/text.rs` | Headings in a larger font; without it, headings as images |
| Graphics | APC `_G` | `src/kitty/graphics.rs` | Images and diagrams, later formulas |
| Unicode placeholders | U+10EEEE with graphics | `src/kitty/graphics.rs` | Images that scroll with text, work in pagers and tmux |
| Protocol queries | OSC 66 tests, APC `_G` query | `src/kitty/query.rs` | Telling whether kitty's protocols work |
| Hyperlinks | OSC 8 | `src/render/output.rs` | Clickable links |
| Text attributes and colours | SGR | `src/render/output.rs` | Bold, italic, colours |
| Styled underlines | SGR 4:x, SGR 58 | `src/render/output.rs` | Heading underline styles; the dashed underline between table rows in the border colour |
| Cursor movement, erase | CSI C, CSI K, CSI H, CSI 2J | `src/render/output.rs` | Backgrounds beside scaled headings and across the width; the pager's rows |
| Line erase | CR, CSI 2K; CSI B, ESC 7, ESC 8 | `src/term/progress.rs`, `src/kitty/query.rs` | Wiping the progress line; erasing the rows the text sizing tests drew |
| Alternate screen, cursor | CSI ?1049h/l, CSI ?25l/h | `src/term/screen.rs` | The pager's screen of its own |
| Focus reporting | CSI ?1004h/l; CSI I, CSI O | `src/term/screen.rs`, `src/pager/keys.rs` | Telling the pager that its tmux pane is shown again |
| Mouse reporting | CSI ?1000h/l, CSI ?1006h/l; CSI < … M/m, CSI M | `src/term/screen.rs`, `src/pager/keys.rs` | The wheel scrolling the pager, also inside tmux; in reading mode, a click selecting a band and the sideways wheel scrolling it |
| Synchronized output | CSI ?2026h/l | `src/render/output.rs` | Pager frames without flicker |
| Cell size query | CSI 16 t | `src/kitty/query.rs` | The cell size in pixels where the window size has none, and inside tmux, where the window's may be tmux's guess |
| Generic queries | CSI 6n, OSC 10, OSC 11, OSC 4, CSI > q, CSI c | `src/term/probe.rs` | Cursor position, foreground, background and palette colours, terminal version, end of the answers |
| tmux passthrough | DCS `tmux;` … ST, with each ESC doubled | `src/term/tmux.rs` | kitty's graphics, and the probe's graphics and cell size queries, through tmux to the terminal outside |
| Image checks | APC `_G` `a=p,U=1` and `a=d,d=i` with a placement id | `src/kitty/graphics.rs`, `src/term/check.rs` | Whether kitty behind tmux holds the images kiri sent |

Sequences that only kitty's protocols define live under `src/kitty/`: text
sizing, graphics, Unicode placeholders and the queries that detect them;
`src/term/probe.rs` sends those along with the generic queries it spells
out itself. OSC 8 hyperlinks, SGR attributes, underline styles and cursor
movement are general terminal features that `src/render/output.rs` writes
itself; for scaled text it calls `src/kitty/text.rs`, and for images
`src/render/encoder.rs`, which sends and deletes them, and
`src/kitty/graphics.rs`, which writes those sequences and the
placeholders.
Underline styles began in kitty but are now common; kiri uses them only
when the probe finds kitty's protocols or `--mode kitty` asks for them.

## Modules

```
src/main.rs                command line (clap)
src/lib.rs                 library root
src/input.rs               whether a file is a document, a table or an image
src/model.rs               document model
src/parse/mod.rs           comrak tree -> document model
src/parse/html.rs          the HTML tags kiri reads, <img> first
src/parse/csv.rs           CSV and TSV -> a document of one table
src/parse/front_matter.rs  front matter: recognition and YAML, TOML and
                           JSON values
src/caps.rs                what the terminal can do, as plain data
src/term/mod.rs            terminal I/O, all of it under src/term/; whether
                           stdin and stdout are terminals
src/term/detect.rs         capability detection from the mode, the environment,
                           the window and the probe
src/term/window.rs         the window size in cells and pixels
src/term/probe.rs          the probe's queries to the terminal
src/term/check.rs          asking kitty behind tmux whether it holds the
                           images kiri sent
src/term/answers.rs        reading the terminal's answers
src/term/tmux.rs           inside tmux: asking tmux, and wrapping kitty's
                           graphics in tmux's passthrough
src/term/screen.rs         the pager's screen, raw mode and keys
src/term/progress.rs       the line that tells how far loading images has
                           got
src/term/interrupt.rs      signals while programs draw diagrams
src/term/keys.rs           Space or Enter while kiri waits for the images
src/term/testing.rs        pseudo-terminals for the tests
src/testing.rs             scratch directories for the tests
src/panics.rs              panics kiri catches on purpose, such as a load's
                           in the background, which the pager's screen
                           stays open for
src/config.rs              the environment variables kiri reads, the
                           configuration file, and each setting resolved
                           from the flags, the environment, the file and
                           its default, with where it comes from
src/doctor.rs              the --doctor report as Markdown
src/list.rs                the --list-emojis output: Markdown for a terminal,
                           tab-separated lines for a pipe
src/theme/mod.rs           colours and sizes the renderer uses
src/theme/paper.rs         the light and dark paper themes
src/theme/file.rs          theme files
src/images/mod.rs          loading the images a document shows
src/images/background.rs   loading images on threads of their own, each
                           stoppable, as the pager does after a reload
src/images/decode.rs       decoding image files, raster and SVG
src/images/drawn.rs        diagrams as their renderers drew them, made images
src/images/heading.rs      headings drawn as images of their text, each word
                           from a whole cell
src/images/pipeline.rs     a document's images for a run: renderers from the
                           settings, the signal guard, the loading
src/images/stamps.rs       when the files behind the images were last changed
src/images/url.rs          data URLs, file URLs and paths, read up to a limit
src/images/svg.rs          SVG images and the fonts for their text
src/reading/mod.rs         reading mode: text in a proportional font, drawn
                           as pixels
src/reading/fonts.rs       the fonts of SVG text handed to parley, with
                           fallbacks for emoji and other scripts
src/reading/text.rs        styled text laid out in lines with parley
src/reading/paint.rs       laid-out text drawn as pixels: glyph outlines,
                           chips behind inline code, rules
src/reading/colr.rs        colour glyphs, such as emoji, from the COLR table
src/reading/page.rs        a run of text blocks laid out as GitHub sets them,
                           drawn a part at a time
src/compare.rs             which blocks stayed, changed, were added or
                           removed between two documents
src/diagrams/mod.rs        diagrams written in code blocks, drawn as images
                           for the image loader
src/diagrams/language.rs   the diagram languages and what kiri knows of each
src/diagrams/mermaid/mod.rs
                           Mermaid's settings, directives and mmdc command
src/diagrams/mermaid/merman.rs
                           Mermaid through merman (feature `mermaid`)
src/diagrams/math.rs       formulas through RaTeX
src/diagrams/external.rs   diagrams drawn by a command outside kiri
src/diagrams/find.rs       finding a command, such as mmdc, when none is
                           configured
src/color.rs               colours and underline styles for theme and render
src/size.rs                text sizes for theme and render
src/render/mod.rs          document model -> terminal output
src/render/layout/mod.rs   document model -> styled lines of a fixed width;
                           files under their headers
src/render/layout/code.rs  code blocks and their language label
src/render/layout/list.rs  bullet, numbered and task lists
src/render/layout/quote.rs block quotes
src/render/layout/front_matter.rs
                           front matter as a table or a code block
src/render/wrap.rs         word wrapping, splitting and cutting styled text
src/render/table.rs        tables with box-drawing characters, or stacked;
                           tables in cells, header columns, and how rows
                           are told apart
src/render/image.rs        image sizes in pixels and terminal cells
src/render/block.rs        which block of the document a line comes from
src/render/style.rs        styled text between layout and output
src/render/script.rs       Unicode's super- and subscript characters
src/render/output.rs       styled lines -> bytes (SGR, OSC 8, CSI C/K, OSC 66),
                           cursor placement and synchronized output
src/render/encoder.rs      the images the terminal holds, and under which ids
src/render/reading.rs      reading mode's pages as rows of images, each part
                           drawn when it is first shown
src/render/loading.rs      the pixels of what stands in for an image while
                           it loads, and of the band below it
src/render/highlight.rs    syntax highlighting for code blocks (syntect)
src/pager/mod.rs           the pager: frames and the loop of keys
src/pager/view.rs          the lines a window shows, and scrolling
src/pager/keys.rs          keys from the terminal and what they do
src/pager/prompt.rs        the search prompt on the status line
src/pager/search.rs        search matches in the laid-out lines
src/pager/fade.rs          a style fading into another after a jump or
                           a reload
src/pager/files.rs         where each of several files starts in the stream,
                           found from the blocks of its lines
src/pager/bands.rs         reading mode's bands: how far each is scrolled,
                           and which is selected
src/pager/links.rs         reading mode's selected link
src/pager/changes.rs       where a reload changed the laid-out lines, and
                           which change auto-follow moves to
src/pager/tint.rs          the lighter background over that change's lines
src/pager/loading.rs       what stands in for an image while it loads after
                           a reload, and which images load first
src/pager/status.rs        the status line
src/pager/help.rs          the help panel that lists the keys
src/pager/warnings.rs      the warning line above the status line
src/kitty/mod.rs           kitty-specific escape sequences
src/kitty/text.rs          text sizing protocol (OSC 66)
src/kitty/graphics.rs      graphics protocol with Unicode placeholders
src/kitty/query.rs         queries that detect kitty's protocols
crates/kiri-tr-image-utils raster image decoding and scaling, a crate of its
                           own in the workspace
```

Decoding raster images and scaling them live in their own crate, a path
dependency of kiri, because generic code is compiled in the crate that
calls it: the BMP, TGA and QOI decoders of the image crate are generic, and
so is most of fast_image_resize. As a separate crate it can be optimised in
debug builds, where it decodes and scales a large photo in tenths of a
second at most instead of seconds, while kiri's own code stays easy to
debug.

## Builds

The default build, `full`, has every Cargo feature; a build without
default features (`--no-default-features`) is the minimal one. `full`
adds the image formats the `image` crate decodes cheaply (BMP, ICO,
TIFF, QOI, PNM and TGA) and the `mermaid` feature, which draws Mermaid
diagrams with merman inside kiri. The minimal build shows PNG, JPEG,
GIF, WebP and SVG, draws formulas, and draws Mermaid only through an
external command: one the configuration file gives, or `mmdc` when kiri
finds it. Both
builds take the same options and settings.

## Choices

- **Rust:** one fast binary with no runtime, and mature crates for each part:
  comrak (Markdown), syntect (syntax highlighting), image and resvg (raster
  and SVG images), csv (CSV and TSV files), rustix (terminal modes and pager
  input), merman (Mermaid diagrams), RaTeX (formulas).
- **RaTeX for formulas:** a Rust port of KaTeX that merman already
  brings for its math. It draws a formula in milliseconds, with KaTeX's
  fonts built in as paths, so it needs no fonts or network. The minimal
  build has it too, for formulas: it adds about 3 MB to its stripped
  binary (14.3 to 17.2 MB), and next to nothing to the full build, which
  had it for merman's math already.
- **merman for Mermaid:** a Rust implementation of Mermaid that checks
  its parsing, layout and SVG against Mermaid's own, so diagrams look as
  GitHub draws them. It draws a diagram in milliseconds, needs no fonts,
  files or network, and its SVG goes through the resvg kiri already has.
  It adds about 24 MB to a stripped binary. Its ELK layout is EPL-2.0,
  its math may bundle OFL-1.1 font data, and the CSS crates it brings
  (cssparser, cssparser-macros, selectors, dtoa-short) are MPL-2.0, all
  accepted; their notices must go with distributed binaries, MPL-2.0's
  with where to get the source. mermaid-cli (`mmdc`), Mermaid itself
  in a headless Chrome, takes about a second a diagram and about 1.1 GB
  with Node and Chrome, so it is the minimal build's renderer and a
  configured choice rather than the default. mermaid-rs-renderer was
  left out: its layout has visible errors, and it keeps process-wide
  mutable state and writes a font copy to the user's cache directory.
- **Text first:** the alternatives were rendering the whole document as an
  image, through a headless browser or a custom layout engine. Both look
  closer to a browser but lose text selection, search and the terminal font,
  and the browser route adds a heavy dependency and seconds of start-up. They
  remain possible later as backends.
