# kiri – kitty reader

kiri shows Markdown documents in the [kitty](https://sw.kovidgoyal.net/kitty/)
terminal the way a browser would: headings in a genuinely larger font,
images, formulas and diagrams drawn inline, and links you can click. It works
like `kitten icat`, but for documents. Other document formats may follow.

> **Status:** early development. Print mode works; images, formulas and
> the interactive pager are still to come. See the [roadmap](docs/roadmap.md).

## Why

Existing terminal Markdown viewers limit themselves to plain ANSI text, so
every heading is the same size as body text and anything visual is left out.
kitty can do more: its text sizing protocol draws text at several sizes, and
its graphics protocol displays images that scroll with the text. kiri is built
around those features. Other terminals get ANSI colours instead, and output
that is not a terminal gets plain text.

## Usage

```sh
make release
target/release/kiri README.md
```

kiri asks the terminal what it supports and uses the best it gets: kitty's
large headings and clickable links, plain ANSI colours elsewhere, and plain
text when the output is a pipe.

With [`NO_COLOR`](https://no-color.org) set to anything but an empty value,
kiri uses no colours in any mode. Bold, italic, underlines, links and heading
sizes stay; code blocks are not highlighted, and inline code is shown in
backticks.

| Option | Meaning |
|---|---|
| `--mode auto\|plain\|ansi\|kitty` | Override what kiri detects |
| `--width N` | Render for `N` cells instead of the terminal width |
| `--theme FILE` | Colours and heading sizes; see [Theme](docs/theme.md) |
| `-M`, `--no-margins` | No margins or padding, so text and code copy cleanly |

An interactive pager (`kiri -i`) with search and link navigation is
planned.

## Documentation

- [Roadmap](docs/roadmap.md)
- [Architecture](docs/architecture.md)
- [Theme](docs/theme.md)
- [AGENTS.md](AGENTS.md) – conventions for coding agents and contributors

## License

Not decided yet.
