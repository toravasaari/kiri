---
title: Reading mode
description: The text in a proportional font, as a browser shows it.
tags: [reading, experimental]
---

# Reading mode

A document for checking reading mode by eye in a real kitty window. Each
section says what you should see there with the default theme; anything
else is a bug. Reading mode is experimental, and its option's name may
still change:

```sh
kiri --reading examples/S09-reading-mode.md
kiri --reading --light examples/S09-reading-mode.md
kiri --reading --dark examples/S09-reading-mode.md
```

Without `--reading` this document looks like the others. With it, the
headings, paragraphs, lists and quotes are in the system's sans-serif font
at about four fifths of a row's height, in a column 42 times that wide at
most, at the left of an area half as wide again in the middle of the
window. The front matter above, the code blocks and the table stay as they
are without reading mode, between the parts in the proportional font. The
text cannot be selected or copied, as it is drawn as images.

## Text

A paragraph with *emphasis*, **strong text**, ~~strikethrough~~, inline
code such as `make check`, a key such as <kbd>Ctrl</kbd>, and
[a link](https://example.com) in blue, without an underline. Lines are one
and a half times the font size apart, also the lines with inline code,
whose grey chip sits a little lower than the line and has no padding
spaces around it. Inline code breaks only at its spaces: a long one such as
`src/render/reading.rs` or `--reading-mode-experimental-flag` moves to the
next line whole rather than breaking at a slash or a hyphen.

Other scripts come from fonts that have them: Привіт, Ελληνικά,
日本語のテキスト, العربية and עברית. Emoji are in colour: 🎉, 🦀 and 😀;
a © stays text.

A line break inside a paragraph,\
starts a new line here.

---

Above, a thematic break: a grey bar a quarter of the font size thick
across the column.

## Headings

Below, the six heading levels at GitHub's sizes: two, one and a half, one
and a quarter, one, seven eighths and 0.85 times the text, bold, and in
the heading colour. Levels one and two have a thin grey rule under them.
A heading is further from the text above it than from the text below it.

# Level one

## Level two

### Level three

#### Level four

##### Level five

###### Level six

## Lists

The markers are right-aligned just before the text, on the baseline of
the item's first line: discs, then circles, then squares deeper in.
Items in a tight list are half a line apart.

- An item
- An item long enough to wrap onto a second line in a window of usual
  width, whose second line starts under the first line's text
  - A nested item, with a circle
    - Deeper still, with a square

1. A numbered item
2. Another one
3. The numbers end at the same place, also at ten:
4. Four
5. Five
6. Six
7. Seven
8. Eight
9. Nine
10. Ten

- [x] A finished task, with a blue box and a white tick
- [ ] An open task, with a grey outline

## Quotes

> A quote has a grey bar down its left edge, and its text is grey and
> starts a little after the bar.
>
> A second paragraph in the same quote, under the same bar.

## Between the text

A code block and a table show as they do without reading mode, at the
left with the usual margin:

```rust
fn main() {
    println!("Hello, kiri");
}
```

| Name | Kind |
|------|------|
| Fern | plant |

And the text goes on below them in the proportional font.
