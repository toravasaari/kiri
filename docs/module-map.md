# Module map

This page is made from the `//!` documents of `src/` by `scripts/module-map.py`. Do not edit it: change the document of the file and run `make module-map`. A file that is only test code has no row; it is named as "Tests:" on the row of the file that declares it.

## The typesetting engine

### [`src/typeset/`](../src/typeset/mod.rs)

The typesetting engine: the document model, parsing, themes, the look, both layout engines and the images and diagrams drawn from given data. Tests: `measured.rs`, `measured/`, `parity.rs`, `parity/`.

| File | What it does |
|---|---|
| [`align.rs`](../src/typeset/align.rs) | `Align`: the one way from a place in one version of a document to the place in the next, worked out from how the two versions compared. |
| [`anchors.rs`](../src/typeset/anchors.rs) | The ids of headings, as GitHub makes them, so that `[a](#installation)` goes where it does there. |
| [`block.rs`](../src/typeset/block.rs) | The table of the blocks of the shown documents, which tells each laid-out line the block it comes from. |
| [`caps.rs`](../src/typeset/caps.rs) | What the output terminal can do, as plain data: the renderer and the pager take it as input, and `term` finds it out. |
| [`color.rs`](../src/typeset/color.rs) | Colours and underline styles: the values the theme and the renderers share, and the page's colours as RGB, which mixing them needs. |
| [`compare.rs`](../src/typeset/compare.rs) | The comparison of two versions of a document: which blocks of one stayed, changed, were added or were removed in the other. Tests: `tests.rs`. |
| [`contents.rs`](../src/typeset/contents.rs) | The contents of the shown documents: their headings as a flat list, and the heading the reader is in. Tests: `tests.rs`. |
| [`diagnostic.rs`](../src/typeset/diagnostic.rs) | Diagnostics: what kiri found wrong while it read, loaded or drew a document, and tells the reader without stopping. |
| [`driver.rs`](../src/typeset/driver.rs) | The driver of the layout: it walks what is to be laid out, one document or several files, and gives back the lines and the table of their blocks. Tests: `tests.rs`. |
| [`facts.rs`](../src/typeset/facts.rs) | What kiri knows of particular terminals by their name, in one table beside the capabilities. |
| [`fonts.rs`](../src/typeset/fonts.rs) | The one font database that text is drawn with, in SVG images and in reading mode, and the lists of the families kiri falls back to. |
| [`grid.rs`](../src/typeset/grid.rs) | Rows of cells, as a table or a front matter's map has them, built from the document model for both layouts. |
| [`input.rs`](../src/typeset/input.rs) | What a file given on the command line is: a Markdown document, a CSV or TSV table, an image or diagram to show on its own, or other text to show as it is. |
| [`inputs.rs`](../src/typeset/inputs.rs) | `Inputs`: the options that a layout takes besides the document, such as the images, the capabilities and the theme. Tests: `tests.rs`. |
| [`laid.rs`](../src/typeset/laid.rs) | `Laid`, a layout and the questions that are asked of it. Tests: `tests.rs`. |
| [`layers.rs`](../src/typeset/layers.rs) | Highlights over the lines of a layout: what the state engine wants shown on a range of the document, and how both layouts paint it. |
| [`loading.rs`](../src/typeset/loading.rs) | The pixels of what stands in for an image while it loads: the image the block showed before, dimmed, and a thin moving band below it. |
| [`locate.rs`](../src/typeset/locate.rs) | Where a stretch of the document is shown, and what is at a cell of the screen. Tests: `tests.rs`. |
| [`model.rs`](../src/typeset/model.rs) | Document model shared by all input formats and backends. |
| [`position.rs`](../src/typeset/position.rs) | A place in a document: which file, which block, and where in the block. |
| [`rendition.rs`](../src/typeset/rendition.rs) | The renditions of an embed: the ways it can be shown, in the order it prefers them, which of them a terminal shows, and the choice between them. |
| [`size.rs`](../src/typeset/size.rs) | Text sizes: the values the theme and the renderers share, as kitty's text sizing protocol takes them. Tests: `tests.rs`. |
| [`source_rows.rs`](../src/typeset/source_rows.rs) | `SourceRows`: the rows of a CSV or TSV file as a table of the document model holds them, with the source held once and where each row starts. |
| [`stretch.rs`](../src/typeset/stretch.rs) | Where a large block is cut: its lines in stretches, each a range of whole lines that the layout lays out on its own. |
| [`style.rs`](../src/typeset/style.rs) | Styled text: the unit between layout and output. |
| [`targets.rs`](../src/typeset/targets.rs) | What a document has that can be followed or drawn: its links and its embeds, as stretches of its text. Tests: `tests.rs`. |
| [`text.rs`](../src/typeset/text.rs) | What a document says, as text: the texts of each block, and the text of the shown documents as a whole. Tests: `props.rs`, `tests.rs`. |
| [`walk.rs`](../src/typeset/walk.rs) | The walk over the pieces of a file, as a place that can be kept, so that a walk that stops can go on where it stopped. |
| [`wrap.rs`](../src/typeset/wrap.rs) | Word wrapping for styled text. Tests: `props.rs`, `tests.rs`. |

### [`src/typeset/diagrams/`](../src/typeset/diagrams/mod.rs)

The diagram languages that kiri draws itself: each has a file here, and one entry in `language.rs`.

| File | What it does |
|---|---|
| [`error.rs`](../src/typeset/diagrams/error.rs) | `DiagramError`: why a diagram could not be drawn, such as `no output file` or a renderer's own message. |
| [`language.rs`](../src/typeset/diagrams/language.rs) | The languages kiri draws diagrams in, and what it knows about each: plain data, with a function where languages differ in what they do. |
| [`math.rs`](../src/typeset/diagrams/math.rs) | Formulas in LaTeX, as ` ```math ` blocks and `$$…$$` give them, drawn by RaTeX inside kiri. |
| [`names.rs`](../src/typeset/diagrams/names.rs) | What kiri says and knows about a diagram language by its name: its theme names, how a message calls it, and the page its source chooses. |
| [`output.rs`](../src/typeset/diagrams/output.rs) | `Output`: what an external command writes, which a language's configuration file's text depends on. |
| [`renderer.rs`](../src/typeset/diagrams/renderer.rs) | The `Renderer` trait, the renderers built into kiri, and the stand-in for no renderer at all. |
| [`request.rs`](../src/typeset/diagrams/request.rs) | What a diagram renderer is spoken to in: the page's theme, a request to draw, and the drawing that comes back. |

### [`src/typeset/diagrams/mermaid/`](../src/typeset/diagrams/mermaid/mod.rs)

What kiri knows about Mermaid, whichever renderer draws it: its directives and front matter, the settings kiri gives a diagram, and mermaid-cli's command line.

| File | What it does |
|---|---|
| [`merman.rs`](../src/typeset/diagrams/mermaid/merman.rs) | Mermaid diagrams, drawn by merman inside kiri. |

### [`src/typeset/images/`](../src/typeset/images/mod.rs)

The pure parts of loading images, with the store of what was loaded.

| File | What it does |
|---|---|
| [`animation.rs`](../src/typeset/images/animation.rs) | The frames of an animated GIF, WebP or APNG, composed to full frames. Tests: `tests.rs`. |
| [`build.rs`](../src/typeset/images/build.rs) | Building the frames of an animated picture, off the pager's loop: a pure function over owned inputs, so that a thread of its own can run it. Tests: `tests.rs`. |
| [`decode.rs`](../src/typeset/images/decode.rs) | Decoding image files, raster and SVG, each within a size limit. Tests: `tests.rs`. |
| [`drawn.rs`](../src/typeset/images/drawn.rs) | Diagrams made into images as their renderer drew them. Tests: `tests.rs`. |
| [`error.rs`](../src/typeset/images/error.rs) | `ImageError`: why an image could not be loaded, in words for a warning. |
| [`heading.rs`](../src/typeset/images/heading.rs) | Headings drawn as images of their text, for a terminal that draws images but cannot draw large text. Tests: `tests.rs`. |
| [`key.rs`](../src/typeset/images/key.rs) | `ImageKey`: what an image is made from, the key into `Images`. |
| [`marks.rs`](../src/typeset/images/marks.rs) | The marks drawn into an animated picture: a play button over its first frame when stopped, and a replay icon in the corner of its last frame once it has played. Tests: `tests.rs`. |
| [`options.rs`](../src/typeset/images/options.rs) | How the images of a document are loaded, and its diagrams drawn, and the documents shown together whose images are loaded with each other's. |
| [`plan.rs`](../src/typeset/images/plan.rs) | The plan of which images a document wants: its lone images, the code blocks a renderer draws, and, when asked for, headings and images in text. Tests: `tests.rs`. |
| [`size.rs`](../src/typeset/images/size.rs) | How large an image is shown, in pixels and in terminal cells. Tests: `props.rs`, `tests.rs`. |
| [`store.rs`](../src/typeset/images/store.rs) | The store of loaded images that layout reads: every image of a document by its key, loaded or not, with what stands in for the ones still loading. |
| [`svg.rs`](../src/typeset/images/svg.rs) | SVG images: parsing, and drawing at the size they are shown. |
| [`url.rs`](../src/typeset/images/url.rs) | What an image URL means: the bytes in a `data:` URL, the local file that a `file:` URL or a path names, and its scheme. |
| [`warnings.rs`](../src/typeset/images/warnings.rs) | The warnings about images and diagrams that did not load: the `Diagnostic`s made from an `ImageKey` and an `ImageError`. |

### [`src/typeset/layout/`](../src/typeset/layout/mod.rs)

Document model to styled lines of a fixed width. Tests: `summary.rs`, `tests.rs`.

| File | What it does |
|---|---|
| [`centred.rs`](../src/typeset/layout/centred.rs) | A centred wrapper such as `<p align="center">` in the cell layout: its paragraphs, headings and pictures centred in its room. |
| [`code.rs`](../src/typeset/layout/code.rs) | Code blocks and the colours code takes from its highlighting theme. |
| [`embed.rs`](../src/typeset/layout/embed.rs) | Images and formulas inside running text: made into spans that are the cells of an image, and placed in the lines the wrapper made of them. |
| [`front_matter.rs`](../src/typeset/layout/front_matter.rs) | Front matter, as a table of its keys and values or as a code block. |
| [`header.rs`](../src/typeset/layout/header.rs) | The header of a file among several: its rows, and the one row of it that stays at the top of the pager when the rows are scrolled past. |
| [`highlight.rs`](../src/typeset/layout/highlight.rs) | Syntax highlighting for code blocks, with the grammars of two-face and the themes of syntect. |
| [`image_block.rs`](../src/typeset/layout/image_block.rs) | An image block: a paragraph of only an image, or of only a link around one, laid out as the rendition its offers resolve to. |
| [`image_file.rs`](../src/typeset/layout/image_file.rs) | An image file shown on its own, as one of several files is. |
| [`inlines.rs`](../src/typeset/layout/inlines.rs) | The layout of inline content: text, emphasis, code, links, scripts, and the images and formulas that are drawn in the text. Tests: `tests.rs`. |
| [`layers.rs`](../src/typeset/layout/layers.rs) | The text layout's painter for the state engine's highlights (`typeset/layers.rs`). Tests: `tests.rs`. |
| [`list.rs`](../src/typeset/layout/list.rs) | Bullet, numbered and task lists. |
| [`quote.rs`](../src/typeset/layout/quote.rs) | Block quotes and GitHub alerts. |
| [`rows.rs`](../src/typeset/layout/rows.rs) | Long tables in the pager's cell layout, laid out a stretch of rows at a time. |
| [`script.rs`](../src/typeset/layout/script.rs) | Unicode's superscript and subscript characters, for `<sup>` and `<sub>` where text cannot be drawn smaller. |
| [`table.rs`](../src/typeset/layout/table.rs) | Tables drawn with box-drawing characters, or stacked when too narrow. Tests: `tests.rs`. |

### `src/typeset/layout/highlight/`

| File | What it does |
|---|---|
| [`languages.rs`](../src/typeset/layout/highlight/languages.rs) | The names a fence may give a language, and the grammar each one gets. Tests: `tests.rs`. |

### [`src/typeset/parse/`](../src/typeset/parse/mod.rs)

Markdown source to document model, through comrak's syntax tree. Tests: `inline_places.rs`, `tests.rs`.

| File | What it does |
|---|---|
| [`csv.rs`](../src/typeset/parse/csv.rs) | CSV and TSV source to document model: one table, the first row as its header, as GitHub shows such a file. Tests: `grown.rs`, `props.rs`, `tests.rs`. |
| [`footnotes.rs`](../src/typeset/parse/footnotes.rs) | Footnotes: a reference in the text and the notes after the document, built from the anchors, links and lists that the document model already has. Tests: `tests.rs`. |
| [`front_matter.rs`](../src/typeset/parse/front_matter.rs) | Front matter: a block of metadata at the very start of a file, between two delimiter lines, as Jekyll, Hugo and GitLab read it. Tests: `tests.rs`. |
| [`html.rs`](../src/typeset/parse/html.rs) | The few HTML tags in Markdown that kiri reads instead of showing them as source. Tests: `tests.rs`. |
| [`unknown.rs`](../src/typeset/parse/unknown.rs) | Markdown elements that the parser leaves out of the document. |
| [`wrappers.rs`](../src/typeset/parse/wrappers.rs) | The HTML elements that wrap blocks of Markdown, joined into one block that holds the blocks between their tags. Tests: `tests.rs`. |

### `src/typeset/parse/html/`

| File | What it does |
|---|---|
| [`wrapper.rs`](../src/typeset/parse/html/wrapper.rs) | The HTML that centres what it holds, `<p>` and `<div>` with `align="center"`, `<center>` and a heading with it, read whole or as a tag alone. Tests: `tests.rs`. |

### [`src/typeset/reading/`](../src/typeset/reading/mod.rs)

Reading mode: the document's text laid out in a proportional font and drawn as pixels, as a browser shows it.

| File | What it does |
|---|---|
| [`animated.rs`](../src/typeset/reading/animated.rs) | Animated pictures in reading mode: a hole on the page where a picture goes, carrying its id, and the drawing of parts with the pictures in their states. |
| [`colr.rs`](../src/typeset/reading/colr.rs) | Colour glyphs, as emoji fonts such as Noto Color Emoji draw them with the COLR table, painted with tiny-skia. |
| [`engine.rs`](../src/typeset/reading/engine.rs) | Reading mode's pages shown in the terminal, through kitty's Unicode placeholders. Tests: `tests.rs`. |
| [`fonts.rs`](../src/typeset/reading/fonts.rs) | The fonts reading mode lays text out in: the shared fontdb database that SVG images use (`typeset::fonts::Fonts`), handed to parley. |
| [`gallery.rs`](../src/typeset/reading/gallery.rs) | The pictures of a page in reading mode, found in the images that were loaded for the document. |
| [`glyph_cache.rs`](../src/typeset/reading/glyph_cache.rs) | Glyphs drawn once, in a cache, since drawing a glyph's outline is most of what drawing text costs and the same glyph comes again and again. Tests: `tests.rs`. |
| [`inlines.rs`](../src/typeset/reading/inlines.rs) | The inline layout of reading mode: the runs of styled text that a paragraph's inlines make, with the pictures that stand in them. |
| [`layers.rs`](../src/typeset/reading/layers.rs) | Reading mode's painter for the state engine's highlights: an overlay on the page, made of the boxes of the matches and the frame of the selected link. |
| [`look.rs`](../src/typeset/reading/look.rs) | Reading mode's brushes: the one look, resolved into the colours the page is drawn with (`page_look`). |
| [`octicons.rs`](../src/typeset/reading/octicons.rs) | The icons GitHub shows on its alerts, drawn as pictures: reading mode's text has no symbols font to take them from. |
| [`page.rs`](../src/typeset/reading/page.rs) | A run of a document's text blocks laid out as a page, as GitHub sets them, with their margins collapsing between blocks as in CSS. Tests: `finds.rs`, `summary.rs`, `tests.rs`. |
| [`paint.rs`](../src/typeset/reading/paint.rs) | Laid-out text drawn as pixels: its glyphs' outlines, through skrifa, filled with tiny-skia. Tests: `tests.rs`. |
| [`parts_draw.rs`](../src/typeset/reading/parts_draw.rs) | Drawing the parts of a reading-mode page: the pixels of a part, and of a slice of a band, through the glyphs every part shares. |
| [`pen.rs`](../src/typeset/reading/pen.rs) | Reading mode's pen for the driver, which holds the pieces that reading mode lays out as one stream of pixels. |
| [`pixels.rs`](../src/typeset/reading/pixels.rs) | The pixels of a picture on a reading-mode page, premultiplied once when the picture is made, so that drawing it is a blit and not a copy. |
| [`places.rs`](../src/typeset/reading/places.rs) | Which block and which text a row of a reading page shows, and what of the document each text on the page shows. Tests: `tests.rs`. |
| [`text.rs`](../src/typeset/reading/text.rs) | Text laid out in a proportional font with parley: runs of styled text in, lines with their baselines and the boxes of any part of the text out. Tests: `tests.rs`. |

### `src/typeset/reading/page/`

| File | What it does |
|---|---|
| [`centred.rs`](../src/typeset/reading/page/centred.rs) | A centred wrapper such as `<p align="center">` in reading mode's page: its paragraphs, headings and pictures centred in its room. |
| [`code.rs`](../src/typeset/reading/page/code.rs) | Code blocks and diagrams in reading mode's page, and the bands that a long one is cut into. |
| [`details.rs`](../src/typeset/reading/page/details.rs) | `<details>` blocks and their summary in reading mode's page: the summary, and the blocks under it. |
| [`front_matter.rs`](../src/typeset/reading/page/front_matter.rs) | Front matter in reading mode's page: its keys and values as a grid, with the source in a code block when it is shown as source. |
| [`heading.rs`](../src/typeset/reading/page/heading.rs) | Headings in reading mode's page: their text in the heading's size and colour, with a rule below the first two levels. |
| [`html.rs`](../src/typeset/reading/page/html.rs) | Raw HTML in reading mode's page, which is shown as its source until there is an HTML backend. |
| [`list.rs`](../src/typeset/reading/page/list.rs) | Lists in reading mode's page: their items, markers and task boxes. |
| [`origins.rs`](../src/typeset/reading/page/origins.rs) | What of the document a page's texts and pictures show: the bookkeeping of the flow that the map in `places.rs` reads. |
| [`picture.rs`](../src/typeset/reading/page/picture.rs) | Pictures in reading mode's flow: images, diagrams and formulas, placed at the left or centred, and what a picture that is not ready shows instead. |
| [`quote.rs`](../src/typeset/reading/page/quote.rs) | Block quotes and GitHub alerts in reading mode's page: the blocks of a quote indented past a bar, and an alert with its title in the kind's colour. |
| [`reach.rs`](../src/typeset/reading/page/reach.rs) | How far right a reading-mode page draws, part by part (30 rows each). |
| [`table.rs`](../src/typeset/reading/page/table.rs) | Tables in reading mode's flow: how their columns share the room, and how their cells, nested grids and the rows of a table are laid out. |

### [`src/typeset/theme/`](../src/typeset/theme/mod.rs)

Colours and sizes the renderer uses, and the theme file that changes them.

| File | What it does |
|---|---|
| [`file.rs`](../src/typeset/theme/file.rs) | The theme file: TOML that changes the defaults. Tests: `tests.rs`. |
| [`look.rs`](../src/typeset/theme/look.rs) | The one look: what the theme and the terminal's capabilities decide about how a document looks, resolved once, for both layouts to read. |
| [`paper.rs`](../src/typeset/theme/paper.rs) | The paper themes: the document as a page of fixed greys. |

## The terminal layer

### [`src/present/`](../src/present/mod.rs)

The terminal layer: the bytes kiri sends to a terminal and the decoding of what the terminal sends back. Tests: `seen.rs`.

| File | What it does |
|---|---|
| [`answers.rs`](../src/present/answers.rs) | The terminal's answers to the probe's queries, read without I/O. Tests: `props.rs`. |
| [`cell_query.rs`](../src/present/cell_query.rs) | Asking the terminal for the size of its cells: the query bytes and the reading of the answer. |
| [`clipboard.rs`](../src/present/clipboard.rs) | Copying text to the terminal's clipboard with OSC 52: it builds the bytes and writes them to an `impl Write`. |
| [`encoder.rs`](../src/present/encoder.rs) | Scales the images a layout shows and sends them through `kitty::graphics`, keeping what the terminal holds of them in the ledger. Tests: `tests.rs`. |
| [`keys.rs`](../src/present/keys.rs) | Keys read from the terminal, and the `Decoder` that turns the bytes the terminal sends into `Key` values. Tests: `props.rs`, `tests.rs`. |
| [`ledger.rs`](../src/present/ledger.rs) | What the terminal holds of the images kiri sent it, kept from what kiri sends and from the terminal's answers. Tests: `tests.rs`. |
| [`output.rs`](../src/present/output.rs) | Styled lines to bytes: SGR attributes, OSC 8 hyperlinks, cursor movement, scaled text and images. Tests: `tests.rs`. |
| [`presenter.rs`](../src/present/presenter.rs) | Writes a `Scene` to an `impl Write` through the `Writer` as one synchronized update: the lines row by row, then the bars in the order of the scene. Tests: `area_tests.rs`, `tests.rs`. |
| [`render.rs`](../src/present/render.rs) | The entry points that lay a document out and write it, one for each kind of thing shown. |
| [`scene.rs`](../src/present/scene.rs) | What a frame of the pager shows, as data: a `Scene` made of surfaces, bottom first, and `flatten`, which lays the lines of a scene on the rows of the window. |
| [`tmux.rs`](../src/present/tmux.rs) | Running inside tmux: kitty's graphics sequences reach kitty only wrapped in tmux's passthrough, `ESC P tmux ; … ESC \` with every ESC inside doubled. Tests: `tests.rs`. |

### `src/present/encoder/`

| File | What it does |
|---|---|
| [`frames.rs`](../src/present/encoder/frames.rs) | The frames of an animation that an encoder sends, for an animated image and for a picture that plays in a part of a page of reading mode. |
| [`sixel.rs`](../src/present/encoder/sixel.rs) | The sixel side of the image encoder: the cache of the slices of a placement's rows and `sixel_row`, which makes a row's bytes from them. |

### [`src/present/kitty/`](../src/present/kitty/mod.rs)

kitty's escape sequences: the graphics protocol, the text sizing protocol, and the queries that tell whether a terminal speaks them.

| File | What it does |
|---|---|
| [`graphics.rs`](../src/present/kitty/graphics.rs) | kitty's graphics protocol: sending images and showing them with Unicode placeholders. Tests: `tests.rs`. |
| [`query.rs`](../src/present/kitty/query.rs) | Queries that tell whether a terminal speaks kitty's protocols, and how to read the answers. |
| [`text.rs`](../src/present/kitty/text.rs) | kitty's text sizing protocol (OSC 66). |

### [`src/present/sixel/`](../src/present/sixel/mod.rs)

Sixel: the bytes of a picture for terminals that draw pixels written at the cursor, pure: pixels in, `DCS ... ST` sequences out, no system calls. Tests: `tests.rs`.

| File | What it does |
|---|---|
| [`cache.rs`](../src/present/sixel/cache.rs) | `Lru`, a least-recently-used cache with a budget in bytes, for the slices kiri has encoded and the pictures it prepared them from. |
| [`encode.rs`](../src/present/sixel/encode.rs) | The sixel encoder: a picture made ready once and cut into slices of one cell's height, each written as one sixel sequence. |
| [`palette.rs`](../src/present/sixel/palette.rs) | Colours for the sixel encoder: flattening a picture onto a colour, counting colours and reducing them to a palette. |
| [`row.rs`](../src/present/sixel/row.rs) | The row writer: the bytes that put a sixel in the cells of a text row. |

## The state engine

### [`src/session/`](../src/session/mod.rs)

The state engine: the pager's state, its decisions, and the components and panels that it is made of. Tests: `unlaid.rs`.

| File | What it does |
|---|---|
| [`animation.rs`](../src/session/animation.rs) | When which frame of an animated image shows: the state of each animation of the document, as the pager decides it, on the clock it is given. Tests: `tests.rs`. |
| [`areas.rs`](../src/session/areas.rs) | The map of the window's areas: the one place that drawing and the mouse both ask which part of the window is what, and the order a key goes in. Tests: `tests.rs`. |
| [`bands.rs`](../src/session/bands.rs) | Reading mode's bands in the pager: the code blocks and tables that scroll sideways. Tests: `tests.rs`. |
| [`changes.rs`](../src/session/changes.rs) | Where a reload changed the document, as ranges of the new version, and which of the changes auto-follow moves the view to. |
| [`commands.rs`](../src/session/commands.rs) | The pager's commands as data: each has its topic, its keys, its text in the help panel and the action it sends. |
| [`deck.rs`](../src/session/deck.rs) | The deck of cards over the document: which is on top, where a key and the pointer go, what Escape closes and what the status line shows. Tests: `tests.rs`. |
| [`document.rs`](../src/session/document.rs) | The state of one document, which each view has of its own: its edition, its search, the selected link, its bands, its warnings and its animations. |
| [`edition.rs`](../src/session/edition.rs) | The documents shown, as a version of them: the versions of their files and what is worked out of the documents once for each version. |
| [`fade.rs`](../src/session/fade.rs) | A style that fades into another over a moment. |
| [`files.rs`](../src/session/files.rs) | The files the pager shows as one stream: where each starts, which one a line is in, where `:n` and `:p` go, and the lines of each file's header. |
| [`follow.rs`](../src/session/follow.rs) | What Enter on a link does, worked out from the link and the paths of the files shown: no file system is asked. |
| [`frame.rs`](../src/session/frame.rs) | What the screen shows of one layout of the document: its lines, the view of them, the status line and the panels, as a frame to draw. |
| [`history.rs`](../src/session/history.rs) | The places to go back to, as in a browser: where the reader was when they followed a link, so that Backspace brings them back. |
| [`layers.rs`](../src/session/layers.rs) | The highlights over the document as one list of ranges. |
| [`links.rs`](../src/session/links.rs) | What Tab stops at: the links of the document, and the pictures that play, as ranges of its text in the order of the document. Tests: `tests.rs`. |
| [`loading.rs`](../src/session/loading.rs) | What stands in for the images of a reloaded document while they load in the background. |
| [`plan.rs`](../src/session/plan.rs) | The startup plan: the decisions a run makes before it shows anything, such as whether a document is paged, and when its images load. Tests: `tests.rs`. |
| [`prompt.rs`](../src/session/prompt.rs) | The search prompt: the term as the reader types it on the status line. |
| [`reveal.rs`](../src/session/reveal.rs) | Bringing a range of the document into view: the lines that show it, the band it is in, and why it is not shown when it is not. |
| [`search.rs`](../src/session/search.rs) | Where a search term is in the document, and where that is shown. Tests: `tests.rs`. |
| [`settings.rs`](../src/session/settings.rs) | The types of the settings that decide how the pager works. |
| [`sidebar.rs`](../src/session/sidebar.rs) | The contents bar at the left of the window: whether it is shown, how wide it is, which of its entries are folded, and what it draws. Tests: `tests.rs`, `tests_coloured.rs`, `tests_files.rs`, `tests_scrollbar.rs`. |
| [`sources.rs`](../src/session/sources.rs) | The session: what the state engine knows of what the reader is looking at. Tests: `tests.rs`. |
| [`state.rs`](../src/session/state.rs) | What the reader does in the pager besides scrolling, which stays when the window changes size: the state, and its answers to keys and to the passing of time. |
| [`status.rs`](../src/session/status.rs) | The status line at the bottom of the pager. Tests: `tests.rs`. |
| [`step.rs`](../src/session/step.rs) | The pager's step: one message in, the effects it asks for out. Tests: `tests.rs`. |
| [`view.rs`](../src/session/view.rs) | Which lines of a document fill the window, how scrolling moves them, and the rows at its top that a sticky file header covers. Tests: `tests.rs`. |
| [`warnings.rs`](../src/session/warnings.rs) | The warning line: one line just above the status line that tells the first of the document's warnings, right-aligned. |

### [`src/session/panels/`](../src/session/panels/mod.rs)

A panel as a document: its look, its frame and its scroll.

| File | What it does |
|---|---|
| [`help.rs`](../src/session/panels/help.rs) | The help as rows of keys and what they do, made from the command table by topic. |

### `src/session/sidebar/`

| File | What it does |
|---|---|
| [`colors.rs`](../src/session/sidebar/colors.rs) | The colours of the contents bar, taken from the theme's `[contents]` table or derived from the rest of the theme. Tests: `tests.rs`. |

### `src/session/status/`

| File | What it does |
|---|---|
| [`path.rs`](../src/session/status/path.rs) | The path of the views on the status line: the names of the views on the deck, `start.md › other.md › third.md`, with the last one in bold. |

## The platform

| File | What it does |
|---|---|
| [`cli.rs`](../src/cli.rs) | The command line: the clap definition of kiri's options, and what each of them asks for, as `Options`, with the flag it comes from. |
| [`doctor.rs`](../src/doctor.rs) | `kiri --doctor`: a report, in Markdown, of what kiri found out about the terminal and which settings it uses. |
| [`lib.rs`](../src/lib.rs) | kiri renders Markdown documents for the terminal. Tests: `testing.rs`. |
| [`list.rs`](../src/list.rs) | `kiri --list-emojis`: the emoji shortcodes kiri shows as emoji, from the same data the parser uses, so the list matches what this build renders. |
| [`main.rs`](../src/main.rs) | The kiri binary: the platform's entry point. |
| [`panics.rs`](../src/panics.rs) | Panics that kiri catches on purpose, told apart from the others. |
| [`reports.rs`](../src/reports.rs) | The reports that kiri writes itself instead of reading them from a file: `--doctor` and `--list-emojis`. |

### [`src/config/`](../src/config/mod.rs)

Settings from the command line, the environment and the configuration file, read once at start-up. Tests: `tests.rs`.

| File | What it does |
|---|---|
| [`environment.rs`](../src/config/environment.rs) | The process's environment: the variables kiri reads, from one table, and the directories that `HOME` and the XDG variables give. |
| [`file.rs`](../src/config/file.rs) | The configuration file: where it is, what it may set, and reading it. |
| [`registry.rs`](../src/config/registry.rs) | The registry of settings, each declared once below with its configuration-file key, the module that reads it, what a change needs and its medium. |
| [`resolve.rs`](../src/config/resolve.rs) | Resolving: the setting that the flags, the environment, the file and the default choose between them. |
| [`types.rs`](../src/config/types.rs) | The setting types: where a value comes from, the names its values have, and the types of the settings that are not the pager's. |

### [`src/diagrams/`](../src/diagrams/mod.rs)

Diagrams written as code, such as Mermaid, drawn as images.

| File | What it does |
|---|---|
| [`external.rs`](../src/diagrams/external.rs) | Diagrams drawn by a program outside kiri, such as mermaid-cli's `mmdc`. Tests: `tests.rs`. |
| [`find.rs`](../src/diagrams/find.rs) | Finding the program that draws a language's diagrams, when the configuration file names none. |

### [`src/frontend/`](../src/frontend/mod.rs)

The frontends: what shows the files that kiri is given, in the pager or printed. Tests: `sim.rs`, `tests/`.

| File | What it does |
|---|---|
| [`given.rs`](../src/frontend/given.rs) | A file among several, ready to show: what it is shown as (`Shown`), what is told after it (`Drawn`), and the loading of the images of the documents among them. |
| [`loads.rs`](../src/frontend/loads.rs) | The frontends' loading of a document's images and diagrams, and the reloading of its file for the pager. Tests: `tests.rs`. |
| [`open.rs`](../src/frontend/open.rs) | Opening a file at a path, the README of a directory or standard input, and choosing the parser for a text file's input format. |
| [`pager.rs`](../src/frontend/pager.rs) | Showing the files in the pager as one stream. |
| [`print.rs`](../src/frontend/print.rs) | Printing the files, for output that is not a terminal's pager. |
| [`route.rs`](../src/frontend/route.rs) | The route: how the files that kiri is given are shown. |

### [`src/images/`](../src/images/mod.rs)

The images a document refers to, and what is loaded of them.

| File | What it does |
|---|---|
| [`pipeline.rs`](../src/images/pipeline.rs) | The jobs that load the images of a run's documents, and the bringing of the images up to date for a reload. Tests: `tests.rs`. |
| [`stamps.rs`](../src/images/stamps.rs) | When the files behind a document's images were last changed, so that a reload loads again an image whose file has. |

### [`src/load/`](../src/load/mod.rs)

Loading: the platform's work of reading files and running the programs that draw, for the engines to draw from the data it gives them. Tests: `testing.rs`, `tests.rs`.

| File | What it does |
|---|---|
| [`builds.rs`](../src/load/builds.rs) | The builds of the frames of animated pictures: a build is a job for each place the picture is shown, and comes as one result once all its jobs are done. |
| [`fonts.rs`](../src/load/fonts.rs) | The system's fonts: the one place that scans them. |
| [`jobs.rs`](../src/load/jobs.rs) | The kinds of work the loads do: what is fetched, what is computed from it, and how an image is loaded from a job. |
| [`lane.rs`](../src/load/lane.rs) | The CPU lane: the work that takes the processor and waits for no file or program, which is the building of an animated picture's frames. Tests: `tests.rs`. |
| [`loads.rs`](../src/load/loads.rs) | The loads in the background: the document shows at once, and each image comes as it is ready. Tests: `tests.rs`. |
| [`order.rs`](../src/load/order.rs) | The order of the images that the loads prefer, and taking the next of what waits by it: the loads' queue and the CPU lane both wait in it. |
| [`renderers.rs`](../src/load/renderers.rs) | The renderers a run draws with, and the threads that draw several diagrams at once. |
| [`runner.rs`](../src/load/runner.rs) | The runner of the loads: where the jobs that `Loads::reconcile` starts run. |
| [`theme.rs`](../src/load/theme.rs) | Reading a theme file: the file's text is read here, and turned into a theme by `typeset::theme::parse`, which does no I/O. |
| [`threads.rs`](../src/load/threads.rs) | Starting the threads that the loads' queue and the CPU lane work on, with the stack of the main thread. |
| [`url.rs`](../src/load/url.rs) | Loading the image at a URL: the local file a path or a `file:` URL names, opened and read up to a size limit, and a `data:` URL decoded. |
| [`watch.rs`](../src/load/watch.rs) | What the pager asks of the file behind a document: to read it again, to look at it, and when to look. |

### [`src/pager/`](../src/pager/mod.rs)

The interactive pager: the document on a screen of its own, scrolled with keys. Tests: `tests/`.

| File | What it does |
|---|---|
| [`animate.rs`](../src/pager/animate.rs) | The pager's side of animated images: what the loop does with the animations the state engine decided on (`src/session/animation.rs`). |
| [`idle.rs`](../src/pager/idle.rs) | The idle work of a layout: what the loop does while no key waits. |
| [`layouts.rs`](../src/pager/layouts.rs) | What the loop keeps by the sheet of the deck: the layouts, and for each view what it shows and where its view was. |

### [`src/term/`](../src/term/mod.rs)

The terminal itself: finding out what it can do, its window and the pager's screen. Tests: `pty.rs`, `testing.rs`.

| File | What it does |
|---|---|
| [`check.rs`](../src/term/check.rs) | Asking kitty, behind tmux, whether it holds the images kiri sent: tmux drops output for a client that cannot keep up, and the images in it with it. Tests: `props.rs`. |
| [`detect.rs`](../src/term/detect.rs) | What the output terminal can do: from the mode, the environment, the window and the probe. Tests: `tests.rs`. |
| [`interrupt.rs`](../src/term/interrupt.rs) | Signals that would end kiri while programs outside kiri draw diagrams: Ctrl-C (SIGINT), Ctrl-\ (SIGQUIT), SIGTERM and SIGHUP. |
| [`keys.rs`](../src/term/keys.rs) | The keys read while kiri waits for a document's images before the pager opens: Space or Enter opens it at once. |
| [`printable.rs`](../src/term/printable.rs) | Control characters of a document replaced, so that text told on standard error cannot act on the terminal: done before a warning is written. |
| [`probe.rs`](../src/term/probe.rs) | Sending escape-sequence queries to the terminal and reading its answers. Tests: `tests.rs`. |
| [`progress.rs`](../src/term/progress.rs) | The line that tells how far loading a document's images has got, such as `kiri: loading images 2/5…`. |
| [`screen.rs`](../src/term/screen.rs) | The pager's screen: the alternate screen, and raw mode for keys. Tests: `tests.rs`. |
| [`tmux.rs`](../src/term/tmux.rs) | Running `tmux` and reading what it says about the pane kiri runs in: the platform side of running inside tmux. Tests: `tests.rs`. |
| [`window.rs`](../src/term/window.rs) | The size of a terminal window. |
