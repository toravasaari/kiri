---
title: Alerts
description: The five GitHub alert types, with their colours and icons.
tags: [alerts, note, tip, warning]
---

# Alerts

A document for checking kiri's alerts by eye in a real kitty window. Each
section says what you should see there with the default theme; anything else
is a bug. How to view it with the themes and without colours is in
[README.md](README.md):

```sh
kiri examples/S05-alerts.md
```

## The five types

Each alert has a bar down its left edge, an icon and a bold title in a
colour of its own, over a faint tint of that colour as wide as the text.
The colours and icons are the ones GitHub uses. kitty draws the icons from
the symbols font it bundles, so outside kitty an alert has no icon. Inline
code in an alert has a stronger tint of the colour than the rest.

> [!NOTE]
> A note: blue, with a circled "i" for an info icon and the title "Note".
> Inline code such as `make check` has a stronger blue tint.

> [!TIP]
> A tip: green, with a light bulb and the title "Tip".

> [!IMPORTANT]
> Important: purple, with a speech bubble holding an exclamation mark and
> the title "Important".

> [!WARNING]
> A warning: yellow-brown, with a warning triangle and the title
> "Warning".

> [!CAUTION]
> A caution: red, with an octagon holding an exclamation mark, as on a
> stop sign, and the title "Caution".

## Paper and without colours

On the paper themes the alerts keep their colours, in GitHub's darker
shades on the light page and its lighter ones on the dark page, each over
a faint tint of its colour. Without colours, with `NO_COLOR`, an alert
keeps its bar, icon and bold title, with no colour and no tint. In plain
text output it is an ordinary quote, with the title on its first line:

```sh
kiri --light examples/S05-alerts.md
kiri --dark examples/S05-alerts.md
NO_COLOR=1 kiri examples/S05-alerts.md
kiri --mode plain examples/S05-alerts.md
```

## In reading mode

With `--reading` the alerts are drawn in the proportional font, in the
column: the same bar, tint, colours and bold title, but the icon is a
picture, so it is there in every terminal that draws images, not only in
kitty with its symbols font. Look for the circled `i`, the light bulb,
the speech bubble, the triangle and the octagon, each in its kind's
colour:

```sh
kiri --reading examples/S05-alerts.md
```
