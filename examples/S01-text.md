---
title: Text
description: Headings, emphasis, links, footnotes, wide characters, emoji and HTML tags.
tags: [text, headings, links, emoji]
---

# Text

Each section says what to look for; anything else is a bug. Themes and
colourless output: [README.md](README.md):

```sh
kiri examples/S01-text.md
```

## Heading levels

Six levels. Levels one to three are larger than the text, at 2, 1.5 and
1.25 times its size, and levels one and two have a line under them. Levels
four to six are at the normal size: four and five in the text's colour,
six in grey.

# Level one

## Level two

### Level three

#### Level four

##### Level five

###### Level six

### Headings with `code`, <kbd>Ctrl</kbd> and *emphasis*

Code, a key and emphasis in a heading look as they do in the text, at
the heading's size.

## Headings at the edge of the screen

With `--no-reading` (or `M`), scroll through this document a line at a
time (`j`, then `k`). A heading that does not fit the rows left at the
bottom of the screen is drawn one size smaller there, two rows for a
heading of three, one row for a heading of two, instead of leaving a gap,
and grows to its size when the line above it scrolls off. The status line
stays clean the whole way. In reading mode the headings are pictures, and
they are shown whole or not at all.

## Words in headings

No gap shows inside a word of these headings, long ones such as
"documentation" included. The spare pixels go between the words, so some
spaces are a little wider than others.

# Several words in a heading at level one

### Several words in a heading at level three

#### Several words in a heading at level four

##### Several words in a heading at level five

### Documentation for configuration

## Emphasis and strikethrough

A paragraph with *emphasis*, **strong text**, ***both at once*** and
~~strikethrough~~. `NO_COLOR` keeps them.

## Empty blocks

An empty heading follows. It shows only its line, with no extra blank
line.

#

Then an empty code block, a two-row box as wide as the text.

```
```

In the list below, the second item shows its bullet and nothing after it.

- First item
-
- Third item

## Wide characters

CJK, emoji, flags and joined emoji take two cells and never split:
日本語のテキストと漢字, 🦀, 🇫🇮 🇯🇵, 👨‍👩‍👧 and ❤️. Lines with them end inside
the window.

### 日本語の見出し 🦀 🇫🇮

It is as large as the other level-three headings.

```
let 名前 = "東京"; // 🦀 🇫🇮 👨‍👩‍👧
```

## Emoji shortcodes

A shortcode between colons becomes its emoji, as on GitHub: :rocket:,
:heart:, :coffee: and :tada:. The same goes in a heading and in a table
cell:

### Shipped :rocket:

| Mood | Emoji | Note |
|------|:-----:|------|
| Love | :heart: | the borders stay in line |
| Fire | :fire: | the borders stay in line |

An unknown code stays as written: :not_a_real_code:. So does one in
inline code, `:rocket:`, and in a code block:

```
echo ":rocket:"
```

`kiri --list-emojis` lists the codes it knows, as a table in a terminal.

## HTML tags

A few HTML tags are drawn as GitHub shows them. Others, such as `<abbr>`,
stay as source in a muted colour. A `<span>` draws nothing of its own, so
what is in it is read as if its tags were not there: a <span>span, with
**bold**</span> in a line.

### Line breaks

A `<br>` ends the line, with or without a slash: First line<br>second
line<br/>third line.

### Keys

Press <kbd>Ctrl</kbd>+<kbd>C</kbd> to copy, or <kbd>Page Down</kbd> to
move on. A `<kbd>` looks like inline code.

### Bold, italic and code

Some <b>bold</b> and <strong>strong</strong> text, <i>italic</i> and
<em>emphasis</em>, <s>struck</s>, <strike>crossed</strike> and
<del>deleted</del> words, and <code>code</code> in running text look like
their Markdown forms, and they nest, as in <b><i>bold italic</i></b>. An <ins>inserted</ins> word is
underlined, as a browser draws it. These stay as source in the muted
colour: a tag that never closes, as in <b>this one, the tags of a `<code>`
with other tags inside, as in <code>a <b>b</b></code>, and `<u>`, which
GitHub does not show: <u>underlined</u>. With `html = "all"` in the
`[markdown]` table of the configuration file, the `<u>` is underlined.

### Entities

Named entities in a one-line `<details>` show as their characters. The
summary reads "Fish & chips — 5 €", and the line after it "One line — © …
&mdash; &Mdash;": `&amp;mdash;` shows as written, and so does an unknown
or wrongly cased name such as `&Mdash;`.

<details><summary>Fish &amp; chips &mdash; 5&nbsp;&euro;</summary>One line &mdash; &copy; &hellip; &amp;mdash; &Mdash;</details>

### HTML headings

A block of one `<h1>` to `<h6>` is drawn as the Markdown heading of that
level: a level 3, a level 4 and a level 2 below. Attributes are ignored,
except that `align="center"` centres the heading. A `<h7>` stays as source.

<h3>An HTML heading</h3>

<h4 align="center">With an <code>align</code>, drawn centred</h4>

<h2>The <code>kiri</code> &mdash; <i>tool</i></h2>

<h7>No such heading stays as source</h7>

### Centred heads

A title and a row of links in a `<div align="center">`, `<p align="center">`
or `<center>` are drawn in the middle of the line, as a README shows its head.
A list, a quote or a table in such a wrapper is drawn where it is, not
centred. GitHub does not show `<center>`, so at the default level its two
tags show in the muted colour and what is in it is not centred; the
`<center>` examples below are centred with `html = "all"` in the `[markdown]`
table of the configuration file.

<div align="center">
  <h3>A centred title</h3>
  <a href="https://example.com/docs">Docs</a> &middot;
  <a href="https://example.com/issues">Issues</a>
</div>

<div align="center">

A line in a wrapper set apart by blank lines is centred, and its list is not:

- the first item
- the second item

</div>

A Markdown paragraph ends an open `<p>`, as in a browser. So a `<p>` with
`align="center"` round blank lines and Markdown centres nothing, and its two
tags show in the muted colour. In a `<div align="center">`, as above, the same
lines are centred. Nothing below is centred:

<p align="center">

A line of Markdown, not centred.

</p>

A `<p align="center">` whose `</p>` is left out ends where a browser ends
it, at the next `<p>`, `<div>`, `<center>` or `<details>`, and what it
holds so far is centred. Both lines below are centred:

<p align="center">A centred line, its end tag left out.
<p align="center">The second line.

A `<details>` ends such a `<p>` as well: the line below is centred, and
the details after it is drawn at the left.

<p align="center">A centred line before a details.
<details>
<summary>Not centred</summary>

The details and what it holds are not centred.

</details>

A `<center>` in a `<center>`, with text beside its start tag, is read as a
browser reads it: with `html = "all"` all three lines below are centred.

<center>

<center>Text beside the tag

A centred line inside both.

</center>

A centred line after the inner end tag.

</center>

A centred element right after an end tag in the same HTML block, or in a
`<details>` of one line, is centred as it is on a line of its own:

<center>

A centred line.

</center>
<p align="center">The next centred line, right after the end tag.</p>

<details><summary>A details of one line</summary><p align="center">Centred inside it.</p></details>

### Plain wrappers and broken tags

A `<div>` or a `<p>` that centres nothing draws nothing of its own: what is
in it is read as if the tags were not there. Each of the two lines below is
a paragraph at the left, without a tag to be seen.

<div>
A line in a plain div.
</div>

<p>A line in a plain p.</p>

A tag that is never closed, or that holds what it may not, has no effect, and
it shows in the muted colour; what it holds is read as it would be without
it. The heading below is never closed, so its `<h2>` shows muted and the
words after it are a paragraph, not a heading:

<h2>A heading never closed

A `<details>` that is never closed is broken in the same way. Below, its
`<details>` and `<summary>` show muted, and the paragraph after them is not
inside a details:

<details>
<summary>Never closed</summary>

A paragraph after it, at the left.

### Tags with no effect

A tag that kiri does not know, such as `<abbr>`, and one that is outside the
level, such as `<center>` at the default level, show in the muted colour,
and what they hold is read as if they were not there: in this line the
<abbr title="Hypertext Markup Language">HTML</abbr> shows its two tags muted
and the word between them as the text round it. A stray end tag between
blocks is a muted line of its own:

</div>

With `html_tags = "hide"` in the `[markdown]` table of the configuration
file the tags are left out, and so is the muted line above. Source that
kiri shows as written, such as a `<table>`, is never left out.

### Sub and sup

Water is H<sub>2</sub>O, and x<sup>2</sup> + y<sup>2</sup> = r<sup>2</sup>
on the 2<sup>nd</sup> try. They are drawn smaller, lowered or raised. A
terminal that cannot do that gets Unicode's script characters, such as ²
and ₂, when every character has one, and else the text as it is:
e<sup>iπ</sup>. A superscript right before a subscript, as in
x<sup>2</sup><sub>i</sub>, keeps each at its own height, and struck, as in
~~x<sup>2</sup><sub>i</sub>~~, the line follows each of them.

### Marked text

Some text is <mark>marked as found</mark>, as a highlighter would, even
**<mark>in bold</mark>**: black on yellow by default, greys on the paper
themes, reverse video without colours.

```sh
kiri --light examples/S01-text.md
kiri --dark examples/S01-text.md
NO_COLOR=1 kiri examples/S01-text.md
```

### Details

A `<details>` block is always open: the summary in bold after a triangle,
`▼`, then the content as Markdown at the text's indent.

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

In a list item its triangle lines up with the item's text, and in a quote
the quote's bar is beside it:

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

Without a summary it is called Details:

<details>

Content of a block that has no summary.

</details>

## Links

A long link wraps onto the next line:
[a link whose text goes on long enough to span two lines in most windows](https://example.com/visual-check).
Every line of it is underlined. In the pager, Tab selects the first link
on the screen and goes on to the next, round at the end, scrolling to show
it; it shows in reverse video on both its lines, and the status line says
where it goes. Enter copies the address, Shift+Tab selects the one before
and Escape lets go. The selection stays through a reload with `r`.

Two links to one address that touch are one link, even when the style
changes between them:
[plain part](https://example.com/parts)**[bold part](https://example.com/parts)**
is underlined as one, and Tab selects both parts in one step, also when
a line breaks between them.

Inside tmux, links open once tmux.conf has
`set -as terminal-features ',xterm-kitty:hyperlinks'`. Without it the
address follows the link in dimmed `<…>`, and `kiri --doctor` says so on
its Hyperlinks row.

### Links to a place in the document

[The thematic break below](#thematic-break) links to a heading, written as
GitHub writes it. Tab selects it and Enter takes you there, with the
heading on the top line; a click does the same. Backspace comes back, also
after a reload, and says `nothing to go back to` when there is nowhere to
go. [This one](#no-such-heading) says `no #no-such-heading here` and the
view stays.

With several files, a link such as `[the other file](other.md#heading)`
jumps to the file when both are given to kiri.

### Links to another Markdown file

Start this document alone, with `kiri examples/S01-text.md`. Then [the lists
and quotes example](S02-lists-and-quotes.md) opens over this document with
Tab and Enter, with its name on the status line. Backspace comes back to
the place you left, and Escape does not close the file. With a second view
open, the status line shows the path `S01-text.md › S02-lists-and-quotes.md`,
the current name in bold; a click on an earlier name goes back to that
view, and in a narrow window the start of the path becomes `…`.
[The block quotes of the same file](S02-lists-and-quotes.md#block-quotes)
opens with that heading on the top line. [A missing file](no-such-file.md)
says `cannot open examples/no-such-file.md` on the status line and nothing
opens. A link to a file that is not Markdown, such as [the pattern
image](S06b-pattern.png), is copied as an address away is.

A link can have a title: [a link with a title](https://example.com/titled "The title of this link").
Tab shows the title on the status line instead of the address; Enter
still copies the address.

## Footnotes

A footnote is a small raised number in the text.[^first] Tab selects it and
Enter takes you to the note, which is listed at the end of this document
below a rule, as on GitHub, with its number dimmed in brackets, `[1]`.
Backspace comes back. The note ends in `↩`, a link to the reference, and a
note that has two references[^second] gets `↩` and `↩²`, one to each; this
one[^second] is the second. The notes are numbered as they are first
referred to, whatever order they are written in. A note of several
paragraphs and a code block is indented under its number, with the links
back after the last line.

## Thematic break

A line of three hyphens is a rule. Here it is as wide as the text, like the line
under a level one or two heading:

---

In a quote or a list item it is as wide as the text there:

> Before the rule.
>
> ---
>
> After the rule, with the quote's bar beside both.

- Before the rule.

  ---

  After the rule, lined up with the item's text.

[^second]: A note with two paragraphs and a block of code.

    The second paragraph is indented under the first.

    ```
    code in a note
    ```

[^first]: A short note.
