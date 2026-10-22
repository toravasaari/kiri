# Tables

A document for checking kiri's tables by eye in a real kitty window. Each
section says what you should see there with the default theme; anything else
is a bug. How to view it with the themes and without colours is in
[README.md](README.md):

```sh
cargo run -- examples/S03a-tables.md
```

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

## Wide characters in tables

| 名前 | Emoji | Flag |
|------|-------|------|
| 東京 | 🦀    | 🇯🇵   |
| Oslo | 🌲🌲  | 🇳🇴   |
| Koti | 👨‍👩‍👧    | 🇫🇮   |

Every row of the table above has its borders in line.
