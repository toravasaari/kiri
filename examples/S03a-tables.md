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

Each section says what to look for; anything else is a bug. Themes and
colourless output: [README.md](README.md):

```sh
kiri examples/S03a-tables.md
```

The front matter above the first heading is a table too. Its `columns`
list is drawn as a small table for each map, one under the other, and its
`layout` map has a table in its cells and one more inside that. All their
borders stay in line.

## Narrow tables

This table is wider than the text, so its cells wrap inside the columns,
the right-aligned notes included. The borders stay in line, `new` is
inline code, and the link is a link. No short word is split while another
column can give up room.

| Element | Status | Notes |
|:--------|:------:|------:|
| Headings | done | Scaled with OSC 66 in kitty, bold and coloured elsewhere, with a rule or a band from the theme |
| Tables | `new` | Wrapped inside their columns when the window is narrow, keeping *inline* styles and [links](https://example.com) |
| Links | `new` | [A link with a long text wraps over the lines of its cell in a narrow window](https://example.com/cell) |

In the pager, Tab selects the link in the last row as one link: every
line of it shows in reverse video at once.

## Inline code in tables

The commands in this table wrap at their spaces, and the table stays a box
down to about 35 columns. With `inline_code_wrap = false` in your theme,
each command stays on one line at about 70 columns and wider.

| Step | Command | What it does |
|------|---------|--------------|
| Stage | `git add -p` | Picks the changes to commit, hunk by hunk |
| Commit | `git commit -m "Fix the parser"` | Records them with a message |

## Stacked tables

This table cannot fit without splitting short words, so each row becomes
a block: one line per cell, the header in bold, a colon and the value, with
thin rules between the rows. Empty cells show their header alone, and the
styles inside cells still show. Below about 27 columns each value goes
under its header.

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

By default every second row is shaded, the header not. `NO_COLOR` shades
nothing. The notes below are long enough to wrap, to show how each style
treats the lines of a value:

| Step | Note |
|------|------|
| One | The first row, whose note is long enough to wrap onto a second line in most windows, and then some more to be sure of it |
| Two | The second row, shaded by default, and its note is just as long, so that its wrapped lines show how each style marks them |
| Three | The third row, short |
| Four | The fourth row, shaded by default too, with a note long enough to wrap onto a second line in a window of average width |

The other styles change every table drawn with borders; a stacked table
keeps its layout. `lines` draws a thin rule between the rows, `indent`
starts a wrapped value's later lines two cells in, `underline` underlines
each row's last line except the last row's, and `off` marks nothing. Plain
text output marks the rows only with `lines` and `indent`:

```sh
kiri --table-rows underline examples/S03a-tables.md
```

## Wide characters in tables

| 名前 | Emoji | Flag |
|------|-------|------|
| 東京 | 🦀    | 🇯🇵   |
| Oslo | 🌲🌲  | 🇳🇴   |
| Koti | 👨‍👩‍👧    | 🇫🇮   |

Every row has its borders in line.

## A CSV file

A CSV file is a table too, with its first row as the header. The file
[S03b-table.csv](S03b-table.csv) has four columns and three rows, and the
quoted note "Lake view, open all year" stays in one cell with its comma:

```sh
kiri examples/S03b-table.csv
```

## A long table

In the pager, a table of more than 128 rows is laid out a stretch of rows
at a time, so that a CSV file of a million rows opens at once and shows
every row. Its columns are as wide as its first 128 rows ask for. The file
[S03c-long-table.csv](S03c-long-table.csv) has 300 rows, and the note of
day 200 is longer than any before it: it wraps in its column, and the
border lines up above and below it. `:w`, or `r`, fits the columns to every
row: the note column widens for day 200, as far as the window lets it:

```sh
kiri examples/S03c-long-table.csv
```

Printed with `-p`, the table is laid out whole, its columns as wide as all
its rows ask for.

## In reading mode

In reading mode, which is the default, the tables are in the proportional
font, as wide as their content, and the front matter is as wide as the text
column. Nested tables shade their own rows, also inside a shaded row of the
table around them. A table is a rounded frame with lines between the columns
and under the header; for a line round every cell, as GitHub draws tables,
use `table_style = "grid"` in a theme file:

```sh
echo 'table_style = "grid"' > /tmp/grid.toml
kiri --theme /tmp/grid.toml examples/S03a-tables.md
```
