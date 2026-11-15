---
title: Code
description: Highlighting, wrapping and inline code.
tags: [code, highlighting]
editor:
  highlighting:
    themes:
      dark:
        name: base16-ocean.dark
        background: "#2b303b"
      light:
        name: InspiredGitHub
        background: "#ffffff"
---

# Code

Each section says what to look for; anything else is a bug. Themes and
colourless output: [README.md](README.md):

```sh
kiri examples/S04a-code.md
```

The front matter above the first heading is a table. Its `editor` key nests
three levels of tables, and the fourth level, the value of `themes`, is
highlighted YAML on a code background, not one more table.

## Highlighting

A code block is coloured by its language, the word after the opening
fence: keywords, strings, comments and numbers each have a colour, and the
language's name is in the box's top right corner.

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

A block marked `text`, or in a language kiri has no grammar for, is in the
plain colour, still in its box with its name, and so is a block with no
language:

```text
[theme]
name = "kiri" # a comment
size = 3
```

```
plain text, in the plain colour
```

## A file shown as text

Only a Markdown file is rendered. Any other text file is shown as it is,
highlighted when kiri knows its language from its extension or name:
`config.yaml`, `Makefile` and `.gitignore` are highlighted, `notes.txt` and
`LICENSE` are not. It has no box, starts at the left margin, and wraps at
the window's edge.

```sh
kiri examples/S04b-config.yaml
kiri examples/S04c-notes.txt
git diff | kiri
```

The YAML file has coloured keys, strings and a comment, and the text file
the terminal's own colours. Piped JSON or a diff is shown the same way,
while piped Markdown is still rendered.

## Code blocks

A line too long for the window wraps inside the block, the continuation
at the code's left edge, the background ending at the same right edge on
every line.

```rust
let line = "long enough to wrap in most windows, with its spaces kept as written and nothing lost at the break";
```

Inline code keeps its runs of spaces: `git  commit   -m` has two spaces
after `git` and three after `commit`.

## Code in HTML

A `<pre>` block, with a `<code>` in it or without, is a code block as a
fence is; its language is the `lang` of the `<pre>` or a `language-` class
of the `<code>`. The two here look like fenced ones: the Rust one coloured,
with `<hi> & bye` in its string, and the `sh` one with `make check`.

<pre><code class="language-rust">fn main() {
    println!("&lt;hi&gt; &amp; bye");
}
</code></pre>

<pre lang="sh">
make check
</pre>

A `<pre>` in a one-line `<details>` is a code block too, under the bold
summary:

<details><summary>Code in a details</summary><pre lang="sh">make check</pre></details>

A `<pre>` with other tags inside stays as source in the muted colour:

<pre>
<u>x</u>
</pre>
