# Example themes

Variations on the default theme. Each file changes only what its name says;
compare them with:

```sh
kiri --theme examples/themes/rules.toml tests/fixtures/headings.md
```

| File | Idea |
|---|---|
| `rules.toml` | GitHub-style rules under the top two levels instead of underlines |
| `background.toml` | Headings on a coloured band, indented by one cell |
| `indent.toml` | Headings indented by one cell |
| `opaque.toml` | A solid document background instead of the terminal's |

To check a theme by eye on the cases that have broken before, render
[`examples/visual-check.md`](../visual-check.md) with it, and
[`examples/S06a-images.md`](../S06a-images.md) for images
on the theme's background.
