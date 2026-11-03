---
title: Formulas
description: LaTeX formulas in math blocks and between $$, and what stays text.
tags: [formulas, math]
---

# Formulas

A document for checking kiri's formulas by eye in a real kitty window.
Each section says what you should see there with the default theme;
anything else is a bug. How to view it with the themes and without
colours is in [README.md](README.md):

```sh
kiri examples/S08-formulas.md
```

## A formula in a math block

A code block in `math` is drawn as a formula in kitty, as GitHub draws
it, and stays code elsewhere, with `--no-images` and with
`--diagrams off`. The formula below stands in the middle of the text's
width, in the colour of the text, with no box or panel behind it. Its
letters are a little larger than the text around it, as on GitHub, and
the sum's limits and the fraction make it four rows high:

```math
\sum_{k=1}^{n} k = \frac{n(n+1)}{2}
```

## Formulas between double dollars

`$$` on lines of their own work the same way. The quadratic formula,
with a square root over the whole of b² − 4ac:

$$
x = \frac{-b \pm \sqrt{b^2 - 4ac}}{2a}
$$

A matrix in round brackets, two rows of two:

$$
R(\theta) = \begin{pmatrix} \cos\theta & -\sin\theta \\ \sin\theta & \cos\theta \end{pmatrix}
$$

Two aligned lines, their equals signs one above the other, with words
in upright letters at the end of the second:

$$
\begin{aligned}
  (a + b)^2 &= (a + b)(a + b) \\
            &= a^2 + 2ab + b^2 \quad \text{for any } a, b
\end{aligned}
$$

A formula in the middle of a paragraph, such as $$e^{i\pi} + 1 = 0$$
here, splits it: the text before it, the formula centred on a line of
its own, and the rest of the text below it.

## In lists and quotes

A formula in a list item is centred in the item's own width, to the
right of the bullet:

- Pythagoras:

  $$a^2 + b^2 = c^2$$

> A formula in a quote is centred between the quote's bar and the right
> edge:
>
> $$\int_0^1 x^2 \, dx = \frac{1}{3}$$

> [!TIP]
> And in an alert, in the text's colour, not the alert's:
>
> $$\lim_{x \to 0} \frac{\sin x}{x} = 1$$

## On paper

On the light and dark paper themes, a formula is in the page's text
colour: near black on white, light grey on dark grey.

```sh
kiri --light examples/S08-formulas.md
kiri --dark examples/S08-formulas.md
```

## A formula that fails to draw

A formula that cannot be drawn stays code. The one below cannot, because
its fraction's brace is never closed:

$$
\frac{1}{2
$$

Below the code, in red, kiri says why, in lines that start with a cross
and "formula could not be drawn". A warning on standard error says the
same and names the formula by its line, `kiri: formula on line …: …`.
`--render-errors` chooses one of them, as for diagrams.

## Formulas in a line

An inline formula, `$…$` or `` $`…`$ ``, is drawn in its line, in text style
at about the size of the text, with its own baseline on the line's. One
that fits the row, as $E = mc^2$ and $`x_1`$ do, takes no more than a row
of the text, and the line stays one row high.

One a little too high for the row, as $\frac{a^2 + b^2}{c^2 + d^2}$ is here,
is drawn a little smaller to stay in it, and the line is still one row.

A tall one makes its line two rows high, with the text of the line centred
between the rows: in $\begin{pmatrix}a&b\\c&d\end{pmatrix}$ here, the words of this
line are in the middle of the formula's height. Where kitty has no text
sizing, the text is on the lower row and the formula covers both.

## What stays text

A price such as $5 and $6 is no formula in any mode and stays as written,
dollar signs and all, and so do inline formulas in a table cell, which are
drawn as text for now:

| Formula | Written |
|---|---|
| $x^2$ | `$x^2$` |
