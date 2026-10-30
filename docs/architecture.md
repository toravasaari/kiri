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
id while a band scrolls, and the status line says where it goes.
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

kiri is one crate. Its code is in four parts, and every part is a module
of the crate, not a crate of its own. Each of the three engine parts is
one top-level module, `src/typeset/`, `src/present/` and `src/session/`,
whose public surface is exported from its root and whose inside is
private. The fourth part, the platform, is `src/main.rs` and the rest of
`src/` outside those three directories.

### The four parts

- **The platform:** `src/main.rs` and the rest of `src/` outside the three
  part directories. It owns all the I/O: the command line and the settings
  files, the shells (interactive and print), the work runner (threads,
  programs, watchers) and `term/`, where kiri talks to the terminal.
  `main.rs` wires the parts together.
- **The terminal layer, `src/present/`:** pure and sans-IO. It turns what
  is shown into bytes for an `impl Write` (`kitty/` and the encoders) and
  decodes keys and the terminal's answers to the probe. It makes no system
  calls.
- **The state engine, `src/session/`:** pure. It holds the pager's state
  and its decisions, and the components and panels that the pager is made
  of.
- **The typesetting engine, `src/typeset/`:** pure given its resources;
  the fonts, for one, are passed in. It owns the document model, parsing
  and the input formats, the themes and the look, both layout engines (the
  cells and reading mode), and the images and diagrams that it draws from
  data it is given.

Dependencies point down this list: the typesetting engine uses none of the
others, the terminal layer uses it, the state engine uses both, and the
platform uses all three. The mixed modules below are where today's code
still points the other way.

### Every module

The table has a row for each file of `src/`. **Part** is the part the
module's code belongs to; **mixed** means that its non-test code does the
work of two parts, usually I/O inside what would otherwise be pure. **After
the move** says where the file goes in step V2, which moves whole modules
and nothing else; no re-export is left at the old path. A mixed module
stays where it is, and the row names the pieces, where each goes, and the
step that splits it:

- **B2:** one font database, loaded by the platform and handed to the
  engines.
- **B4:** startup decisions that need no I/O move out of `main.rs` into
  `session/plan.rs`.
- **B5:** `config.rs` is split, and each setting gets an owner.
- **D1:** the pager's loop and its state are split, the state into the
  state engine and the loop into the platform.
- **D3:** sessions and loading: the platform reads, fetches and runs, and
  the engines draw from the data it gives them.
- **E5:** the terminal layer is built: the presenter, the encoders, and the
  key and probe decoding that today sit in `term/`, `render/` and
  `pager/`.

A file that stays while the module above it moves keeps its place on disk,
and its `mod` line in the moved parent gets a `#[path]`, until the step
that splits it.

A few files arrive with the branch `fix/reading-leftovers`, which is not
merged yet: `term/cell_query.rs`, `term/clipboard.rs` and
`pager/tests/links.rs`. They have rows too.

| Module | Part | After the move |
|---|---|---|
| `src/lib.rs` | platform | stays; declares the three parts |
| `src/main.rs` | mixed | stays. Platform: the command line, opening files and standard input, the shells, loading, the file reload. State engine: the decisions that need no I/O (`surely_paged`, `pages_alone`, `printing`) go to `session/plan.rs` in B4. B6 makes the one-file and many-files routes one |
| `src/config.rs` | mixed | stays. Platform: reading the environment and the file, and resolving a setting from the flags, the environment, the file and its default. State engine: the setting types the pager reads (`Paging`, `SearchCase`, `Follow`, `FollowJump`). B5 |
| `src/config/tests.rs` | follows `config.rs` | stays; goes with its pieces in B5 |
| `src/doctor.rs` | platform | stays (it searches `PATH` through `diagrams::find`) |
| `src/list.rs` | platform | stays (the `--list-emojis` shell) |
| `src/panics.rs` | platform | stays |
| `src/testing.rs` | platform | stays (test support) |
| `src/caps.rs` | typesetting | `src/typeset/caps.rs` |
| `src/color.rs` | typesetting | `src/typeset/color.rs` |
| `src/size.rs` | typesetting | `src/typeset/size.rs` |
| `src/model.rs` | typesetting | `src/typeset/model.rs` |
| `src/input.rs` | typesetting | `src/typeset/input.rs` |
| `src/compare.rs` | typesetting | `src/typeset/compare.rs` |
| `src/parse/mod.rs` | typesetting | `src/typeset/parse/mod.rs` |
| `src/parse/html.rs` | typesetting | `src/typeset/parse/html.rs` |
| `src/parse/csv.rs` | typesetting | `src/typeset/parse/csv.rs` |
| `src/parse/csv/tests.rs` | typesetting | `src/typeset/parse/csv/tests.rs` |
| `src/parse/front_matter.rs` | typesetting | `src/typeset/parse/front_matter.rs` |
| `src/parse/front_matter/tests.rs` | typesetting | `src/typeset/parse/front_matter/tests.rs` |
| `src/parse/tests.rs` | typesetting | `src/typeset/parse/tests.rs` |
| `src/theme/mod.rs` | typesetting | `src/typeset/theme/mod.rs` |
| `src/theme/paper.rs` | typesetting | `src/typeset/theme/paper.rs` |
| `src/theme/file.rs` | mixed | stays. Typesetting: parsing the theme file's text. Platform: `load`, which reads the file. B5 |
| `src/kitty/mod.rs` | terminal layer | `src/present/kitty/mod.rs` |
| `src/kitty/graphics.rs` | terminal layer | `src/present/kitty/graphics.rs` |
| `src/kitty/query.rs` | terminal layer | `src/present/kitty/query.rs` |
| `src/kitty/text.rs` | terminal layer | `src/present/kitty/text.rs` |
| `src/render/mod.rs` | mixed | stays. Typesetting: `Inputs`, the render options and `heading_look`, the front matter helpers. Terminal layer: `render`, `render_files`, `render_file`, `render_image` and `render_image_on`, which lay out and write in one call. E5 |
| `src/render/block.rs` | typesetting | `src/typeset/block.rs` |
| `src/render/style.rs` | typesetting | `src/typeset/style.rs` |
| `src/render/image.rs` | typesetting | `src/typeset/image_size.rs` |
| `src/render/wrap.rs` | typesetting | `src/typeset/wrap.rs` |
| `src/render/layout/mod.rs` | typesetting | `src/typeset/layout/mod.rs` |
| `src/render/layout/code.rs` | typesetting | `src/typeset/layout/code.rs` |
| `src/render/layout/front_matter.rs` | typesetting | `src/typeset/layout/front_matter.rs` |
| `src/render/layout/list.rs` | typesetting | `src/typeset/layout/list.rs` |
| `src/render/layout/quote.rs` | typesetting | `src/typeset/layout/quote.rs` |
| `src/render/table.rs` | typesetting | `src/typeset/layout/table.rs` |
| `src/render/table/tests.rs` | typesetting | `src/typeset/layout/table/tests.rs` |
| `src/render/script.rs` | typesetting | `src/typeset/layout/script.rs` |
| `src/render/highlight.rs` | typesetting | `src/typeset/layout/highlight.rs` |
| `src/render/reading.rs` | typesetting | `src/typeset/reading/engine.rs` |
| `src/render/output.rs` | terminal layer | `src/present/output.rs` |
| `src/render/encoder.rs` | terminal layer | `src/present/encoder.rs` |
| `src/render/loading.rs` | terminal layer | `src/present/loading.rs` |
| `src/reading/mod.rs` | typesetting | `src/typeset/reading/mod.rs` |
| `src/reading/colr.rs` | typesetting | `src/typeset/reading/colr.rs` |
| `src/reading/octicons.rs` | typesetting | `src/typeset/reading/octicons.rs` |
| `src/reading/page.rs` | typesetting | `src/typeset/reading/page.rs` |
| `src/reading/paint.rs` | typesetting | `src/typeset/reading/paint.rs` |
| `src/reading/text.rs` | typesetting | `src/typeset/reading/text.rs` |
| `src/reading/fonts.rs` | mixed | stays. Typesetting: the fonts handed to parley, with the fallbacks. Platform: loading the system fonts and memory-mapping the font files, lazily, on the first layout. B2 |
| `src/images/mod.rs` | mixed | stays. Typesetting: `ImageKey`, `Images` and the plan of which images a document wants. Platform: `load`, `load_with` and `load_parts`, which read files and run the renderers. D3 |
| `src/images/decode.rs` | typesetting | `src/typeset/images/decode.rs` |
| `src/images/drawn.rs` | typesetting | `src/typeset/images/drawn.rs` |
| `src/images/drawn/tests.rs` | typesetting | `src/typeset/images/drawn/tests.rs` |
| `src/images/heading.rs` | typesetting | `src/typeset/images/heading.rs` |
| `src/images/svg.rs` | mixed | stays. Typesetting: `Svg`, parsing and drawing. Platform: `Fonts::system`, which scans the system's fonts. B2 |
| `src/images/url.rs` | mixed | stays. Typesetting: `data:` URLs and the reading of a URL's parts. Platform: `read_file`, which opens files. D3 |
| `src/images/background.rs` | platform | stays |
| `src/images/pipeline.rs` | platform | stays |
| `src/images/pipeline/tests.rs` | platform | stays |
| `src/images/stamps.rs` | platform | stays |
| `src/diagrams/mod.rs` | mixed | stays. Typesetting: `Request`, `Drawn`, the `Renderer` trait and the choice among renderers. Platform: `render_in_parallel`, which draws on threads. D3 |
| `src/diagrams/language.rs` | typesetting | `src/typeset/diagrams/language.rs` |
| `src/diagrams/math.rs` | typesetting | `src/typeset/diagrams/math.rs` |
| `src/diagrams/mermaid/mod.rs` | typesetting | `src/typeset/diagrams/mermaid/mod.rs` |
| `src/diagrams/mermaid/merman.rs` | typesetting | `src/typeset/diagrams/mermaid/merman.rs` |
| `src/diagrams/external.rs` | platform | stays |
| `src/diagrams/external/tests.rs` | platform | stays |
| `src/diagrams/find.rs` | platform | stays |
| `src/pager/mod.rs` | mixed | stays. State engine: `State`, the key and tick handling, the frame. Platform: `page_through` and `show`, the loop, with the `Reload`, `InlineLoader`, `Loads` and `Terminal` it does its I/O through. D1 |
| `src/pager/tests/mod.rs` | follows `pager/mod.rs` | stays; goes with its pieces in D1 |
| `src/pager/tests/files.rs` | follows `pager/mod.rs` | stays; D1 |
| `src/pager/tests/following.rs` | follows `pager/mod.rs` | stays; D1 |
| `src/pager/tests/images.rs` | follows `pager/mod.rs` | stays; D1 |
| `src/pager/tests/links.rs` | follows `pager/mod.rs` | stays; D1 (arrives with `fix/reading-leftovers`) |
| `src/pager/tests/loading.rs` | follows `pager/mod.rs` | stays; D1 |
| `src/pager/tests/paging.rs` | follows `pager/mod.rs` | stays; D1 |
| `src/pager/tests/reading.rs` | follows `pager/mod.rs` | stays; D1 |
| `src/pager/tests/reload.rs` | follows `pager/mod.rs` | stays; D1 |
| `src/pager/tests/search.rs` | follows `pager/mod.rs` | stays; D1 |
| `src/pager/tests/view.rs` | follows `pager/mod.rs` | stays; D1 |
| `src/pager/keys.rs` | mixed | stays. Terminal layer: `Key` and the `Decoder` that turns bytes into keys. State engine: `Action`, `BINDINGS` and `action`. E5 for the decoding, D1 for the actions |
| `src/pager/bands.rs` | state engine | `src/session/bands.rs` |
| `src/pager/changes.rs` | state engine | `src/session/changes.rs` |
| `src/pager/fade.rs` | state engine | `src/session/fade.rs` |
| `src/pager/files.rs` | state engine | `src/session/files.rs` |
| `src/pager/help.rs` | state engine | `src/session/help.rs` |
| `src/pager/links.rs` | state engine | `src/session/links.rs` |
| `src/pager/loading.rs` | state engine | `src/session/loading.rs` |
| `src/pager/prompt.rs` | state engine | `src/session/prompt.rs` |
| `src/pager/search.rs` | state engine | `src/session/search.rs` |
| `src/pager/status.rs` | state engine | `src/session/status.rs` |
| `src/pager/status/tests.rs` | state engine | `src/session/status/tests.rs` |
| `src/pager/tint.rs` | state engine | `src/session/tint.rs` |
| `src/pager/view.rs` | state engine | `src/session/view.rs` |
| `src/pager/warnings.rs` | state engine | `src/session/warnings.rs` |
| `src/term/mod.rs` | platform | stays |
| `src/term/answers.rs` | terminal layer | `src/present/answers.rs` |
| `src/term/cell_query.rs` | terminal layer | `src/present/cell_query.rs` (arrives with `fix/reading-leftovers`) |
| `src/term/clipboard.rs` | terminal layer | `src/present/clipboard.rs` (arrives with `fix/reading-leftovers`; it builds the OSC 52 bytes and writes them to an `impl Write`) |
| `src/term/check.rs` | mixed | stays. Platform: asking kitty behind tmux, on the terminal. Terminal layer: reading what it answered, `lost` and `sequences`. E5 |
| `src/term/detect.rs` | mixed | stays. Platform: `capabilities` and `examine`, which read the window, the environment and the probe's answer. Terminal layer: turning those into `Capabilities` (`with_probe`, `with_window`, `through_tmux`). E5 |
| `src/term/probe.rs` | mixed | stays. Platform: sending the probe and reading the answer on the controlling terminal. Terminal layer: the text of the queries. E5 |
| `src/term/tmux.rs` | mixed | stays. Platform: running `tmux` and reading its answer. Terminal layer: `Wrapping` and `passthrough`, which wrap kitty's sequences for tmux. E5 |
| `src/term/window.rs` | mixed | stays. Platform: `window_size`, the ioctl. Terminal layer: `WindowSize`. E5 |
| `src/term/screen.rs` | platform | stays |
| `src/term/screen/tests.rs` | platform | stays |
| `src/term/progress.rs` | platform | stays |
| `src/term/interrupt.rs` | platform | stays |
| `src/term/keys.rs` | platform | stays |
| `src/term/pty.rs` | platform | stays (test support) |
| `src/term/testing.rs` | platform | stays (test support) |

Decoding raster images and scaling them live in `crates/kiri-tr-image-utils`,
which is not a module of any part but a crate of its own in the workspace,
a path dependency of kiri, because generic code is compiled in the crate that
calls it: the BMP, TGA and QOI decoders of the image crate are generic, and
so is most of fast_image_resize. As a separate crate it can be optimised in
debug builds, where it decodes and scales a large photo in tenths of a
second at most instead of seconds, while kiri's own code stays easy to
debug.

Once the move is done, a module's tests, its snapshots and its `tests/`
directory go with it, and the table's paths are the final ones.

### Where a new thing goes

New code goes where this map puts it. When the map has no place for it,
that is a question for the maintainer, not a reason to add it to a hub
file (`main.rs`, `pager/mod.rs`, `config.rs`, `render/mod.rs`,
`render/layout/mod.rs`, `render/reading.rs`, `reading/page.rs`,
`images/mod.rs` and `diagrams/mod.rs`).

| A new… | Goes in |
|---|---|
| command-line option | the platform's command-line code; today `Cli` in `src/main.rs` |
| startup decision, such as whether a run pages | `src/session/plan.rs`, with unit tests (B4) |
| key or action | the component's list of commands in the state engine (`src/session/`); until the commands are data (A2), `BINDINGS` in `src/pager/keys.rs` |
| setting | the registry of setting keys, each with its owner, its effect and its medium (B5); until then `config.rs`, named after its owner |
| use of a file, the network or a program | the platform's work runner (D3), and the trust decision in a `Policy` (B7); today `src/images/background.rs`, `src/images/pipeline.rs` and `src/diagrams/external.rs` |
| block type or input format | the typesetting engine: the type in `src/typeset/model.rs`, parsing in `src/typeset/parse/`, its layout in `src/typeset/layout/`, and its reading mode layout in a file of its own in `src/typeset/reading/` (C7) |
| diagram language | `src/typeset/diagrams/`: a file and one entry in `language.rs`; a program that draws it is run by the platform |
| image format | the decoder in `src/typeset/images/decode.rs` |
| theme or look setting | `src/typeset/theme/` |
| warning | a `Diagnostic` with a code of its own (B3); today a `Warning` in `src/pager/mod.rs` |
| highlight, or anything else that changes from frame to frame | a layer in the state engine (A3) |
| panel, such as help or a file list | a document in the panel's look, built by the state engine (A4) |
| feature of the pager | a file of its own in `src/session/`, never a new branch in `src/pager/mod.rs` |
| terminal feature | the I/O in `src/term/`, the bytes in the terminal layer (`src/present/`), and kitty's own sequences in `src/present/kitty/` |

Steps named in the table: A2 makes commands and the key map data, A3 makes
highlights layers, A4 makes panels documents, B3 replaces the warning types
by one `Diagnostic`, B7 puts trust decisions in one `Policy`, and C7 splits
reading mode's layout by block type.

### What `main.rs` is

`src/main.rs` is the binary's entry point and, today, far more than the
command line: it is some 2,500 lines with no unit tests. Besides `Cli` and
its clap definitions it opens files and standard input; has two routes, one
for a single file and one for several; prints documents; starts the pager;
loads images and diagrams, in the foreground, on threads and in the
background; reloads files for the pager (`FileReload`); and runs
`--doctor` and `--list-emojis`. Some of it is decisions that need no I/O,
and the table says where they go. It does not belong to any one part, and
is meant to shrink to wiring the others together.

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
