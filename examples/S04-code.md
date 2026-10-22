# Code

A document for checking kiri's code blocks and inline code by eye in a real
kitty window. Each section says what you should see there with the default
theme; anything else is a bug. How to view it with the themes and without
colours is in [README.md](README.md):

```sh
cargo run -- examples/S04-code.md
```

## Highlighting

A code block is coloured by its language, the word after the opening
fence: keywords, strings, comments and numbers each get a colour of their
own, on the code background, and the language's name is in the top right
corner of the box. On a light page the colours are for a light
background.

```sh
if make check; then
    echo "all green" # a comment
fi
```

```python
def greet(name):
    """Say hello."""
    return f"Hello, {name}!"  # a comment
```

```yaml
theme:
  name: kiri
  sizes: [1, 2, 3]
```

```json
{"name": "kiri", "tags": ["markdown", "kitty"], "stable": false}
```

kiri has no grammar for every language. TOML is one it lacks, so the block
below is in the code's plain colour, still in its box with its name in the
corner, and so is a block with no language at all:

```toml
[theme]
name = "kiri" # a comment
size = 3
```

```
plain text, in the plain colour
```

## Code blocks

A line too long for the window wraps inside the block. The continuation
starts at the same left edge as the code, and the block's background ends
at the same right edge on every line.

```rust
let line = "long enough to wrap in most windows, with its spaces kept as written and nothing lost at the break";
```

Inline code keeps its runs of spaces: `git  commit   -m` has two spaces
after `git` and three after `commit`.
