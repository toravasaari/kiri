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
gets the file as it is instead, from `src/frontend/print.rs`.

The document can also come from standard input, which errors call
`<stdin>`, or from a directory, which stands for its README. Both are
resolved in `src/frontend/open.rs`, before `src/typeset/input.rs` looks at
the bytes, so an image piped in is shown on its own as well.

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
document on its own, without a header, and has nothing to reload. A
document given alone goes the same way, with a few differences from
several files: the pager reads it again when it changes, if it is a
regular file, and the warnings about it are told without its name.

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
it keeps the same block at the top; `src/present/keys.rs` turns the bytes
the terminal sends into keys; `src/session/commands.rs` holds the
commands, each with its topic, keys, help text and action, and the keys
are looked up there, and `src/session/help.rs` builds the help panel from
them; `src/session/prompt.rs` keeps the search term as it is typed, and
`src/session/search.rs` finds it in the laid-out lines, and
`src/session/layers.rs` lays the highlights over them, splitting a line where
a match starts and ends, so the matches can be highlighted without a
change to the writer;
`src/session/fade.rs` mixes the colours a match fades through after a jump,
those of the status line's `reloaded` as it fades out, and those of the
warning line;
`src/session/files.rs` finds where each of several files starts from the
blocks of the lines, so the status line names the file at the top of the
view and `:n` and `:p` move from one file's start to another's; the names
and the count are those of the sources in the session
(`src/session/sources.rs`), which `src/frontend/pager.rs` makes from the
files given and the pager's loop owns, and a reload brings the source's
version forward; `src/session/status.rs` and `src/session/help.rs` make the
status line and the help panel as styled spans, and
`src/session/warnings.rs` the line of text above the status line that tells
the first warning; `src/pager/mod.rs` draws each frame, row by row through
the backend's writer, and lays the document out again when the window
changes size or when `r` reloads it. For that the pager is given a `Reload`,
as it is given a `Terminal` for keys: it asks the `Reload` for the document
and the images again, and keeps its place by the block at the top: it
compares the old document with the new one, and follows the block at the top
to the block that stayed or changed from it, so a block added above the view
does not move it. When the block at the top is gone, the view goes to the
next block that is still there. A view at the end of the document goes to
the new end, so a document that grew shows what was added. Following on auto
then moves the view to a change: `src/session/changes.rs` finds the runs of
lines whose blocks changed or were added, and the place of a removal with
neither beside it, and chooses one as the follow jump setting says, and
`src/session/tint.rs` gives the lines of that change a lighter background
for a moment, fading out through `src/session/fade.rs`. While the pager
follows the document, it asks the `Reload` on each poll for keys whether the
document changed, and reads it again once a change still looks the same 100
ms, a poll's time, after it was first seen, so that a file caught half written
is not shown. The terminal itself, the alternate screen, raw mode and
reading keys from the controlling terminal, is opened in
`src/term/screen.rs`.

A reload shows the new text at once: the `Reload` keeps the images the
document still shows and sets the others aside, and the pager loads what
the new document wants and does not hold in the background, in its
`Loads`, which it polls beside the follow poll. What a document wants is
asked of the frontend's planner (`Planner`, in `src/pager/mod.rs`): the
jobs of its images, diagrams and headings, in the order of the document,
with the pictures in the middle of text when reading mode is on. The
pager asks again only when the document is read again, when `M` turns
reading mode on or off, or when the cells change; scrolling and the like
leave the plan as it is.
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
that show it, and `src/session/loading.rs` orders the images by how far
those lines are from the view, a line below before a line above, and the
images no line shows, such as a heading's or a picture in the text, after
them in the order of the document. The loop gives `Loads` the wanted images
in that order after each layout, and tells it the order again at each poll,
as the view may have moved; `Loads` hands the waiting jobs out in it. Say
the reader is at the end when a reload adds three diagrams: the last one
starts first. An image that comes is laid out as after a resize, but the
top line stays the same line, so the view does not move (the pager keeps
the image aside, as the layout uses the images, and puts it in as the
next layout starts), and the tint of auto-follow's change stays on its
blocks; auto-follow jumped already when the text came. A load stops when
its image is no longer wanted: a newer reload drops it, `M` turns off the
reading mode that wanted it, or the cells changed and the heading images
of the old ones are not wanted any more. A diagram that failed is drawn
again only when the document is read again, not at each layout. `q`, or a
signal, stops the programs that draw. `close_programs` in
`src/frontend/loads.rs` waits for them to end once the pager has left its
screen, with `kiri: closing background programs…` on standard error while that
takes more than a second, but not for merman, which cannot stop. `Images`
notes which outcomes came in the background; once the pager has left its
screen, `src/frontend/pager.rs` writes the warnings of those that failed and are
still shown, as it does before the pager opens for images loaded first.

With `initial_load = "background"`, the initial load goes the same way when
the pager surely opens. With paging on `auto`, `src/frontend/loads.rs` tells
that by laying the documents out with a box of three rows,
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

A page is drawn opaque when the theme gives it a background whose colour
is known. Then all of its text is blended by kiri over the same ground,
shaded rows or not. Otherwise the page stays transparent, so that the
terminal's background, also a translucent one or an image, shows through
it as it does around it, and the terminal blends the text. `B` in the
pager gives the theme the background the terminal reported, one shade off
unless `background_offset` is false, as kitty shows its own background
through a cell of exactly that colour; or it takes the theme's away. Either
lasts as long as the pager runs.

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
the next line, which the pager tells by the link's `Rc`. Touching links to
one address share one `Rc`, so they stay one link across a line break too. A link is numbered in the block it starts in, and the
selected one is drawn in reverse video by its layer in
`src/session/layers.rs`; a link in a heading drawn as an image has no
spans, so it is not there to select. Tab also reaches a link whose band
is scrolled so that it is out of view, and scrolls the band to it as a
search match does. Enter copies the selected
link's URL with OSC 52 (`src/present/clipboard.rs`), wrapped in tmux's
passthrough where the images are.
`Reading` also holds a cache of the pictures scaled for the last layout,
by image, its pixels and size, so that a layout scales only what is new
and lets go of what it did not show.

## Diagrams

A diagram stays a code block in the document model, `Block::CodeBlock` with
its language, so the parser knows nothing about renderers and a diagram that
cannot be drawn is shown as the code it already is. Whether it is drawn is
decided when images are loaded. `src/typeset/images/key.rs` keys each image by
an `ImageKey`, a URL or a block (its language, in lower case, its source, and
a fingerprint of how it is drawn: the page it is for, the formula colours and
the panel, so that a diagram drawn for a dark page is not the one a light page
asks for), with the number of the document it is in, so that documents shown
together keep their own images, and `src/typeset/images/plan.rs` collects the
code blocks whose language a renderer draws besides the lone images.
`src/load/mod.rs` hands all of a document's diagrams to the renderer at once,
then `src/typeset/images/drawn.rs` decodes each image it gets back by its
content, as it decodes an image file, so sizing, the width limit and redrawing
after a window resize work as for any image. An SVG is decoded as an SVG file
is; a raster image, such as the PNG of a browser's drawing, becomes
`Source::Dense`, pixels with their size in CSS pixels, so that it is sized in
CSS pixels as an SVG is. Its pixels are reduced there to the most an image can
be shown at, as many cells as one image covers each way, so that a large
diagram holds no more memory than showing it needs; its size in CSS pixels
stays. That size is not in the key, on purpose: only a font change moves it,
and a new key would draw every diagram again, running its program, with
the diagram showing as code until then. The gain would be sharper raster
diagrams after a larger font; a dimmed old drawing standing in meanwhile
would make it worth doing. There it also grows
an SVG diagram whose text runs past its edges, and lays one drawn in colours
for the other page, light or dark, on a panel of its own background, after the
reduction, so the panel's copy is small too.

A heading is drawn as an image the same way, where the terminal draws images
but not large text, as kitty inside tmux: `ImageKey::Heading` holds its level,
its words, each marked with the inline code or key it is in and whether it is
emphasised, the cell size it is drawn for and a fingerprint of the look of its
level, and `src/typeset/images/heading.rs` draws the text through the SVG
renderer in the system's sans-serif font, emphasis in italics, and inline code
and keys in its monospace font on the background of inline code, at the size
and in the colours that `render::heading_look` takes from the one look
(`src/typeset/theme/look.rs`), which is resolved from the theme and the
terminal, as the text layout's inline code takes them, so
`src/typeset/images/` knows nothing of themes. Other inline styles are drawn
as the heading's text. Each word starts on a whole cell, and `Source::Text`
tells the columns each word covers, so layout wraps a heading too long for the
window as the sized one wraps: each line writes the placeholders of the
image's columns its words cover, which kitty's placeholders allow from any
column, and the image stays one whatever the width. A heading whose image did
not come is laid out as text, without a warning.

Renderers live in `src/typeset/diagrams/` (the trait and those built in) and
`src/diagrams/` (the programs that draw: `external.rs` and `find.rs`). The
choice between them, `Renderers`, is in `src/load/renderers.rs`. None of them
is under `src/present/kitty/` or in the parser.
A renderer implements the `Renderer` trait: whether it draws a language,
and a diagram's source and a `Theme`, for a light or a dark page, with
the colour of the page's text for a formula, turned into an image file, SVG or raster, and the page its colours are for. It
knows nothing about terminals, so an HTML backend could use the same
image.

`src/diagrams/external.rs` leans on the image engine
(`src/typeset/images/`). `External` takes its pixel limit `MAX_PIXELS`, its
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
- **Together:** `Renderers`, in `src/load/renderers.rs`, draws each
  language with its external command when it has one and with the
  built-in renderer otherwise. An external command that fails does not
  fall back to the built-in one.

A reload, `r` in the pager, loads only the images the document does not
have yet: a diagram is kept by its key, which holds its source, so one
that did not change is not drawn again, unless it failed to draw, such as
for a time limit; and an image from a local file is loaded again when its
modification time, which `src/images/stamps.rs` keeps, is not the one it
was loaded at. Other images are kept as they are. What is loaded loads
in the background, `src/load/queue.rs`, on threads of their own,
with at most four programs drawing at once, also when reloads overlap;
while four draw, an image file loads before the diagrams waiting for a
program. Each load has a stop flag of its own, which `External` looks at;
a signal or `q` sets them all.

`src/images/pipeline.rs` builds the renderers from the settings and the
environment, given as values, and the jobs that load a document's images
with them; it starts none. The images that load before the pager, which
print and the pager's first load wait for, go through the same queue:
`src/frontend/loads.rs` gives the jobs to `Loads::want`, waits in one loop
that also serves the wait below, and puts the images in the order of the
document, not the order they came in, so that the warnings come in the
document's order. The loop tells `src/term/progress.rs`, which shows on the
terminal how far loading has got when it takes long, and another caller
may tell it elsewhere; the line counts the diagrams for the load first, and
every load for the wait. Where the pager surely opens and standard error
is a terminal, the wait loads in the background instead, while
`src/term/keys.rs` reads keys with the terminal in a mode that keeps
Ctrl-C a signal; Space or Enter hands the loads still under way to the
pager, and the mode, with any key typed meanwhile, is gone before the
pager or the shell takes the terminal.

The programs run in process groups of their own, which Ctrl-C from the
terminal does not reach, nor a signal sent to kiri alone. So one value,
`Guard` in `src/frontend/loads.rs`, decides who catches SIGINT, SIGQUIT,
SIGTERM and SIGHUP before the pager's screen does, for the whole run, and
nothing else in the frontends asks for `Platform::interrupt`, whose
default calls `Interrupt::catch` from `src/term/interrupt.rs`. Without a program drawing and without keys read,
nobody does and the signals end kiri at once. While kiri waits for loads
that a program draws, or reads Space and Enter with Ctrl-C still a
signal, the guard catches them: the loads stop, SIGINT first so that
`mmdc` can close its browser, their files are removed, and kiri then ends
by the signal that came. Once the loads have come, the guard is released,
so a signal ends kiri at once again, though it keeps its handlers, without
which the signals would do nothing while the documents are printed. It is
held while the loads the reader skipped go on into the pager, and gone
when the pager's screen catches the signals, from before its terminal is
raw until it has been restored, or until the screen fails to open, when
they end kiri again while the document is printed. In raw mode Ctrl-C is
the key `^C`, which quits. A signal that was ignored when kiri started, as
under `nohup`, stays ignored in all of it. When a diagram file drawn by a
program and documents waited for load together, there is still one
guard, so Ctrl-C stops the programs of both. A new diagram language is a
new entry in `LANGUAGES` in `src/typeset/diagrams/language.rs`, in a
module of its own as Mermaid's is, with a default command, a built-in
renderer or both; the rest follows from the entry.

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
the writer the output goes through (`term::wrapped`, and the pager's
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
from. `src/term/` and `src/config/` hand it the detection, the
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
| Focus reporting | CSI ?1004h/l; CSI I, CSI O | `src/term/screen.rs`, `src/present/keys.rs` | Telling the pager that its tmux pane is shown again |
| Mouse reporting | CSI ?1000h/l, CSI ?1006h/l; CSI < … M/m, CSI M | `src/term/screen.rs`, `src/present/keys.rs` | The wheel scrolling the pager, also inside tmux; in reading mode, a click selecting a band and the sideways wheel scrolling it |
| Synchronized output | CSI ?2026h/l | `src/present/output.rs` | Pager frames without flicker |
| Cell size query | CSI 16 t; the answer CSI 6 ; height ; width t | `src/present/kitty/query.rs`, `src/present/cell_query.rs`, `src/present/keys.rs` | The cell size in pixels where the window size has none, and inside tmux, where the window's may be tmux's guess: asked at start, and by the pager when the window changes size, so that it follows a new font size |
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
private. The fourth part, the platform, is `src/main.rs`, the frontends in
`src/frontend/` and the rest of `src/` outside those three directories.

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
- **The platform:** `src/main.rs`, `src/frontend/` and the rest of `src/`
  outside the three part directories. It owns all the I/O: the command line
  and the settings files, the frontends (interactive and print), the work
  runner (threads, programs, watchers) and `term/`, where kiri talks to the
  terminal. `main.rs` reads the command line and starts the frontends,
  which wire the parts together.

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

- **The config split (done):** `config.rs` is divided, and each setting is
  declared once, with its owner, in `src/config/registry.rs`.
- **The font database:** one font database, loaded by the platform and
  handed to the engines.
- **The startup plan:** the decisions of the frontends are in
  `session/plan.rs`, with what they ask of the terminal and the files
  passed in as values.
- **The pager's loop:** the pager's state and its loop are split, the state
  into the state engine and the loop into the platform, as messages in and
  effects out.
- **Loading and sessions:** the platform reads, fetches and runs, and the
  engines draw from the data it gives them. Its first step is done: the
  mixed modules are split into pure pieces in the typesetting engine and
  the platform's pieces in `src/load/`. The work runner and the sessions
  are left.
- **The terminal layer split:** the presenter, the encoders, and the key and
  probe decoding that today sit in `term/`, `render/` and `pager/` are
  gathered in the terminal layer.
- **Commands as data, the selected band as a layer, panels as documents,
  the trust policy, reading mode's block modules:** later work
  that the map below names, for the places that do not exist yet.

| Module | Part | After the move |
|---|---|---|
| `src/lib.rs` | platform | stays; declares the three parts |
| `src/main.rs` | platform | stays, and shrinks to the entry point. It reads the command line (`src/cli.rs`), resolves the settings from it, the environment and the configuration file, hands them and the files to `src/frontend/` as `Asked` and `Configured`, and turns the `Failure` it gets back into an exit code; its `Real` is the `Platform` of the process. State engine: the startup decisions, such as whether a run pages, are in `src/session/plan.rs`; the frontends read the terminal and the files and pass the answers in |
| `src/load.rs` | platform | moved to `src/frontend/loads.rs`, by way of `src/loading.rs`; the library's `src/load/` is a different module |
| `src/frontend/mod.rs` | platform | new. The frontends' module: the `Platform` trait, through which they reach files, standard input and output, the environment and the terminal, and also the loads of images (`loads`), the catching of the signals (`interrupt`) and what a wait for loads waits with (`pause`, a `Pause`), each with a default for the process; and `Failure`; `Screen` (in `pager.rs`), `Asked` and `Configured` (in `route.rs`) and what `main.rs` and `src/reports.rs` call, `show_files`, `show_document` and `shown`, are re-exported from the files that hold them. `main.rs` implements `Platform` for the process (`Real`); the simulated frontend does it in memory |
| `src/frontend/route.rs` | platform | new. The one route: `show_files` shows one file as the special case of several, with no header, and with reloading, following and the unnamed warnings for it alone; `show_given` chooses between the pager and print; `Context`, `Run` and `Show`; `Asked`, what the command line asks for, and `Configured`, the settings as `main.rs` resolved them; `show_document` shows a report kiri writes itself |
| `src/frontend/open.rs` | platform | new. Opening a file, the README of a directory or standard input far enough to tell what it is (`Opened`), and `document`, which chooses the parser for its input format |
| `src/frontend/given.rs` | platform | new. A file ready to show (`Given`, `Shown`, `Drawn`), and loading the images of the documents among the files given |
| `src/frontend/pager.rs` | platform | new. The interactive frontend: the `Screen` trait, the pager's screen, which the platform opens; `page_files` gathers the files into one stream, loads their images first or in the background and runs the pager's loop on the terminal's screen, and prints them after all where it does not open |
| `src/frontend/print.rs` | platform | new. The print frontend: `print_files`, each file under its header as it comes, and `Held`, whose `check` makes sure kitty holds the images that were sent (the pager's print fallback uses it too) |
| `src/frontend/loads.rs` | platform | moved from `src/load.rs`. Loading a document's images and diagrams: before the pager through the queue of `src/load/queue.rs` and one wait loop (`load_parts`, `wait_or_skip`, `finish_loading`), which the progress line of `src/term/progress.rs` follows, `Pause`, what a wait waits with (for the loads, and for Space and Enter on a terminal), which the platform gives, and `Guard`, the one value that catches Ctrl-C and the other signals before the pager's screen does; on threads and in the background; and reloading its file for the pager: the loads, and `FileReload`, which reads the file again and brings the images up to date for it but starts no load. The frontends call them. Its seed is the work runner that roadmap item G7.3.3 fills in |
| `src/frontend/sim.rs` | platform | moved from `src/pager/sim.rs`; test support. The simulated frontend: a terminal on a virtual clock, files in memory and loads, run by a `Runner` of its own, that come when a script says (the pager's, and the route's: the images that load before the pager opens, through the platform's `loads`, `pause` and `interrupt`, which the sim answers with a virtual clock, its keys and no signal handler), wanted by a planner the script gives (what a reload brings, and the pictures `M` shows), which run the loop in `src/pager/mod.rs` through its `Terminal` and `Reload` traits, or the whole route through `SimPlatform`, its `Platform`, and give back the bytes written to the terminal and to standard error. It asks `Pager::wait` how long the pager waits for keys and moves the clock by that, so a test runs without a pseudo-terminal and without waiting. What stays on the pty is what only a real terminal or process shows: raw mode and the alternate screen, signals (Ctrl-C while the images load, `q` stopping a program that draws) and the programs themselves; the sim's pictures stand in for diagrams, whose renderers are not there without the default features. A screen model that reads the bytes, which knows kitty, comes later |
| `src/frontend/tests/mod.rs`, `route.rs`, `paging.rs`, `settings.rs`, `stdin.rs`, `follow.rs`, `reload.rs`, `loads.rs`, `wait.rs`, `close.rs` | follows `frontend/` | new. The route's tests: `show_files` run in the simulated frontend, for one file, several, standard input, a missing file, `r` and paging without keys; `paging.rs` has the paging and printing decisions, `settings.rs` the settings that change the route (paging, diagrams, reading mode); `stdin.rs` standard input; `follow.rs` following a file; `reload.rs` `r`; `loads.rs` the images that load in the background or before the pager opens, scripted as pictures, which are the twins of the pty tests that run a program for a diagram; `wait.rs` the wait for them, with its progress line and hint and the Space or Enter that opens the pager before they come; and `close`, which ends the pager: it leaves the screen first and then waits for the programs |
| `src/load/mod.rs` | platform | new. The platform's loading for the engines: `load`, `load_with` and `load_parts`, which load the images a document wants, read their files and have the renderers draw its diagrams. A module of the library, beside the frontends' `src/frontend/loads.rs` |
| `src/load/url.rs` | platform | new. `fetch_url`, which reads the file a URL names, with how the file looked (`Fetched`), or the bytes of a `data:` URL, with `read_file`, which opens the file; and `load_url`, which decodes what `fetch_url` read |
| `src/load/tests.rs` | follows `load/mod.rs` | new. The tests of loading, and the helpers (`fixtures`, `load_source`, `open`, `Stub`) that the other tests of images load with |
| `src/load/renderers.rs` | platform | new. `Renderers`, which draws each language with its external command when it has one and with the built-in renderer otherwise, and `render_in_parallel`, which draws several diagrams at once on threads |
| `src/load/queue.rs` | platform | moved from `src/images/background.rs`. The loads in the background: `Loads`, which the pager polls and which starts and stops the loads through one call, `reconcile`, given the images wanted, whose order in the list is the order they load in, and `want`, which the pager calls with a version's jobs and which of them `session/loading.rs` says are kept or missing (`Wanted`: kept, missing, or to load again, with their `Job`, which holds what is fetched first and what is computed from it), and gives what came as `Arrived`, with the look of the file it was read from (`wait_by` waits until one comes or a deadline passes, so that a wait without keys wakes when a load does rather than after a fixed sleep), the stop flag and number of each load, so that a result of a load that was stopped or replaced is dropped, and the `Runner` that runs the loads started: `Threads`, with the jobs waiting, the threads that load them and the places of the programs outside kiri that draw, or a test's, as the simulated frontend's script says. Tero decided on 2026-10-31 that the queue and the jobs share this one file |
| `src/load/theme.rs` | platform | new. `load`, which reads a theme file and has `typeset::theme::parse` turn its text into a theme; `config::load_theme` calls it |
| `src/load/watch.rs` | platform | new. What the pager asks of the file behind its document: `Reload`, which reads it again, with the images brought up to date for it, and tells whether it changed, and `Reloaded`, what a reread came to (the document, the document with warnings, or why it failed). They were in `src/pager/mod.rs`; the frontends and the simulated frontend implement `Reload` |
| `src/reports.rs` | platform | new. The reports kiri writes itself: `--doctor` and `--list-emojis`, rendered for a terminal or written as text, and, through `frontend::show_document`, shows one through the same route as a file. Their content is made by `src/doctor.rs` and `src/list.rs` |
| `src/cli.rs` | platform | new. The command line: the clap `Cli`, whose flags take their help from the settings registry, and `options()`, which turns the flags into settings |
| `src/config/mod.rs` | platform | stays (was `src/config.rs`); the module's description and its public surface |
| `src/config/environment.rs` | platform | stays. `Environment`, the table of the variables kiri reads, reading the process's variables, and the directories that `HOME` and the XDG variables give. Capability detection takes its variables (`TMUX`, `TMUX_PANE`, `NO_COLOR`, `COLORTERM`, `TERM_PROGRAM` and `VTE_VERSION`) as arguments, not `Environment`, in the terminal layer split |
| `src/config/file.rs` | platform | stays. The configuration file: where it is, reading it (`load`) and the deserialiser of the `[diagrams]` tables |
| `src/config/registry.rs` | platform | new. The registry of settings: each setting once, with its key, the module that reads it, what a change of it needs, its medium and its scope. It generates `Config` (what the file sets), `Options`, `Choices`, `Settings` and `choose` |
| `src/config/resolve.rs` | platform | stays. Resolving a setting from the flags, the environment, the file and its default: `resolve`, `variable`, the theme's source, `settings` and loading the theme |
| `src/config/types.rs` | platform | stays. The setting types: `Setting`, `Origin`, `Named`, `ThemeSource`, and the types of the settings that are not the pager's, `Passthrough` among them: capability detection, the command line and `--doctor` read it, and capability detection takes its value, not `Environment` |
| `src/typeset/diagnostic.rs` | typesetting | new. `Diagnostic`, one type for what kiri tells without stopping: its code, the document it belongs to, the image or diagram it is about, its line and its text. Whoever finds the trouble makes one; standard error and the pager's warning line turn it into text |
| `src/session/step.rs` | state engine | new. The pager's step: `step(&mut Pager, Msg) -> Vec<Effect>`. `Msg` is what comes in (keys read, a poll's answer, a reload's result) and `Effect` what goes out (draw, poll, reload, quit, lay out again). It does no I/O and reads no clock; the loop in `src/pager/mod.rs` carries the effects out and sends back what they found. Its tests drive it without a terminal |
| `src/session/step/tests.rs` | state engine | new; the step driven without a terminal |
| `src/session/state.rs` | state engine | new. What the reader does in the pager besides scrolling: `State` with its parts, `Change` and `Look`, and its answers to keys and to time |
| `src/session/frame.rs` | state engine | new. What the screen shows of one layout: `Layout`, `Status` and the `Frame` they make |
| `src/session/layers.rs` | state engine | new. The highlights over the document as one list of layers, each a paint (a background mixed toward the text colour, colours, reverse video, a box, a frame) at an address in its layout's own terms: `Layer`, `Paint`, `At`, `stack`, which makes the list from the tint, the search and the selected link, and `paint`, which paints the text layout's lines and reading mode's pages with it. A highlight is a layer here, not a branch in the pager |
| `src/session/settings.rs` | state engine | new. The setting types the pager reads (`Paging`, `SearchCase`, `Follow`, `FollowJump` and `InitialLoad`), without the code that reads them from the flags, the environment and the file |
| `src/session/sources.rs` | state engine | new. The session: `Session`, which holds a `Source` for each file given (a lone file and standard input are a session of one) with its `SourceId`, its name and its `Version`, which a reload brings forward, and the `View` that is built, with its `ViewId` and the `SourceId`s it shows. Ids come from the session and are not used twice. The documents and their layout are not in it yet; the pager reads the names and the count of its files from it. The file is not `session.rs` because clippy's `module_inception` forbids a module named like its parent |
| `src/config/tests.rs` | follows `config/` | stays |
| `src/doctor.rs` | platform | stays (it searches `PATH` through `diagrams::find`) |
| `src/list.rs` | platform | stays (the content of `--list-emojis`, which `src/reports.rs` shows) |
| `src/panics.rs` | platform | stays |
| `src/testing.rs` | platform | stays (test support) |
| `src/caps.rs` | typesetting | moved to `src/typeset/caps.rs` |
| `src/color.rs` | typesetting | moved to `src/typeset/color.rs` |
| `src/size.rs` | typesetting | moved to `src/typeset/size.rs` |
| `src/model.rs` | typesetting | moved to `src/typeset/model.rs` |
| `src/input.rs` | typesetting | moved to `src/typeset/input.rs` |
| `src/compare.rs` | typesetting | moved to `src/typeset/compare.rs` |
| `src/parse/mod.rs` | typesetting | moved to `src/typeset/parse/mod.rs` |
| `src/parse/html.rs` | typesetting | moved to `src/typeset/parse/html.rs` |
| `src/parse/csv.rs` | typesetting | moved to `src/typeset/parse/csv.rs` |
| `src/parse/csv/tests.rs` | typesetting | moved to `src/typeset/parse/csv/tests.rs` |
| `src/parse/front_matter.rs` | typesetting | moved to `src/typeset/parse/front_matter.rs` |
| `src/parse/front_matter/tests.rs` | typesetting | moved to `src/typeset/parse/front_matter/tests.rs` |
| `src/parse/tests.rs` | typesetting | moved to `src/typeset/parse/tests.rs` |
| `src/theme/mod.rs` | typesetting | moved to `src/typeset/theme/mod.rs` |
| `src/theme/paper.rs` | typesetting | moved to `src/typeset/theme/paper.rs` |
| `src/theme/file.rs` | typesetting | moved to `src/typeset/theme/file.rs`: parsing the theme file's text. Its `load`, which read the file, moved to `src/load/theme.rs`, and `config::load_theme` calls it |
| `src/kitty/mod.rs` | terminal layer | moved to `src/present/kitty/mod.rs` |
| `src/kitty/graphics.rs` | terminal layer | moved to `src/present/kitty/graphics.rs` |
| `src/kitty/query.rs` | terminal layer | moved to `src/present/kitty/query.rs` |
| `src/kitty/text.rs` | terminal layer | moved to `src/present/kitty/text.rs` |
| `src/render/mod.rs` | mixed | stays. Typesetting: `Inputs`, the render options (`FrontMatterStyle`, `Headings`, `TableRows`, `RenderErrors`), `heading_look`, `heading_images` (whether headings take images, given the capabilities), and the front matter helpers. Terminal layer: `render`, `render_files`, `render_file`, `render_image` and `render_image_on`, which lay out and write in one call. The terminal layer split |
| `src/render/block.rs` | typesetting | moved to `src/typeset/block.rs` |
| `src/render/style.rs` | typesetting | moved to `src/typeset/style.rs` |
| `src/render/image.rs` | typesetting | moved to `src/typeset/images/size.rs` |
| `src/render/wrap.rs` | typesetting | moved to `src/typeset/wrap.rs` |
| `src/render/layout/mod.rs` | typesetting | moved to `src/typeset/layout/mod.rs` |
| `src/render/layout/code.rs` | typesetting | moved to `src/typeset/layout/code.rs` |
| `src/render/layout/front_matter.rs` | typesetting | moved to `src/typeset/layout/front_matter.rs` |
| `src/render/layout/list.rs` | typesetting | moved to `src/typeset/layout/list.rs` |
| `src/render/layout/quote.rs` | typesetting | moved to `src/typeset/layout/quote.rs` |
| `src/render/table.rs` | typesetting | moved to `src/typeset/layout/table.rs` |
| `src/render/table/tests.rs` | typesetting | moved to `src/typeset/layout/table/tests.rs` |
| `src/render/script.rs` | typesetting | moved to `src/typeset/layout/script.rs` |
| `src/render/highlight.rs` | typesetting | moved to `src/typeset/layout/highlight.rs` |
| `src/render/reading.rs` | typesetting | moved to `src/typeset/reading/engine.rs` |
| `src/render/output.rs` | terminal layer | moved to `src/present/output.rs` |
| `src/render/encoder.rs` | terminal layer | moved to `src/present/encoder.rs` |
| `src/render/loading.rs` | mixed | stays. Typesetting: `Ink` (the struct), `dimmed` and `band`, which draw pixels. Terminal layer: `Ink::new`, the one function that takes the output's `Page`, and `band_offset`. The terminal layer split |
| `src/reading/mod.rs` | typesetting | moved to `src/typeset/reading/mod.rs` |
| `src/reading/colr.rs` | typesetting | moved to `src/typeset/reading/colr.rs` |
| `src/reading/octicons.rs` | typesetting | moved to `src/typeset/reading/octicons.rs` |
| `src/reading/page.rs` | typesetting | moved to `src/typeset/reading/page.rs` |
| `src/reading/paint.rs` | typesetting | moved to `src/typeset/reading/paint.rs` |
| `src/reading/text.rs` | typesetting | moved to `src/typeset/reading/text.rs` |
| `src/reading/fonts.rs` | mixed | moved to `src/typeset/reading/fonts.rs`. Typesetting: `Fonts`, the fonts handed to parley, with the fallbacks. Platform: loading the system fonts and memory-mapping the font files, lazily, on the first layout; the font database moves that loading out |
| `src/images/mod.rs` | platform | stays. Its typesetting pieces moved to `src/typeset/images/`: `ImageKey` to `key.rs`, `ImageError` and `size` to `error.rs`, `Images` and `Placeholder` to `store.rs`, `LoadOptions` and `Part` to `options.rs`, and the plan of which images a document wants (`plan`, `Wanted`, `collect_blocks`, `diagram_languages`) to `plan.rs`, each with its tests. Its platform pieces, `load`, `load_with` and `load_parts`, moved to `src/load/mod.rs`. What is left declares the pipeline and the stamps, and re-exports those items, and `Loads` from `src/load/queue.rs`, as `kiri::images::…` |
| `src/images/decode.rs` | typesetting | moved to `src/typeset/images/decode.rs` |
| `src/images/drawn.rs` | typesetting | moved to `src/typeset/images/drawn.rs` |
| `src/images/drawn/tests.rs` | typesetting | moved to `src/typeset/images/drawn/tests.rs` |
| `src/images/heading.rs` | typesetting | moved to `src/typeset/images/heading.rs` |
| `src/images/svg.rs` | mixed | stays. Typesetting: `Svg`, parsing and drawing, and `Fonts`, the database handed in. Platform: `Fonts::system`, which scans the system's fonts. The font database |
| `src/images/url.rs` | typesetting | moved to `src/typeset/images/url.rs`: `data:` URLs and the reading of a URL's parts, which do no I/O. `read_file`, `decode_file` and `load_url`, which open files, moved to `src/load/url.rs`, where `decode_file` has since been split between `fetch_url` and `load_url` |
| `src/images/background.rs` | platform | moved to `src/load/queue.rs` |
| `src/images/pipeline.rs` | platform | stays. Builds the renderers from the settings and the jobs of the images a document wants (`jobs`, which starts none, with the headings and the pictures in text among them), and brings the images up to date for a reload (`reload_in_background`, which sets aside what it cannot keep and stops the load of an image whose file changed, and starts nothing). It no longer loads anything in the foreground, nor catches signals: `load_documents`, `Loaded` and `Interrupted` are gone, and `src/frontend/loads.rs` gives the jobs to `Loads::want` in `src/load/queue.rs`, as the pager does; only the wait before the pager opens (`load_in_background`) reconciles on its own. `planned_keys` tells the keys of the images that would load, which the frontend stands boxes in for to count the document's rows before they do |
| `src/images/pipeline/tests.rs` | platform | stays |
| `src/images/stamps.rs` | platform | stays |
| `src/images/warnings.rs` | typesetting | moved to `src/typeset/images/warnings.rs`. Makes the `Diagnostic`s for the images and diagrams that did not load, from `ImageKey` and `ImageError` |
| `src/diagrams/mod.rs` | platform | stays. What it held for the typesetting engine moved to `src/typeset/diagrams/`: `Theme`, `FormulaColors`, `INLINE_FORMULA`, `Drawn` and `Request` to `request.rs`; the `Renderer` trait, `BuiltIn`, `NoRenderer` and `baseline_depth` to `renderer.rs`; `theme_name`, `display_name`, `extension`, `chosen_theme`, `dark_background`, `noun`, `name` and `centred` to `names.rs`. `Renderers` and `render_in_parallel` moved to `src/load/renderers.rs`. It declares `external` and `find`, and re-exports those items as `kiri::diagrams::…` |
| `src/typeset/diagrams/error.rs` | typesetting | new. `DiagramError`, why a diagram could not be drawn, such as `no output file` or a renderer's own message; `src/diagrams/mod.rs` re-exports it, as it does `Language` |
| `src/diagrams/language.rs` | typesetting | moved to `src/typeset/diagrams/language.rs` |
| `src/diagrams/math.rs` | typesetting | moved to `src/typeset/diagrams/math.rs` |
| `src/diagrams/mermaid/mod.rs` | typesetting | moved to `src/typeset/diagrams/mermaid/mod.rs` |
| `src/diagrams/mermaid/merman.rs` | typesetting | moved to `src/typeset/diagrams/mermaid/merman.rs` |
| `src/diagrams/external.rs` | platform | stays. `Output`, what a command writes, which a language's configuration file depends on, moved to `src/typeset/diagrams/output.rs` |
| `src/diagrams/external/tests.rs` | platform | stays |
| `src/diagrams/find.rs` | platform | stays |
| `src/pager/mod.rs` | platform | stays. The loop and the carrying out of effects: `run`, which takes the `Loads` of the reloads' images from its caller, and the `Driver` that lays the document out again and again, `show` and `carry`, which wait for a message, step the pager and do what its effects say, and `Tools::close`, which does what ends a layout, with the `Reload` (in `src/load/watch.rs`), `Planner`, `Loads` and `Terminal` it does its I/O through. `Origin::plan` asks the frontend's planner for the jobs of the images the shown content wants, again only when the document is read again, reading mode turns on or off, or the cells change, and `Origin::want` gives them to `Loads::want` after each layout, in the order `loading::wanted` and `loading::nearest` say, and at each poll to tell the order again. The decisions, what a key, a poll, a resize, a load or a reload's result asks for, are in `src/session/step.rs` |
| `src/pager/tests/simulated.rs` | follows `pager/mod.rs` | stays; goes with its pieces in the pager's loop. The pager's tests that run in the simulated frontend instead of a pseudo-terminal |
| `src/pager/tests/mod.rs` | follows `pager/mod.rs` | stays; goes with its pieces in the pager's loop |
| `src/pager/tests/files.rs` | follows `pager/mod.rs` | stays; goes with its pieces in the pager's loop |
| `src/pager/tests/following.rs` | follows `pager/mod.rs` | stays; goes with its pieces in the pager's loop |
| `src/pager/tests/images.rs` | follows `pager/mod.rs` | stays; goes with its pieces in the pager's loop |
| `src/pager/tests/links.rs` | follows `pager/mod.rs` | stays; goes with its pieces in the pager's loop |
| `src/pager/tests/loading.rs` | follows `pager/mod.rs` | stays; goes with its pieces in the pager's loop |
| `src/pager/tests/paging.rs` | state engine | moved to `src/session/plan.rs`, its tests with it |
| `src/pager/tests/reading.rs` | follows `pager/mod.rs` | stays; goes with its pieces in the pager's loop |
| `src/pager/tests/reload.rs` | follows `pager/mod.rs` | stays; goes with its pieces in the pager's loop |
| `src/pager/tests/search.rs` | follows `pager/mod.rs` | stays; goes with its pieces in the pager's loop |
| `src/pager/tests/view.rs` | follows `pager/mod.rs` | stays; goes with its pieces in the pager's loop |
| `src/pager/tests/commands.rs` | follows `pager/mod.rs` | new; the pager's answers to its commands |
| `src/pager/tests/layers.rs` | follows `pager/mod.rs` | new; how the highlights combine |
| `src/pager/keys.rs` | terminal layer | moved to `src/present/keys.rs`: `Key` and the `Decoder` that turns bytes into keys. The lookup of a key's action is in `src/session/commands.rs` |
| `src/session/commands.rs` | state engine | new. `Action`, `COMMANDS` (topic, keys, help text, action and the settings that change it), `action` and `hint`, the key the status line shows for an action; it reads `Key` from `src/present/keys.rs` |
| `src/pager/bands.rs` | state engine | moved to `src/session/bands.rs` |
| `src/pager/changes.rs` | state engine | moved to `src/session/changes.rs` |
| `src/pager/fade.rs` | state engine | moved to `src/session/fade.rs` |
| `src/pager/files.rs` | state engine | moved to `src/session/files.rs` |
| `src/pager/help.rs` | state engine | moved to `src/session/help.rs` |
| `src/pager/links.rs` | state engine | moved to `src/session/links.rs` |
| `src/pager/loading.rs` | state engine | moved to `src/session/loading.rs`, which chooses what stands in for the images of a reloaded document and, for the loop, which of the images a version wants load: `wanted` (kept or missing, nearest the view first), `starting` (those with nothing to stand in for them yet) and `images_wanted` (the `--no-images` rule) |
| `src/pager/prompt.rs` | state engine | moved to `src/session/prompt.rs` |
| `src/pager/search.rs` | state engine | moved to `src/session/search.rs`; what lays the matches over the lines is in `src/session/layers.rs` |
| `src/pager/status.rs` | state engine | moved to `src/session/status.rs` |
| `src/pager/status/tests.rs` | state engine | moved to `src/session/status/tests.rs` |
| `src/pager/tint.rs` | state engine | moved to `src/session/tint.rs` |
| `src/pager/view.rs` | state engine | moved to `src/session/view.rs` |
| `src/pager/warnings.rs` | state engine | moved to `src/session/warnings.rs` |
| `src/term/mod.rs` | platform | stays |
| `src/term/answers.rs` | terminal layer | moved to `src/present/answers.rs` |
| `src/term/cell_query.rs` | terminal layer | moved to `src/present/cell_query.rs`. It uses `tmux::passthrough`, which stays in `term/tmux.rs` until the terminal layer split |
| `src/term/clipboard.rs` | terminal layer | moved to `src/present/clipboard.rs` (it builds the OSC 52 bytes and writes them to an `impl Write`). It uses `tmux::passthrough`, which stays in `term/tmux.rs` until the terminal layer split |
| `src/term/check.rs` | mixed | stays. Platform: asking kitty behind tmux, on the terminal. Terminal layer: reading what it answered, `lost` and `sequences`. The terminal layer split |
| `src/term/detect.rs` | mixed | stays. Platform: `capabilities` and `examine`, which read the window, the environment and the probe's answer. Terminal layer: turning those into `Capabilities` (`with_probe`, `with_window`, `through_tmux`). The terminal layer split |
| `src/term/probe.rs` | mixed | stays. Platform: sending the probe and reading the answer on the controlling terminal. Terminal layer: the text of the queries. The terminal layer split |
| `src/term/tmux.rs` | mixed | stays. Platform: running `tmux` and reading its answer, and `run_id`, which reads the process id, the clock and the OS's random seed. Terminal layer: `Wrapping` and `passthrough`, which wrap kitty's sequences for tmux, and `Pane`, `prepare` and `ClientCells`, which are plain data or take the run (`prepare`, `ClientCells::window`) as a value. The terminal layer split passes the run id in as a value |
| `src/term/window.rs` | mixed | stays. Platform: `window_size`, the ioctl. Terminal layer: `WindowSize`. The terminal layer split |
| `src/term/screen.rs` | platform | stays |
| `src/term/screen/tests.rs` | platform | stays |
| `src/term/progress.rs` | platform | stays. The progress line `kiri: loading images 2/5…` for the one wait loop of `src/frontend/loads.rs`, `until_over`, with or without the hint that Space opens the pager, and the line of `while_waiting`. The line no longer has a thread of its own: `while_working` is gone |
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

The move created the first five of these files; they are not in the table above,
because they had no path before it. Each declares the modules that moved into
its directory and exports the public surface of its part. The files after
them, the nine from `src/typeset/images/key.rs` to
`src/typeset/diagrams/output.rs`, came with the split of the mixed modules
(images and diagrams); the ones after those were new before it, and the last
three came later and are test only.

| New file | What it is |
|---|---|
| `src/typeset/mod.rs` | new: the root of the typesetting engine |
| `src/present/mod.rs` | new: the root of the terminal layer |
| `src/session/mod.rs` | new: the root of the state engine |
| `src/typeset/images/mod.rs` | new: declares `decode`, `drawn`, `error`, `heading`, `key`, `options`, `plan`, `size`, `store`, `url` and `warnings`, and exports the types of images: `ImageKey`, `ImageError`, `Images`, `Placeholder`, `LoadOptions` and `Part` |
| `src/typeset/diagrams/mod.rs` | new: declares `error`, `language`, `math`, `mermaid`, `names`, `output`, `renderer` and `request` |
| `src/typeset/images/key.rs` | new: `ImageKey`, what an image is made from, the key into `Images` |
| `src/typeset/images/error.rs` | new: `ImageError`, why an image could not be loaded, and `size`, a size limit in words |
| `src/typeset/images/store.rs` | new: `Images`, every image of a document by its key, loaded or not, and `Placeholder`, what stands in for one that is loading |
| `src/typeset/images/options.rs` | new: `LoadOptions`, how a document's images are loaded and its diagrams drawn, and `Part`, one of the documents shown together |
| `src/typeset/images/plan.rs` | new: the plan of which images a document wants: `plan`, `Wanted`, `collect_blocks`, `inline_images`, `shown_urls`, `keys` and `diagram_languages` |
| `src/typeset/diagrams/request.rs` | new: `Theme`, `FormulaColors`, `INLINE_FORMULA`, `Drawn` and `Request`, what a renderer is spoken to in |
| `src/typeset/diagrams/renderer.rs` | new: the `Renderer` trait with `render_one_by_one`, `BuiltIn`, `NoRenderer` and `baseline_depth` |
| `src/typeset/diagrams/names.rs` | new: what kiri says of a language by its name: `theme_name`, `display_name`, `extension`, `chosen_theme`, `dark_background`, `noun`, `name` and `centred` |
| `src/typeset/diagrams/output.rs` | new: `Output`, what an external command writes |
| `src/typeset/reading/page/table.rs` | new: the tables of reading mode's flow, their columns, cells and nested grids |
| `src/typeset/reading/page/code.rs` | new: code blocks, diagrams and the bands that a long one is cut into |
| `src/typeset/reading/page/picture.rs` | new: images, diagrams and formulas placed in the flow |
| `src/typeset/reading/page/quote.rs` | new: block quotes and GitHub alerts |
| `src/typeset/reading/page/list.rs` | new: lists and their task boxes |
| `src/typeset/reading/page/front_matter.rs` | new: front matter, as a grid of its keys and values |
| `src/typeset/reading/page/details.rs` | new: `<details>` blocks and their summary |
| `src/typeset/reading/page/heading.rs` | new: headings |
| `src/typeset/reading/page/html.rs` | new: raw HTML, shown as its source |
| `src/typeset/theme/look.rs` | new: the one look, resolved from the theme and the capabilities, which both layouts read: colours, heading styles, and a spacing table for each medium, in cells for the text layout and in pixels (GitHub's values) for reading mode |
| `src/typeset/reading/look.rs` | new: reading mode's brushes, the one look resolved into the colours its page is drawn with (`page_look`). The path is for the maintainer to confirm |
| `src/typeset/parity.rs` | new, test only: lays out blocks in both layouts and compares what each decided, against the list of known differences |
| `src/typeset/layout/summary.rs` | new, test only: the cell layout's view of a block, for the parity test |
| `src/typeset/reading/page/summary.rs` | new, test only: the pixel layout's view of a block, for the parity test |

### Exceptions to the direction

An exception is code that uses a part below its own in the list of parts,
or that does the work of another part. `tests/it/guard_direction.rs` holds
the complete list of the crossings between files, each with the work that
removes it, and `make check` fails on a crossing that is not on the list
or on a listed one that is gone. A file's part is its directory's, and a
reference into a mixed platform module (`images`, `diagrams`, `render`,
`pager`, `config`, `term`) from a part above the platform is a crossing
too. The rules cover non-test code only: test modules may use any part.

The crossings in the list fall into these kinds:

| Kind | Removed by |
|---|---|
| typesetting uses the platform parts of `render` | the terminal layer split |
| typesetting uses the font loading in `images/svg.rs` | the font database |
| typesetting uses `present::kitty::graphics` | a small change of its own, giving `caps.rs` the constant |
| the terminal layer uses `term::tmux` or `render::loading` | the terminal layer split |

The test reads files by directory, so it does not see the crossings inside
a mixed module, which are the pieces of a file such as
`render/loading.rs` or `term/detect.rs` that reach another part. They are in
the table below, with the work that removes each. The test does not hold
them, so this table is the record of them.

| Crossing inside a mixed module, not seen by the test | Removed by |
|---|---|
| `Ink::new` in `render/loading.rs` (typesetting's `Ink`) takes the terminal layer's `output::Page` | the terminal layer split, which moves `Ink::new` to the terminal layer |
| `detect` in `term/detect.rs` (terminal layer) calls `tmux::run_id` (platform) | the terminal layer split, which passes the run id in |
| `term/detect.rs` and `term/tmux.rs` (terminal layer) take `config::Environment` (platform) and `config::Passthrough` (a setting type) | the terminal layer split, which passes the variables and the `Passthrough` value as arguments |
| `typeset/reading/fonts.rs` (typesetting) loads and memory-maps system fonts | the font database |

### Where a new thing goes

New code goes where this map puts it. When the map has no place for it,
that is a question for the maintainer, not a reason to add it to a hub
file (`main.rs`, `frontend/mod.rs`, `pager/mod.rs`, `render/mod.rs`,
`typeset/layout/mod.rs`, `typeset/reading/engine.rs`,
`typeset/reading/page.rs`, `images/mod.rs` and `diagrams/mod.rs`).
**No path yet** means that the work named creates the place; until then, ask.

| A new… | Goes in |
|---|---|
| command-line option | `src/cli.rs`, and the setting it sets in the registry in `src/config/` |
| startup decision, such as whether a run pages | `src/session/plan.rs`, made by the startup plan; take what it needs from the terminal as values |
| key or action | an entry in `COMMANDS` in `src/session/commands.rs`, and the pager's answer to its action |
| setting | its entry in `src/config/registry.rs`, which names its key, its owner, its effect and its medium, and the code of its owner |
| use of a file, the network or a program | no path yet, loading and sessions makes the platform's work runner, and the trust policy a `Policy`; until then `src/load/`, `src/frontend/loads.rs`, `src/images/pipeline.rs` or `src/diagrams/external.rs` |
| block type or input format | the typesetting engine's registry of block types and formats, no path yet (today `src/typeset/model.rs`, `src/typeset/parse/` and `src/typeset/layout/`); in reading mode a module per block type in `src/typeset/reading/page/`, made by reading mode's block modules; the blocks without one yet, such as paragraphs, stay in `page.rs` |
| diagram language | `src/typeset/diagrams/`: a file and one entry in `language.rs`; a program that draws it is run by the platform |
| image format | the decoder in `src/typeset/images/decode.rs` |
| theme or look setting | `src/typeset/theme/` |
| warning | a `Diagnostic` in `src/typeset/diagnostic.rs`, with a code of its own: where it came from, its kind and its text; one about an image or diagram is made in `src/typeset/images/warnings.rs` |
| highlight, or anything else that changes from frame to frame | a layer in the list that `stack` makes in `src/session/layers.rs`: a paint and an address, and nothing new in `marked` |
| panel, such as help or a file list | a document in the panel's look, no path yet, made by panels as documents, built in `src/session/` |
| feature of the pager | a file of its own in `src/session/`, never a new branch in `src/pager/mod.rs` |
| way to show the files given, such as a server | a file of its own in `src/frontend/`, as `pager.rs` and `print.rs` are, reaching the files, streams and terminal through `Platform`; never in `frontend/mod.rs` |
| terminal feature | the I/O in `src/term/`, the bytes in the terminal layer (`src/present/`), and kitty's own sequences in `src/present/kitty/` |

The paths marked new are for the maintainer to confirm.

### What `main.rs` is

`src/main.rs` is the binary's entry point: some 200 lines, with the
command line in `src/cli.rs`. It implements `Platform` for the process
(`Real`), reads the command line, resolves the
settings and hands them to the frontends in `src/frontend/`, which open
files and standard input, have one route in which a single file is the
special case of several, print documents, start the pager, and load the
images and diagrams and reload files for the pager; and it calls
`src/reports.rs` for `--doctor` and `--list-emojis`. The decisions made at
start need no I/O, and they are in `src/session/plan.rs`. It is meant to
stay as little as this.

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
