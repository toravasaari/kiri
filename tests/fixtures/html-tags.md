# HTML tags

## Line breaks

First line<br>second line<br/>third line<br />fourth line<BR>fifth.

## Keys and marks

Press <kbd>Ctrl</kbd>+<kbd>C</kbd> to copy, or <kbd>Page Down</kbd> to
move on.

Some text is <mark>marked as found</mark>, even **<mark>in bold</mark>**.

A <kbd>key that never closes stays as source.

## Scripts: x<sup>2</sup> and H<sub>2</sub>O

Water is H<sub>2</sub>O, x<sup>2</sup> + y<sup>2</sup> = r<sup>n+1</sup> on
the 2<sup>nd</sup> try, and x<sup>a<sub>i</sub></sup> keeps one size. Unicode
has no script characters for π or for code, so e<sup>iπ</sup> and
a<sub>`b`</sub> have none.

## Details

<details>
<summary>A summary with <kbd>Esc</kbd></summary>

The content is *Markdown*, as GitHub shows it:

- one
- two

</details>

<details><summary>Written on one line</summary>The content &amp; its
<b>tags</b> are plain text.</details>

<details>

Without a summary, the block is called Details.

</details>

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

<details>
<summary>Outer, no blank line before the inner</summary>
<details>
<summary>Inner</summary>

Content of the inner one.

</details>

Content of the outer one.

</details>

<details>
<summary>With an HTML paragraph</summary>
<p>
The paragraph's text, read as plain text.

</p>
</details>

<details>
<summary>Text right after the end</summary>

Content.

</details>
Text after the details, in the same HTML block.

## Bold, italic and code

Some <b>bold</b> and <strong>strong</strong> text, <i>italic</i> and
<em>emphasis</em>, <s>struck</s> and <del>deleted</del>, and
<code>code  with  spaces</code> in running text.

Nested <b><i>bold italic</i></b>, and **Markdown <em>around</em> HTML**.

A lone <b>bold never closes, a <b>mismatched</strong> pair and
<code>code with <b>bold</b> inside</code> stay as source.

<details>
<summary>A <b>bold</b> and <code>code</code> summary</summary>

Body of the details.

</details>

## Entities

<details><summary>Fish &amp; chips &mdash; 5&nbsp;&euro; &foo;</summary>One line &mdash; &copy; &hellip; &amp;mdash; &Mdash;</details>

## Other tags

Text with <span>a span</span> and <u>an underline</u> stays as source.

<div>
A div block stays as source.
</div>

<details>
<summary>Never closed</summary>

Runs to the end of the document.
