# Lists and quotes

A document for checking kiri's lists and block quotes by eye in a real kitty
window. Each section says what you should see there with the default theme;
anything else is a bug. How to view it with the themes and without colours
is in [README.md](README.md):

```sh
cargo run -- examples/S02-lists-and-quotes.md
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
