# Text

A document for checking kiri's text by eye in a real kitty window. Each
section says what you should see there with the default theme; anything else
is a bug. How to view it with the themes and without colours is in
[README.md](README.md):

```sh
cargo run -- examples/S01-text.md
```

## Heading levels

Level one is three times the normal size, levels two and three twice,
and levels four to six normal size. Underlines, weight, colour and italic
tell the levels of the same size apart.

# Level one

## Level two

### Level three

#### Level four

##### Level five

###### Level six

## Empty blocks

Between this sentence and the next there is an empty heading. It should be
invisible: no line, and no extra blank line.

#

After the empty heading. Next comes an empty code block, which is invisible
in the same way.

```
```

After the empty code block. In the list below, the second item is empty:
its line shows the bullet and nothing after it.

- First item
-
- Third item

## Wide characters

CJK, emoji, flags and joined emoji each take two cells and never split:
日本語のテキストと漢字, 🦀, 🇫🇮 🇯🇵, 👨‍👩‍👧 and ❤️. Lines with them still end
inside the window.

### 日本語の見出し 🦀 🇫🇮

The heading above holds wide characters and is drawn as large as the other
level-three headings.

```
let 名前 = "東京"; // 🦀 🇫🇮 👨‍👩‍👧
```

## Links

The link at the end of this sentence is long enough to wrap onto the next
line: [a link whose text goes on long enough to span two lines in most windows](https://example.com/visual-check).
Every line of it is underlined and opens the same page.
