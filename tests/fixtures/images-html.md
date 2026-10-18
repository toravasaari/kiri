# HTML images

An `<img>` tag on a line of its own is shown like a Markdown image:

<img src="images/icon.png" alt="Icon">

So is an image wrapped the way READMEs centre a logo; kiri does not centre
it:

<p align="center">
  <img src="images/pattern.png" alt="Test pattern">
</p>

A `<picture>` shows its `<img>`, whatever its sources say:

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="images/portrait.png">
  <img src="images/transparent.png" alt="Transparent in a picture">
</picture>

In running text, <img src="images/icon.png" alt="Fish &amp; chips"> stays alt
text, as Markdown images do.

A missing file shows its alt text:

<img src="images/no-such-file.png" alt="Missing image">

These stay HTML source: a block with text, a block with two images and an
`<img>` without a `src`.

<p>Text and <img src="images/icon.png" alt="an icon"></p>

<p><img src="images/icon.png" alt="One"><img src="images/icon.png" alt="Two"></p>

<img alt="No source">
