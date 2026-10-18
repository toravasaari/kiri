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

`width` and `height` are CSS pixels, as in a browser. Given one, the image
keeps its shape:

<img src="images/pattern.png" alt="Half width" width="160">

Given both, it takes their shape:

<img src="images/pattern.png" alt="Squashed" width="160" height="40">

A small image is enlarged to the size asked for:

<img src="images/icon.png" alt="Enlarged icon" width="64">

A percentage is of the text width; the text width still bounds the image:

<img src="images/pattern.png" alt="Quarter width" width="25%">

<img src="images/icon.png" alt="Too wide" width="1000" height="50">

A linked image is shown, but cannot be clicked yet:

<p align="center">
  <a href="https://example.com"><img src="images/icon.png" alt="Linked icon"></a>
</p>

In running text, <img src="images/icon.png" alt="Fish &amp; chips"> stays alt
text, as Markdown images do, and a linked
<a href="https://example.com"><img src="images/icon.png" alt="icon"></a> is a link.

A missing file shows its alt text:

<img src="images/no-such-file.png" alt="Missing image">

These stay HTML source: a block with text, a block with two images and an
`<img>` without a `src`.

<p>Text and <img src="images/icon.png" alt="an icon"></p>

<p><img src="images/icon.png" alt="One"><img src="images/icon.png" alt="Two"></p>

<img alt="No source">
