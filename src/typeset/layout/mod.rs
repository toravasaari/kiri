//! Document model to styled lines of a fixed width.
//!
//! This is the cell layout engine's driver: it walks the blocks of a
//! document, hands each kind to its own file (code, lists, quotes, front
//! matter, tables) and gives the runs that reading mode lays out to that
//! engine. It reads the images that are loaded and never loads one.
//!
//! What does not belong here is the layout of one kind of block, which has
//! a file beside this one; word wrapping, tables and colours, which have
//! theirs; pixels; and anything about a terminal's bytes or a key.
//!
//! A new block kind gets a file of its own in the typesetting engine
//! (`src/typeset/layout/`), and its pixel layout goes to the reading
//! engine (`src/typeset/reading/`). Bytes go to the terminal layer
//! (`src/present/`).

pub mod highlight;
pub mod script;
pub mod table;

mod code;
mod embed;
mod front_matter;
mod image_block;
mod image_file;
mod inlines;
mod list;
mod quote;
#[cfg(test)]
pub(crate) mod summary;

pub use code::CodeColors;
pub(crate) use code::{code_rows, expand_tabs};

use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::ops::Range;
use std::rc::Rc;

use crate::render::{FrontMatterStyle, Headings, Inputs, RenderErrors, TableRows};
use crate::typeset::block::{BlockId, Blocks};
use crate::typeset::caps::Capabilities;
use crate::typeset::color::Color;
use crate::typeset::grid::table_grid;
use crate::typeset::images::size::{self, Placement, Shows};
use crate::typeset::images::{ImageError, ImageKey, Images, Source, heading_words};
use crate::typeset::images::{Looks, diagram_look_for};
use crate::typeset::model::{Block, Document, File, Inline, Table};
use crate::typeset::reading::engine::Reading;
use crate::typeset::reading::page;
use crate::typeset::reading::places::Ids;
use crate::typeset::rendition::{self, PictureState, RenditionKind};
use crate::typeset::size::TextSize;
use crate::typeset::style::{Band, Line, Loading, Origin, Spaces, Span, Style, push, push_span};
use crate::typeset::text::{self, Cursor};
use crate::typeset::theme::{BandColor, Look};
use crate::typeset::wrap::{split, width, wrap};

/// `document` laid out, without the table of its blocks, as [`file`] has
/// it.
pub fn layout(document: &Document, inputs: &Inputs) -> Vec<Line> {
    document_stream(document, inputs).lines
}

/// `document` laid out as a lone document, with the blocks its lines come
/// from.
fn document_stream(document: &Document, inputs: &Inputs) -> Stream {
    let mut blocks = Blocks::default();
    let lines = with_layout_in(inputs, 0, &mut blocks, |layout, width, margin| {
        let mut lines = Vec::new();
        layout.top_level(&document.blocks, width, margin, &mut lines);
        settle_bands(&mut lines);
        lines
    });
    finished(Stream { lines, blocks }, inputs)
}

/// Puts each band in `lines` where it is drawn: on the blank line after
/// its block when there is one, a little below its top, so that the band
/// takes no row and nothing moves when the image comes; otherwise over the
/// bottom of the block's last line. It starts where the block's text or
/// image does.
fn settle_bands(lines: &mut [Line]) {
    let marked: Vec<usize> = (0..lines.len())
        .filter(|&i| lines[i].loading.is_some())
        .collect();
    for index in marked {
        let Some(mut loading) = lines[index].loading.take() else {
            continue;
        };
        let line = &lines[index];
        let mut indent = width(&line.lead);
        if line.image.is_some() {
            indent += width(&line.spans);
        }
        let blank = (lines.get(index + 1)).is_some_and(|below| {
            below.is_blank() && below.lead.is_empty() && below.block.is_none()
        });
        let at = if blank { index + 1 } else { index };
        loading.above += usize::from(blank);
        if let Some(band) = &mut loading.band {
            (band.indent, band.low) = (indent + band.indent, !blank);
        }
        lines[at].loading = Some(loading);
    }
}

/// What a line says of the loading image `key`: shown by `above` lines
/// above it too, and with `band` drawn over it where the terminal places
/// bands. This is the one place that decides it, for the text layout and
/// the reading mode alike; the writer, the presenter and the model of the
/// screen draw what the line holds.
pub(crate) fn loading(caps: &Capabilities, key: ImageKey, above: usize, band: Band) -> Loading {
    Loading {
        key,
        above,
        band: caps.places_bands().then_some(band),
    }
}

/// Where an image block stands in the room it has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Align {
    /// At the left edge, as images and diagrams are.
    Left,
    /// In the middle, as a formula is.
    Centre,
}

/// What is laid out: one document, or several files one after another.
#[derive(Debug, Clone, Copy)]
pub enum Content<'a> {
    Document(&'a Document),
    Files(&'a [File]),
}

/// `content` laid out with `inputs`.
pub fn content(content: Content, inputs: &Inputs) -> Stream {
    match content {
        Content::Document(document) => document_stream(document, inputs),
        Content::Files(shown) => files(shown, inputs),
    }
}

/// Ends a layout: the pictures of reading mode that it did not show are
/// let go.
fn finished(stream: Stream, inputs: &Inputs) -> Stream {
    end_layout(inputs);
    stream
}

fn end_layout(inputs: &Inputs) {
    inputs.images.end_notes();
    if let Some(reading) = &inputs.reading {
        reading.pictures.borrow_mut().finish();
    }
}

/// Lines laid out, with the blocks they come from.
#[derive(Debug, Default)]
pub struct Stream {
    pub lines: Vec<Line>,
    /// The blocks that the lines' [`Line::block`] numbers.
    pub blocks: Blocks,
}

/// `files` laid out one after another, a blank line apart, each as
/// [`file`] lays it out. The blank line between two files has no block,
/// as the blank lines between blocks have none.
pub fn files(files: &[File], inputs: &Inputs) -> Stream {
    let mut stream = Stream::default();
    for (number, shown) in files.iter().enumerate() {
        if number > 0 {
            stream.lines.push(Line::blank());
        }
        let lines = file_into(shown, number, inputs, &mut stream.blocks);
        stream.lines.extend(lines);
    }
    finished(stream, inputs)
}

/// `file`, numbered `number` among the files shown together, laid out
/// under a header that names it, as [`Layout::file_header`] draws it, and
/// a blank line. An image file below it is fitted to the window with its
/// header, however many rows a long name takes. The table of its blocks is
/// not kept, so the block numbers of its lines point into nothing;
/// [`files`] keeps the table.
pub fn file(file: &File, number: usize, inputs: &Inputs) -> Vec<Line> {
    let lines = file_into(file, number, inputs, &mut Blocks::default());
    end_layout(inputs);
    lines
}

/// [`file`], with its blocks added to `blocks`. The header lines come from
/// a block of their own, which has no path.
fn file_into(file: &File, number: usize, inputs: &Inputs, blocks: &mut Blocks) -> Vec<Line> {
    with_layout_in(inputs, number, blocks, |layout, width, margin| {
        let mut lines = layout.file_header(&file.name, width, margin);
        let id = layout.add_block();
        stamp(&mut lines, id);
        // The header and the blank line below it.
        let above = u16::try_from(lines.len() + 1).unwrap_or(u16::MAX);
        let layout = Layout {
            reserved: layout.reserved.saturating_add(above),
            ..*layout
        };
        let mut body = Vec::new();
        layout.top_level(&file.document.blocks, width, margin, &mut body);
        append_block(&mut lines, body, true);
        settle_bands(&mut lines);
        lines
    })
}

/// What `lay_out` returns for a layout with `inputs` of the document
/// numbered `number`, given the width of the text and its margin.
#[cfg(test)]
fn with_layout<T>(
    inputs: &Inputs,
    number: usize,
    lay_out: impl FnOnce(&Layout, usize, usize) -> T,
) -> T {
    with_layout_in(inputs, number, &mut Blocks::default(), lay_out)
}

/// [`with_layout`], with the blocks that are laid out added to `blocks`.
fn with_layout_in<T>(
    inputs: &Inputs,
    number: usize,
    blocks: &mut Blocks,
    lay_out: impl FnOnce(&Layout, usize, usize) -> T,
) -> T {
    let Inputs {
        images,
        caps,
        theme,
        front_matter,
        headings,
        table_rows,
        render_errors,
        reading,
        diagram_panel,
    } = inputs;
    // Without colours, the layout leaves out what only shows a colour, such
    // as heading fills and the padding around inline code. The built-in
    // theme's greys follow the terminal's palette, which may hide them.
    let adjusted;
    let looks = Looks {
        diagram: diagram_look_for(theme, caps, *diagram_panel),
        headings: crate::render::heading_look(*headings, false, theme, caps),
    };
    let theme = if !caps.color {
        adjusted = theme.clone().without_colors();
        &adjusted
    } else if theme.built_in {
        adjusted = theme
            .clone()
            .with_visible_greys(&caps.palette, caps.background);
        &adjusted
    } else {
        theme
    };
    let look = Look::new(theme, caps);
    let mut width = usize::from(caps.width);
    if look.max_width > 0 {
        width = width.min(usize::from(look.max_width));
    }
    let width = width.max(10);
    let marks = RefCell::new(Marks::new(std::mem::take(blocks)));
    let cursor = Cell::new(Cursor::off());
    let layout = Layout {
        marks: &marks,
        cursor: &cursor,
        images,
        caps,
        look: &look,
        front_matter: *front_matter,
        headings: *headings,
        table_rows: *table_rows,
        render_errors: *render_errors,
        // Only where the terminal draws images, which reading mode's text
        // is.
        reading: reading.as_ref().filter(|_| caps.draws_images()),
        document: number,
        looks,
        // The row below for the shell's prompt or the pager's status line.
        reserved: 1,
        embed_width: None,
    };

    // Everything but headings and thematic breaks keeps text_margin free
    // on both sides.
    let margin = usize::from(look.cells.text_margin).min(width / 4);
    let laid_out = lay_out(&layout, width, margin);
    *blocks = marks.into_inner().table;
    laid_out
}

/// `lines` with `margin` cells before each one that is not blank.
fn with_margin(mut lines: Vec<Line>, margin: usize) -> Vec<Line> {
    if margin > 0 {
        for line in &mut lines {
            if !line.is_blank() {
                line.lead_insert(Span::new(&" ".repeat(margin), Style::default()), false);
            }
        }
    }
    lines
}

/// The bytes of each of `words` in `text`, found by their characters one
/// after another, leaving out white space; none when they are not there as
/// they are.
fn word_places(
    text: &str,
    words: &[crate::typeset::images::HeadingWord],
) -> Option<Vec<Range<usize>>> {
    let mut chars = text.char_indices().filter(|(_, c)| !c.is_whitespace());
    let mut places = Vec::new();
    for word in words {
        let (mut start, mut end) = (None, 0);
        for c in word.text.chars() {
            let (at, found) = chars.next()?;
            if found != c {
                return None;
            }
            start.get_or_insert(at);
            end = at + found.len_utf8();
        }
        places.push(start?..end);
    }
    Some(places)
}

/// Says that the image rows of `lines` stand in the place of the bytes
/// `doc` of the text of their block, as a picture.
fn show_as_picture(lines: &mut [Line], doc: Range<usize>) {
    let doc =
        u32::try_from(doc.start).unwrap_or(u32::MAX)..u32::try_from(doc.end).unwrap_or(u32::MAX);
    for line in lines {
        if let Some(image) = &mut line.image {
            image.origin = Some(Origin::picture(0, doc.clone()));
        }
    }
}

/// Marks `lines` as coming from the block `id`, which is also the innermost
/// block of those that have none yet.
fn stamp(lines: &mut [Line], id: BlockId) {
    for line in lines {
        line.block = Some(id);
        line.inner.get_or_insert(id);
    }
}

/// Marks the `lines` that no block inside the block `id` has marked as its
/// own: a quote's bars, the blank lines of a list.
fn mark_inner(lines: &mut [Line], id: BlockId) {
    for line in lines {
        line.inner.get_or_insert(id);
    }
}

/// Appends a block's lines, after a blank line if `blank` and something
/// came before. A block without lines, such as an empty code block, gets
/// no separator either.
fn append_block(lines: &mut Vec<Line>, block_lines: Vec<Line>, blank: bool) {
    if block_lines.is_empty() {
        return;
    }
    if blank && !lines.is_empty() {
        lines.push(Line::blank());
    }
    lines.extend(block_lines);
}

#[derive(Debug, Clone, Copy, Default)]
struct Context {
    /// Inside a quote or list, where scaled or indented headings would break
    /// the prefix.
    nested: bool,
    list_depth: usize,
    /// Tight list item: no blank lines between its blocks.
    tight: bool,
}

#[derive(Clone, Copy)]
struct Layout<'a> {
    images: &'a Images,
    caps: &'a Capabilities,
    look: &'a Look,
    front_matter: FrontMatterStyle,
    headings: Headings,
    table_rows: TableRows,
    render_errors: RenderErrors,
    /// Reading mode, which lays the text blocks out as pages of their own.
    reading: Option<&'a Reading>,
    /// The number of the document laid out among those shown together,
    /// which the keys of its images carry.
    document: usize,
    /// The blocks laid out so far and where the one being laid out is,
    /// which every copy of the layout shares.
    marks: &'a RefCell<Marks>,
    /// Where the inlines being laid out are in the text of their block,
    /// shared as `marks` is.
    cursor: &'a Cell<Cursor>,
    /// What the keys of diagrams and headings name of how they are drawn.
    looks: Looks,
    /// The rows of the window that an image file shown on its own leaves
    /// to others: the shell's prompt or the pager's status line below it,
    /// and the header above it.
    reserved: u16,
    /// The width in cells that images in running text may take, where
    /// they are drawn in the text: set by the blocks that draw them.
    embed_width: Option<usize>,
}

/// Where the block being laid out is, and the table that records the
/// blocks laid out.
struct Marks {
    table: Blocks,
    path: Vec<usize>,
}

impl Marks {
    fn new(table: Blocks) -> Self {
        Self {
            table,
            path: Vec::new(),
        }
    }

    /// Adds the block at the current path, of document `document`.
    fn add(&mut self, document: usize) -> BlockId {
        self.table.add(document, self.path.clone())
    }
}

impl Layout<'_> {
    /// Lays out the document's own blocks: headings stay at the edge and
    /// thematic breaks span the terminal; everything else keeps `margin`
    /// free on both sides. The blocks inside a details block are laid out
    /// as the blocks around it are.
    ///
    /// Each block's lines get a new entry in the table of `marks`, which
    /// also knows where the block being laid out is: the index of each
    /// `<details>` around it, then its own index among its siblings, so the
    /// second block inside the first `<details>` is `[0, 1]`. The summary of
    /// a details block is marked as that block, and the blank line between
    /// blocks has no block.
    fn top_level(&self, blocks: &[Block], width: usize, margin: usize, lines: &mut Vec<Line>) {
        // The blocks that reading mode lays out together, until a block it
        // does not lay out comes.
        let mut run = Vec::new();
        self.top_level_into(blocks, width, margin, lines, &mut run);
        self.reading_page(&mut run, width, lines);
    }

    /// Adds the block at the current path to the table.
    fn add_block(&self) -> BlockId {
        self.marks.borrow_mut().add(self.document)
    }

    /// Runs `lay_out` one step further along the path: inside the block or
    /// item numbered `step`.
    fn step<T>(&self, step: usize, lay_out: impl FnOnce() -> T) -> T {
        self.marks.borrow_mut().path.push(step);
        let laid_out = lay_out();
        self.marks.borrow_mut().path.pop();
        laid_out
    }

    /// `top_level`, adding the blocks reading mode lays out to `run`
    /// rather than laying out what is left of it at the end.
    fn top_level_into<'b>(
        &self,
        blocks: &'b [Block],
        width: usize,
        margin: usize,
        lines: &mut Vec<Line>,
        run: &mut Vec<(Vec<usize>, Cow<'b, Block>)>,
    ) {
        // Where the other blocks start and how wide they are: in reading
        // mode, from the area's left edge and as wide as the text's column,
        // so that they line up with the text, a centred formula in the
        // middle of the column and a wide image shrunk to it.
        let (lead, inner) = match self.reading {
            Some(_) => {
                let window = usize::from(self.caps.width).max(width);
                let area = super::reading::engine::Area::of(self.look, self.caps, window);
                (usize::from(area.left), usize::from(area.column))
            }
            None => (margin, width - 2 * margin),
        };
        for (index, block) in blocks.iter().enumerate() {
            // Hidden front matter is nothing, which the page would still
            // give a place.
            let hidden = matches!(block, Block::FrontMatter(_))
                && self.front_matter == FrontMatterStyle::Hide;
            let path = || [&self.marks.borrow().path[..], &[index]].concat();
            // A details block is its summary and then its blocks, each
            // with a place of its own, as the text layout has them.
            if let (Some(_), Block::Details { summary, blocks }) = (self.reading, block)
                && blocks.iter().all(page::lays_out)
            {
                let summary = Block::Details {
                    summary: summary.clone(),
                    blocks: Vec::new(),
                };
                run.push((path(), Cow::Owned(summary)));
                self.step(index, || {
                    self.top_level_into(blocks, width, margin, lines, run);
                });
                continue;
            }
            if self.reading.is_some() && page::lays_out(block) && !hidden {
                run.push((path(), Cow::Borrowed(block)));
                continue;
            }
            self.reading_page(run, width, lines);
            self.marks.borrow_mut().path.push(index);
            // The block is in the table before the blocks inside it.
            let id = self.add_block();
            if hidden {
                self.marks.borrow_mut().table.hide(id);
            }
            let mut block_lines = match block {
                Block::ThematicBreak => vec![self.rule(usize::from(self.caps.width).max(width))],
                Block::Heading { .. } | Block::ImageFile(_) => {
                    self.block(block, width, Context::default())
                }
                Block::Details { summary, blocks } => {
                    let summary = self.summary(summary, inner);
                    let mut summary = with_margin(summary, lead);
                    stamp(&mut summary, id);
                    append_block(lines, summary, true);
                    self.top_level(blocks, width, margin, lines);
                    self.marks.borrow_mut().path.pop();
                    continue;
                }
                _ => with_margin(self.block(block, inner, Context::default()), lead),
            };
            stamp(&mut block_lines, id);
            append_block(lines, block_lines, true);
            self.marks.borrow_mut().path.pop();
        }
    }

    /// Lays out the blocks of `run`, each with its path, as a page of
    /// reading mode, and empties it.
    fn reading_page(
        &self,
        run: &mut Vec<(Vec<usize>, Cow<'_, Block>)>,
        width: usize,
        lines: &mut Vec<Line>,
    ) {
        let Some(reading) = self.reading.filter(|_| !run.is_empty()) else {
            return;
        };
        // Each block and the blocks inside it are in the table, in the order
        // of the document, before the page is laid out.
        let ids: Vec<Ids> = (run.iter())
            .map(|(path, block)| {
                let mut marks = self.marks.borrow_mut();
                let outer = std::mem::replace(&mut marks.path, path.clone());
                let ids = Ids::new(block, |inside| {
                    let whole = [&path[..], inside].concat();
                    let before = std::mem::replace(&mut marks.path, whole);
                    let id = marks.add(self.document);
                    marks.path = before;
                    id
                });
                marks.path = outer;
                ids
            })
            .collect();
        let owned: Vec<Cow<Block>> = run.drain(..).map(|(_, block)| block).collect();
        let blocks: Vec<&Block> = owned.iter().map(AsRef::as_ref).collect();
        // Reading mode sets its own width: the theme's widest text is for
        // text in the terminal's font.
        let window = usize::from(self.caps.width).max(width);
        let shown = (self.document, self.looks.diagram);
        let page = super::reading::engine::lines(
            &blocks,
            &ids,
            reading,
            (
                self.look,
                &self.code_colors(),
                (self.table_rows != TableRows::Off, self.front_matter),
            ),
            (self.images, self.render_errors.inline(), shown),
            self.caps,
            window,
        );
        append_block(lines, page, true);
    }

    /// The blocks inside another block, one below another, each in the
    /// table at the path one step further than the block's.
    fn blocks(&self, blocks: &[Block], width: usize, context: Context) -> Vec<Line> {
        let mut lines = Vec::new();
        for (index, block) in blocks.iter().enumerate() {
            let block_lines = self.step(index, || {
                let id = self.add_block();
                let mut block_lines = self.block(block, width, context);
                mark_inner(&mut block_lines, id);
                block_lines
            });
            append_block(&mut lines, block_lines, !context.tight);
        }
        lines
    }

    fn block(&self, block: &Block, width: usize, context: Context) -> Vec<Line> {
        // What a block lays out of its own text starts text number 0, and a
        // block with none leaves nothing to show of the document.
        self.text_is(None);
        match block {
            Block::FrontMatter(front_matter) => self.front_matter(front_matter, width),
            Block::Heading { level, content } => {
                self.text_is(Some(0));
                self.heading(*level, content, width, context)
            }
            Block::Paragraph(content) => {
                self.text_is(Some(0));
                let mut lines = self.paragraph(content, &Style::default(), width);
                // A lone image drawn as a picture stands in the place of
                // its alt text, which is the paragraph's.
                if Inline::lone_image(content).is_some() {
                    let length: usize = content.iter().map(text::inline_len).sum();
                    show_as_picture(&mut lines, 0..length);
                }
                lines
            }
            Block::CodeBlock {
                language,
                code,
                whole_file,
                ..
            } => {
                let language = language.as_deref();
                if let Some(mut lines) = self.diagram(language, code, width) {
                    show_as_picture(&mut lines, 0..text::without_last_break(code).len());
                    return lines;
                }
                self.text_is(Some(0));
                let mut lines = self.code_block(language, code, *whole_file, width);
                lines.extend(self.render_error(language, code, width));
                if let Some(language) = language {
                    let key = ImageKey::diagram(language, code, self.looks.diagram, self.document);
                    self.loading_band(&key, width, &mut lines);
                }
                lines
            }
            Block::BlockQuote(blocks) => self.quote(blocks, width, context),
            Block::Alert {
                kind,
                title,
                blocks,
            } => self.alert(*kind, title.as_deref(), blocks, width, context),
            Block::List(list) => self.list(list, width, context),
            Block::Details { summary, blocks } => {
                let mut lines = self.summary(summary, width);
                append_block(
                    &mut lines,
                    self.blocks(blocks, width, context),
                    !context.tight,
                );
                lines
            }
            Block::Table(table) => self.table(table, width),
            Block::ThematicBreak => vec![self.rule(width)],
            Block::Html(html) => {
                let style = self.muted(Style::default());
                let mut start = 0;
                let mut lines = Vec::new();
                for raw in html.split_inclusive('\n') {
                    let line = raw.strip_suffix('\n').unwrap_or(raw);
                    let line = line.strip_suffix('\r').unwrap_or(line);
                    let origin = Origin::exact(0, start as u32..(start + line.len()) as u32);
                    start += raw.len();
                    let span = Span::new(line, style.clone()).showing(origin);
                    lines.extend(split(&[span], width).into_iter().map(Line::new));
                }
                lines
            }
            Block::Note(note) => {
                let content = [Inline::Text(note.clone())];
                self.paragraph(&content, &self.muted(Style::default()), width)
            }
            Block::Error(message) => {
                let spans = [Span::new(
                    &format!("✗ {message}"),
                    self.style(Style::fg(self.look.error)),
                )];
                wrap(&spans, width).into_iter().map(Line::new).collect()
            }
            Block::ImageFile(name) => self.image_file(name, width),
        }
    }

    /// The header of the file `name`: a band of its own colour across
    /// `width`, an empty row, the name and another empty row, with a bar
    /// down its left edge and the name `margin` cells in, at least two:
    /// the directory muted and the file's own name in bold. Nothing in
    /// Markdown draws such a band. Plain text gets the name between
    /// arrows instead, as `head` writes it.
    fn file_header(&self, name: &str, width: usize, margin: usize) -> Vec<Line> {
        if !self.caps.styling {
            let spans = [Span::new(&format!("==> {name} <=="), Style::default())];
            return wrap(&spans, width).into_iter().map(Line::new).collect();
        }
        let band = self.band();
        let base = Style {
            bg: band,
            // A name keeps its spaces as they are.
            spaces: Spaces::Keep,
            ..Style::default()
        };
        let bar = Style {
            fg: self.look.file_header.bar,
            ..base.clone()
        };
        let indent = margin.max(2);
        let (directory, file) = name.split_at(name.rfind('/').map_or(0, |at| at + 1));
        let mut spans = Vec::new();
        push(&mut spans, directory, &self.muted(base.clone()));
        let bold = Style {
            bold: true,
            ..base.clone()
        };
        push(&mut spans, file, &bold);
        // A cell of the band is left after the longest row.
        let rows = wrap(&spans, width.saturating_sub(indent + 1));
        let row = |content: Vec<Span>| {
            let mut spans = vec![Span::new("▌", bar.clone())];
            // Without a band, an empty row ends at the bar.
            if band.is_some() || !content.is_empty() {
                push(&mut spans, &" ".repeat(indent - 1), &base);
            }
            spans.extend(content);
            if band.is_some() {
                let used = super::wrap::width(&spans);
                push(&mut spans, &" ".repeat(width.saturating_sub(used)), &base);
            }
            Line::new(spans)
        };
        let mut lines = vec![row(Vec::new())];
        lines.extend(rows.into_iter().map(row));
        lines.push(row(Vec::new()));
        lines
    }

    /// The colour of the band of a file's header: the theme's, or else a
    /// little of the text colour mixed into the page. `None` where the
    /// page is not known as RGB.
    fn band(&self) -> Option<Color> {
        match self.look.file_header.background {
            BandColor::Color(color) => color,
            BandColor::Auto => {
                // Light text on a dark page, as most terminals have it.
                let contrast = match self.dark() {
                    true => Color::Rgb(0xff, 0xff, 0xff),
                    false => Color::Rgb(0, 0, 0),
                };
                let amount = if self.dark() {
                    self.look.cells.band_dark
                } else {
                    self.look.cells.band_light
                };
                let text = self.look.foreground.or(self.caps.foreground);
                self.mixed_into_page(text.or(Some(contrast)), amount)
            }
        }
    }

    fn heading(&self, level: u8, content: &[Inline], width: usize, context: Context) -> Vec<Line> {
        let heading = self.look.heading(level);
        let sized = self.caps.text_sizing && self.headings != Headings::Plain;
        let size = if sized && !context.nested {
            heading.size
        } else {
            TextSize::NORMAL
        };
        let style = self.style(Style {
            bold: heading.bold,
            italic: heading.italic,
            fg: heading.color,
            bg: heading.background,
            underline: heading.underline,
            size,
            ..Style::default()
        });
        // Indent and fill carry the background but not the underline.
        let background = self.style(Style {
            bg: heading.background,
            ..Style::default()
        });
        let fill_style = Style {
            size,
            ..background.clone()
        };
        // A nested heading's indent would land before the quote bar or the
        // list marker, which are prepended later.
        let indent = usize::from(heading.indent).min(width / 4) * usize::from(!context.nested);
        let available = width - indent;
        let mut text = Vec::new();
        // Images and formulas in sized text are drawn in its rows.
        let drawing = Layout {
            embed_width: (size.scale() > 1).then_some(available),
            ..*self
        };
        drawing.inlines(content, &style, &mut text);
        let empty = text.is_empty();
        let mut spans = Vec::new();
        if !self.caps.styling {
            push(
                &mut spans,
                &format!("{} ", "#".repeat(usize::from(level))),
                &style,
            );
        }
        spans.extend(text);
        let lead = if indent > 0 {
            vec![Span::new(&" ".repeat(indent), background.clone())]
        } else {
            Vec::new()
        };
        let line = |mut spans: Vec<Span>, fill_style: &Style| {
            let mut trail = Vec::new();
            if background.bg.is_some() {
                super::wrap::fill(&mut spans, available, fill_style);
                // Scaled text covers a multiple of the scale; cells of
                // normal size fill the rest of the width.
                let used = super::wrap::width(&spans);
                let rest = available.saturating_sub(used);
                push(&mut trail, &" ".repeat(rest), &background);
            }
            Line {
                lead: lead.clone(),
                trail,
                ..Line::new(spans)
            }
        };
        let image = match context.nested || empty {
            true => None,
            false => self.heading_image(level, content, available),
        };
        // An empty heading takes no room, but keeps its rule.
        let mut lines: Vec<Line> = if empty {
            Vec::new()
        } else if let Some(rows) = image {
            (rows.into_iter())
                .map(|mut row| {
                    let written = row.image.as_ref().map_or(0, |image| image.columns.len());
                    if let Some(image) = &mut row.image {
                        image.background = background.bg;
                    }
                    if background.bg.is_some() {
                        let rest = available.saturating_sub(written);
                        push(&mut row.trail, &" ".repeat(rest), &background);
                    }
                    Line {
                        lead: lead.clone(),
                        ..row
                    }
                })
                .collect()
        } else {
            (embed::lines(self, &spans, available).into_iter())
                .map(|row| line(row.spans, &fill_style))
                .collect()
        };
        if heading.rule && self.caps.styling {
            // A top-level heading's rule spans the whole terminal, past
            // max_width, to mark the section boundary.
            let rule_width = if context.nested {
                width
            } else {
                usize::from(self.caps.width).max(width)
            };
            lines.push(self.rule(rule_width));
        }
        lines
    }

    /// The rows of the image of the top-level heading of `level` with
    /// `content`, at most `room` cells wide, when headings show as images
    /// and its image loaded. A heading too wide for the room wraps as the
    /// sized one does: each line shows the columns of the image that its
    /// words cover. Only when a word alone is wider than the room is the
    /// whole image shrunk to the room, on one line.
    fn heading_image(&self, level: u8, content: &[Inline], room: usize) -> Option<Vec<Line>> {
        if !crate::render::heading_images(self.headings, self.caps) {
            return None;
        }
        let cell = size::cell_size(self.caps);
        let words = heading_words(content);
        let key = ImageKey::heading_of(level, words.clone(), cell)
            .with_look(self.looks.heading(level))
            .in_document(self.document);
        let Some(Ok(Source::Text {
            image,
            breaks,
            ends,
        })) = self.images.get(&key)
        else {
            return None;
        };
        if words.is_empty() || ends.len() != words.len() || breaks.len() + 1 != words.len() {
            return None;
        }
        let (width, height) = image.dimensions();
        let most = self.caps.max_image_cells();
        let columns = u16::try_from(width.div_ceil(u32::from(cell.0))).ok()?;
        let rows = u16::try_from(height.div_ceil(u32::from(cell.1))).ok()?;
        // Kitty places an image at most this many cells wide.
        if columns > most || rows > most {
            return None;
        }
        let room = u16::try_from(room).unwrap_or(u16::MAX);
        let starts: Vec<u16> = std::iter::once(0).chain(breaks.iter().copied()).collect();
        // The words that stay on one line together: each with those glued
        // to it, such as a comma after inline code.
        let mut groups: Vec<Range<usize>> = Vec::new();
        for (index, word) in words.iter().enumerate() {
            match groups.last_mut() {
                Some(group) if word.glued => group.end = index + 1,
                _ => groups.push(index..index + 1),
            }
        }
        let width_of =
            |words: &Range<usize>| ends[words.end - 1].saturating_sub(starts[words.start]);
        let wide = groups.iter().any(|group| width_of(group) > room);
        let (size, lines) = if wide {
            let shrunk = size::size_in((width, height), size::Unit::Pixel, cell, room, rows)?;
            (shrunk, vec![(0..shrunk.columns, 0..words.len())])
        } else {
            let full = size::Size {
                width,
                height,
                columns,
                rows,
            };
            // As many words on each line as its room holds, the gap after
            // the last one aside.
            let mut lines = Vec::new();
            let mut first = 0;
            for group in &groups {
                if group.start > first && width_of(&(first..group.end)) > room {
                    lines.push((starts[first]..ends[group.start - 1], first..group.start));
                    first = group.start;
                }
            }
            lines.push((starts[first]..ends[words.len() - 1], first..words.len()));
            (full, lines)
        };
        let placement = Rc::new(Placement::block(key, size, Shows::Image));
        // Where each word is in the heading's text, if the words are in it
        // as they are.
        let mut flat = String::new();
        text::flatten(content, &mut flat, &mut |_, _| {});
        let at = word_places(&flat, &words);
        let mut rows = Vec::new();
        for (columns, shown) in lines {
            let doc = match &at {
                Some(at) => at[shown.start].start..at[shown.end - 1].end,
                None => 0..flat.len(),
            };
            let origin = Origin::picture(0, doc.start as u32..doc.end as u32);
            let mut text = String::new();
            for word in &words[shown] {
                if !text.is_empty() && !word.glued {
                    text.push(' ');
                }
                text.push_str(&word.text);
            }
            for row in 0..size.rows {
                let mut line = Line::image(Rc::clone(&placement), row);
                if let Some(image) = &mut line.image {
                    image.columns = columns.clone();
                    image.text = Some(text.clone());
                    image.origin = Some(origin.clone());
                }
                rows.push(line);
            }
        }
        Some(rows)
    }

    /// The summary of a details block, in bold after an open triangle, as
    /// a browser shows an open `<details>`.
    fn summary(&self, summary: &[Inline], width: usize) -> Vec<Line> {
        let style = self.style(Style {
            bold: true,
            ..Style::default()
        });
        self.text_is(Some(0));
        let mut content = Vec::new();
        self.inlines(summary, &style, &mut content);
        // The summary is one line, so a line break in it is a space. As in
        // the HTML it comes from, a run of spaces shows as one, also after
        // the triangle, which ends in one.
        let mut space = true;
        let mut spans = Vec::new();
        push(&mut spans, "▼ ", &style);
        for span in content {
            let mut text = String::with_capacity(span.text.len());
            for c in span.text.chars().map(|c| if c == '\n' { ' ' } else { c }) {
                if !(c == ' ' && space) {
                    text.push(c);
                }
                space = c == ' ';
            }
            // A span that changed shows what it did as a whole.
            let span = match text == span.text {
                true => span,
                false => span.changed(text),
            };
            if !span.text.is_empty() {
                push_span(&mut spans, span);
            }
        }
        wrap(&spans, width).into_iter().map(Line::new).collect()
    }

    fn paragraph(&self, content: &[Inline], style: &Style, width: usize) -> Vec<Line> {
        if let Some(lines) = self.image_block(content, width) {
            return lines;
        }
        let mut spans = Vec::new();
        let drawing = Layout {
            embed_width: Some(width),
            ..*self
        };
        drawing.inlines(content, style, &mut spans);
        let mut lines = embed::lines(self, &spans, width);
        if let Some(lone) = Inline::lone_image(content) {
            let key = ImageKey::from(lone.url.as_str()).in_document(self.document);
            self.loading_band(&key, width, &mut lines);
        }
        lines
    }

    /// A code block drawn as a diagram, when it was.
    fn diagram(&self, language: Option<&str>, code: &str, width: usize) -> Option<Vec<Line>> {
        // Spares building a key for every code block of a document
        // without images.
        if self.images.is_empty() && !self.images.loading() {
            return None;
        }
        let language = language?;
        let key = ImageKey::diagram(language, code, self.looks.diagram, self.document);
        let align = match crate::typeset::diagrams::names::centred(language) {
            true => Align::Centre,
            false => Align::Left,
        };
        self.placed(key, size::Wanted::default(), width, align)
    }

    /// Why the code block `code` in `language` could not be drawn as a
    /// diagram, in the error colour, to go below it; nothing when it was
    /// drawn, is no diagram, or the setting keeps the reason out of the
    /// document.
    fn render_error(&self, language: Option<&str>, code: &str, width: usize) -> Vec<Line> {
        let language = match language {
            Some(language) if self.render_errors.inline() && !self.images.is_empty() => language,
            _ => return Vec::new(),
        };
        let key = ImageKey::diagram(language, code, self.looks.diagram, self.document);
        let Some(Err(ImageError::Diagram { reason, .. })) = self.images.get(&key) else {
            return Vec::new();
        };
        // The language as written, as the warning on standard error has it.
        let noun = crate::typeset::diagrams::names::noun(language);
        let text = format!("✗ {noun} could not be drawn: {reason}");
        let style = self.style(Style::fg(self.look.error));
        let (indent, max) = self.below_code(width);
        // Each line of the reason wrapped on its own and keeping its indent,
        // such as a pointer to the column of a mistake in the line above.
        let mut lines = Vec::new();
        for line in text.lines() {
            let text = line.trim_start();
            let own = (line.len() - text.len()).min(max.saturating_sub(1));
            let spans = [Span::new(text, style.clone())];
            for mut spans in wrap(&spans, max - own) {
                if indent + own > 0 {
                    spans.insert(0, Span::new(&" ".repeat(indent + own), Style::default()));
                }
                lines.push(Line::new(spans));
            }
        }
        lines
    }

    /// The image `key` at the size `wanted`, at most `width` cells wide,
    /// placed in that width as `align` says, when the terminal can draw it
    /// and it loaded; while it loads, its placeholder at the size the old
    /// image would have, when it has one, and the band below that tells
    /// it is loading.
    fn placed(
        &self,
        key: ImageKey,
        wanted: size::Wanted,
        width: usize,
        align: Align,
    ) -> Option<Vec<Line>> {
        if !rendition::shows(RenditionKind::Picture, self.caps) {
            return None;
        }
        let (natural, shows) = match rendition::picture_state(self.images, &key) {
            PictureState::Loaded(source) => (source.size(), Shows::Image),
            PictureState::Dimmed(old) => (old.size(), Shows::Placeholder),
            PictureState::Rows(rows) => {
                let (width, height) = size::cell_size(self.caps);
                let size = size::Size {
                    width: u32::from(width),
                    height: u32::from(height) * u32::from(rows),
                    columns: 1,
                    rows,
                };
                let shows = Shows::Placeholder;
                let mut lines = image_rows(Placement::block(key.clone(), size, shows));
                self.loading_band(&key, 1, &mut lines);
                return Some(lines);
            }
            PictureState::Missing => return None,
        };
        let max_cells = self.caps.max_image_cells();
        let max_columns = u16::try_from(width).unwrap_or(u16::MAX).min(max_cells);
        let size = size::size(
            natural,
            wanted,
            size::cell_size(self.caps),
            max_columns,
            max_cells,
        )?;
        let columns = usize::from(size.columns);
        let mut lines = image_rows(Placement::block(key.clone(), size, shows));
        let padding = match align {
            Align::Left => 0,
            Align::Centre => width.saturating_sub(columns) / 2,
        };
        if padding > 0 {
            for line in &mut lines {
                line.prepend(Span::new(&" ".repeat(padding), Style::default()));
            }
        }
        self.loading_band(&key, columns, &mut lines);
        Some(lines)
    }

    /// Marks the last of `lines` with the band that tells that the image
    /// `key` is loading, `columns` cells wide, when it is; [`settle_bands`]
    /// moves it to the blank line after the block, when there is one.
    fn loading_band(&self, key: &ImageKey, columns: usize, lines: &mut [Line]) {
        if self.images.placeholder(key).is_none() || !self.caps.draws_images() {
            return;
        }
        let above = lines.len().saturating_sub(1);
        let Some(last) = lines.last_mut() else {
            return;
        };
        let columns = u16::try_from(columns)
            .unwrap_or(u16::MAX)
            .clamp(1, self.caps.max_image_cells());
        let band = Band {
            columns,
            indent: 0,
            low: true,
        };
        last.loading = Some(loading(self.caps, key.clone(), above, band));
    }

    fn table(&self, table: &Table, width: usize) -> Vec<Line> {
        let header_style = self.style(Style {
            bold: true,
            ..Style::default()
        });
        let body_style = Style::default();
        // The cells are the texts of the table, a row at a time.
        let number = Cell::new(0);
        let grid = table_grid(table, |content, header| {
            self.text_is(Some(number.replace(number.get() + 1)));
            let mut spans = Vec::new();
            let style = if header { &header_style } else { &body_style };
            self.inlines(content, style, &mut spans);
            table::Content::Text(spans)
        });
        table::layout(&grid, width, &self.table_styles(header_style))
    }

    /// The styles of a table whose headers are in `header`.
    fn table_styles(&self, header: Style) -> table::Styles {
        let rows = match self.table_rows {
            TableRows::Off => table::Rows::Plain,
            TableRows::Lines => table::Rows::Lines,
            TableRows::Indent => table::Rows::Indent,
            // Without underline styles there is no underline colour, and
            // each cell's underline would take its own text colour.
            TableRows::Underline if self.caps.styling && self.caps.styled_underlines => {
                table::Rows::Underline {
                    color: self.look.table_border.filter(|_| self.caps.color),
                }
            }
            TableRows::Underline => table::Rows::Plain,
            TableRows::Zebra => match self.stripe() {
                Some((stripe, page)) => table::Rows::Stripes { stripe, page },
                None => table::Rows::Plain,
            },
        };
        table::Styles {
            border: self.style(Style::fg(self.look.table_border)),
            header,
            rows,
            base: None,
        }
    }

    /// The colour of a striped row and the page's: a little of the text
    /// colour mixed into the page, so that it shows on a light page and a
    /// dark one alike. `None` when either colour is not known as RGB, or
    /// when the terminal could not show so small a difference, without
    /// colours or 24-bit colour.
    fn stripe(&self) -> Option<(Color, Color)> {
        if !(self.caps.styling && self.caps.color && self.caps.truecolor) {
            return None;
        }
        let text = self.look.foreground.or(self.caps.foreground);
        Some((
            self.mixed_into_page(text, self.look.cells.stripe)?,
            self.page()?,
        ))
    }

    /// The page's colour as RGB: the theme's background, or else the
    /// terminal's. `None` when it is not known.
    fn page(&self) -> Option<Color> {
        let page = self.look.background.or(self.caps.background)?;
        page.rgb(&self.caps.palette)
    }

    /// `amount` of `color` mixed into the page. `None` when either is not
    /// known as RGB.
    fn mixed_into_page(&self, color: Option<Color>, amount: f32) -> Option<Color> {
        self.page()?.mix(color?.rgb(&self.caps.palette)?, amount)
    }

    /// Whether the page is dark, as it is taken to be when the terminal
    /// did not tell.
    fn dark(&self) -> bool {
        self.caps.dark_background.unwrap_or(true)
    }

    fn rule(&self, width: usize) -> Line {
        Line::new(vec![Span::new(
            &"─".repeat(width),
            self.style(Style::fg(self.look.rule)),
        )])
    }

    /// Text played down, as [`style::muted`](super::style::muted) has it.
    fn muted(&self, style: Style) -> Style {
        self.style(super::style::muted_in(self.look.muted, style))
    }

    /// Drops all styling when the terminal gets plain text.
    fn style(&self, style: Style) -> Style {
        if self.caps.styling {
            style
        } else {
            Style::default()
        }
    }
}

/// The lines of an image at `placement`, one for each row it covers.
pub(crate) fn image_rows(placement: Placement) -> Vec<Line> {
    let rows = placement.size.rows;
    let placement = Rc::new(placement);
    (0..rows)
        .map(|row| Line::image(Rc::clone(&placement), row))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::typeset::images::Placeholder;
    use crate::typeset::parse::parse;
    use crate::typeset::style::text;

    #[test]
    fn plain_headings_have_no_text_sizing() {
        let document = parse("# Kiri\n");
        let mut inputs = inputs();
        let sized = layout(&document, &inputs);
        assert!(sized.iter().any(|line| line.rows() > 1), "{sized:?}");
        inputs.headings = Headings::Plain;
        let lines = layout(&document, &inputs);
        assert!(lines.iter().all(|line| line.rows() == 1), "{lines:?}");
        let mut out = Vec::new();
        crate::render::render(&document, &inputs, &mut out).unwrap();
        let out = String::from_utf8(out).unwrap();
        assert!(!out.contains("\x1b]66;"), "{out:?}");
    }

    #[test]
    fn a_raw_html_block_wraps_to_the_width_and_keeps_its_text() {
        let source = format!("<div>\n  {}</div>\n", "word ".repeat(30));
        let lines = layout(&parse(&source), &inputs());
        let texts: Vec<String> = lines.iter().map(|line| text(&line.spans)).collect();
        assert!(
            texts.iter().all(|line| line.chars().count() <= 60),
            "{texts:?}"
        );
        assert_eq!(texts.concat(), source.replace('\n', ""), "{texts:?}");
    }

    fn inputs() -> Inputs {
        Inputs::new(Capabilities::kitty(60))
    }

    fn lone(source: &str) -> Stream {
        content(Content::Document(&parse(source)), &inputs())
    }

    fn file_of(name: &str, source: &str) -> File {
        File {
            name: name.to_owned(),
            document: parse(source),
        }
    }

    /// Each line's text with the path of its block: `None` for a line of
    /// no block, and an empty path for the header of a file.
    fn marked(stream: &Stream) -> Vec<(String, Option<Vec<usize>>)> {
        let table = &stream.blocks;
        stream
            .lines
            .iter()
            .map(|line| {
                let path = line.block.map(|id| table.path(id).to_vec());
                (text(&line.spans), path)
            })
            .collect()
    }

    /// The paths of the blocks of the lines that have text, one for each
    /// run of lines of a block, skipping the blank ones.
    fn paths(stream: &Stream) -> Vec<Vec<usize>> {
        let mut paths: Vec<Vec<usize>> = Vec::new();
        for (text, path) in marked(stream) {
            let Some(path) = path else { continue };
            if !text.trim().is_empty() && paths.last() != Some(&path) {
                paths.push(path);
            }
        }
        paths
    }

    /// What each line of `source` laid out with `inputs` shows of an
    /// image: its text when it shows none, and then the band drawn over
    /// it, if any: how wide, how far in, and whether low or high in the
    /// row.
    fn shown(source: &str, inputs: &Inputs) -> Vec<String> {
        let lines = layout(&parse(source), inputs);
        (lines.iter())
            .map(|line| {
                let shown = match &line.image {
                    Some(row) => {
                        let placement = &row.placement;
                        let size = placement.size;
                        let what = format!("{:?} {}", placement.shows, placement.key);
                        format!("{what} {}×{} row {}", size.columns, size.rows, row.row)
                    }
                    None => text(&line.spans).trim_end().to_owned(),
                };
                match line
                    .loading
                    .as_ref()
                    .and_then(|loading| loading.band.as_ref())
                {
                    Some(band) => {
                        let at = if band.low { "low" } else { "high" };
                        format!("{shown} [band {} at {} {at}]", band.columns, band.indent)
                    }
                    None => shown,
                }
            })
            .collect()
    }

    /// Inputs for kitty with cells of 10 × 20 pixels, a CSS pixel to a
    /// pixel, and `key` loading with `placeholder` in its place.
    fn loading(key: impl Into<ImageKey>, placeholder: Placeholder) -> Inputs {
        let mut inputs = Inputs::new(Capabilities {
            cell_pixels: Some((10, 20)),
            ..Capabilities::kitty(60)
        });
        let key = inputs.drawn(key);
        inputs.images.wait(key, placeholder);
        inputs
    }

    /// The pixels of an image of `width` × `height`.
    fn pixels(width: u32, height: u32) -> crate::typeset::images::Source {
        crate::typeset::images::Source::Raster(::image::RgbaImage::new(width, height))
    }

    #[test]
    fn an_image_that_loads_shows_its_old_image_at_its_size_and_the_band_in_the_blank_line_below() {
        let inputs = loading("a.png", Placeholder::Dimmed(pixels(40, 30)));
        assert_eq!(
            shown("Above\n\n![a](a.png)\n\nBelow\n", &inputs),
            [
                "Above",
                "",
                "Placeholder a.png 4×2 row 0",
                "Placeholder a.png 4×2 row 1",
                " [band 4 at 2 high]",
                "Below",
            ]
        );
    }

    #[test]
    fn a_terminal_that_places_no_band_gets_the_loading_data_and_no_band() {
        let source = "![a](a.png)\n\nBelow\n";
        let cells = Some((10, 20));
        let tmux = |caps: Capabilities| Capabilities {
            cell_pixels: cells,
            tmux_passthrough: true,
            ..caps
        };
        let sixel = Capabilities::sixel(60);
        let terminals = [
            ("kitty", Capabilities::kitty(60), true),
            ("kitty under tmux", tmux(Capabilities::kitty(60)), false),
            ("sixel", sixel, false),
        ];
        for (name, caps, placed) in terminals {
            let caps = Capabilities {
                cell_pixels: cells,
                ..caps
            };
            let mut inputs = Inputs::new(caps);
            let key = inputs.drawn("a.png");
            inputs.images.wait(key, Placeholder::Dimmed(pixels(40, 30)));
            let lines = layout(&parse(source), &inputs);
            let loading: Vec<_> = lines
                .iter()
                .filter_map(|line| line.loading.as_ref())
                .collect();
            // The order the images load in needs the key and the lines above
            // whether or not a band is drawn.
            assert_eq!(loading.len(), 1, "{name}");
            assert_eq!(loading[0].key, ImageKey::from("a.png"), "{name}");
            assert_eq!(loading[0].above, 2, "{name}");
            assert_eq!(loading[0].band.is_some(), placed, "{name}");
        }
    }

    #[test]
    fn without_a_blank_line_below_the_band_goes_over_the_bottom_of_the_last_row() {
        let inputs = loading("a.png", Placeholder::Dimmed(pixels(40, 30)));
        // At the end, and in a tight list, after the bullet.
        assert_eq!(
            shown("Above\n\n![a](a.png)\n", &inputs),
            [
                "Above",
                "",
                "Placeholder a.png 4×2 row 0",
                "Placeholder a.png 4×2 row 1 [band 4 at 2 low]",
            ]
        );
        let lines = shown("- ![a](a.png)\n- b\n", &inputs);
        assert_eq!(lines.len(), 3, "{lines:?}");
        assert!(lines[1].ends_with("row 1 [band 4 at 4 low]"), "{lines:?}");
    }

    #[test]
    fn a_placeholder_takes_the_size_the_old_image_had_and_the_new_block_asks_for() {
        let inputs = loading("a.png", Placeholder::Dimmed(pixels(40, 30)));
        assert_eq!(
            shown("<img src=\"a.png\" width=\"80\">\n", &inputs),
            [
                "Placeholder a.png 8×3 row 0",
                "Placeholder a.png 8×3 row 1",
                "Placeholder a.png 8×3 row 2 [band 8 at 2 low]",
            ]
        );
    }

    #[test]
    fn a_diagram_that_loads_without_a_size_stays_code_with_the_band_over_its_last_row() {
        let key = ImageKey::rendered("mermaid", "flowchart TD\n");
        let inputs = loading(key, Placeholder::Unsized);
        let lines = shown("```mermaid\nflowchart TD\n```\n", &inputs);
        assert!(
            lines.iter().any(|line| line.contains("flowchart TD")),
            "{lines:?}"
        );
        assert!(
            lines.last().unwrap().ends_with("[band 56 at 2 low]"),
            "{lines:?}"
        );
    }

    #[test]
    fn an_image_that_loads_without_a_size_shows_its_alt_text_with_the_band_over_it() {
        let inputs = loading("a.png", Placeholder::Unsized);
        let lines = shown("![a picture](a.png)\n", &inputs);
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(lines[0].contains("a picture"), "{lines:?}");
        assert!(lines[0].ends_with("[band 56 at 2 low]"), "{lines:?}");
    }

    /// The band in `lines`, where it is, which image it tells of and how
    /// many lines above it show that.
    fn band_of(lines: &[Line]) -> (usize, String, usize) {
        let (at, loading) = (lines.iter().enumerate())
            .find_map(|(at, line)| Some((at, line.loading.as_ref()?)))
            .expect("a band");
        (at, loading.key.to_string(), loading.above)
    }

    #[test]
    fn a_band_tells_which_image_loads_and_the_lines_above_it_that_show_it() {
        let inputs = loading("a.png", Placeholder::Dimmed(pixels(40, 30)));
        // On the blank line below the two rows of the image.
        let below = layout(&parse("Above\n\n![a](a.png)\n\nBelow\n"), &inputs);
        assert_eq!(band_of(&below), (4, "a.png".to_owned(), 2));
        // Over the last of them.
        let over = layout(&parse("Above\n\n![a](a.png)\n"), &inputs);
        assert_eq!(band_of(&over), (3, "a.png".to_owned(), 1));
        // A diagram's code, from its first line.
        let key = ImageKey::rendered("mermaid", "a\nb\n");
        let inputs = loading(key.clone(), Placeholder::Unsized);
        let code = layout(&parse("```mermaid\na\nb\n```\n"), &inputs);
        assert_eq!(
            band_of(&code),
            (code.len() - 1, key.to_string(), code.len() - 1)
        );
    }

    #[test]
    fn a_box_of_rows_takes_those_rows_for_an_image_and_a_diagram() {
        let inputs = loading("a.png", Placeholder::Rows(3));
        assert_eq!(
            shown("<img src=\"a.png\" width=\"80\">\n", &inputs),
            [
                "Placeholder a.png 1×3 row 0",
                "Placeholder a.png 1×3 row 1",
                "Placeholder a.png 1×3 row 2 [band 1 at 2 low]",
            ]
        );
        let code = "flowchart TD\n  A --> B\n  B --> C\n  C --> D\n";
        let inputs = loading(ImageKey::rendered("mermaid", code), Placeholder::Rows(3));
        let lines = shown(&format!("```mermaid\n{code}```\n"), &inputs);
        assert_eq!(lines.len(), 3, "{lines:?}");
        assert!(lines[0].starts_with("Placeholder mermaid"), "{lines:?}");
    }

    #[test]
    fn an_image_that_failed_and_loads_again_shows_its_old_image_as_a_reload_does() {
        let source = "![a](a.png)\n";
        let mut failed = Inputs::new(Capabilities {
            cell_pixels: Some((10, 20)),
            ..Capabilities::kitty(60)
        });
        // `insert` drops a placeholder, so the wait comes after it.
        failed.images.insert("a.png", Err(ImageError::Remote));
        failed
            .images
            .wait("a.png".into(), Placeholder::Dimmed(pixels(40, 30)));
        let reloading = loading("a.png", Placeholder::Dimmed(pixels(40, 30)));
        let lines = shown(source, &failed);
        assert_eq!(lines, shown(source, &reloading));
        assert!(lines[0].starts_with("Placeholder a.png 4×2"), "{lines:?}");
    }

    #[test]
    fn nothing_tells_of_a_load_where_the_terminal_draws_no_images() {
        let mut inputs = Inputs::new(Capabilities::ansi(60));
        inputs
            .images
            .wait("a.png".into(), Placeholder::Dimmed(pixels(40, 30)));
        let lines = shown("![a picture](a.png)\n", &inputs);
        assert_eq!(lines.len(), 1, "{lines:?}");
    }

    #[test]
    fn a_block_marks_all_its_lines_and_the_blank_between_blocks_none() {
        let stream = lone("One\n\n- a\n\n  b\n\nTwo\n");
        let lines = marked(&stream);
        let blocks: Vec<_> = lines
            .iter()
            .map(|(_, path)| path.as_ref().map(|path| path[0]))
            .collect();
        // The blank line between the two paragraphs of the loose list item
        // is inside the list, so it keeps the list's block.
        let a = lines
            .iter()
            .position(|(text, _)| text.contains('a'))
            .unwrap();
        assert!(stream.lines[a + 1].is_blank(), "{lines:?}");
        assert_eq!(blocks[a + 1], Some(1), "{lines:?}");
        // Blank separators between blocks have no block.
        assert_eq!(blocks.iter().filter(|block| block.is_none()).count(), 2);
        assert_eq!(paths(&stream), [[0], [1], [2]]);
        // Only blank lines go without a block.
        for (text, path) in &lines {
            if path.is_none() {
                assert!(text.is_empty(), "{lines:?}");
            }
        }
    }

    #[test]
    fn two_lines_of_a_block_are_the_same_block_and_two_blocks_are_not() {
        let stream = lone(
            "One two three four five six seven eight nine ten eleven twelve thirteen fourteen fifteen sixteen\n\nNext\n",
        );
        let ids: Vec<_> = stream.lines.iter().filter_map(|line| line.block).collect();
        assert!(ids.len() >= 3, "{ids:?}");
        assert_eq!(ids[0], ids[1]);
        assert_ne!(ids[0], ids[ids.len() - 1]);
    }

    #[test]
    fn a_details_summary_is_the_details_block_and_its_content_is_deeper() {
        let stream = lone(
            "Before\n\n<details>\n<summary>Outer</summary>\n\nIn outer\n\n\
             <details>\n<summary>Inner</summary>\n\nDeep\n\n</details>\n\n\
             Back in outer\n\n</details>\n\nAfter\n",
        );
        assert_eq!(
            paths(&stream),
            [
                vec![0],
                vec![1],
                vec![1, 0],
                vec![1, 1],
                vec![1, 1, 0],
                vec![1, 2],
                vec![2]
            ],
            "{:?}",
            marked(&stream)
        );
        let lines = marked(&stream);
        let path = |find: &str| {
            let (_, path) = lines.iter().find(|(text, _)| text.contains(find)).unwrap();
            path.clone()
        };
        assert_eq!(path("Outer"), Some(vec![1]));
        assert_eq!(path("Inner"), Some(vec![1, 1]));
        assert_eq!(path("Deep"), Some(vec![1, 1, 0]));
    }

    #[test]
    fn a_details_in_reading_mode_keeps_a_place_for_its_summary_and_each_block_in_it() {
        let source = "Before\n\n<details>\n<summary>Outer</summary>\n\nIn outer\n\n\
                      <details>\n<summary>Inner</summary>\n\nDeep\n\n</details>\n\n\
                      Back in outer\n\n</details>\n\nAfter\n";
        let inputs = Inputs {
            reading: Some(crate::typeset::reading::engine::Reading {
                pictures: Default::default(),
                fonts: Rc::new(crate::typeset::reading::Fonts::fixture()),
                wide: crate::typeset::reading::page::Wide::Wrap,
            }),
            caps: Capabilities {
                cell_pixels: Some((10, 20)),
                ..Capabilities::kitty(80)
            },
            ..Inputs::new(Capabilities::kitty(80))
        };
        let stream = content(Content::Document(&parse(source)), &inputs);
        let mut seen = Vec::new();
        for id in stream.lines.iter().filter_map(|line| line.block) {
            let path = stream.blocks.path(id).to_vec();
            if seen.last() != Some(&path) {
                seen.push(path);
            }
        }
        assert_eq!(
            seen,
            [
                vec![0],
                vec![1],
                vec![1, 0],
                vec![1, 1],
                vec![1, 1, 0],
                vec![1, 2],
                vec![2]
            ]
        );
    }

    #[test]
    fn blocks_are_found_in_the_source_map_by_their_path() {
        let document = parse(
            "Before\n\n<details>\n<summary>Outer</summary>\n\nIn outer\n\n</details>\n\n- item\n",
        );
        let stream = content(Content::Document(&document), &inputs());
        let mut seen = Vec::new();
        for line in stream.lines.iter().filter_map(|line| line.block) {
            if seen.last() != Some(&line) {
                seen.push(line);
            }
        }
        let lines: Vec<_> = seen
            .iter()
            .map(|&id| document.places.line(stream.blocks.get(id).path()))
            .collect();
        assert_eq!(lines, [Some(1), Some(3), Some(6), Some(10)]);
    }

    #[test]
    fn the_lines_of_blocks_are_those_of_their_own_file() {
        let shown = [
            file_of("a.md", "One\n\nTwo\n"),
            file_of("b.md", "\n\nThree\n"),
        ];
        let stream = files(&shown, &inputs());
        let line_of = |find: &str| {
            let line = stream
                .lines
                .iter()
                .find(|line| text(&line.spans).contains(find))
                .unwrap();
            let info = stream.blocks.get(line.block.unwrap());
            shown[info.file()].document.places.line(info.path())
        };
        assert_eq!(line_of("Two"), Some(3));
        assert_eq!(line_of("Three"), Some(3));
        // The header of a file is not in the source.
        let header = stream.lines[0].block.unwrap();
        let info = stream.blocks.get(header);
        assert_eq!(shown[info.file()].document.places.line(info.path()), None);
    }

    #[test]
    fn files_mark_their_headers_with_the_file() {
        let shown = [file_of("a.md", "One\n\nTwo\n"), file_of("b.md", "Three\n")];
        let stream = files(&shown, &inputs());
        let table = &stream.blocks;
        let file_of_line = |line: &Line| line.block.map(|id| table.get(id).file());
        let start = stream
            .lines
            .iter()
            .position(|line| file_of_line(line) == Some(1))
            .unwrap();
        // The header, up to the first block, has the file's entry.
        let header = &stream.lines[..stream
            .lines
            .iter()
            .position(|line| text(&line.spans).contains("One"))
            .unwrap()
            - 1];
        assert!(header.len() >= 3);
        for line in header {
            assert_eq!(file_of_line(line), Some(0));
            assert_eq!(table.path(line.block.unwrap()), &[] as &[usize]);
        }
        // The blank line before the second file has no block.
        assert!(stream.lines[start - 1].is_blank());
        assert_eq!(stream.lines[start - 1].block, None);
        // The second file's header and block.
        assert_eq!(file_of_line(&stream.lines[start]), Some(1));
        let three = stream
            .lines
            .iter()
            .find(|line| text(&line.spans).contains("Three"))
            .unwrap();
        assert_eq!(file_of_line(three), Some(1));
        assert_eq!(table.path(three.block.unwrap()), [0]);
        // The blank line between a header and the first block has none.
        let first = stream
            .lines
            .iter()
            .position(|line| text(&line.spans).contains("One"))
            .unwrap();
        assert_eq!(stream.lines[first - 1].block, None);
    }

    /// The paths the layout records for a document with blocks nested in
    /// every kind of container are paths of blocks of the document, as
    /// `compare` counts them.
    #[test]
    fn the_paths_of_the_blocks_table_are_paths_of_the_documents_blocks() {
        let document = parse(crate::typeset::compare::NESTED);
        let known: Vec<Vec<usize>> = crate::typeset::compare::blocks(&document)
            .into_iter()
            .map(|(path, _)| path)
            .collect();
        let reading = Inputs {
            reading: Some(crate::typeset::reading::engine::Reading {
                pictures: Default::default(),
                fonts: Rc::new(crate::typeset::reading::Fonts::fixture()),
                wide: crate::typeset::reading::page::Wide::Wrap,
            }),
            caps: Capabilities {
                cell_pixels: Some((10, 20)),
                ..Capabilities::kitty(80)
            },
            ..Inputs::new(Capabilities::kitty(80))
        };
        for inputs in [inputs(), reading] {
            let stream = content(Content::Document(&document), &inputs);
            let mut recorded = 0;
            for line in &stream.lines {
                if let Some(id) = line.block {
                    let path = stream.blocks.path(id);
                    recorded += 1;
                    assert!(known.iter().any(|known| known == path), "{path:?}");
                }
            }
            assert!(recorded > 0);
        }
    }

    /// The lines of the fixture and its table agree: the blocks are in
    /// order, each is used, and all belong to the one document.
    fn assert_agree(fixture: &str) {
        let source = std::fs::read_to_string(format!("tests/fixtures/{fixture}")).unwrap();
        let stream = lone(&source);
        let mut seen = Vec::new();
        for line in &stream.lines {
            if let Some(id) = line.block {
                assert_eq!(stream.blocks.get(id).file(), 0);
                if seen.last() != Some(&id) {
                    seen.push(id);
                }
            }
        }
        let paths: Vec<_> = seen.iter().map(|&id| stream.blocks.path(id)).collect();
        // Each block is one run of lines, so none is seen twice, and the
        // blocks follow each other as the document does.
        for (number, path) in paths.iter().enumerate() {
            assert!(!paths[..number].contains(path), "{fixture}: {path:?}");
            assert!(!path.is_empty());
        }
        assert!(
            paths.windows(2).all(|pair| pair[0] < pair[1]),
            "{fixture}: {paths:?}"
        );
        // Every block of the table is a block of the document, in the
        // document's order, and the innermost block of some line unless it
        // holds blocks that have all its lines, or is not shown.
        let known: Vec<Vec<usize>> = (parse(&source).blocks().into_iter())
            .map(|(path, _)| path)
            .collect();
        let table: Vec<&[usize]> = (stream.blocks.ids())
            .map(|id| stream.blocks.path(id))
            .collect();
        assert!(
            table.windows(2).all(|pair| pair[0] < pair[1]),
            "{fixture}: {table:?}"
        );
        let inner: Vec<_> = stream.lines.iter().filter_map(|line| line.inner).collect();
        for id in stream.blocks.ids() {
            let path = stream.blocks.path(id);
            assert!(
                known.iter().any(|known| known == path),
                "{fixture}: {path:?}"
            );
            // A block whose lines are all of the blocks inside it, such as a
            // list, is the innermost block of none.
            let holds_blocks = table
                .iter()
                .any(|at| at.len() > path.len() && at.starts_with(path));
            // An empty heading and an empty code block have no lines.
            let empty = (parse(&source).blocks().into_iter()).any(|(at, block)| {
                at == path
                    && match block {
                        Block::Heading { content, .. } => content.is_empty(),
                        Block::CodeBlock { code, .. } => code.is_empty(),
                        _ => false,
                    }
            });
            assert!(
                inner.contains(&id) || holds_blocks || empty || stream.blocks.is_hidden(id),
                "{fixture}: {path:?} is the innermost block of no line"
            );
        }
    }

    #[test]
    fn the_table_and_the_lines_agree_for_blocks_in_quotes_and_lists() {
        assert_agree("nested.md");
    }

    #[test]
    fn the_table_and_the_lines_agree_for_html_tags() {
        assert_agree("html-tags.md");
    }

    /// Inputs for kitty with cells of 10 × 20 pixels, a CSS pixel to a
    /// pixel, and `key` loaded as `image`.
    fn loaded(key: impl Into<ImageKey>, image: crate::typeset::images::Source) -> Inputs {
        let mut inputs = Inputs::new(Capabilities {
            cell_pixels: Some((10, 20)),
            ..Capabilities::kitty(60)
        });
        let key = inputs.drawn(key);
        inputs.images.insert(key, Ok(image));
        inputs
    }

    /// Each line of `source` laid out with `inputs`, as its lead and
    /// spans show, with a row of an image as how many cells it covers.
    fn rows(source: &str, inputs: &Inputs) -> Vec<String> {
        let lines = layout(&parse(source), inputs);
        (lines.iter())
            .map(|line| {
                let before = format!("{}{}", text(&line.lead), text(&line.spans));
                match &line.image {
                    Some(row) => format!("{before}⟦{} cells⟧", row.placement.size.columns),
                    None => before.trim_end().to_owned(),
                }
            })
            .collect()
    }

    #[test]
    fn a_formula_is_centred_and_a_diagram_is_not() {
        // 4 cells in the 56 between the margins of 2.
        let formula = loaded(ImageKey::rendered("math", "x\n"), pixels(40, 20));
        assert_eq!(
            rows("$$x$$\n", &formula),
            [format!("{}⟦4 cells⟧", " ".repeat(2 + 26))]
        );
        let diagram = loaded(ImageKey::rendered("mermaid", "x\n"), pixels(40, 20));
        assert_eq!(rows("```mermaid\nx\n```\n", &diagram), ["  ⟦4 cells⟧"]);
        let image = loaded("a.png", pixels(40, 20));
        assert_eq!(rows("![a](a.png)\n", &image), ["  ⟦4 cells⟧"]);
    }

    #[test]
    fn a_formula_is_centred_in_the_room_of_its_list_item_quote_or_alert() {
        let formula = || loaded(ImageKey::rendered("math", "x\n"), pixels(40, 20));
        // 4 cells in the 54 after the bullet.
        assert_eq!(
            rows("- $$x$$\n", &formula()),
            [format!("  • {}⟦4 cells⟧", " ".repeat(25))]
        );
        let quoted = rows("> $$x$$\n", &formula());
        assert_eq!(quoted.len(), 1, "{quoted:?}");
        assert!(
            quoted[0].ends_with(&format!("{}⟦4 cells⟧", " ".repeat(25))),
            "{quoted:?}"
        );
        let alert = rows("> [!NOTE]\n> $$x$$\n", &formula());
        assert!(
            alert
                .last()
                .unwrap()
                .ends_with(&format!("{}⟦4 cells⟧", " ".repeat(25))),
            "{alert:?}"
        );
    }

    #[test]
    fn a_formula_wider_than_the_room_fills_it() {
        let formula = loaded(ImageKey::rendered("math", "x\n"), pixels(1000, 20));
        assert_eq!(rows("$$x$$\n", &formula), ["  ⟦56 cells⟧"]);
    }

    #[test]
    fn a_formula_that_loads_is_centred_with_its_band() {
        let key = ImageKey::rendered("math", "x\n");
        let inputs = loading(key, Placeholder::Dimmed(pixels(40, 20)));
        assert_eq!(
            shown("$$x$$\n\nBelow\n", &inputs),
            [
                "Placeholder math block 4×1 row 0",
                " [band 4 at 28 high]",
                "Below",
            ]
        );
    }
}
