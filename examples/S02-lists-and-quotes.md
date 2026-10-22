---
title: Lists and quotes
description: Bullet, numbered and task lists, and block quotes inside them.
tags: [lists, quotes, tasks]
---

# Lists and quotes

A document for checking kiri's lists and block quotes by eye in a real kitty
window. Each section says what you should see there with the default theme;
anything else is a bug. How to view it with the themes and without colours
is in [README.md](README.md):

```sh
kiri examples/S02-lists-and-quotes.md
```

## Lists

- ### A heading in a list item

  The heading starts right after the bullet, at normal size.

  ```
  code in an item
  ```

  The code block above lines up with the item's text.

  | Key | Value |
  |-----|-------|
  | b   | 2     |

  > A quote in an item; its bar lines up with the item's text.

1. The first paragraph of a loose item.

   A second paragraph of the same item, after a blank line and lined up
   with the first.

2. The second item, separated from the first by a blank line.

   - A list nested in a loose item.

## Task lists

In a bullet list, a task item, written with a box in square brackets,
shows a box in place of its bullet: an empty box, ☐, for an open task
and a checked one, ☑, for a finished task. A task nested under another
is indented like any other item, and a task that wraps continues under
its own text, not under its box. In a numbered list the number stays and
the box follows it. In plain text output the boxes stay in square
brackets, as written.

- [x] A finished task, with a checked box
- [ ] An open task, with an empty box
  - [x] A nested task
  - A nested item that is not a task, with a bullet
- A plain item among the tasks
- [ ] A task whose text is long enough to wrap onto a second line in a
  narrow window, where it lines up under the words above, not the box

1. [x] A numbered task, done
2. [ ] A numbered task, open

## Block quotes

> ### A heading in a quote
>
> The heading above is normal size, starts right after the bar, and the
> bar runs beside it.
>
> ```rust
> let quoted = true;
> ```
>
> The bar continues beside the code block, and blank lines inside the
> quote end at the bar.
>
> | Key | Value |
> |-----|------:|
> | a   | 1     |
> | bb  | 22    |
>
> The table sits after the bar, with its values aligned right.
>
> - A list in a quote
>   - A deeper item

> An outer quote.
>
> > A nested quote: two bars, side by side.
> >
> > Its blank line ends at the second bar.
>
> Back in the outer quote, with one bar.
