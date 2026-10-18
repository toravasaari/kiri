# kiri – kitty reader

kiri shows Markdown documents in the [kitty](https://sw.kovidgoyal.net/kitty/)
terminal the way a browser would: headings in a genuinely larger font,
images, formulas and diagrams drawn inline, and links you can click. It works
like `kitten icat`, but for documents. Other document formats may follow.

> **Status:** early development. Print mode and images work; formulas,
> diagrams and the interactive pager are still to come. See the
> [roadmap](docs/roadmap.md).

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
target/release/kiri photo.jpg
```

kiri asks the terminal what it supports and uses the best it gets: kitty's
large headings and clickable links, plain ANSI colours elsewhere, and plain
text when the output is a pipe.

kiri follows your terminal's colour scheme. `--light` and `--dark` draw
the document on a page of fixed colours instead, white or dark grey, like
a page in a browser: text and everything around it in greys, and colour
only in code blocks. They also choose code colours for a light or dark
background, which kiri otherwise takes from the terminal's background.

With [`NO_COLOR`](https://no-color.org) set to anything but an empty value,
kiri uses no colours in any mode. Bold, italic, underlines, links and heading
sizes stay; code blocks are not highlighted, and inline code is shown in
backticks.

Tables too wide for the window wrap the text inside their cells, without
splitting any word of ten cells or fewer. When that is not enough, each row
is shown as its own block of `Header: value` lines instead.

An image that is the only thing in its paragraph is drawn as a block, at
its size in CSS pixels as a browser would show it and no wider than the
text; images inside running text show their alt text for now. HTML `<img>`
tags work too, with `width` and `height` in pixels or a percentage width,
also when a link, `<p>`, `<div>` or `<picture>` wraps them. Only such a
size enlarges an image.

An image file given on its own, `kiri photo.jpg`, is shown as large as
fits the window but never enlarged, even when images are off. Its
extension and first bytes tell kiri that it is an image, and `--format`
can say so instead. A terminal without kitty's graphics gets an error.

Images come from `data:` URLs and local files: absolute paths, paths
relative to the document and `file:` URLs, with any `?query` or `#fragment`
left out as in a browser. kiri reads only regular files up to the
`--max-image-size` limit, which also caps a compressed SVG once
decompressed, and does not fetch remote images. kiri shows PNG, JPEG, GIF
(the first frame), WebP and SVG images, and in its default, full build
also BMP, ICO, TIFF, QOI, PNM and TGA. A build without default features
(`cargo build --no-default-features`) leaves those out and shows their
alt text with a warning.

| Option | Meaning |
|---|---|
| `-f`, `--format FORMAT` | What the file is: `markdown`, `image`, or an image format such as `png` |
| `--mode auto\|plain\|ansi\|kitty` | Override what kiri detects |
| `--width N` | Render for `N` cells instead of the terminal width |
| `--theme FILE` | Colours and heading sizes; see [Theme](docs/theme.md) |
| `--light`, `--dark` | A light or dark page in greys; see [Paper themes](docs/theme.md#paper-themes) |
| `-M`, `--no-margins` | No margins or padding, so text and code copy cleanly |
| `-I`, `--no-images` | Show images as their alt text, without reading them |
| `--images` | Show images, whatever the environment and configuration say |
| `--max-image-size MB` | Largest image file to read: 10 MB unless set, at most 100 |
| `--doctor` | Show what kiri finds out about the terminal and which settings it uses |
| `--raw` | With `--doctor`: the report as Markdown text, for a bug report |

A [configuration file](docs/config.md), `~/.config/kiri/config.toml`, can
choose the theme and turn images off, and so can the environment variables
`KIRI_THEME` and `KIRI_NO_IMAGES`.

When the output looks wrong, `kiri --doctor` shows what kiri found out
about the terminal, which settings it uses and where each came from, with
a sample of headings, a table and an image drawn below. `kiri --doctor
--raw` gives the report as Markdown without the sample, to paste into a
bug report; piped to a file, the report is Markdown with the sample.

An interactive pager (`kiri -i`) with search and link navigation is
planned.

## Documentation

- [Roadmap](docs/roadmap.md)
- [Architecture](docs/architecture.md)
- [Configuration](docs/config.md)
- [Theme](docs/theme.md)
- [Visual check](examples/visual-check.md) – a document for checking the
  output by eye, and [another for images](examples/visual-check-images.md)
- [AGENTS.md](AGENTS.md) – conventions for coding agents and contributors

## License

Not decided yet.
