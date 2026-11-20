---
title: Contents
description: The contents bar beside the document, with headings of several levels.
tags: [contents, headings, navigation]
---

# Contents

A document for checking the contents bar by eye in a real kitty window, in a
window of 110 columns or more. Each section says what to look for; anything
else is a bug. In a window this wide the bar shows by itself; the other two
commands show it or hide it when the pager opens:

```sh
kiri examples/S10-contents.md
kiri --toc on examples/S10-contents.md
kiri --toc off examples/S10-contents.md
```

Press `t` to go into the bar. The bar at the left shows the headings in this
document, and the one you are reading is marked. In the bar `j` and `k`, or
the arrows, move the selection, `h` and `l` fold a heading and open it again,
`g` and `G` go to the first and the last heading, and Enter goes to the
selected heading and gives the focus back to the text. `0` or Esc gives the
focus back without moving. `T` shows and hides the bar.

In a terminal with colours the bar has a background a shade apart from the
document's, a line down its right edge, and the headings of the first two
levels in bold, in the colour of the headings; the deeper ones are quieter. The
heading you are reading has a coloured mark at the left of its row and a
tinted row. After `t` the line at the right and the selected row turn
the colour of links, and the selected row is drawn in reverse, so that you
see at once that the keys go to the bar. The status line says it too: a
"contents" badge and the keys of the bar, `Up Down move`, `Enter jump` and
the rest, as many as the window has room for. The colours come from the
theme; see the `[contents]` table in `docs/theme.md`. With `--light` or
`--dark` the bar is in greys.

## First part

The bar shows the level of each heading by its indent. A heading with
headings under it has a triangle: `▾` when they are listed, `▸` when they are
folded away.

### A subsection

Its headings are indented one level more than the heading above. Press `h`
on "First part" in the bar to fold them, and `l` to open them again.

#### A deeper level

Each level is indented two cells more than the one above it.

##### Deeper still

A heading whose text is too long for the bar is cut with an ellipsis, as the
heading below is.

###### The deepest level, with a heading whose text does not fit the bar and is cut short

Level six is the deepest Markdown has; its heading is listed like the others.

## Second part

A part with a single subsection, so that folding it hides one line.

### Only one subsection

Text under the heading. Enter on this entry in the bar brings the heading
to the top of the window.

## Third part

The last part has no subsection. The mark stays on its heading while the
text below it is read, and at the end of the document the last heading on
the screen is the one marked.

Some text, so that the document runs on for more than one window. Press `G`
in the bar and the selection goes to the last heading, while the text stays
where it is; Enter there goes to the heading. With
`toc_preview = true` in the configuration file the text goes there too, and Esc
brings it back.

## The mouse

With the mouse on, the wheel over the bar scrolls its list three rows at a
time, and nothing else moves: the selection, the mark on the heading you are
reading and the text stay where they are. Scroll the document, or jump, and
the bar shows the heading you are reading again. Over the text the wheel
scrolls the text as before.

When the list is longer than the bar, a scroll bar runs down the right edge,
next to the line, with a thumb that shows which part of the list you see. Click
above or below the thumb to page the list, or press on the thumb and drag it.
Scroll the text and the list stays still until the heading you are reading
would leave it; it then moves just enough to keep it in the last row, or the
first when you go back up. Run kiri with `NO_COLOR=1` and the thumb is
drawn in blocks on a blank track.

A click on a heading in the bar goes to it, as Enter does, and Backspace comes
back. A click on a triangle folds the entry or opens it again and leaves the
focus where it was. A click on an empty row or on the line at the right edge
does nothing. Press on that line and drag it sideways to change the width of
the bar, between 16 columns and half the window, with 40 columns left for the
text; the text is laid out again beside it, and what you were reading stays
in view.

In the bar `PgUp` and `PgDn` move the selection by as many rows as the bar
has, and `Home` and `End` go to the first and the last, without scrolling
the text.

## Several files

Open two files, `kiri S10-contents.md S01-text.md`, and the bar lists each
file as an entry with its headings under it. A file is set apart: a blank
row comes before it (not before the first), and its name is underlined in the
colour of the headings, the line going on to the end of the row. The blank row
is no entry: the keys and a click pass over it.

## In a view

A link to another Markdown file opens it over this one, with a bar of its own.
Follow [the text example](S01-text.md) with Tab and Enter: the bar lists the
headings of that document, and `t`, the keys of the bar, the mouse and `?`
work there as they do here. Backspace comes back to this document.
