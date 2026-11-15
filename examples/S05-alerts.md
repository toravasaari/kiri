---
title: Alerts
description: The five GitHub alert types, with their colours and icons.
tags: [alerts, note, tip, warning]
---

# Alerts

Each section says what to look for; anything else is a bug. Themes and
colourless output: [README.md](README.md):

```sh
kiri --no-reading examples/S05-alerts.md
```

## The five types

Each alert has a bar down its left edge, an icon and a bold title in its
own colour, over a faint tint of that colour, as GitHub draws them. Outside
kitty the text layout has no icon. Inline code has a stronger tint.

> [!NOTE]
> A note: blue, with a circled "i". Inline code such as `make check` has a
> stronger tint.

> [!TIP]
> A tip: green, with a light bulb.

> [!IMPORTANT]
> Important: purple, with a speech bubble holding an exclamation mark.

> [!WARNING]
> A warning: yellow, gold on a light page, with a triangle.

> [!CAUTION]
> A caution: red, with an octagon holding an exclamation mark.

## Paper and without colours

On the paper themes the alerts keep their colours. The tints on `--light`
are pale: blue, green, lilac, lemon yellow (not beige or brown, with a
gold bar and title) and pink. On `--dark` they are dark shades. With
`NO_COLOR` an alert keeps its bar, icon and bold title, without colour or
tint. In plain text output it is an ordinary quote with the title on its
first line:

```sh
kiri --no-reading --light examples/S05-alerts.md
kiri --no-reading --dark examples/S05-alerts.md
NO_COLOR=1 kiri --no-reading examples/S05-alerts.md
kiri --no-reading --mode plain examples/S05-alerts.md
```

## In reading mode

In reading mode, which is the default, the alerts are in the proportional
font, and the icon is a picture, so it shows in every terminal that draws
images, not only in kitty:

```sh
kiri examples/S05-alerts.md
kiri --light examples/S05-alerts.md
```
