# Inspiration

Ideas from other Markdown tools, kept for reference. Nothing here is
planned: an idea that becomes work moves to the [roadmap](roadmap.md).

## Front matter

Tools in which a document's front matter changes how it is presented.
kiri would read such settings only when its configuration file allows it
(roadmap 4.8.2).

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

## Front matter tables

- [Forgejo](https://forgejo.org/) draws front matter as a table with one
  column per key, as GitHub used to; kiri draws GitHub's present table,
  keys in a header column (roadmap 4.10.4).

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

## Other terminal Markdown readers

kiri stands on its own, but where another familiar tool already does
something well, kiri need not do it differently.

- [mdcat](https://github.com/swsnr/mdcat) printed Markdown to the
  terminal with images in kitty and iTerm2, much as kiri's print mode
  does. It is archived and no longer maintained, so its users look for
  something else; kiri could accept its options where they mean the same
  thing.
- [glow](https://github.com/charmbracelet/glow) is the best known
  terminal Markdown reader. Started without a file, it lists the Markdown
  files of the directory or git repository to choose from, and it can read
  a document from a URL or a GitHub repository. Its pager keys are those
  of `less`.
- [kitty-markdown-viewer](https://github.com/calii23/kitty-markdown-viewer)
  (`mdv`) is a full-screen viewer for kitty. A table of contents beside
  the text follows the reading position, links show their target when
  the mouse is over them, and a selection made with the mouse is copied
  as its Markdown source.

## Other formats through pandoc

[Pandoc](https://pandoc.org/) reads dozens of formats, among them
reStructuredText, AsciiDoc, Org, LaTeX, HTML, docx and EPUB, and
`pandoc -t json` writes any of them as one syntax tree. A converter from
that tree into kiri's document model would show them all, with pandoc as
an optional external program, as mermaid-cli is for diagrams.
