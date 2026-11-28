# HTML lists in the shapes of real documents

Each part below is one shape of an HTML list, in words of our own. A list that
is well formed is drawn as a Markdown list; one that is broken keeps its tags
dim and shows what it holds on its own.

<ul>
  <li>Salt</li>
  <li>Flour</li>
  <li>Water</li>
</ul>

<ol start="3">
  <li>Third, with no end tag
  <li>Fourth, with none either
  <li>Fifth, with one</li>
</ol>

<ul>
  <li>Fruit
    <ul>
      <li>Apples</li>
      <li>Pears</li>
    </ul>
  </li>
  <li>Bread
    <ol>
      <li>Rye</li>
      <li>Oat</li>
    </ol>
  </li>
</ul>

<ul>
  <li><p>A paragraph in an item.</p></li>
  <li><p>Two paragraphs</p><p>in this one.</p></li>
</ul>

<ul>
<li>

An item with a blank line round its text.

Its second paragraph, in Markdown.

</li>
<li>A short item</li>
</ul>

<details>
<summary>A list in a details</summary>

<ol>
  <li><input type="checkbox" checked disabled> Done</li>
  <li><input type="checkbox" disabled> Still to do</li>
</ol>

</details>

<ul>
  <li><input type="checkbox" checked> A ticked task</li>
  <li><input type="checkbox"> An open task</li>
  <li><input type="radio" checked> A radio, which has no effect</li>
  <li>Text, then <input type="checkbox"> a box with no effect</li>
</ul>

- A Markdown item that holds an HTML list:

  <ul>
    <li>First inner</li>
    <li>Second inner</li>
  </ul>

> A quote that holds an HTML list:
>
> <ol>
>   <li>First quoted</li>
>   <li>Second quoted</li>
> </ol>

<ul></ul>

<ul>
  Text that a list may not hold directly.
  <li>An item beside it</li>
</ul>

<div>
  <li>An item with no list round it</li>
</div>

<ul>
  <li>A list that is never closed
  <li>Its second item
