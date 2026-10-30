---
title: Text
description: Headings, emphasis, links, wide characters, emoji and HTML tags.
tags: [text, headings, links, emoji]
---

# Text

A document for checking kiri's text by eye in a real kitty window. Each
section says what you should see there with the default theme; anything else
is a bug. How to view it with the themes and without colours is in
[README.md](README.md):

```sh
kiri examples/S01-text.md
```

## Heading levels

Level one is two and a half times the normal size, and each level after
it a quarter smaller, down to the normal size at level six. Every level is
bold and blue, without an underline: size alone tells them apart.

# Level one

## Level two

### Level three

#### Level four

##### Level five

###### Level six

### Headings with `code`, <kbd>Ctrl</kbd> and *emphasis*

Inline code and a key in a heading look as they do in the text, at the
heading's size, in the inline code colour on its background, and the
emphasis is in italics: with kitty's text sizing, and with
`--headings images` alike.

## Words in headings

Headings at levels one, three, four and five are drawn at a fraction of
the cells they take, and kitty draws them a few pixels narrower than
those cells. Words stay whole: no gap shows inside "heading" or
"several", and the few pixels go between words instead, so some spaces
are a little wider than others. Long words such as "documentation" and
"configuration" stay whole too. Each heading starts at the left margin,
except one whose first word is longer than kitty can draw in one go, such
as the last one here: it starts a few pixels to the right.

# Several words in a heading at level one

### Several words in a heading at level three

#### Several words in a heading at level four

##### Several words in a heading at level five

### Documentation for configuration

## Emphasis and strikethrough

A paragraph with *emphasis* in italics, **strong text** in bold,
***both at once***, and ~~strikethrough~~ with a line through its letters.
They are attributes of the text, not colours, so `NO_COLOR` keeps them;
plain text output has none of them.

## Empty blocks

Between this sentence and the next there is an empty heading. It should be
invisible: no line, and no extra blank line.

#

After the empty heading. Next comes an empty code block: a box of the code
background, two rows high and as wide as the text, also on paper. Without
colours it is invisible, as the heading is.

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

## Emoji shortcodes

A shortcode between colons becomes its emoji, as on GitHub: :rocket: is a
rocket, :heart: a heart, :coffee: a cup of coffee and :tada: a party
popper. Each takes two cells, and the line still ends inside the window.
The same goes in a heading and in a table cell:

### Shipped :rocket:

| Mood | Emoji | Note |
|------|:-----:|------|
| Love | :heart: | the borders stay in line |
| Fire | :fire: | the borders stay in line |

A code that kiri does not know stays as written: :not_a_real_code:. So
does one in inline code, `:rocket:`, and one in a code block:

```
echo ":rocket:"
```

`kiri --list-emojis` lists the codes it knows, as a table in a terminal.

## HTML tags

A few HTML tags are drawn as GitHub shows them. Others, such as `<span>`,
stay as source in a muted colour.

### Line breaks

A `<br>` ends the line, with or without a slash: First line<br>second
line<br/>third line.

### Keys

Press <kbd>Ctrl</kbd>+<kbd>C</kbd> to copy, or <kbd>Page Down</kbd> to
move on. A `<kbd>` is drawn as inline code is, in the same colours, and
between backticks without colours.

### Sub and sup

Water is H<sub>2</sub>O, and x<sup>2</sup> + y<sup>2</sup> = r<sup>2</sup>
on the 2<sup>nd</sup> try. With kitty's text sizing a `<sub>` or `<sup>`
is drawn at two thirds of the size around it, lowered or raised; other
terminals get Unicode's script characters, such as ² and ₂, when every
character has one, and else the text as it is: e<sup>iπ</sup>.

### Marked text

Some text is <mark>marked as found</mark>, as a highlighter would, even
**<mark>in bold</mark>**. With the default theme the marked text is black
on yellow, in your terminal's own palette colours. On the paper themes it
is in greys: black on light grey on `--light`, white on dark grey on
`--dark`. Without colours it is in reverse video.

```sh
kiri --light examples/S01-text.md
kiri --dark examples/S01-text.md
NO_COLOR=1 kiri examples/S01-text.md
```

### Details

A `<details>` block is always shown open, as a browser shows one with the
`open` attribute: its summary is in bold after a triangle, `▼`, and its
content follows, drawn as Markdown, at the same indent as the text around
it.

<details>
<summary>A summary with <kbd>Esc</kbd></summary>

The content is *Markdown*, as GitHub shows it:

- one
- two

</details>

<details>
<summary>Outer</summary>
<details>
<summary>Inner</summary>

Content of the inner one, under its own triangle.

</details>

Content of the outer one, after the inner block.

</details>

One in a list item has its triangle lined up with the item's text, and
one in a quote has the quote's bar beside it:

- A list item
  <details>
  <summary>Inside a list</summary>

  Content in the list.

  </details>

> <details>
> <summary>Inside a quote</summary>
>
> Content in the quote.
>
> </details>

Without a summary, a block is called Details:

<details>

Content of a block that has no summary.

</details>

## Links

The link at the end of this sentence is long enough to wrap onto the next
line: [a link whose text goes on long enough to span two lines in most windows](https://example.com/visual-check).
Every line of it is underlined and opens the same page. In the pager, where
the terminal shows links as hyperlinks (inside tmux, with its hyperlinks
feature, set up in the next paragraph), Tab selects a link on the screen:
this one shows in reverse video on both its lines, and the status line says
where it goes. Enter copies its address to the clipboard, Shift+Tab selects
the one before, and Escape lets it go. The link stays selected while either
of its lines is on the screen.

A link written in two parts of one style change is still one link:
[plain part](https://example.com/parts)**[bold part](https://example.com/parts)**
is underlined as one link, and Tab selects both parts in one step.

Inside tmux the link opens just as it does outside, once tmux.conf has
the line `set -as terminal-features ',xterm-kitty:hyperlinks'` (for
kitty), tmux has read it, and you have detached and attached again.
Without that line the link does not open and its address follows it in
`<…>`, dimmed, as in a terminal without links, and `kiri --doctor`
says on its Hyperlinks row that tmux does not pass links on, with the
line to add.

## Thematic break

A line of three hyphens on its own is a rule. Between this paragraph and
the next one it runs across the whole window, from its left edge to its
right edge, in the theme's rule colour:

---

The rule above has no margin on either side. Inside a quote or a list
item, a rule is as wide as the text there:

> Before the rule.
>
> ---
>
> After the rule, with the quote's bar beside both.

- Before the rule.

  ---

  After the rule, lined up with the item's text.
