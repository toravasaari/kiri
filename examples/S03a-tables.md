---
title: Tables
description: Narrow, stacked and wide tables, and how their rows are marked.
tags: [tables, wrapping, rows]
columns:
  - name: Language
    type: text
    align: left
  - name: Released
    type: number
    align: right
layout:
  narrow:
    below: 40 columns
    cells: wrap
  stacked:
    when: a word does not fit
    rows: one block each
---

# Tables

A document for checking kiri's tables by eye in a real kitty window. Each
section says what you should see there with the default theme; anything else
is a bug. How to view it with the themes and without colours is in
[README.md](README.md):

```sh
kiri examples/S03a-tables.md
```

The front matter at the top of this document is a table too, above the
first heading. Its `columns` key is a list of maps, so each map is drawn as
a small table of its own, one under the other in the same cell. Its
`layout` key is a map of maps, so the table of its keys has a table in its
cells, and one more inside that. The borders of all of them stay in line.

## Narrow tables

This table is wider than the default text width of 100 cells, so the text
of its cells wraps inside the columns, the right-aligned notes included.
The borders stay in line, `new` is inline code, and the link is a link.
No word of ten cells or fewer is split while another column can give up
room.

| Element | Status | Notes |
|:--------|:------:|------:|
| Headings | done | Scaled with OSC 66 in kitty, bold and coloured elsewhere, with a rule or a band from the theme |
| Tables | `new` | Wrapped inside their columns when the window is narrow, keeping *inline* styles and [links](https://example.com) |

## Inline code in tables

The commands in this table wrap at their spaces like the text beside
them, and the table stays a box down to about 35 columns. With
`inline_code_wrap = false` in your theme, each command stays on one line
while the last column keeps room for its text, at about 70 columns and
wider; in a narrower window the table looks the same as by default.

| Step | Command | What it does |
|------|---------|--------------|
| Stage | `git add -p` | Picks the changes to commit, hunk by hunk |
| Commit | `git commit -m "Fix the parser"` | Records them with a message |

## Stacked tables

This table cannot fit without splitting short words, so each row becomes
its own block: one line per cell, the header in bold, a colon and the
value. A value that wraps continues at the left edge. Thin rules in the
table border colour separate the rows, with none above the first or below
the last. Empty cells show their header alone, and the styles inside
cells still show. In a window narrower than about 27 columns, each value
goes on the lines under its header instead.

| Language | Paradigm | Typing | Memory | Concurrency | Released | Toolchain | License | Notes |
|----------|----------|--------|--------|-------------|---------:|-----------|---------|-------|
| Rust | multi-paradigm | statically | ownership | async/await, threads | 2015 | cargo/rustup | MIT/Apache-2.0 | A crab 🦀, `cargo build`, and [the book](https://doc.rust-lang.org/book/) |
| Python | multi-paradigm | dynamically | garbage-collected | asyncio, threads | 1991 | pip/uv | PSF-2.0 | *Batteries included*: a large standard library, an interactive shell, and a package index with hundreds of thousands of projects to install |
| Lisp | functional | dynamically | garbage-collected | | 1958 | | | |

> The same table in a quote, stacked inside the bar:
>
> | Language | Paradigm | Typing | Memory | Concurrency | Released | Toolchain | License | Notes |
> |----------|----------|--------|--------|-------------|---------:|-----------|---------|-------|
> | Rust | multi-paradigm | statically | ownership | async/await, threads | 2015 | cargo/rustup | MIT/Apache-2.0 | A crab 🦀 |
> | Lisp | functional | dynamically | garbage-collected | | 1958 | | | |

## Row distinction

By default every second row of a table is shaded: the second, the fourth
and so on, but not the header. The shade is the page colour with a little
of the text colour mixed in, so it is slightly lighter than a dark page
and slightly darker than a light one. It needs 24-bit colour and the
page colour, which kitty tells, so with `NO_COLOR` nothing is shaded, nor
in a terminal that does not tell, unless the theme sets a page colour.
The notes in the table below are long enough to wrap, to show how each
style treats the lines of a value:

| Step | Note |
|------|------|
| One | The first row, whose note is long enough to wrap onto a second line in most windows, and then some more to be sure of it |
| Two | The second row, shaded by default, and its note is just as long, so that its wrapped lines show how each style marks them |
| Three | The third row, short |
| Four | The fourth row, shaded by default too, with a note long enough to wrap onto a second line in a window of average width |

The same document with each of the other styles. They change every
table in it that is drawn with borders; a stacked table keeps its
layout:

```sh
kiri --table-rows lines examples/S03a-tables.md
kiri --table-rows indent examples/S03a-tables.md
kiri --table-rows underline examples/S03a-tables.md
kiri --table-rows off examples/S03a-tables.md
```

- `lines` draws a thin rule between the rows, in the border colour.
- `indent` starts the second and later lines of a wrapped value two cells
  in, so that each row's first line stands out.
- `underline` underlines the last line of each row except the last row,
  with a dashed line in the border colour. It needs underline styles, as
  kitty has them.
- `off` marks nothing: the rows run together.

In plain text output, from `--mode plain` or a pipe, only `lines` and
`indent` mark the rows.

## Wide characters in tables

| 名前 | Emoji | Flag |
|------|-------|------|
| 東京 | 🦀    | 🇯🇵   |
| Oslo | 🌲🌲  | 🇳🇴   |
| Koti | 👨‍👩‍👧    | 🇫🇮   |

Every row of the table above has its borders in line.

## A CSV file

A CSV file is shown as a table too, with its first row as the header. The
file [S03b-table.csv](S03b-table.csv) has four columns and three rows. You
should see the same borders and the same alternating shades as above, the
header in bold, and the quoted note "Lake view, open all year" in one cell
with its comma:

```sh
kiri examples/S03b-table.csv
```
