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
