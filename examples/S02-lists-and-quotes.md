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
