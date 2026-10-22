# Code

A document for checking kiri's code blocks and inline code by eye in a real
kitty window. Each section says what you should see there with the default
theme; anything else is a bug. How to view it with the themes and without
colours is in [README.md](README.md):

```sh
cargo run -- examples/S04-code.md
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
