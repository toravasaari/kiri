# Inspiration

Ideas from other Markdown tools, kept for reference. Nothing here is
planned: an idea that becomes work moves to the [roadmap](roadmap.md).

## Front matter

Tools in which a document's front matter changes how it is presented.
kiri would read such settings only when its configuration file allows it
(roadmap, Uncategorized).

- [Marp](https://github.com/marp-team/marp-core/blob/main/docs/markdown.md):
  `math: katex` or `math: mathjax` chooses the math library for the
  document ("Choose a math library").
- [Quarto](https://quarto.org/docs/output-formats/html-basics.html):
  `html-math-method` chooses how formulas are drawn and `toc` adds a
  table of contents, both under `format: html:`.
  [Execution options](https://quarto.org/docs/computations/execution-options.html)
  such as `execute: echo: false` apply to the whole document there and to
  one code block in the block.
- [Pandoc](https://pandoc.org/MANUAL.html#metadata-blocks): variables such
  as `lang` and `mainfont` come from the document's metadata block. A value
  on the command line overrides the document's, the order kiri follows.
- [R Markdown](https://pkg.yihui.org/rmarkdown-book/html-document.html):
  output options under `output: html_document:`, such as `toc: true`.
- [Hugo](https://gohugo.io/content-management/mathematics/): `math: true`
  in a page's front matter loads math for that page only.
- [MkDocs Material](https://squidfunk.github.io/mkdocs-material/setup/setting-up-navigation/):
  `hide: [navigation, toc]` hides the sidebars of one page.
- [Docusaurus](https://docusaurus.io/docs/api/plugins/@docusaurus/plugin-content-docs):
  `hide_table_of_contents` in a document's front matter.
- [Obsidian](https://obsidian.md/help/properties): `cssclasses` styles one
  note with CSS snippets.

## Known Markdown processors

- [Marp](https://github.com/marp-team/marpit/blob/main/docs/directives.md)
  makes slide decks from Markdown. A deck is split into slides at `---`,
  directives such as `theme`, `paginate`, `header` and `footer` go in the
  front matter or in HTML comments, and `![bg](image.jpg)` sets a slide's
  background. Themes are CSS, which kiri cannot follow, but a part of the
  rest could be a presentation mode in the pager: one slide a screen,
  headings large through the text sizing protocol, and the page number on
  the status line.
- [presenterm](https://github.com/mfontanini/presenterm) shows Markdown
  slides in the terminal. It is written in Rust, shows images in kitty and
  other terminals, and draws LaTeX and Typst formulas and Mermaid and d2
  diagrams.
- [Quarto](https://quarto.org/docs/computations/execution-options.html)
  runs code blocks whose language is in braces, such as ` ```{python} `.
  kiri shows such a block without highlighting today, with `{python}` as
  its language; reading it as `python` would highlight it. Running the
  code is another matter, one for trust as in renderers from the
  configuration file (roadmap 4.3.3).
