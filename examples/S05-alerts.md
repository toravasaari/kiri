# Alerts

A document for checking kiri's alerts by eye in a real kitty window. Each
section says what you should see there with the default theme; anything else
is a bug. How to view it with the themes and without colours is in
[README.md](README.md):

```sh
cargo run -- examples/S05-alerts.md
```

## Types

Each alert has a bar down its left edge, an icon and a bold title in a
colour of its own, over a faint tint of that colour as wide as the text.
The colours stay on paper. Outside kitty the icon is left out, and
without colours an alert is a plain quote with its title on the first
line.

> [!NOTE]
> A note: blue, with an info icon and the title "Note". Inline code such
> as `make check` has a stronger blue tint.

> [!WARNING]
> A warning: yellow-brown, with a warning triangle and the title
> "Warning".
