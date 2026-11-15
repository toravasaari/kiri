---
title: Formulas
description: LaTeX formulas in math blocks and between $$, and what stays text.
tags: [formulas, math]
---

# Formulas

Each section says what to look for; anything else is a bug. Themes and
colourless output: [README.md](README.md):

```sh
kiri examples/S08-formulas.md
```

## A formula in a math block

A code block in `math` is drawn as a formula in kitty, as GitHub draws
it, and stays code elsewhere, with `--no-images` and `--diagrams off`. The
formula stands in the middle of the text's width, in the text's colour,
with no box behind it and its letters a little larger than the text
around it:

```math
\sum_{k=1}^{n} k = \frac{n(n+1)}{2}
```

## Formulas between double dollars

`$$` on lines of their own work the same way. The quadratic formula, with
a square root over the whole of b² − 4ac:

$$
x = \frac{-b \pm \sqrt{b^2 - 4ac}}{2a}
$$

A matrix in round brackets, two rows of two:

$$
R(\theta) = \begin{pmatrix} \cos\theta & -\sin\theta \\ \sin\theta & \cos\theta \end{pmatrix}
$$

Two aligned lines, with the equals signs one above the other and upright
words at the end of the second:

$$
\begin{aligned}
  (a + b)^2 &= (a + b)(a + b) \\
            &= a^2 + 2ab + b^2 \quad \text{for any } a, b
\end{aligned}
$$

A formula in the middle of a paragraph, such as $$e^{i\pi} + 1 = 0$$
here, splits it: the formula is centred on a line of its own.

## In lists and quotes

A formula in a list item is centred in the item's width, right of the
bullet:

- Pythagoras:

  $$a^2 + b^2 = c^2$$

> A formula in a quote is centred between the bar and the right edge:
>
> $$\int_0^1 x^2 \, dx = \frac{1}{3}$$

> [!TIP]
> In an alert it is in the text's colour, not the alert's:
>
> $$\lim_{x \to 0} \frac{\sin x}{x} = 1$$

## On paper

On the paper themes a formula is in the page's text colour.

```sh
kiri --light examples/S08-formulas.md
kiri --dark examples/S08-formulas.md
```

## A formula that fails to draw

A formula that cannot be drawn stays code. This one cannot, because its
fraction's brace is never closed:

$$
\frac{1}{2
$$

Below the code, in red, kiri says why, in lines that start with a cross
and "formula could not be drawn". A warning on standard error says the same,
`kiri: formula on line …: …`. `--render-errors` chooses one, as for
diagrams.

## Formulas in a line

An inline formula, `$…$` or `` $`…`$ ``, is drawn in its line at about the
size of the text, on the line's baseline. One that fits the row, as
$E = mc^2$ and $`x_1`$ do, keeps the line one row high.

One a little too high, as $\frac{a^2 + b^2}{c^2 + d^2}$ is here, is drawn a
little smaller to stay in the row.

A tall one makes its line higher, with the text centred on it: in
$\begin{pmatrix}a&b\\c&d\end{pmatrix}$ here, the words of this line are
in the middle of the formula's height. Without kitty's text sizing the
matrix takes three rows, and the words are on the middle one.

## What stays text

A price such as $5 and $6 is no formula and stays as written, and so do
inline formulas in a table cell, for now:

| Formula | Written |
|---|---|
| $x^2$ | `$x^2$` |
