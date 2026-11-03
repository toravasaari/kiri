# Formulas

kiri draws a formula written in LaTeX as an image in its block's place,
as GitHub shows it. Both forms GitHub draws as a block work:

````markdown
```math
\sum_{i=1}^{n} i = \frac{n(n+1)}{2}
```

$$
e^{i\pi} + 1 = 0
$$
````

`$$…$$` may also stand on a line of its own inside a paragraph, or share
a line with text, as in `so $$a^2 + b^2 = c^2$$ holds`: the paragraph
is split there, and the formula shows as a block between its two parts,
as on GitHub. A `$$` that is never closed stays text.

kiri draws formulas itself, through [RaTeX](https://crates.io/crates/ratex-parser),
a Rust port of KaTeX, in every build: no program to install, no fonts
and no network. RaTeX reads what KaTeX reads: fractions, sums and
integrals with their limits, matrices, `aligned` lines, `\text{…}` and
the rest of KaTeX's commands.

An inline formula, `$x$` or `` $`x`$ ``, is drawn in the middle of its
line, in the style of the text around it, at about its size: one that fits
the row is drawn in it, as in $E = mc^2$, and a taller one, such as a
matrix, makes its line two rows high, with the text between them. Where
the terminal has no text sizing, the text is on the row the formula's
baseline is on, with rows above and below it as the formula needs: a 2 × 2
matrix takes three.

A formula stands in the middle of the room it has, as on GitHub: of the
text's width, or of a list item's or a quote's. One wider than that is
shrunk to fit it. Images and diagrams stay at the left edge.

A formula is in the colour of the text around it: the theme's
`foreground`, or the terminal's own text colour when kiri knows it, and
otherwise white on a dark page and black on a light one. It has no
background of its own, so the page shows through. A `[formula]` table in
the [theme](theme.md) changes both:

```toml
[formula]
text = "#1a4f8b"
background = "#f4f1e8"
```

Its letters are a little larger than the text around it, as on GitHub:
KaTeX's 1.21 em of a browser's body text, about 19 CSS pixels to the
em, in display style. So a formula of one line, such as `e^{i\pi} + 1 = 0`,
takes two rows, and a fraction three.

## What stays source

- **A price**, such as `$5 and $6`, is no formula and stays text. An
  inline formula that does not draw, or is not drawn yet, shows as written,
  dollar signs and backticks included, and so does one in a table cell,
  which is drawn as text for now.
- **A formula that does not draw**, such as `\frac{a` with its brace
  never closed, stays code. As for a [diagram](diagrams.md), kiri tells
  why below the code, in the theme's `error` colour, as in
  `✗ formula could not be drawn: ParseError at position 7: …`, and in a
  warning on standard error, as in `kiri: formula on line 12: …`.
  [Render errors](config.md#render-errors) choose either one alone.
- **Formulas with images off**, with `--no-images`, or in a terminal
  without kitty's graphics, show as code, as images show their alt text.
- **Formulas with diagrams off**, `--diagrams off`, `KIRI_DIAGRAMS=0` or
  `enabled = false` in the `[diagrams]` table of the configuration file,
  show as code too: kiri draws them as it draws diagrams. They have no
  switch of their own.

`kiri --doctor` lists RaTeX with its version as the renderer of `math`,
under *Diagrams*.
