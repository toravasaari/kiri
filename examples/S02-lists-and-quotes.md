---
title: Lists and quotes
description: Bullet, numbered and task lists, and block quotes inside them.
tags: [lists, quotes, tasks]
---

# Lists and quotes

Each section says what to look for; anything else is a bug. Themes and
colourless output: [README.md](README.md):

```sh
kiri examples/S02-lists-and-quotes.md
```

The bullets, numbers and quote bars are grey, and they show also in a colour
scheme whose grey is its background, such as Solarized Dark.

## Lists

- ### A heading in a list item

  The heading starts right after the bullet.

  ```
  code in an item
  ```

  The code block lines up with the item's text.

  | Key | Value |
  |-----|-------|
  | b   | 2     |

  > A quote in an item; its bar lines up with the item's text.

1. The first paragraph of a loose item.

   A second paragraph, lined up with the first.

2. The second item.

   - A list nested in a loose item.

Bullets change with the level: `•`, `◦`, then `▪` for every level from the
third on, as on GitHub:

- Level one
  - Level two
    - Level three
      - Level four
        - Level five
          - Level six

With `kiri --fa examples/S02-lists-and-quotes.md`, changing one item and
saving tints only that item for a moment, not the whole list.

## Task lists

A task item shows ☐ for an open task and ☑ for a finished one, in place
of its bullet. A task that wraps continues under its own text, not under
its box. In a numbered list the number stays and the box follows it.

- [x] A finished task, with a checked box
- [ ] An open task, with an empty box
  - [x] A nested task
  - A nested item that is not a task, with a bullet
- A plain item among the tasks
- [ ] A task whose text is long enough to wrap onto a second line in a
  narrow window

1. [x] A numbered task, done
2. [ ] A numbered task, open

## HTML lists

A list written in HTML is drawn as a Markdown list, with the same bullets,
numbers and indents. The outer list has bullets and the list inside it is
numbered from 3. The items Third and Fourth have no `</li>` and look like the
others, and so does the list in the details, which is numbered from 1.

<ul>
  <li>An item with an end tag</li>
  <li>An item with a list inside it
    <ol start="3">
      <li>Third
      <li>Fourth
    </ol>
  </li>
  <li>The last item</li>
</ul>

<details>
<summary>A list in a details</summary>

<ol>
  <li>First</li>
  <li>Second</li>
</ol>

</details>

### Task boxes in HTML

With `html = "all"` in the `[markdown]` table of the configuration file, an
item that starts with a checkbox is a task item, as in the task lists above:
☑ for the ticked box, ☐ for the open one, in place of the bullet, and no tag
is shown. At the default level the boxes are muted tags, as in the source.

<ul>
  <li><input type="checkbox" checked disabled> A finished task</li>
  <li><input type="checkbox" disabled> An open task</li>
  <li>A box after the text <input type="checkbox"> stays a tag</li>
</ul>

## Block quotes

> ### A heading in a quote
>
> The bar runs beside the heading.
>
> ```rust
> let quoted = true;
> ```
>
> The bar continues beside the code block and the blank lines.
>
> | Key | Value |
> |-----|------:|
> | a   | 1     |
> | bb  | 22    |
>
> The table sits after the bar, its values aligned right.
>
> - A list in a quote
>   - A deeper item

> An outer quote.
>
> > A nested quote: two bars, side by side.
> >
> > Its blank line reaches the second bar.
>
> Back in the outer quote, with one bar.
