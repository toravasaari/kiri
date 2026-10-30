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
  Markdown file is split off in `src/typeset/parse/front_matter.rs` before comrak
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

A file given on the command line need not be Markdown: `src/typeset/input.rs`
tells an image from a document by its extension and first bytes, and an
image skips the parser and is drawn on its own, as large as fits the
window. A CSV or TSV file, told by its extension or by `--format csv`
or `--format tsv`, goes through a parser of its own, `src/typeset/parse/csv.rs`,
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
resolved in `src/main.rs`, before `src/typeset/input.rs` looks at the bytes, so
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
was written and not what the document is. `src/typeset/compare.rs` compares an
old document with a new one: it finds the blocks that stayed as they were,
then pairs the blocks of the same kind that stand in the gaps between them
as changed, and calls the rest removed or added. When a block that changed
holds other blocks, such as a quote or a list, the blocks inside are
compared the same way. The result is told by place, so that a caller can
ask what became of a block of the new document or which blocks are gone.
The places inside lists have two numbers, the item and then the block in
it; the layout's table has to use them too once it marks those blocks.

The pager shows the same styled lines on the terminal's alternate screen.
`src/session/view.rs` chooses the lines that fit in the window, a scaled
line whole or not at all, and moves them, and when the window changes size
it keeps the same block at the top; `src/pager/keys.rs` turns the bytes
the terminal sends into keys and keys into moves; `src/session/prompt.rs`
keeps the search term as it is typed, and `src/session/search.rs` finds it
in the laid-out lines and splits a line where a match starts and ends, so
the matches can be highlighted without a change to the writer;
`src/session/fade.rs` mixes the colours a match fades through after a jump,
those of the status line's `reloaded` as it fades out, and those of the
warning line;
`src/session/files.rs` finds where each of several files starts from the
blocks of the lines, so the status line names the file at the top of the
view and `:n` and `:p` move from one file's start to another's;
`src/session/status.rs` and `src/session/help.rs` make the status line and the
help panel as styled spans, and `src/session/warnings.rs` the line of text
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
`src/session/changes.rs` finds the runs of lines whose blocks changed or
were added, and the place of a removal with neither beside it, and chooses
one as the follow jump setting says, and `src/session/tint.rs` gives the
lines of that change a lighter background for a moment, fading out
through `src/session/fade.rs`. While the pager follows the
document, it asks the `Reload` on each poll for keys whether the document
changed, and reads it again once a change still looks the same 100 ms, a
poll's time, after it was first seen, so that a file caught half written
is not shown. The terminal itself, the alternate screen, raw mode and
reading keys from the controlling terminal, is opened in
`src/term/screen.rs`.

A reload shows the new text at once: the `Reload` keeps the images the
document still shows and starts the others loading in the background, in
the pager's `Loads`, which the pager polls beside the follow poll.
Meanwhile `src/session/loading.rs` chooses what stands in for each image
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
that show it, and whenever the view moves `src/session/loading.rs` orders
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

The pager keeps one image encoder, `src/present/encoder.rs`, for as long
as it runs. An image the terminal holds is not sent again: after a
resize that leaves it at the same size, or a reload that leaves its
pixels as they were. An image that the lines of a new layout no longer
place, or that changed, is deleted from the terminal, and when the
pager quits, every image it sent is.

## Reading mode

`--reading`, experimental, shows the text as a browser does: in a
proportional font, drawn as images. `src/typeset/reading/` knows nothing about
terminals. It lays a run of text blocks (headings, paragraphs, lists,
quotes, thematic breaks, code blocks, highlighted as the text layout
highlights them, and tables, whose columns share the room as a browser
shares it) out with parley, in the fonts of the fontdb
database that SVG text uses, and draws them with tiny-skia, colour emoji
included. GitHub's sizes and spacing are its measure: 16 pixels of text in
rows of 20, a column of 42 times that, margins that collapse between
blocks. `src/typeset/reading/engine.rs` turns such a run into a page: its rows are
image rows, as a large image's are, of parts 30 rows tall that the encoder
draws when a line first shows them, so the pager draws only the parts in
view. The rows keep their blocks, so the pager keeps its place across a
resize. Images, diagrams and formulas are in the page at their natural
size in CSS pixels, shrunk to the column, a formula in the middle of it;
the page gets them through `Pictures`, which `src/typeset/reading/engine.rs`
writes over the loaded images, as it gets code's colours through
`Highlight`, so `src/typeset/reading/` knows nothing of the loader. A picture
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
(`src/typeset/reading/octicons.rs`), and the blocks inside it in the room left.
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
band's block in `src/session/bands.rs`: the topmost band on the screen is
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
`src/session/search.rs` finds a term in the texts of a page, so a match is
on the row of its line of text, and a match in a band scrolls the band to
it and selects it. The marks are drawn into the page's image, a box in
the theme's match colours with the text drawn over it, and so is a box
round the selected link: what is drawn over a part is an overlay, named by
a hash of the marks and the link in that part alone, in the key of the
rows it is on, and kept by the encoder under its page and that hash, so
that the terminal keeps the part with it and the part without and moving
the current match draws only the parts it leaves and enters. A link is an
OSC 8 hyperlink round the placeholders it covers (`src/present/kitty/graphics.rs`),
which kitty opens as it does a link in text; in a band the cells move with
the scrolling.
`src/session/links.rs` selects one of the links on the screen with Tab and
Shift+Tab, numbering them from the page's own list so that a link keeps its
id while a band scrolls, and the status line says where it goes. In the
text layout a link is the run of spans on one line that share a hyperlink
address, and a link that wraps stays one link when the same link carries onto
the next line. A link is numbered in the block it starts in, and the
selected one is drawn in reverse video by `search::mark`; a link in a
heading drawn as an image has no spans, so it is not there to select. Tab
also reaches a link whose band is scrolled so that it is out of view, and
scrolls the band to it as a search match does. Enter copies the selected
link's URL with OSC 52 (`src/present/clipboard.rs`), wrapped in tmux's
passthrough where the images are.
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
`src/typeset/images/drawn.rs` decodes each image it gets back by its content,
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
and `src/typeset/images/heading.rs` draws the text through the SVG renderer in
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

Renderers live in `src/diagrams/`, outside `src/present/kitty/` and the parser.
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
`Language` in `src/typeset/diagrams/language.rs`: plain data, such as its name,
its display name, the extensions of its files, its theme names and the
command kiri looks for, with functions where languages differ, such as
reading the page a diagram chooses from its source, finding its first
line for a warning, writing a configuration file for its program, and
its built-in renderer. Mermaid's entry and what it needs, its directives,
the settings kiri gives a diagram and mermaid-cli's command line, are in
`src/typeset/diagrams/mermaid/mod.rs`, compiled in every build; only the part
that runs merman is behind the `mermaid` feature. The rest of kiri asks
the table: input detection for diagram files, the configuration file's
`[diagrams.<language>]` tables, `--doctor`'s rows and the image loader's
panel colour.

- **Built-in:** `BuiltIn` draws a language with the renderer its entry
  has in this build: Mermaid through merman, in a build with the
  `mermaid` feature, and formulas through RaTeX, in every build
  (`src/typeset/diagrams/math.rs`). `src/typeset/diagrams/mermaid/merman.rs` fixes merman's SVG
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
`src/typeset/diagrams/language.rs`, in a module of its own as Mermaid's is, with
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
`src/typeset/caps.rs`, passed to the renderer, so tests can render for any
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
| Text sizing | OSC 66 (kitty 0.40+) | `src/present/kitty/text.rs` | Headings in a larger font; without it, headings as images |
| Graphics | APC `_G` | `src/present/kitty/graphics.rs` | Images and diagrams, later formulas |
| Unicode placeholders | U+10EEEE with graphics | `src/present/kitty/graphics.rs` | Images that scroll with text, work in pagers and tmux |
| Protocol queries | OSC 66 tests, APC `_G` query | `src/present/kitty/query.rs` | Telling whether kitty's protocols work |
| Hyperlinks | OSC 8 | `src/present/output.rs` | Clickable links |
| Text attributes and colours | SGR | `src/present/output.rs` | Bold, italic, colours |
| Styled underlines | SGR 4:x, SGR 58 | `src/present/output.rs` | Heading underline styles; the dashed underline between table rows in the border colour |
| Cursor movement, erase | CSI C, CSI K, CSI H, CSI 2J | `src/present/output.rs` | Backgrounds beside scaled headings and across the width; the pager's rows |
| Line erase | CR, CSI 2K; CSI B, ESC 7, ESC 8 | `src/term/progress.rs`, `src/present/kitty/query.rs` | Wiping the progress line; erasing the rows the text sizing tests drew |
| Alternate screen, cursor | CSI ?1049h/l, CSI ?25l/h | `src/term/screen.rs` | The pager's screen of its own |
| Focus reporting | CSI ?1004h/l; CSI I, CSI O | `src/term/screen.rs`, `src/pager/keys.rs` | Telling the pager that its tmux pane is shown again |
| Mouse reporting | CSI ?1000h/l, CSI ?1006h/l; CSI < … M/m, CSI M | `src/term/screen.rs`, `src/pager/keys.rs` | The wheel scrolling the pager, also inside tmux; in reading mode, a click selecting a band and the sideways wheel scrolling it |
| Synchronized output | CSI ?2026h/l | `src/present/output.rs` | Pager frames without flicker |
| Cell size query | CSI 16 t; the answer CSI 6 ; height ; width t | `src/present/kitty/query.rs`, `src/present/cell_query.rs`, `src/pager/keys.rs` | The cell size in pixels where the window size has none, and inside tmux, where the window's may be tmux's guess: asked at start, and by the pager when the window changes size, so that it follows a new font size |
| Generic queries | CSI 6n, OSC 10, OSC 11, OSC 4, CSI > q, CSI c | `src/term/probe.rs` | Cursor position, foreground, background and palette colours, terminal version, end of the answers |
| tmux passthrough | DCS `tmux;` … ST, with each ESC doubled | `src/term/tmux.rs` | kitty's graphics, the probe's graphics query, and the probe's and the pager's cell size queries, through tmux to the terminal outside |
| Image checks | APC `_G` `a=p,U=1` and `a=d,d=i` with a placement id | `src/present/kitty/graphics.rs`, `src/term/check.rs` | Whether kitty behind tmux holds the images kiri sent |

Sequences that only kitty's protocols define live under `src/present/kitty/`: text
sizing, graphics, Unicode placeholders and the queries that detect them;
`src/term/probe.rs` sends those along with the generic queries it spells
out itself. OSC 8 hyperlinks, SGR attributes, underline styles and cursor
movement are general terminal features that `src/present/output.rs` writes
itself; for scaled text it calls `src/present/kitty/text.rs`, and for images
`src/present/encoder.rs`, which sends and deletes them, and
`src/present/kitty/graphics.rs`, which writes those sequences and the
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

- **The typesetting engine, `src/typeset/`:** pure given its resources;
  the fonts, for one, are passed in. It owns the document model, parsing
  and the input formats, the themes and the look, both layout engines (the
  cells and reading mode), and the images and diagrams that it draws from
  data it is given.
- **The terminal layer, `src/present/`:** pure and sans-IO. It turns what
  is shown into bytes for an `impl Write` (`kitty/` and the encoders) and
  decodes keys and the terminal's answers to the probe. It makes no system
  calls.
- **The state engine, `src/session/`:** pure. It holds the pager's state
  and its decisions, and the components and panels that the pager is made
  of.
- **The platform:** `src/main.rs` and the rest of `src/` outside the three
  part directories. It owns all the I/O: the command line and the settings
  files, the shells (interactive and print), the work runner (threads,
  programs, watchers) and `term/`, where kiri talks to the terminal.
  `main.rs` wires the parts together.

Each part uses only the parts above it in this list: the typesetting
engine uses none of the others, the terminal layer uses the typesetting
engine, the state engine uses both, and the platform uses all three. A
mixed module (below) has code in two parts, so its pieces follow this rule
one by one. The exceptions to the rule that exist today are listed after
the table.

### Every module

The table has a row for each file of `src/`. **Part** is the part the
module's code belongs to; **mixed** means that its non-test code does the
work of two parts, usually I/O inside what would otherwise be pure. **After
the move** says where the file goes when the code is moved into the parts.
That move is of whole modules and nothing else, and leaves no re-export at
the old path. The rules for the move:

- A whole module moves to the path in its row. Its `mod` line goes from the
  old parent to the new one: when the old parent stays, it drops the
  line, and the new parent declares the module.
- A mixed file under a parent that moves moves with it, and its row says
  which work splits it. Its I/O is on the list of exceptions until then.
- A mixed module whose parent does not move stays at its path, and the row
  names its pieces, where each goes and the work that splits it.
- `src/lib.rs` declares the three parts, and each part's root exports its
  public surface. The files for the new parents are in their own table
  below.

The later work that the rows name, in words:

- **The config split:** `config.rs` is divided, and each setting gets an
  owner.
- **The font database:** one font database, loaded by the platform and
  handed to the engines.
- **The startup plan:** the decisions of `main.rs` move into
  `session/plan.rs`, with what they ask of the terminal and the files
  passed in as values.
- **One route:** the routes for one file and for several become one.
- **The pager's loop:** the pager's state and its loop are split, the state
  into the state engine and the loop into the platform, as messages in and
  effects out.
- **Loading and sessions:** the platform reads, fetches and runs, and the
  engines draw from the data it gives them.
- **The terminal layer split:** the presenter, the encoders, and the key and
  probe decoding that today sit in `term/`, `render/` and `pager/` are
  gathered in the terminal layer.
- **Commands as data, layers, panels as documents, diagnostics, the trust
  policy, reading mode's block modules:** later work that the map below
  names, for the places that do not exist yet.

A few files arrive with the branch `fix/reading-leftovers`, which is not
merged yet: `term/cell_query.rs`, `term/clipboard.rs` and
`pager/tests/links.rs`. They have rows too.

| Module | Part | After the move |
|---|---|---|
| `src/lib.rs` | platform | stays; declares the three parts |
| `src/main.rs` | mixed | stays. Platform: the command line, opening files and standard input, the shells, loading, the file reload. State engine: the decisions `printing`, `pages_alone`, `surely_paged` and `may_load_later`, in the startup plan. Only `printing` is free of I/O: `pages_alone` and `may_load_later` ask whether stdout is a terminal, and `surely_paged` measures images and reads the window size. The plan takes the answers in as values read by `main.rs`. One route makes the one-file and many-files routes one |
| `src/config.rs` | mixed | stays. Platform: `Environment`, which reads the process's variables, the file, and resolving a setting from the flags, the environment, the file and its default. State engine: the setting types the pager reads (`Paging`, `SearchCase`, `Follow`, `FollowJump`). `Passthrough` stays a setting type with the others, though it is read by capability detection, the command line and `--doctor`; capability detection takes its value, and the variables it reads (`TMUX`, `TMUX_PANE`, `NO_COLOR`, `COLORTERM`, `TERM_PROGRAM` and `VTE_VERSION`), as arguments, not `Environment`. The config split |
| `src/config/tests.rs` | follows `config.rs` | stays; goes with its pieces in the config split |
| `src/doctor.rs` | platform | stays (it searches `PATH` through `diagrams::find`) |
| `src/list.rs` | platform | stays (the `--list-emojis` shell) |
| `src/panics.rs` | platform | stays |
| `src/testing.rs` | platform | stays (test support) |
| `src/caps.rs` | typesetting | moved |
| `src/color.rs` | typesetting | moved |
| `src/size.rs` | typesetting | moved |
| `src/model.rs` | typesetting | moved |
| `src/input.rs` | typesetting | moved |
| `src/compare.rs` | typesetting | moved |
| `src/parse/mod.rs` | typesetting | moved |
| `src/parse/html.rs` | typesetting | moved |
| `src/parse/csv.rs` | typesetting | moved |
| `src/parse/csv/tests.rs` | typesetting | moved |
| `src/parse/front_matter.rs` | typesetting | moved |
| `src/parse/front_matter/tests.rs` | typesetting | moved |
| `src/parse/tests.rs` | typesetting | moved |
| `src/theme/mod.rs` | typesetting | moved |
| `src/theme/paper.rs` | typesetting | moved |
| `src/theme/file.rs` | mixed | moved. Typesetting: parsing the theme file's text. Platform: `load`, which reads the file; the config split moves `load` out |
| `src/kitty/mod.rs` | terminal layer | moved |
| `src/kitty/graphics.rs` | terminal layer | moved |
| `src/kitty/query.rs` | terminal layer | moved |
| `src/kitty/text.rs` | terminal layer | moved |
| `src/render/mod.rs` | mixed | stays. Typesetting: `Inputs`, the render options (`FrontMatterStyle`, `Headings`, `TableRows`, `RenderErrors`) and `heading_look`, the front matter helpers. Terminal layer: `render`, `render_files`, `render_file`, `render_image` and `render_image_on`, which lay out and write in one call. The terminal layer split |
| `src/render/block.rs` | typesetting | moved |
| `src/render/style.rs` | typesetting | moved |
| `src/render/image.rs` | typesetting | moved |
| `src/render/wrap.rs` | typesetting | moved |
| `src/render/layout/mod.rs` | typesetting | moved |
| `src/render/layout/code.rs` | typesetting | moved |
| `src/render/layout/front_matter.rs` | typesetting | moved |
| `src/render/layout/list.rs` | typesetting | moved |
| `src/render/layout/quote.rs` | typesetting | moved |
| `src/render/table.rs` | typesetting | moved |
| `src/render/table/tests.rs` | typesetting | moved |
| `src/render/script.rs` | typesetting | moved |
| `src/render/highlight.rs` | typesetting | moved |
| `src/render/reading.rs` | typesetting | moved |
| `src/render/output.rs` | terminal layer | moved |
| `src/render/encoder.rs` | terminal layer | moved |
| `src/render/loading.rs` | mixed | stays. Typesetting: `Ink` (the struct), `dimmed` and `band`, which draw pixels. Terminal layer: `Ink::new`, the one function that takes the output's `Page`, and `band_offset`. The terminal layer split |
| `src/reading/mod.rs` | typesetting | moved |
| `src/reading/colr.rs` | typesetting | moved |
| `src/reading/octicons.rs` | typesetting | moved |
| `src/reading/page.rs` | typesetting | moved |
| `src/reading/paint.rs` | typesetting | moved |
| `src/reading/text.rs` | typesetting | moved |
| `src/reading/fonts.rs` | mixed | moved. Typesetting: `Fonts`, the fonts handed to parley, with the fallbacks. Platform: loading the system fonts and memory-mapping the font files, lazily, on the first layout; the font database moves that loading out |
| `src/images/mod.rs` | mixed | stays. Typesetting: `ImageKey`, `ImageError`, `Images`, the plan of which images a document wants, and `LoadOptions`, the drawing options, which carry `progress`, a callback that the platform's `load_parts` calls. Platform: `load`, `load_with` and `load_parts`, which read files and run the renderers. Loading and sessions |
| `src/images/decode.rs` | typesetting | moved |
| `src/images/drawn.rs` | typesetting | moved |
| `src/images/drawn/tests.rs` | typesetting | moved |
| `src/images/heading.rs` | typesetting | moved |
| `src/images/svg.rs` | mixed | stays. Typesetting: `Svg`, parsing and drawing, and `Fonts`, the database handed in. Platform: `Fonts::system`, which scans the system's fonts. The font database |
| `src/images/url.rs` | mixed | stays. Typesetting: `data:` URLs and the reading of a URL's parts. Platform: `read_file`, which opens files. Loading and sessions |
| `src/images/background.rs` | platform | stays |
| `src/images/pipeline.rs` | platform | stays |
| `src/images/pipeline/tests.rs` | platform | stays |
| `src/images/stamps.rs` | platform | stays |
| `src/diagrams/mod.rs` | mixed | stays. Typesetting: `Request`, `Drawn`, `Theme`, `FormulaColors`, `INLINE_FORMULA`, `centred`, `dark_background`, `baseline_depth`, `noun`, `name`, `BuiltIn` and the `Renderer` trait. Platform: the external choice, which is `Renderers` with its `External` and `chosen`, and `render_all`'s parallel run through `render_in_parallel`, on threads. Loading and sessions |
| `src/diagrams/language.rs` | typesetting | moved |
| `src/diagrams/math.rs` | typesetting | moved |
| `src/diagrams/mermaid/mod.rs` | typesetting | moved |
| `src/diagrams/mermaid/merman.rs` | typesetting | moved |
| `src/diagrams/external.rs` | platform | stays |
| `src/diagrams/external/tests.rs` | platform | stays |
| `src/diagrams/find.rs` | platform | stays |
| `src/pager/mod.rs` | mixed | stays. State engine: `State`, `Warning`, the key and tick handling, the frame. Platform: `page_through` and `show`, the loop, with the `Reload`, `InlineLoader`, `Loads` and `Terminal` it does its I/O through, and the load scheduling, `load_headings` and `prefer`, which use `Loads`; the order from `loading::nearest` goes in as values. The pager's loop |
| `src/pager/tests/mod.rs` | follows `pager/mod.rs` | stays; goes with its pieces in the pager's loop |
| `src/pager/tests/files.rs` | follows `pager/mod.rs` | stays; goes with its pieces in the pager's loop |
| `src/pager/tests/following.rs` | follows `pager/mod.rs` | stays; goes with its pieces in the pager's loop |
| `src/pager/tests/images.rs` | follows `pager/mod.rs` | stays; goes with its pieces in the pager's loop |
| `src/pager/tests/links.rs` | follows `pager/mod.rs` | stays; goes with its pieces in the pager's loop (arrives with `fix/reading-leftovers`) |
| `src/pager/tests/loading.rs` | follows `pager/mod.rs` | stays; goes with its pieces in the pager's loop |
| `src/pager/tests/paging.rs` | follows `pager/mod.rs` | stays; goes with its pieces in the pager's loop |
| `src/pager/tests/reading.rs` | follows `pager/mod.rs` | stays; goes with its pieces in the pager's loop |
| `src/pager/tests/reload.rs` | follows `pager/mod.rs` | stays; goes with its pieces in the pager's loop |
| `src/pager/tests/search.rs` | follows `pager/mod.rs` | stays; goes with its pieces in the pager's loop |
| `src/pager/tests/view.rs` | follows `pager/mod.rs` | stays; goes with its pieces in the pager's loop |
| `src/pager/keys.rs` | mixed | stays. Terminal layer: `Key` and the `Decoder` that turns bytes into keys. State engine: `Action`, `BINDINGS` and `action`. The terminal layer split for the decoding, the pager's loop for the actions |
| `src/pager/bands.rs` | state engine | moved |
| `src/pager/changes.rs` | state engine | moved |
| `src/pager/fade.rs` | state engine | moved |
| `src/pager/files.rs` | state engine | moved |
| `src/pager/help.rs` | state engine | moved |
| `src/pager/links.rs` | state engine | moved |
| `src/pager/loading.rs` | state engine | moved |
| `src/pager/prompt.rs` | state engine | moved |
| `src/pager/search.rs` | state engine | moved |
| `src/pager/status.rs` | state engine | moved |
| `src/pager/status/tests.rs` | state engine | moved |
| `src/pager/tint.rs` | state engine | moved |
| `src/pager/view.rs` | state engine | moved |
| `src/pager/warnings.rs` | state engine | moved |
| `src/term/mod.rs` | platform | stays |
| `src/term/answers.rs` | terminal layer | moved |
| `src/term/cell_query.rs` | terminal layer | moved (arrives with `fix/reading-leftovers`). It uses `tmux::passthrough`, which stays in `term/tmux.rs` until the terminal layer split |
| `src/term/clipboard.rs` | terminal layer | moved (arrives with `fix/reading-leftovers`; it builds the OSC 52 bytes and writes them to an `impl Write`). It uses `tmux::passthrough`, which stays in `term/tmux.rs` until the terminal layer split |
| `src/term/check.rs` | mixed | stays. Platform: asking kitty behind tmux, on the terminal. Terminal layer: reading what it answered, `lost` and `sequences`. The terminal layer split |
| `src/term/detect.rs` | mixed | stays. Platform: `capabilities` and `examine`, which read the window, the environment and the probe's answer. Terminal layer: turning those into `Capabilities` (`with_probe`, `with_window`, `through_tmux`). The terminal layer split |
| `src/term/probe.rs` | mixed | stays. Platform: sending the probe and reading the answer on the controlling terminal. Terminal layer: the text of the queries. The terminal layer split |
| `src/term/tmux.rs` | mixed | stays. Platform: running `tmux` and reading its answer, and `run_id`, which reads the process id, the clock and the OS's random seed. Terminal layer: `Wrapping` and `passthrough`, which wrap kitty's sequences for tmux, and `Pane`, `prepare` and `ClientCells`, which are plain data or take the run (`prepare`, `ClientCells::window`) as a value. The terminal layer split passes the run id in as a value |
| `src/term/window.rs` | mixed | stays. Platform: `window_size`, the ioctl. Terminal layer: `WindowSize`. The terminal layer split |
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

The move creates these files; they are not in the table above, because they do
not exist yet. Each declares the modules that moved into its directory and
exports the public surface of its part.

| New file | What it is |
|---|---|
| `src/typeset/mod.rs` | new: the root of the typesetting engine |
| `src/present/mod.rs` | new: the root of the terminal layer |
| `src/session/mod.rs` | new: the root of the state engine |
| `src/typeset/images/mod.rs` | new: declares `decode`, `drawn`, `heading` and `size`; the pure pieces of `images/mod.rs` arrive in it with loading and sessions |
| `src/typeset/diagrams/mod.rs` | new: declares `language`, `math` and `mermaid`; the pure pieces of `diagrams/mod.rs` arrive in it with loading and sessions |

The move also changes the paths that name moved files elsewhere: `AGENTS.md`
says that kitty's sequences live under `src/present/kitty/`, which becomes
`src/present/kitty/`, and the sections of this document above name today's
paths.

### Exceptions to the direction

An exception is code that uses a part below its own in the list of parts,
or that does the work of another part. This table lists the exceptions
known today, each with the work that removes it. It is not a proof that
there are no others. A script resolved every `crate::` and `super::` path
in the non-test code of every file that the table puts wholly in a part,
and found the first two rows; the rest were found by reading the pieces of
mixed modules. The direction rules and this table cover non-test code
only: test modules may use any part. The direction test that the guards
work adds will hold the complete list and check it. A mixed module
is read by piece, so edges from whole files into one go to the pieces named
in its row.

| Edge | Removed by |
|---|---|
| `caps.rs` (typesetting) uses `kitty::graphics::MAX_CELLS` (terminal layer) | giving `caps.rs` the constant, a small change of its own |
| `diagrams/language.rs` and `diagrams/mermaid/mod.rs` (typesetting) use `diagrams::external::Output` (platform) | loading and sessions, which moves `Output` to the typesetting side |
| `Ink::new` in `render/loading.rs` (typesetting's `Ink`) takes the terminal layer's `output::Page` | the terminal layer split, which moves `Ink::new` to the terminal layer |
| `term/clipboard.rs` and `term/cell_query.rs` (terminal layer) import `passthrough` from `term/tmux.rs`, which is at a platform path until the split | the terminal layer split, which moves `passthrough` |
| `pages_alone` and `may_load_later` (state engine) call `term::stdout_is_terminal` (platform) | the startup plan, which takes the answer as a value |
| `surely_paged` (state engine) calls `images::measure_in_background` and `term::window_size` (platform) | the startup plan, which takes the measured layout (or the window rows) and the window size as values |
| `detect` in `term/detect.rs` (terminal layer) calls `tmux::run_id` (platform) | the terminal layer split, which passes the run id in |
| `term/detect.rs` and `term/tmux.rs` (terminal layer) take `config::Environment` (platform) and `config::Passthrough` (a setting type) | the terminal layer split, which passes the variables and the `Passthrough` value as arguments |
| `reading/fonts.rs` (typesetting) loads and memory-maps system fonts | the font database |
| `theme/file.rs` (typesetting) has `load`, which reads a file | the config split |

### Where a new thing goes

New code goes where this map puts it. When the map has no place for it,
that is a question for the maintainer, not a reason to add it to a hub
file (`main.rs`, `pager/mod.rs`, `config.rs`, `render/mod.rs`,
`render/layout/mod.rs`, `render/reading.rs`, `reading/page.rs`,
`images/mod.rs` and `diagrams/mod.rs`). **No path yet** means that the
work named creates the place; until then, ask.

| A new… | Goes in |
|---|---|
| command-line option | `src/cli.rs`, new, made by one route; until then `Cli` in `src/main.rs` |
| startup decision, such as whether a run pages | `src/session/plan.rs`, new, made by the startup plan; take what it needs from the terminal as values |
| key or action | `src/session/commands.rs`, new, made by commands as data; until then `BINDINGS` in `src/pager/keys.rs` |
| setting | no path yet, the config split makes the registry of setting keys, each with its owner, its effect and its medium; until then `src/config.rs`, named after its owner |
| use of a file, the network or a program | no path yet, loading and sessions makes the platform's work runner, and the trust policy a `Policy`; until then `src/images/background.rs`, `src/images/pipeline.rs` or `src/diagrams/external.rs` |
| block type or input format | the typesetting engine's registry of block types and formats, no path yet (today `src/typeset/model.rs`, `src/typeset/parse/` and `src/typeset/layout/`); in reading mode a module per block type in `src/typeset/reading/`, new, made by reading mode's block modules |
| diagram language | `src/typeset/diagrams/`: a file and one entry in `language.rs`; a program that draws it is run by the platform |
| image format | the decoder in `src/typeset/images/decode.rs` |
| theme or look setting | `src/typeset/theme/` |
| warning | a `Diagnostic` with a code of its own, no path yet, made by diagnostics; until then a `Warning` in `src/pager/mod.rs` |
| highlight, or anything else that changes from frame to frame | a layer, no path yet, made by layers, in the state engine (`src/session/`) |
| panel, such as help or a file list | a document in the panel's look, no path yet, made by panels as documents, built in `src/session/` |
| feature of the pager | a file of its own in `src/session/`, never a new branch in `src/pager/mod.rs` |
| terminal feature | the I/O in `src/term/`, the bytes in the terminal layer (`src/present/`), and kitty's own sequences in `src/present/kitty/` |

The paths marked new are for the maintainer to confirm.

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
