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
  in it itself, in `src/typeset/parse/html/`: the tags of each HTML block
  become tokens (`html.rs`), and the tokens and the Markdown blocks between
  them make one tree of block elements (`tree.rs`), so an element is read
  whole whatever blank lines cut it. `blocks.rs` turns that tree into blocks
  of the model, and `inline.rs` joins the tags of running text into inlines.
  Both decide by the table of elements (`elements.rs`, one row for each
  element) and by the setting `html`: GitHub's level, the default, or `all`,
  which also reads `<u>`, `<center>` and task boxes. An `<img>` in HTML is the
  same image as one in Markdown. A tag with no effect at the level, one kiri
  does not know, or a broken one is drawn dim, or left out with
  `html_tags = "hide"`, and what it holds is read as it would be without it.
  Front matter at the start of a Markdown file is split off in
  `src/typeset/parse/front_matter.rs` before comrak sees the text and read
  as YAML, TOML or JSON into the model's own value tree;
  `--front-matter off` leaves it to comrak. A formula in `$$…$$` becomes a
  code block in `math`, as a ` ```math ` block is, so that a backend that
  cannot draw it shows its source.
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
window. Only a Markdown extension makes a file Markdown; any other text is
shown as it is, as `--format text` shows it (below). A CSV or TSV file, told
by its extension or by `--format csv` or `--format tsv`, goes through a parser
of its own, `src/typeset/parse/csv.rs`, into a document of one table, so the
backends draw it as any other table. That table keeps the file's text and
where each row starts in it, and makes the cells of rows only when they are
asked for; the document's text reads a cell from the same text by its
number, so a search in a large file makes no cells. The table holds every
row. The pager's cell layout lays a table of more rows than a stretch out a
stretch of rows at a time (`src/typeset/layout/rows.rs`), with the widths of
its columns chosen at its first stretch from what they ask for over all its
rows: a CSV's as its parse found them in its one pass (`ColumnTexts`), a
Markdown table's when it has at most 20 000 cells, and otherwise its first
stretch's until `:w`. It only measures the stretches far
from the view; print mode and reading mode lay a table out whole, and show a
large one capped, its first 100 000 cells and a note of the rows left out
(`Table::capped`).
A Mermaid file becomes a document of one Mermaid code block, whose
diagram is drawn as in any document and then shown on its own as an
image file is, or the document is shown when it is not drawn.
Text that is not Markdown, which is any other text file and what
`--format text` names, skips the parser too: the file becomes a document of
one code block, in the language its name or extension names when the
highlighter has a grammar for it (`Makefile`, `.gitignore`, `config.yaml`),
so it is highlighted, wrapped and paged as any code block is, and without
highlighting otherwise (`notes.txt`, `LICENSE`). The block is marked as a
whole file, so that it is drawn bare, without the background band, padding
and language label that set a code block off in a Markdown document.
Plain output, without styling, gets the file as it is instead, from
`src/frontend/print.rs`. A file whose name tells no language is looked at
by `sniff` in `src/typeset/input.rs`, which knows the formats whose first
characters Markdown cannot reasonably have (JSON, JSON Lines, a unified
diff, a shebang, XML and HTML), for highlighting only.

The document can also come from standard input, which errors call
`<stdin>`, or from a directory, which stands for its README. Both are
resolved in `src/frontend/open.rs`, before `src/typeset/input.rs` looks at
the bytes, so an image piped in is shown on its own as well. Neither has a
name to tell what it is, so they stay Markdown unless `--format` says
otherwise: a README without an extension always, and standard input unless
`sniff` finds one of the formats above in it. A pipe given by its path,
as `kiri <(git diff)` gives one, is not a regular file and is read as
standard input is.

Several files on the command line are shown one after another, each under
a header that names it. Each is read as it would be on its own, and a file
that cannot be read or shown is told about while the others are still
shown. In the document model they are `File`s, each a name and a document;
an image file, or a diagram file that is drawn, is a document of one
`Block::ImageFile`, whose pixels kiri reads itself and keys by the file's
name. The layout gives each file its header, and marks each line with the
block it comes from; the header has a block of its own. The documents'
images load together, each document's relative to its own directory, under
keys that carry the document's number among the files. With paging
`auto`, the files go to the pager when their rows together are longer than
the window, whatever they are, as one document does; images and diagrams
are no exception. The pager then shows them as one stream, headers and all,
so a search finds a term in any of them. Whether it opens is decided from
the image files' sizes, which are read from their headers without decoding
a pixel (an SVG is parsed instead) and laid out as the pixels would be.
Reading a size still reads the whole file, up to the size limit, before its
header is parsed, though one file at a time (`read_size_as` in
`src/typeset/images/decode.rs`). A file whose body is broken is counted at
its header's size, and the pager shows it as an error once it is read. An
image file is read when the pager opens, and the pager then holds every
decoded image: 24 photos of 4845 by 2980 pixels take about 1.4 GB. When the
files are printed, including when no screen can open, an image file is read
only as it is shown, so that no more than one is held, about 150 MB for
the same photos.
An image file or a diagram file given alone is one file among one: the
pager shows it as a document on its own, without a header, and has nothing
to reload. A document given alone goes the same way, with a few differences
from several files: the pager reads it again when it changes, if it is a
regular file, and the warnings about it are told without its name.

There is one terminal backend, not one per terminal. It lays the model out
as styled lines of a fixed width, then writes those as bytes. What it may
use comes from the capabilities it is given: plain text for pipes, colours
and attributes for any modern terminal, and kitty's protocols on top where
they work.

Each styled line also says which block of the document it comes from. The
layout keeps a table of the blocks, and a line holds only a small number
that points into it. The table says which file a block is in and where in
its document it is, as a path: its number among the top-level blocks, then
one number into a `<details>`, quote or alert (the number of the block
inside it), or two numbers into a list (the item, then the block in the
item). So the second block inside the first `<details>` has the place 0, 1,
and the first block of the second item of a list that is the third block
of the document has 2, 1, 0. A list item is not a block and has no place
of its own. The model says this once: `Block::contents` names the blocks
that hold other blocks, `Block::children` gives each block inside one with
its steps, and `Document::blocks` lists every block with its place. New code
that walks the blocks of a document uses them, not a match of its own. Two
places still have one: the image plan (`src/typeset/images/plan.rs`) and the
layout's marks. The summary of a `<details>` block belongs to that block. A
file's header has a block of its own with no place in the document. The
blank lines between blocks, and between two files, belong to none. A number means
something only for the layout it came from, so a caller that keeps a place
across a new layout, as the pager does when the window is resized, compares
the blocks themselves.

A block's path, its hash and where it was written are three different
things. The path is worked out by walking the document and is not stored.
The hash comes from `Hash`, derived for the model, and is not stored either:
the id of a reading page is a hash of its blocks. A CSV's table writes its
own: the bytes of its rows' source and the separator, not its cells, so the
same cells written with other bytes, such as `\r\n` line breaks, are
another table. Where a block was written
is the only thing that is stored. A document keeps it in a `SourceMap`,
looked up by the block's path: the line the block starts on, the bytes of
the source it was read from, and whether those bytes are exactly the block
or the larger piece of source it was read from, such as one HTML block that
holds several blocks. A note that kiri adds, an error and the header of a
file have none. The layout's table of blocks does not copy it; a caller that
needs it asks the document, by file and path. The map takes no part when
two documents are compared for equality or hashed, because it says where a
block was written and not what the document is, so a block that only moves
down the file stays the same block. `src/typeset/compare.rs` compares an
old document with a new one by the hash of each block (`Document::hashes`,
made once for each version): it finds the blocks that stayed as they were,
then pairs the blocks of the same kind that stand in the gaps between them
as changed, and calls the rest removed or added. A block with the hash it had
has stayed with everything inside it, and the comparison does not look
inside it. When a block that changed holds other blocks, such as a quote or
a list, the blocks inside are compared the same way. The result is told by
place, so that a caller can ask what became of a block of the new document
or which blocks are gone; a block inside one that stayed is told from it.
The layout's table has to use the same places for the blocks inside quotes,
alerts and lists too once it marks them.

A place in the shown documents is a `Pos` (`src/typeset/position.rs`): the
file, the block's path and where in the block, such as byte 12 of its second
text. It is never a line number or the address of a page, which belong to one
layout. A stretch of a document is a `Range<Pos>`, and the order of places is
the order of the document, with the end of a quote or a list after everything
inside it. What a document says as text is asked of the model, not of a
layout: `src/typeset/text.rs` gives the texts of each block and their
numbers (a layout asks it which text it is laying out, and counts none
itself), a term found in them and the text between two places, and
`src/typeset/targets.rs` the links and embeds as ranges, with where each link
goes as its address says (`Goes`), and the named places a link can go to: each
heading, with its id made as GitHub makes it (`src/typeset/anchors.rs`), and
each `<h1>` to `<h6>` inside a block of HTML (by its `id`, else as a
Markdown heading), and each `<a id>` and `<a name>` of HTML. The contents, the
headings as a flat list of level, text and id (and, when several files are
shown, each file above its headings), are made from those named places in
`src/typeset/contents.rs`, which also says which entry a line of a layout is
in. The contents bar at the window's left (`src/session/sidebar.rs`) draws
that list as a second area of lines, one row for each heading. With the
focus in the bar (`t`), the arrow keys, `j` `k` `h` `l`, `g` `G` and Enter move
the selection, fold and open entries and jump to the heading, as a link to it
does, so Backspace comes back. A click in the document and a search give the focus
back to the document, as a view opened over the window does.
`Document::hashes()` gives the hash of every block and list item by path,
for finding out what changed between two versions. The search looks in
this text, and `Laid` (`src/typeset/laid.rs`, from `src/typeset/locate.rs`) says
where a range of it is shown, or why it is not.

The pager shows the same styled lines on the terminal's alternate screen.
`src/session/view.rs` chooses the lines that fit in the window, a scaled
line whole, or one size smaller to fit the rows left at the bottom, and
moves them, and when the window changes size
it keeps the same block at the top; `src/present/keys.rs` turns the bytes
the terminal sends into keys; `src/session/commands.rs` holds the
commands, each with its topic, keys, help text and action, and the keys
are looked up there, and `src/session/panels/help.rs` lays out the help
from them, by topic, in rows of keys and what they do, which
`src/session/frame.rs` shows in a frame over the screen;
`src/session/deck.rs` keeps the help, the search prompt, the colon and
any document opened over the first as cards on a deck over the document,
and `src/session/prompt.rs` keeps the search term as it is typed in the
prompt's card, and
`src/session/search.rs` finds it in the document's text and asks the layout
which of the matches are shown and on which lines (a match in a part that is
not laid out yet is kept without a line, and gets it when the layout reaches
it), and
`src/session/layers.rs` lists what is highlighted as ranges of the document
(the matches, the selected link, the tint of a change), each with a meaning
and a `z`, which the engines paint: the text layout splits a line where a
match starts and ends (`src/typeset/layout/layers.rs`), and reading mode lays
an overlay over a page (`src/typeset/reading/layers.rs`), so the matches can be
highlighted without a change to the writer;
`src/session/fade.rs` mixes the colours a match fades through after a jump,
those of the status line's `reloaded` as it fades out, and those of the
warning line;
`src/session/files.rs` finds where each of several files starts from the
places the lines show, which it asks the layout's map, so the status line
names the file at the top of the view and `:n` and `:p` move from one
file's start to another's, and tells where each file's header is, which the
layout recorded with a one-row form of it (the sticky row the frame puts over
the top row once the name is scrolled past, when the `sticky_header` setting
is on; `src/session/view.rs` says how
many top rows that covers, so that jumps land below it); the names
and the count are those of the sources in the session
(`src/session/sources.rs`), which `src/frontend/pager.rs` makes from the
files given and the pager's loop owns, and a reload brings the source's
version forward; `src/session/status.rs` makes the status line as styled
spans (a line of its own, a badge and the keys of the bar, while the contents
bar has the focus), and
`src/session/warnings.rs` the line of text above the status line that tells
the first warning; the session makes each frame a scene
(`src/session/frame.rs`) and the terminal layer's presenter
(`src/present/presenter.rs`) writes it; `src/pager/mod.rs` lays the document
out again when the window changes size or when `r` reloads it. For that the
pager is given `SourceFiles`, as it is given a `Terminal` for keys: the loop
builds the job of reading the source again from the session
(`Session::reread`: the file's path and how to read it, `Reread`), and
`SourceFiles` runs it, which gives the document and the images again. The
pager keeps its place as a place of the document: it compares the old
document with the new one, and `Align` (`src/typeset/align.rs`) takes the
place at the top of the view to the new version, so a block added above the
view does not move it. A block that changed keeps the same text and offset as
far as the new block has them, and when the block at the top is gone, the view
goes to the next block that is still there. The current match of a search
moves the same way, and so do the scroll and the selection of the bands of
reading mode, each told by the block it shows. A new layout of the same
document, after a resize, `M` or an image that came, puts the view where its
place was too: in text mode the top line is the one that shows the first
character of the line that was on top; in reading mode it is the same
paragraph and as many lines into it. A view at the end of the document goes to the new end, so a document
that grew shows what was added. Following on auto then moves the view to a
change: `src/session/changes.rs` finds the runs of blocks that changed or were
added, as ranges of the new version, and the place of a removal with neither
beside it, as an empty range. A change is of the innermost block: a paragraph
that changed in a list is the change, not the whole list. It chooses one as the
follow jump setting says, and the lines of that change get a lighter
background for a moment (`Paint::Tint`, painted for both layouts in
`src/typeset/layers.rs`), fading out through
`src/session/fade.rs`. The tint is of the range, so a resize, `M` or `W` does
not take it off, and a change in a part of the layout that is not laid out yet
keeps its tint until it has lines. While the pager follows the document, the
session says what is watched (`Session::watches`: the file, every 100 ms, a
poll's time, since how it looked when it was read), and the loop's `Watcher`
looks at it through `SourceFiles` once a poll's time is over since the last
look, not on every turn: a key that comes before the poll's time is over, or
a step of a fade, does not look at the file again. It reads the document
again once a change still looks the same a poll's time after it was first
seen, so that a file caught half written is not shown. The terminal itself,
the alternate screen, raw mode and reading keys from the controlling
terminal, is opened in `src/term/screen.rs`.

A reload shows the new text at once. `SourceFiles` looks at the file, which is
before it reads it, so that a change made meanwhile counts, then reads and
parses it, keeps the images the document still shows and sets the others
aside; the session then takes the look and the next version of the source,
or, when it could not be read or shown, only the look, so that following does
not read it again until it changes again; `r` still does. A lone file shown as
text, or a CSV or TSV, that only grew at its end, the same as before byte for
byte (`compare::grown`), is not laid out from its start again on an automatic
reload: the layout keeps its stretches and checkpoints, highlighting included,
and goes on from the start of its last stretch (`Laid::grow`, with the map
cut back there and going on), and a CSV's parse goes on after its last row
when that row reads as it did (`csv::parse_grown`). The result is that of a
fresh layout of the grown file; `r`, a change elsewhere in the file, or other
widths of a long table's columns lay it out afresh. The pager loads what
the new document wants and does not hold in the background, in its
`Loads`, which it polls beside the follow poll. What a document wants is
asked of the frontend's planner (`Planner`, in `src/pager/mod.rs`), which is
given the content of a view, the number the keys of its first document's
images carry, and the file each document was read from (`Planned`): the
jobs of its images, diagrams and headings, in the order of the document,
with the pictures in the middle of text, which both layouts draw. The
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
while a band shows, so that it moves. Whether a band is drawn is decided
once, by `Capabilities::places_bands` (kitty's graphics, not through tmux), in
the function `loading` in `src/typeset/layout/mod.rs`, which the text layout
and reading mode both call: on a terminal that places none, the lines hold no
band, and so the writer, the presenter and the test model `Seen` have nothing
to draw or to ask. The status line says `[rendering: 1/3]` in place of the name
meanwhile. The images in view load first, then the nearest: the line that
carries a band knows its image and the lines that show it (a `Loading`, which
holds the band's drawing apart: the drawing is there only where the terminal
places bands, but the image and the lines are always there), and
`src/session/loading.rs` orders the images by how far those lines are from the
view, a line below before a line above, and the images no line shows, such as a
heading's or a picture in the text, after them in the order of the document.
The loop gives `Loads` the wanted images
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
as it runs, and the encoder keeps a ledger, `src/present/ledger.rs`, of
what the terminal holds of the images it sent. An image the terminal
holds is not sent again: after a resize that leaves it at the same
size, or a reload that leaves its pixels as they were. An image that the
lines of a new layout no longer place, or that changed, is deleted from the terminal, and when the
pager quits, every image it sent is.

The ledger also counts the bytes each image the terminal holds takes, as
sent: four a pixel, or three for a still with no transparent pixel, which
kiri sends as RGB. The pager gives the encoder a budget for them,
`image_memory`, 128 MB unless set. Past it, before each frame, the encoder
deletes the images that are not near the view, the ones asked for longest
ago first; only when the near images alone are over the budget does it
delete those too, the oldest first. Near is one screen above the view and
two below (`KEPT_BEHIND` and `KEPT_AHEAD` in `src/pager/animate.rs`). An
image in view, or one the last frame asked for, is never deleted, and one
that was deleted is sent again under the same id when a row asks for it.
The budget is soft: a frame's own sends come after its deletes, so the
terminal can hold more than the budget until the next frame, and a screen
that is over the budget alone is kept. kitty and Ghostty keep about 320 MB
of images for a screen and drop the oldest past that without telling,
which is why the budget is there. Print mode has no budget, and sixel
images are not counted.

The presenter (`src/present/presenter.rs`) keeps what the screen shows, so
a pager frame writes only the rows that changed since the last one, such as
the status line after a key or the picture of one animation. A line taller
than one row is written whole, and the rows it covered before are erased
along with the rows it covers now. A frame is drawn whole, with the screen
cleared first, when the presenter cannot tell what the screen shows: the
first frame, a new window size, a new page background or cell size, a
redraw the reader asked for with Ctrl-L, a frame with a loading band placed
in it and the one after it, and the next frame after the terminal has lost
its images, as tmux makes it do when it shows the pane again.

A scene may hold several lists of lines side by side, each in an `Area`, a
rectangle of the window. Each is written from its area's column, a row is
dirty in its own area alone, and a row of an area that is not the whole
width of the window is erased with ECH over its own cells and ended with ECH
over the cells it has left, never with `2K` or, short of the window's right
edge, `ESC [ K`, which would take what is beside it: a heading's lower row
or a sixel. A sixel that reaches its area's last column is treated as
reaching the window's. A bar belongs to its row of the window, not to an
area: it is written after the lines of that row whenever a row of any area
there is written, and a bar on a row no area has is written in every frame
(and is not erased when it goes). A scene with more than one area, or with
one that is not the whole window, is not written as a scroll. One area over
the whole window is written as before. Which areas the window has is told by
one map in `src/session/areas.rs`, a function of the window's size and the
bar's width, which the session's state decides, that drawing and the mouse
both ask: the document over the whole window, or, when the contents bar
shows, the bar's columns (30, or a third of a narrower window, or what the
reader dragged it to) at the left and the document in the rest. The reader
can drag the bar's width, between 16 columns and half the window, with 40
columns left for the document. The document is laid out as wide as its area
(the window less the bar, and no wider than `--width`), so opening or closing
the bar is a new layout, as a window's resize is, and the place is kept with an
`Anchor`. A click, the wheel and the sideways wheel go to the area under the
pointer: over the contents bar the wheel scrolls its list, and anywhere else
it is the document's, which gets the column in its own area; the help's frame
is answered first, by `panel_pointer`.

On the terminals that were measured to scroll right (Ghostty, foot and
xterm, found by name in the table of facts `src/typeset/facts.rs` and told by
`Capabilities::scroll`), a frame that is the last one shifted, as one scroll
of the pager makes it, is written as a scroll of the whole screen (`CSI n S`
up, `CSI n T` down, no scroll region) and then the rows that are new, the
status line and whatever else differs. The page's background is set for the
scroll, so that the rows that come in are blank in it and not in the
terminal's own. It does not scroll while a shade
shows or when the frame is drawn whole, nor on xterm's sixel: xterm keeps only
16 sixel graphics when it draws its window again, and kiri is not told when it
does. foot keeps every sixel image in its grid, so it scrolls with sixel too.
kitty is left out while it shows a row of a page's picture in the status row
after a scroll (see `ScrollsRight` in `src/typeset/facts.rs`). Any other
terminal is written every row that moved, as before.

The row writer (`Writer::row` in `src/present/output.rs`) gives a row's two
kinds of output apart. The sends are what the terminal keeps from frame to
frame: the data of the images the row shows if the terminal does not hold
them yet, and the delete of an image they replace. The cells are what is
drawn on the screen and goes with the next clear: text, placeholders, sixels
and a loading band, which stays whole among the cells as it is sent anew
after each clear. Each caller decides where the sends go. Printing writes a
row's sends right before its cells. The presenter writes a frame in this
order: the delete of images no longer used, the data of every image the
rows show, the clear (or, for a frame that changes some rows, the erases),
and then the cells and the bars, all inside one synchronized update. A
terminal that does not hold a synchronized update, such as kitty behind
tmux 3.6, which may draw what has come so far, then never shows a row erased
and waiting for its image. There is no switch for it, and nothing in the
presenter knows of tmux. The rows' bytes it keeps to tell what changed are
the cells, so a row that sent its image compares the same as before.

On a terminal that draws sixel the presenter works on the same rows, but an
image row is a row of its own: its bytes are blank cells and the sixel of the
row's part of the image, one cell high, drawn from a cursor saved before the
cells and moved right of them, never written after, so that no character
removes its pixels (`src/present/sixel/row.rs`). The row's bytes are the
data, so a row that is the same is left alone and one that differs is
erased and written again, each row before the next, since no row has scaled
text. The encoder keeps the sixels it encoded by image, row, columns and the
colour under the image, in a cache of 64 MB, and the pictures they are cut
from in one of 128 MB (`src/present/encoder/sixel.rs`); the ledger's table
of placements to ids tells whether an image's pixels are the ones scaled.
Nothing is sent, so nothing is deleted or sent again when the terminal may
have lost something. xterm keeps only 16 of the sixels it drew, and when it
draws its window anew, as after it was minimised, only those come back; the
pager draws the whole screen again when the focus comes back, as it does for
Ctrl-L and a new window size. The hyperlinks of a page are on the blank cells
under its sixels. A loading band is not drawn on sixel (`places_bands` is false
there): each frame of a band would be a new sixel that takes one of those 16
places, and the status line tells the progress.

Printed output has the same rows, written one after another. A sixel drawn on
the last row of the screen scrolls it, and the saved cursor of its row then
lies on the row below, so a row with a sixel is written after a newline and a
step up (`\n ESC M`), which leave the cursor on its row and the sixel clear
of the bottom. What is printed is not drawn again: xterm keeps only 16 of its
sixels when it redraws its window, so a long printed document loses the images
above the last 16 slices then.

## Reading mode

Reading mode, experimental and on by default where the terminal draws
images (`--no-reading` turns it off), shows the text as a browser does: in a
proportional font, drawn as images. `src/typeset/reading/` knows nothing about
terminals. It lays blocks of text (headings, paragraphs, lists,
quotes, thematic breaks, code blocks, highlighted as the text layout
highlights them, and tables, whose columns share the room as a browser
shares it) out with parley, in the fonts of the fontdb
database that SVG text uses, and draws them with tiny-skia, colour emoji
included. GitHub's sizes and spacing are its measure: 16 pixels of text in
rows of 20, a column of 42 times that, margins that collapse between
blocks. The driver (`src/typeset/driver.rs`) walks the pieces of a document
(a block, a `<details>` summary, a file's header) and gives each to the pen of
the engine: the cell layout's, or reading mode's (`src/typeset/reading/pen.rs`),
which holds the pieces it lays out as one stream until a piece it does not
lay out (an image file, an error, hidden front matter) or a file's header
cuts it. So a heading, the paragraphs after it and a `<details>` with its
blocks are one page, and an error between two documents is cells with a page
on each side. A page also ends after the block that passes 3000 rows, and
the next one starts with the rest: where it ends depends on the blocks
alone, so an edit in the last block of a long document renames only the
last page's images. The next page starts with the margin the last one
ended with and the stack adds no blank row between them, so the gap is the
one inside a page, plus the rounding of the page above to whole rows, under
a row. `src/typeset/reading/engine.rs` turns what the pen holds into a
page: its rows are
image rows, as a large image's are, of parts 30 rows tall that the encoder
draws when a line first shows them, so the pager draws only the parts in
view. A part is only as wide as what it draws, plus a font size, so a short
paragraph is a few cells wide and not the whole page (`page/reach.rs`). The
cells right of a narrower part are the page's background where the theme
has one, and the terminal's own where it has none. Under a tint, the gap up
to the page's edge stays plain and only the margin beyond it is tinted. A
band keeps the page's width, and so does a part that a table as wide as the
page reaches. Where the page is drawn opaque, a part narrows only if the
writer paints the page's colour exactly, or a seam would show. The rows keep
their blocks, so the pager keeps its place across a resize.
Images, diagrams and formulas are in the page at their natural
size in CSS pixels, shrunk to the column, a formula in the middle of it;
the page gets them through `Pictures`, which `src/typeset/reading/engine.rs`
writes over the loaded images, as it gets code's colours through
`Highlight`, so `src/typeset/reading/` knows nothing of the loader. A picture
that has not come reserves room, with a loading band over the last row of
it as in the text layout, and one a reload replaces shows the old
pixels dimmed; the page's id covers the pixels of its pictures, so a
picture that came is a new page. An image in the middle of text, which
the loader wants whichever layout is shown, is a box in its line, which
parley places on the baseline and moves to the next line whole. (The
text layout draws it as a span that is the cells of an image,
`src/typeset/layout/embed.rs`.) So is a formula in the middle of text,
`Inline::Formula`, drawn by the formula renderer as a diagram in text
style at the text's size, which also tells how far below its baseline it
reaches, so that the box sits on the line's baseline; until it has come
it is shown as its source, as the other layouts show it. An alert is a
tint, a bar, a title after the kind's icon, which is drawn from an
embedded Octicon outline (`src/typeset/reading/octicons.rs`), and the
blocks inside it in the room left.
Front matter is a table of its keys and values in the column, with a table in a
cell for a value that has keys, or a code block when the front matter shows as
code. A `<details>` block is its summary after a triangle and its blocks, each
with a place of its own for the pager; raw HTML that is not read is its source,
and a note of kiri's own is muted text. What is left as text between the pages,
a file's header, a file's error message and an image file shown on its own,
starts at the area's left edge and is as wide as the text's column: only code
blocks and tables reach into the area. A code block or table wider than the
area wraps to fit it, or, with `--wide scroll` in the pager, or after `W`
there, becomes a band on whole rows of its own, which keeps its lines and words
whole and scrolls sideways under a thin scroll bar, faded on each edge with
more beyond it; a code band's language stays in its top right corner. Its rows
show an image of their own, cut into slices of at most 30 rows as pages are,
keyed by its slice, its offset and whether it is selected. The pager keeps
those by the band's block, which the layout's map gives, in
`src/session/bands.rs`: the topmost band on the screen is selected until a
click, or a search that finds something in another, selects it, ← and → or `H`
and `L` scroll it, and so does a sideways wheel, or the wheel with Shift, over
a band, and the pager then shows the same lines with the band's rows keyed
anew, without laying the document out again. Where the terminal draws no
images, kiri warns and shows the text as usual. In the pager, `M` turns reading
mode on and off and lays the document out again, keeping the block at the top
as a resize does.

A page is drawn opaque when the theme gives it a background whose colour
is known. Then all of its text is blended by kiri over the same ground,
shaded rows or not. Otherwise the page stays transparent, so that the
terminal's background, also a translucent one or an image, shows through
it as it does around it, and the terminal blends the text. `B` in the
pager gives the theme the background the terminal reported, one shade off
unless `background_offset` is false, as kitty shows its own background
through a cell of exactly that colour; or it takes the theme's away. Either
lasts as long as the pager runs.

Links in a page work from the positions the layout knows: it records, in
pixels and a piece for each line, where each link is, in the page's or a
band's own coordinates. A search term is found in the document's text, and
the page says on which rows each match is shown, and where across a band, so
a match is on the row of its line of text, and a match in a band scrolls the
band to it and selects it. The marks are drawn into the page's image, a box in
the theme's match colours with the text drawn over it, and so is a box
round the selected link: what is drawn over a part is an overlay, named by
a hash of the marks and the link in that part alone, in the key of the
rows it is on, and kept by the encoder under its page and that hash, so
that the terminal keeps the part with it and the part without and moving
the current match draws only the parts it leaves and enters. A link is an
OSC 8 hyperlink round the placeholders it covers (`src/present/kitty/graphics.rs`),
which kitty opens as it does a link in text; in a band the cells move with
the scrolling.
`src/session/links.rs` makes the stops of Tab and Shift+Tab: the links of the
document, listed as ranges of its text (`src/typeset/targets.rs`), and the
animated pictures that are in no link, in the order of the document, and the
status line says where a link goes. Two rules are kept there, as they are how a
terminal shows links: a link without an address is no stop, and links of one
address that touch are one stop. A stop is shown where the layout says it is
(`Map::drawn`), which is the same for a link that wraps, so there is no `Rc` to
tell it by, and a stop no line shows, such as a link in a heading drawn as an
image, is not there to select. A link in a part the layout has not laid out
yet is a stop without a line, in its place in the order, until it is laid out
(an animation there is not, as it takes its key from its lines). The selected
stop is a range kept with its
version, so it stays the same after a reload, a resize or `M`; it is drawn in
reverse video in text and with a frame in a page by the layout, as a highlight.
Tab also reaches a stop in a band scrolled out of view, and scrolls the band to
it as a search match does (`Reveal`, `src/session/reveal.rs`). Enter on a link
that goes to a place in a file shown, a heading or a marked place in the same
file or a file given too, jumps there with the place on the top line
(`src/session/follow.rs` works out where from the link and the paths of the
files, and asks no file system), and Backspace goes back to the place the jump
left. Enter on a link to a Markdown file that is not shown opens it over the
view, a new view with its own state, at the anchor of the link; Backspace goes
back to the view below, to the place the link was followed from, and a link
to a file that cannot be read tells why on the status line and opens nothing.
While more than one view is open the status line tells their names as a path
(bare file names, the base's by the file the link was followed in), and a click
on a name below the last goes back to that view, to the place Backspace
pressed again and again would come to; the status line gives the cells of each
name, and the `…` of a cut path stands for the topmost view it hides.
Those places are in the history (`src/session/history.rs`), each the
view's own `Anchor` and the view it is of, moved over a reload by `Align`. Enter
on any other link copies its URL with OSC 52 (`src/present/clipboard.rs`),
wrapped in tmux's passthrough where the images are.
`Reading` also holds a cache of the pictures scaled for the last layout,
by image, its pixels and size, so that a layout scales only what is new
and lets go of what it did not show. It holds, too, a cache of the glyphs
drawn (`src/typeset/reading/glyph_cache.rs`): the coverage of each
monochrome glyph by font, glyph, size, slant, variation and place rounded
to a sixteenth of a pixel, which every part drawn lays on its canvas in the
colour of the text; it is bounded by bytes and starts over when full.

## Diagrams

A diagram stays a code block in the document model, `Block::CodeBlock` with
its language, so the parser knows nothing about renderers and a diagram that
cannot be drawn is shown as the code it already is. Whether it is drawn is
decided when images are loaded. `src/typeset/images/key.rs` keys each image by
an `ImageKey`, a URL or a block (its language, in lower case, its source, and
a fingerprint of how it is drawn: the page it is for, the formula colours,
the panel and the fonts of its text, so that a diagram drawn for a dark page is not the one a light page
asks for), with the number of the document it is in, so that documents shown
together keep their own images, and `src/typeset/images/plan.rs` collects the
code blocks whose language a renderer draws besides the lone images.
Each diagram is a job of its own (`src/load/jobs.rs`) in the queue of `src/load/runner.rs`, drawn by
its renderer, then `src/typeset/images/drawn.rs` decodes each image it gets
back by its content, as it decodes an image file, so sizing, the width limit and redrawing
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
level and of the fonts its text is drawn in, and `src/typeset/images/heading.rs` draws the text through the SVG
renderer in the system's sans-serif font, emphasis in italics, and inline code
and keys in its monospace font on the background of inline code, at the size
and in the colours that `typeset::inputs::heading_look` takes from the one look
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
in the background, `src/load/loads.rs`, on threads of their own (`src/load/runner.rs`),
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
raw until it has been restored and the programs it stopped have
closed, or until the screen fails to open, when
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
The capabilities also name who plays an animated image: kitty does itself
(`Playback::Terminal`), for Ghostty kiri redraws the frames in the pager
(`Playback::Redraw`), and behind tmux or without graphics no one does, so
the first frame shows. This comes from the terminal's name and version,
as kitty 0.47.1 does not answer a query for it and Ghostty 1.1.3
answers every query alike.
Without them, 24-bit colour can still come from `COLORTERM`, or from a
DECRQSS answer that keeps a colour set in 24 bits, and hyperlinks
from `TERM_PROGRAM` or `VTE_VERSION`, but inside tmux from tmux, below;
underline styles come only from the probe. `NO_COLOR` turns colours off in every
mode. These variables come in as part of `config::Environment`, read once
at start-up with kiri's own, so the rules are tested without the process
environment. The result is a plain value, `Capabilities` in
`src/typeset/caps.rs`, passed to the renderer, so tests can render for any
terminal without having one. kiri asks nothing when it runs in the
background of its terminal, where raw mode would get it stopped.

The device attributes answer, `CSI c`, says whether the terminal draws sixel:
attribute 4 in it. The probe reads the first answer, the one from the
terminal kiri writes to. Inside tmux the first two count, tmux's own answer
and the wrapped one of the terminal outside, and sixel is named only when both
list 4, since tmux answers for itself whatever terminal shows its panes. Where
the wrapped query is skipped, no second answer comes and sixel is not named.
The protocol is then chosen in this order: kitty where the terminal has
placeholders, sixel where it lists 4, and kitty without placeholders where it
answers only the graphics query. Sixel is drawn wherever it is chosen, on any
terminal whose attributes list 4, and with `--mode sixel` where they do not.
Behind tmux it is named but not drawn, as what tmux does with a sixel has not
been measured, so its images stay alt text (`Capabilities::is_sixel`).
`--doctor` lists the protocols found.

The limits of sixel, XTSMGRAPHICS, are asked in a second round, only when the
first answer names sixel in this way or `--mode sixel` forces sixel: kitty and
Ghostty do not take the queries, and kitty logs a parse error for each, so
with `--mode sixel` kitty logs them too. The second round ends on another
device attributes query and waits for it as long as the first.

Whether an image block, a paragraph of only an image, shows its picture or
its alt text is chosen from these capabilities by `resolve` in
`src/typeset/rendition.rs`. The block offers the ways it can be shown, its
renditions, in the order it prefers them: its picture, then its alt text.
`resolve` takes the first that the terminal shows and that is loaded, or has
something to stand in for it while it loads, such as the old picture dimmed;
the text layout lays the block out as that one
(`src/typeset/layout/image_block.rs`), and `Layout::placed` sizes the picture
from the same images. A rendition is something the layout lays out in its own
way, not an image protocol: which protocol carries a picture's bytes is the
terminal layer's choice. Images and formulas in a line, diagrams and reading
mode still choose as before.

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
screen) wraps each graphics sequence on its way out, and consecutive ones that
come in one write, as a frame of the pager's does, share a passthrough, so
the renderers write the same bytes inside tmux as outside it.

tmux drops output for a client that cannot keep up, and the images in it
with it. Through tmux the encoder therefore notes the images it sends in its
ledger, and after printing, `src/term/check.rs` asks the terminal about them
with a virtual placement each, which kitty and Ghostty answer with
`ENOENT` for an image they do not hold; the encoder sends those again, a
few times at most, as its ledger counts. The pager writes the same
checks after each frame that sent images, and the terminal's answers
come among the keys.

Terminals also drop images on their own when they hold too many, with or
without tmux, so the pager asks again when the reader stops: 300 ms after
the last key that moved the view, when a layout is shown anew, or when the
focus comes back. Then it checks the images in view that the ledger holds as
sent, and sends again those the terminal answers it lacks. Nothing is asked
on sixel. kitty 0.48.2 showed such an image only after more output,
so the pager then writes a cursor move in a write of its own, a turn later.

tmux passes kitty's graphics on as they come, but draws the cells of a
synchronized update only once the update has ended. An image that replaces
another, such as each step of the turning mark while an animation's frames
are made, would leave its cells blank for that while if the old image were
deleted in the same frame, as it is outside tmux. Through tmux, for every
terminal behind it, Ghostty included, the old image is therefore deleted when
the terminal answers the check of the new one, or, should no answer come, with
the next frame after a new layout, a redraw or the pane coming back, and at
the latest when the pager quits.

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
| Sixel | DCS `P1;P2;P3 q` … ST; ESC 7, ESC 8; ESC M | `src/present/sixel/` | Images on terminals that draw sixel and not kitty's placeholders: one slice of cell height for each image row, so the presenter's row-by-row rewriting works on them; the encoder is pure, and the row writer in `src/present/output.rs` puts each slice over blank cells between a saved cursor and its restoring; printed rows with a sixel are written after `\n ESC M` |
| Hyperlinks | OSC 8 | `src/present/output.rs`, `src/present/kitty/graphics.rs` | Clickable links, also over the cells of an image |
| Clipboard | OSC 52 | `src/present/clipboard.rs` | Copying a link's URL |
| Text attributes and colours | SGR | `src/present/output.rs`, `src/present/kitty/graphics.rs` | Bold, italic, colours; the id of the image a placeholder shows, as its foreground colour |
| Styled underlines | SGR 4:x, SGR 58 | `src/present/output.rs` | Heading underline styles; the dashed underline between table rows in the border colour |
| Cursor movement, erase | CSI C, CSI K, CSI H, CSI 2J, CSI X | `src/present/output.rs` | Backgrounds beside scaled headings and across the width; the pager's rows; erasing what a panel or the warning line covers below the top row of a large heading; a pager frame drawn whole |
| Line erase | CR, CSI 2K; CSI B, ESC 7, ESC 8 | `src/term/progress.rs`, `src/present/kitty/query.rs`, `src/present/output.rs` | Wiping the progress line; erasing the rows the text sizing tests drew; the rows a pager frame rewrites |
| Alternate screen, cursor | CSI ?1049h/l, CSI ?25l/h | `src/term/screen.rs` | The pager's screen of its own |
| Focus reporting | CSI ?1004h/l; CSI I, CSI O | `src/term/screen.rs`, `src/present/keys.rs` | Telling the pager that its tmux pane is shown again, and, with sixel, that the terminal may have drawn its window anew and lost some of what kiri drew |
| Mouse reporting | CSI ?1000h/l, CSI ?1002h/l (motion while a button is held, which drags the help's bar and the contents bar's border), CSI ?1006h/l; CSI < … M/m, CSI M | `src/term/screen.rs`, `src/present/keys.rs` | The wheel scrolling the pager, also inside tmux; a click following the link under it; in reading mode, a click selecting a band and the sideways wheel scrolling it |
| Synchronized output | CSI ?2026h/l | `src/present/output.rs` | Pager frames without flicker |
| Cell size query | CSI 16 t; the answer CSI 6 ; height ; width t | `src/present/kitty/query.rs`, `src/present/cell_query.rs`, `src/present/keys.rs` | The cell size in pixels where the window size has none, and inside tmux, where the window's may be tmux's guess: asked at start, and by the pager when the window changes size, so that it follows a new font size |
| Generic queries | CSI 6n, OSC 10, OSC 11, OSC 4, CSI > q, CSI ? 2;4;0 S, CSI ? 1;1;0 S, DECRQSS after SGR 38;2, CSI c | `src/term/probe.rs` | Cursor position, foreground, background and palette colours, terminal version, the widest sixel and the number of its colour registers, whether a colour set in 24 bits is kept, end of the answers; sixel support, read from the attributes in the answer to CSI c |
| tmux passthrough | DCS `tmux;` … ST, with each ESC doubled | `src/present/tmux.rs` | kitty's graphics, the probe's graphics query, and the probe's and the pager's cell size queries, through tmux to the terminal outside |
| Animation | APC `_G` `a=f` (a frame and its gap), `a=a,r=1,z=` (the first frame's gap) and `a=a,c=1,s=3,v=` (play from the first frame), `s=1`, `c=` (stop, show a frame) | `src/present/kitty/graphics.rs` | Animated GIF, WebP and APNG, which kitty plays itself after it has every frame and the reader has started it; Ghostty ignores them, so kiri sends each frame as an image of its own |
| Shade behind the help | APC `_G` `a=t` (one black, partly transparent square), `a=p` with columns, rows and a negative `z`, and `a=d,d=I`; only where `places_bands` is true | `src/present/kitty/graphics.rs` | The pictures and reading-mode pages round the help's frame darken; text in the text layout is dimmed with SGR 2 as before |
| Image checks | APC `_G` `a=p,U=1` and `a=d,d=i` with a placement id | `src/present/kitty/graphics.rs` | Whether kitty behind tmux holds the images kiri sent |

Sequences that only kitty's protocols define live under `src/present/kitty/`: text
sizing, graphics, Unicode placeholders and the queries that detect them.
Sixel's live under `src/present/sixel/` and tmux's passthrough in
`src/present/tmux.rs`; `tests/it/guard_sequences.rs` keeps each protocol's
in its place. `src/term/probe.rs` sends kitty's queries along with the
generic queries it spells out itself. OSC 8 hyperlinks, SGR attributes, underline styles and cursor
movement are general terminal features that `src/present/output.rs` writes
itself; for scaled text it calls `src/present/kitty/text.rs`, and for images
`src/present/encoder.rs`, which sends and deletes them (with the ledger in
`src/present/ledger.rs` of what the terminal holds), and
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
mixed module (`images`, `diagrams`, `pager`, `config`, `term`) has code in
two parts, so its pieces follow this rule one by one. The guard test allows
no exceptions to the rule today. The crossings that remain inside a mixed
module, which the test cannot see, are listed under "Exceptions to the
direction", below.

### Every module

[docs/module-map.md](module-map.md) lists every file of `src/` with one sentence on what it
is, under its part and its directory. It is made from the `//!` documents of
the files (`make module-map`), so it cannot differ from them, and the whole
description of a module is the document at the top of its file. A file that
is only test code has no row; it is named as "Tests:" on the row of the file
that declares it.

### Exceptions to the direction

The guard test allows no exceptions. `tests/it/guard_direction.rs` fails on
any crossing between files: non-test code uses only its own part and the
parts above it in the list of parts, and `make check` fails on every
crossing. A file's part is its directory's, and a reference into a mixed
platform module (`images`, `diagrams`, `pager`, `config`, `term`) from a
part above the platform is a crossing too. The rules cover non-test code
only: test modules may use any part.
`tests/it/guard_lines.rs` keeps the state engine from finding places in the
lines of a layout. It holds the list of the files of the state engine that
still read the spans of a line, each with its reason: `animation.rs` and
`frame.rs` read them to draw, and no file asks a span for a place of the
document. A file that reads them and is not on the list fails `make check`,
and so does a listed one that no longer reads them. No file of the state
engine reads the block of a line or takes reading mode's bands or pages
from the lines; it asks the map.

The test reads files by directory, so it does not see the crossings inside
a mixed module, which are the pieces of a file such as `term/detect.rs`
that reach another part. They are in the table below, with the work that
removes each. The test does not hold them, so this table is the record of
them.

| Crossing inside a mixed module, not seen by the test | Removed by |
|---|---|
| `detect` in `term/detect.rs` (terminal layer) calls `tmux::run_id` (platform) | the terminal layer split, which passes the run id in |
| `term/detect.rs` (terminal layer) takes `config::Environment` (platform) and `config::Passthrough` (a setting type) | the terminal layer split, which passes the variables and the `Passthrough` value as arguments |

### Where a new thing goes

New code goes where `docs/module-map.md` and this table put it. When they
have no place for it, it goes in the module closest to what it does, with a
ledger entry (AGENTS.md, "Ledger"). A hub file, one of those that `CAPS`
in `tests/it/guard_line_caps.rs` lists, takes it only when nothing else
fits, and its cap rises in the same commit.
**No path yet** means that the work named creates the place; until then,
the same holds.

| A new… | Goes in |
|---|---|
| command-line option | `src/cli.rs`, and the setting it sets in the registry in `src/config/` |
| startup decision, such as whether a run pages | `src/session/plan.rs`, made by the startup plan; take what it needs from the terminal as values |
| key or action | an entry in `COMMANDS` in `src/session/commands.rs`, and the pager's answer to its action |
| modal, or anything else over the document that takes keys before it | a `Card` in the deck in `src/session/deck.rs`, with its answers to keys, the pointer and Escape; never a field of `State` |
| setting | its entry in `src/config/registry.rs`, which names its key, its owner, its effect and its medium, and the code of its owner |
| use of a file, the network or a program | no path yet, loading and sessions makes the platform's work runner, and the trust policy a `Policy`; until then `src/load/`, `src/frontend/loads.rs`, `src/images/pipeline.rs` or `src/diagrams/external.rs` |
| block type or input format | the typesetting engine's registry of block types and formats, no path yet (today `src/typeset/model.rs`, `src/typeset/parse/` and `src/typeset/layout/`); in reading mode a module per block type in `src/typeset/reading/page/`, made by reading mode's block modules; the blocks without one yet, such as paragraphs, stay in `page.rs` |
| diagram language | `src/typeset/diagrams/`: a file and one entry in `language.rs`; a program that draws it is run by the platform |
| image format | the decoder in `src/typeset/images/decode.rs` |
| theme or look setting | `src/typeset/theme/` |
| warning | a `Diagnostic` in `src/typeset/diagnostic.rs`, with a code of its own: where it came from, its kind and its text; one about an image or diagram is made in `src/typeset/images/warnings.rs` |
| highlight, or anything else that changes from frame to frame | a `Highlight` (`src/typeset/layers.rs`): a range of the document, what it means and a `z`, which the state engine puts in the list that `stack` makes in `src/session/layers.rs` and the engines paint (`src/typeset/layout/layers.rs`, `src/typeset/reading/layers.rs`); nothing new in `marked` |
| bringing something into view | `reveal` in `src/session/reveal.rs`, with a `Show` for how it is to be shown |
| state that each view has of its own, such as its search, selected link or edition | a field of `DocumentState` in `src/session/document.rs`; what the views share stays in `State` |
| state that has to survive a reload | a place that `Align` (`src/typeset/align.rs`) aligns, kept with its version in a `Held` (`src/session/edition.rs`), and moved in `State::realign` |
| panel, such as help or a file list | a document in the panel's look in `src/session/panels/`, a `Card` with its `Sheet` in `src/session/deck.rs`, laid out by the loop in `src/pager/layouts.rs` |
| feature of the pager | a file of its own in `src/session/`, never a new branch in `src/pager/mod.rs` |
| way to show the files given, such as a server | a file of its own in `src/frontend/`, as `pager.rs` and `print.rs` are, reaching the files, streams and terminal through `Platform`; never in `frontend/mod.rs` |
| way to show an image block that the layout lays out in its own way | a kind of rendition in `src/typeset/rendition.rs`, with the terminals that show it and its place in an image's offers, and its branch in `src/typeset/layout/image_block.rs`; an image protocol is not one: its bytes go where a terminal feature's do |
| grid of cells, as a table or front matter has it | `src/typeset/grid.rs`, built there from the document model for both layouts, never in one layout alone |
| place in a document (never a line number or the address of a page) | `Pos` in `src/typeset/position.rs` |
| where something is shown on the screen, or what is at a cell of it, or how tall a layout is | `locate`, `hit` and the other queries of `Laid` in `src/typeset/laid.rs`, answered by the `Map` of `src/typeset/locate.rs`; a line, a column or a pixel never leaves the engines |
| link that can be followed | its kind in `Goes` in `src/typeset/targets.rs`, and what following it does in `src/session/follow.rs` |
| the headings of a document as a list, or the heading a line is under | `src/typeset/contents.rs`, made from `targets.named`, never by parsing again; the level and text come from the block at the place, and for a heading in a block of HTML from `Named::inner`, which `targets()` reads |
| something to search or select in a document, such as its text, its links or its embeds | `src/typeset/text.rs`, or `src/typeset/targets.rs` for links and embeds, asked of the model and not of a layout |
| terminal feature | the I/O in `src/term/`, the bytes in the terminal layer (`src/present/`), and kitty's own sequences in `src/present/kitty/` |

### What `main.rs` is

`src/main.rs` is the binary's entry point: some 200 lines, with the
command line in `src/cli.rs`. It implements `Platform` for the process
(`Real`), reads the command line, resolves the
settings and hands them to the frontends in `src/frontend/`, which open
files and standard input, have one route in which a single file is the
special case of several, print documents, start the pager, and load the
images and diagrams and reload files for the pager; and it calls
`src/reports.rs` for `--doctor` and `--list-emojis`. It also makes the run's one `Fonts` with `load::fonts::system`, and the frontends reach it through `Platform::fonts`. The decisions made at
start need no I/O, and they are in `src/session/plan.rs`. It is meant to
stay as little as this. The settings, and so the configuration file, are
read before the files are opened. Before the frontends moved, the files, and
all of standard input, were read first; the order shows only when
`KIRI_CONFIG` names standard input.

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
  comrak (Markdown), syntect and two-face (syntax highlighting: syntect's
  engine and themes, two-face's grammars), image and resvg (raster and SVG
  images), csv (CSV and TSV files), rustix (terminal modes and pager
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
