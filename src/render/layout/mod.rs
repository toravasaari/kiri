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

mod code;
mod front_matter;
mod list;
mod quote;

pub use code::CodeColors;
pub(crate) use code::{expand_tabs, inline_code_background};

use std::borrow::Cow;
use std::ops::Range;
use std::rc::Rc;

use super::block::{BlockId, Blocks};
use super::image::{self, Placement, Shows};
use super::reading::Reading;
use super::script::{self, Script};
use super::style::{Band, Line, Spaces, Span, Style, push};
use super::table::{self, Cells};
use super::wrap::{width, wrap};
use super::{FrontMatterStyle, Headings, Inputs, RenderErrors, TableRows};
use crate::caps::Capabilities;
use crate::color::{Color, Underline};
use crate::images::{ImageError, ImageKey, Images, Placeholder, Source, heading_words};
use crate::model::{Block, Document, File, Image, Inline, SourceLines, Table};
use crate::reading::page;
use crate::size::{TextSize, VerticalAlign};
use crate::theme::{BandColor, Theme};

/// How much of the text colour a striped table row mixes into the page.
const STRIPE: f32 = 0.06;

/// How much of the text colour the band of a file's header mixes into a
/// dark page and a light one: as much on a light page would be a heavy
/// grey.
const BAND_DARK: f32 = 0.3;
const BAND_LIGHT: f32 = 0.12;

/// `document` laid out, without the table of its blocks, as [`file`] has
/// it.
pub fn layout(document: &Document, inputs: &Inputs) -> Vec<Line> {
    document_stream(document, inputs).lines
}

/// `document` laid out as a lone document, with the blocks its lines come
/// from.
fn document_stream(document: &Document, inputs: &Inputs) -> Stream {
    let stream = with_layout(inputs, 0, |layout, width, margin| {
        let mut stream = Stream::default();
        let mut marks = Marks::new(&mut stream.blocks, &document.lines);
        layout.top_level(
            &document.blocks,
            width,
            margin,
            &mut stream.lines,
            &mut marks,
        );
        settle_bands(&mut stream.lines);
        stream
    });
    finished(stream, inputs)
}

/// Puts each band in `lines` where it is drawn: on the blank line after
/// its block when there is one, a little below its top, so that the band
/// takes no row and nothing moves when the image comes; otherwise over the
/// bottom of the block's last line. It starts where the block's text or
/// image does.
fn settle_bands(lines: &mut [Line]) {
    let marked: Vec<usize> = (0..lines.len())
        .filter(|&i| lines[i].band.is_some())
        .collect();
    for index in marked {
        let Some(band) = lines[index].band.take() else {
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
        lines[at].band = Some(Band {
            indent: indent + band.indent,
            low: !blank,
            above: band.above + usize::from(blank),
            ..band
        });
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
    with_layout(inputs, number, |layout, width, margin| {
        let mut lines = layout.file_header(&file.name, width, margin);
        stamp(&mut lines, blocks.add(number, Vec::new(), None));
        // The header and the blank line below it.
        let above = u16::try_from(lines.len() + 1).unwrap_or(u16::MAX);
        let layout = Layout {
            reserved: layout.reserved.saturating_add(above),
            ..*layout
        };
        let mut body = Vec::new();
        let mut marks = Marks::new(blocks, &file.document.lines);
        layout.top_level(&file.document.blocks, width, margin, &mut body, &mut marks);
        append_block(&mut lines, body, true);
        settle_bands(&mut lines);
        lines
    })
}

/// What `lay_out` returns for a layout with `inputs` of the document
/// numbered `number`, given the width of the text and its margin.
fn with_layout<T>(
    inputs: &Inputs,
    number: usize,
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
    } = inputs;
    // Without colours, the layout leaves out what only shows a colour, such
    // as heading fills and the padding around inline code. The built-in
    // theme's greys follow the terminal's palette, which may hide them.
    let adjusted;
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
    let mut width = usize::from(caps.width);
    if theme.max_width > 0 {
        width = width.min(usize::from(theme.max_width));
    }
    let width = width.max(10);
    let layout = Layout {
        images,
        caps,
        theme,
        front_matter: *front_matter,
        headings: *headings,
        table_rows: *table_rows,
        render_errors: *render_errors,
        // Only where the terminal draws images, which reading mode's text
        // is.
        reading: reading.as_ref().filter(|_| caps.draws_images()),
        document: number,
        // The row below for the shell's prompt or the pager's status line.
        reserved: 1,
    };

    // Everything but headings and thematic breaks keeps text_margin free
    // on both sides.
    let margin = usize::from(theme.text_margin).min(width / 4);
    lay_out(&layout, width, margin)
}

/// `lines` with `margin` cells before each one that is not blank.
fn with_margin(mut lines: Vec<Line>, margin: usize) -> Vec<Line> {
    if margin > 0 {
        for line in &mut lines {
            if !line.is_blank() {
                line.lead
                    .insert(0, Span::new(&" ".repeat(margin), Style::default()));
            }
        }
    }
    lines
}

/// Marks `lines` as coming from the block `id`.
fn stamp(lines: &mut [Line], id: BlockId) {
    for line in lines {
        line.block = Some(id);
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
    theme: &'a Theme,
    front_matter: FrontMatterStyle,
    headings: Headings,
    table_rows: TableRows,
    render_errors: RenderErrors,
    /// Reading mode, which lays the text blocks out as pages of their own.
    reading: Option<&'a Reading>,
    /// The number of the document laid out among those shown together,
    /// which the keys of its images carry.
    document: usize,
    /// The rows of the window that an image file shown on its own leaves
    /// to others: the shell's prompt or the pager's status line below it,
    /// and the header above it.
    reserved: u16,
}

/// Where the block being laid out is, and the table that records the
/// blocks laid out.
struct Marks<'a> {
    table: &'a mut Blocks,
    /// The lines of the source the blocks of the document start on.
    source: &'a SourceLines,
    path: Vec<usize>,
}

impl<'a> Marks<'a> {
    fn new(table: &'a mut Blocks, source: &'a SourceLines) -> Self {
        Self {
            table,
            source,
            path: Vec::new(),
        }
    }

    /// Adds the block at the current path, of document `document`.
    fn add(&mut self, document: usize) -> BlockId {
        let line = self.source.get(&self.path);
        self.table.add(document, self.path.clone(), line)
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
    fn top_level(
        &self,
        blocks: &[Block],
        width: usize,
        margin: usize,
        lines: &mut Vec<Line>,
        marks: &mut Marks,
    ) {
        // The blocks that reading mode lays out together, until a block it
        // does not lay out comes.
        let mut run = Vec::new();
        self.top_level_into(blocks, width, margin, lines, marks, &mut run);
        self.reading_page(&mut run, width, lines, marks);
    }

    /// `top_level`, adding the blocks reading mode lays out to `run`
    /// rather than laying out what is left of it at the end.
    fn top_level_into<'b>(
        &self,
        blocks: &'b [Block],
        width: usize,
        margin: usize,
        lines: &mut Vec<Line>,
        marks: &mut Marks,
        run: &mut Vec<(Vec<usize>, Cow<'b, Block>)>,
    ) {
        // Where the other blocks start and how wide they are: in reading
        // mode, from the area's left edge and as wide as the text's column,
        // so that they line up with the text, a centred formula in the
        // middle of the column and a wide image shrunk to it.
        let (lead, inner) = match self.reading {
            Some(_) => {
                let window = usize::from(self.caps.width).max(width);
                let area = super::reading::Area::of(self.caps, window);
                (usize::from(area.left), usize::from(area.column))
            }
            None => (margin, width - 2 * margin),
        };
        for (index, block) in blocks.iter().enumerate() {
            // Hidden front matter is nothing, which the page would still
            // give a place.
            let hidden = matches!(block, Block::FrontMatter(_))
                && self.front_matter == FrontMatterStyle::Hide;
            let path = || [&marks.path[..], &[index]].concat();
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
                marks.path.push(index);
                self.top_level_into(blocks, width, margin, lines, marks, run);
                marks.path.pop();
                continue;
            }
            if self.reading.is_some() && page::lays_out(block) && !hidden {
                run.push((path(), Cow::Borrowed(block)));
                continue;
            }
            self.reading_page(run, width, lines, marks);
            marks.path.push(index);
            let mut block_lines = match block {
                Block::ThematicBreak => vec![self.rule(usize::from(self.caps.width).max(width))],
                Block::Heading { .. } | Block::ImageFile(_) => {
                    self.block(block, width, Context::default())
                }
                Block::Details { summary, blocks } => {
                    let summary = self.summary(summary, inner);
                    let mut summary = with_margin(summary, lead);
                    stamp(&mut summary, marks.add(self.document));
                    append_block(lines, summary, true);
                    self.top_level(blocks, width, margin, lines, marks);
                    marks.path.pop();
                    continue;
                }
                _ => with_margin(self.block(block, inner, Context::default()), lead),
            };
            if !block_lines.is_empty() {
                stamp(&mut block_lines, marks.add(self.document));
            }
            append_block(lines, block_lines, true);
            marks.path.pop();
        }
    }

    /// Lays out the blocks of `run`, each with its path, as a page of
    /// reading mode, and empties it.
    fn reading_page(
        &self,
        run: &mut Vec<(Vec<usize>, Cow<'_, Block>)>,
        width: usize,
        lines: &mut Vec<Line>,
        marks: &mut Marks,
    ) {
        let Some(reading) = self.reading.filter(|_| !run.is_empty()) else {
            return;
        };
        let ids: Vec<BlockId> = (run.iter())
            .map(|(path, _)| {
                let outer = std::mem::replace(&mut marks.path, path.clone());
                let id = marks.add(self.document);
                marks.path = outer;
                id
            })
            .collect();
        let owned: Vec<Cow<Block>> = run.drain(..).map(|(_, block)| block).collect();
        let blocks: Vec<&Block> = owned.iter().map(AsRef::as_ref).collect();
        // Reading mode sets its own width: the theme's widest text is for
        // text in the terminal's font.
        let window = usize::from(self.caps.width).max(width);
        let page = super::reading::lines(
            &blocks,
            &ids,
            reading,
            (
                self.theme,
                &self.code_colors(),
                (self.table_rows != TableRows::Off, self.front_matter),
            ),
            (self.images, self.render_errors.inline(), self.document),
            self.caps,
            window,
        );
        append_block(lines, page, true);
    }

    fn blocks(&self, blocks: &[Block], width: usize, context: Context) -> Vec<Line> {
        let mut lines = Vec::new();
        for block in blocks {
            append_block(
                &mut lines,
                self.block(block, width, context),
                !context.tight,
            );
        }
        lines
    }

    fn block(&self, block: &Block, width: usize, context: Context) -> Vec<Line> {
        match block {
            Block::FrontMatter(front_matter) => self.front_matter(front_matter, width),
            Block::Heading { level, content } => self.heading(*level, content, width, context),
            Block::Paragraph(content) => self.paragraph(content, &Style::default(), width),
            Block::CodeBlock { language, code, .. } => {
                let language = language.as_deref();
                if let Some(lines) = self.diagram(language, code, width) {
                    return lines;
                }
                let mut lines = self.code_block(language, code, width);
                lines.extend(self.render_error(language, code, width));
                if let Some(language) = language {
                    let key = ImageKey::block(language, code).in_document(self.document);
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
                html.lines()
                    .map(|line| Line::new(vec![Span::new(line, style.clone())]))
                    .collect()
            }
            Block::Note(note) => {
                let content = [Inline::Text(note.clone())];
                self.paragraph(&content, &self.muted(Style::default()), width)
            }
            Block::Error(message) => {
                let spans = [Span::new(
                    &format!("✗ {message}"),
                    self.style(Style::fg(self.theme.error)),
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
            fg: self.theme.file_header.bar,
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
        match self.theme.file_header.background {
            BandColor::Color(color) => color,
            BandColor::Auto => {
                // Light text on a dark page, as most terminals have it.
                let contrast = match self.dark() {
                    true => Color::Rgb(0xff, 0xff, 0xff),
                    false => Color::Rgb(0, 0, 0),
                };
                let amount = if self.dark() { BAND_DARK } else { BAND_LIGHT };
                let text = self.theme.foreground.or(self.caps.foreground);
                self.mixed_into_page(text.or(Some(contrast)), amount)
            }
        }
    }

    /// The image file `name` shown on its own, as large as fits the window
    /// with the rows it leaves to others; its alt text, the name, where it
    /// cannot be drawn, at most `width` cells wide.
    fn image_file(&self, name: &str, width: usize) -> Vec<Line> {
        let key = ImageKey::from(name).in_document(self.document);
        if self.caps.draws_images()
            && let Some(Ok(source)) = self.images.get(&key)
            && let Some(size) = image::on_its_own(source, self.caps, self.reserved)
        {
            return image_rows(Placement {
                key,
                size,
                shows: Shows::Image,
            });
        }
        let alt = Inline::Image(Image {
            url: name.to_owned(),
            alt: vec![Inline::Text(name.to_owned())],
            width: None,
            height: None,
        });
        let mut spans = Vec::new();
        self.inline(&alt, &Style::default(), &mut spans);
        wrap(&spans, width).into_iter().map(Line::new).collect()
    }

    fn heading(&self, level: u8, content: &[Inline], width: usize, context: Context) -> Vec<Line> {
        let heading = self.theme.heading(level);
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
        let mut text = Vec::new();
        self.inlines(content, &style, &mut text);
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
        // A nested heading's indent would land before the quote bar or the
        // list marker, which are prepended later.
        let indent = if context.nested {
            0
        } else {
            usize::from(heading.indent).min(width / 4)
        };
        let lead = if indent > 0 {
            vec![Span::new(&" ".repeat(indent), background.clone())]
        } else {
            Vec::new()
        };
        let available = width - indent;
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
            (wrap(&spans, available).into_iter())
                .map(|spans| line(spans, &fill_style))
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
        if !super::heading_images(self.headings, self.caps) {
            return None;
        }
        let cell = image::cell_size(self.caps);
        let words = heading_words(content);
        let key = ImageKey::heading_of(level, words.clone(), cell).in_document(self.document);
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
        let most = self.caps.max_image_cells;
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
            let shrunk = image::size_in((width, height), image::Unit::Pixel, cell, room, rows)?;
            (shrunk, vec![(0..shrunk.columns, 0..words.len())])
        } else {
            let full = image::Size {
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
        let placement = Rc::new(Placement {
            key,
            size,
            shows: Shows::Image,
        });
        let mut rows = Vec::new();
        for (columns, shown) in lines {
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
        let mut spans = Vec::new();
        push(&mut spans, "▼ ", &style);
        self.inlines(summary, &style, &mut spans);
        // The summary is one line, so a line break in it is a space. As in
        // the HTML it comes from, a run of spaces shows as one.
        let mut space = false;
        for span in &mut spans {
            let mut text = String::with_capacity(span.text.len());
            for c in span.text.chars().map(|c| if c == '\n' { ' ' } else { c }) {
                if !(c == ' ' && space) {
                    text.push(c);
                }
                space = c == ' ';
            }
            span.text = text;
        }
        spans.retain(|span| !span.text.is_empty());
        wrap(&spans, width).into_iter().map(Line::new).collect()
    }

    fn paragraph(&self, content: &[Inline], style: &Style, width: usize) -> Vec<Line> {
        if let Some(lines) = self.image_block(content, width) {
            return lines;
        }
        let mut spans = Vec::new();
        self.inlines(content, style, &mut spans);
        let mut lines: Vec<Line> = wrap(&spans, width).into_iter().map(Line::new).collect();
        if let Some(lone) = Inline::lone_image(content) {
            let key = ImageKey::from(lone.url.as_str()).in_document(self.document);
            self.loading_band(&key, width, &mut lines);
        }
        lines
    }

    /// A paragraph of only an image, or of only a link around an image,
    /// shown as the image itself when the terminal can and it loaded.
    /// The image inside a link is not clickable: OSC 8 around placeholders
    /// is not written yet.
    fn image_block(&self, content: &[Inline], width: usize) -> Option<Vec<Line>> {
        let lone = Inline::lone_image(content)?;
        let wanted = image::Wanted {
            width: lone.width,
            height: lone.height,
        };
        let key = ImageKey::from(lone.url.as_str()).in_document(self.document);
        self.placed(key, wanted, width, Align::Left)
    }

    /// A code block drawn as a diagram, when it was.
    fn diagram(&self, language: Option<&str>, code: &str, width: usize) -> Option<Vec<Line>> {
        // Spares building a key for every code block of a document
        // without images.
        if self.images.is_empty() && !self.images.loading() {
            return None;
        }
        let language = language?;
        let key = ImageKey::block(language, code).in_document(self.document);
        let align = match crate::diagrams::centred(language) {
            true => Align::Centre,
            false => Align::Left,
        };
        self.placed(key, image::Wanted::default(), width, align)
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
        let key = ImageKey::block(language, code).in_document(self.document);
        let Some(Err(ImageError::Diagram { reason, .. })) = self.images.get(&key) else {
            return Vec::new();
        };
        // The language as written, as the warning on standard error has it.
        let noun = crate::diagrams::noun(language);
        let text = format!("✗ {noun} could not be drawn: {reason}");
        let style = self.style(Style::fg(self.theme.error));
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
        wanted: image::Wanted,
        width: usize,
        align: Align,
    ) -> Option<Vec<Line>> {
        if !self.caps.draws_images() {
            return None;
        }
        let (natural, shows) = match (self.images.get(&key), self.images.placeholder(&key)) {
            (Some(Ok(source)), _) => (source.size(), Shows::Image),
            (_, Some(Placeholder::Dimmed(old))) => (old.size(), Shows::Placeholder),
            (_, Some(&Placeholder::Rows(rows))) => {
                let (width, height) = image::cell_size(self.caps);
                let size = image::Size {
                    width: u32::from(width),
                    height: u32::from(height) * u32::from(rows),
                    columns: 1,
                    rows,
                };
                let shows = Shows::Placeholder;
                let mut lines = image_rows(Placement {
                    key: key.clone(),
                    size,
                    shows,
                });
                self.loading_band(&key, 1, &mut lines);
                return Some(lines);
            }
            _ => return None,
        };
        let max_cells = self.caps.max_image_cells;
        let max_columns = u16::try_from(width).unwrap_or(u16::MAX).min(max_cells);
        let size = image::size(
            natural,
            wanted,
            image::cell_size(self.caps),
            max_columns,
            max_cells,
        )?;
        let columns = usize::from(size.columns);
        let mut lines = image_rows(Placement {
            key: key.clone(),
            size,
            shows,
        });
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
            .clamp(1, self.caps.max_image_cells);
        last.band = Some(Band {
            columns,
            indent: 0,
            low: true,
            key: key.clone(),
            above,
        });
    }

    fn table(&self, table: &Table, width: usize) -> Vec<Line> {
        let header_style = self.style(Style {
            bold: true,
            ..Style::default()
        });
        let spans = |content: &[Inline], style: &Style| {
            let mut spans = Vec::new();
            self.inlines(content, style, &mut spans);
            spans
        };
        let cells = Cells {
            header: table
                .header
                .iter()
                .map(|c| spans(c, &header_style))
                .collect(),
            rows: (table.rows.iter())
                .map(|row| {
                    (row.iter())
                        .map(|c| spans(c, &Style::default()).into())
                        .collect()
                })
                .collect(),
            alignments: table.alignments.clone(),
            header_column: false,
        };
        table::layout(&cells, width, &self.table_styles(header_style))
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
                    color: self.theme.table_border.filter(|_| self.caps.color),
                }
            }
            TableRows::Underline => table::Rows::Plain,
            TableRows::Zebra => match self.stripe() {
                Some((stripe, page)) => table::Rows::Stripes { stripe, page },
                None => table::Rows::Plain,
            },
        };
        table::Styles {
            border: self.style(Style::fg(self.theme.table_border)),
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
        let text = self.theme.foreground.or(self.caps.foreground);
        Some((self.mixed_into_page(text, STRIPE)?, self.page()?))
    }

    /// The page's colour as RGB: the theme's background, or else the
    /// terminal's. `None` when it is not known.
    fn page(&self) -> Option<Color> {
        let page = self.theme.background.or(self.caps.background)?;
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
            self.style(Style::fg(self.theme.rule)),
        )])
    }

    fn inlines(&self, inlines: &[Inline], style: &Style, out: &mut Vec<Span>) {
        for inline in inlines {
            self.inline(inline, style, out);
        }
    }

    fn inline(&self, inline: &Inline, style: &Style, out: &mut Vec<Span>) {
        match inline {
            Inline::Text(text) => push(out, text, style),
            Inline::Code(code) => self.code(style, out, |style, out| push(out, code, style)),
            // A key is drawn as inline code is.
            Inline::Kbd(content) => {
                self.code(style, out, |style, out| self.inlines(content, style, out))
            }
            Inline::Emphasis(content) => self.inlines(
                content,
                &self.style(Style {
                    italic: true,
                    ..style.clone()
                }),
                out,
            ),
            Inline::Strong(content) => self.inlines(
                content,
                &self.style(Style {
                    bold: true,
                    ..style.clone()
                }),
                out,
            ),
            Inline::Strikethrough(content) => self.inlines(
                content,
                &self.style(Style {
                    strike: true,
                    ..style.clone()
                }),
                out,
            ),
            Inline::Formula { written, .. } => self.inlines(written, style, out),
            Inline::Mark(content) => {
                let mark = self.theme.mark;
                let style = if self.caps.color {
                    Style {
                        fg: mark.foreground.or(style.fg),
                        bg: mark.background.or(style.bg),
                        ..style.clone()
                    }
                } else {
                    Style {
                        reverse: true,
                        ..style.clone()
                    }
                };
                self.inlines(content, &self.style(style), out)
            }
            Inline::Subscript(content) => self.script(Script::Sub, content, style, out),
            Inline::Superscript(content) => self.script(Script::Sup, content, style, out),
            Inline::Link { url, content } => self.link(url, content, style, out),
            Inline::Image(Image { alt, .. }) => {
                let image_style = self.muted(Style {
                    italic: true,
                    ..style.clone()
                });
                push(
                    out,
                    &format!("[image: {}]", Inline::plain_text(alt)),
                    &image_style,
                );
            }
            Inline::SoftBreak => push(out, " ", style),
            Inline::LineBreak => push(out, "\n", style),
            Inline::Html(html) => push(out, html, &self.muted(style.clone())),
        }
    }

    /// A sub- or superscript: with text sizing, `content` at two thirds of
    /// the size around it, in the same rows, a sup at their top and a sub at
    /// their bottom. Without, in Unicode's `script` characters when it is
    /// plain text and every character has one, or else as it is. A script
    /// inside it is drawn as part of it.
    fn script(&self, script: Script, content: &[Inline], style: &Style, out: &mut Vec<Span>) {
        let content = without_scripts(content);
        if self.caps.text_sizing {
            let align = match script {
                Script::Sup => VerticalAlign::Top,
                Script::Sub => VerticalAlign::Bottom,
            };
            let style = Style {
                size: style.size.times(2.0 / 3.0, align),
                ..style.clone()
            };
            return self.inlines(&content, &style, out);
        }
        let text: Option<String> = (content.iter())
            .map(|inline| match inline {
                Inline::Text(text) => Some(text.as_str()),
                _ => None,
            })
            .collect();
        match text.and_then(|text| script::characters(script, &text)) {
            Some(characters) => push(out, &characters, style),
            None => self.inlines(&content, style, out),
        }
    }

    /// Inline code, or what is drawn as it: `content` in the inline code
    /// colours, padded, or between backticks without colour.
    fn code(
        &self,
        style: &Style,
        out: &mut Vec<Span>,
        content: impl FnOnce(&Style, &mut Vec<Span>),
    ) {
        // Spaces in code are kept as written, so it copies as it is.
        let spaces = if self.theme.inline_code_wrap {
            Spaces::Keep
        } else {
            Spaces::KeepTogether
        };
        let style = &Style {
            spaces,
            ..style.clone()
        };
        // Without colour nothing else would mark the code.
        if self.caps.styling && self.caps.color {
            let bg = self.inline_code_background().or(style.bg);
            let code_style = Style {
                fg: self.theme.inline_code.or(style.fg),
                bg,
                ..style.clone()
            };
            // No-break spaces keep the padding attached to the code.
            let padding = if self.theme.inline_code_padding && bg.is_some() {
                "\u{a0}"
            } else {
                ""
            };
            push(out, padding, &code_style);
            content(&code_style, out);
            push(out, padding, &code_style);
        } else {
            push(out, "`", style);
            content(style, out);
            push(out, "`", style);
        }
    }

    fn link(&self, url: &str, content: &[Inline], style: &Style, out: &mut Vec<Span>) {
        // The pager joins a link that wraps over lines by its `Rc`, so all
        // the spans of one link have to share this one, also when the link
        // wraps.
        let link_style = self.style(Style {
            fg: self.theme.link.or(style.fg),
            underline: Underline::Single,
            link: self.caps.hyperlinks.then(|| Rc::from(url)),
            ..style.clone()
        });
        self.inlines(content, &link_style, out);
        // Without clickable links, the target has to be visible.
        if !self.caps.hyperlinks && Inline::plain_text(content) != url {
            let target_style = self.muted(style.clone());
            push(out, &format!(" <{url}>"), &target_style);
        }
    }

    /// Text played down, as [`style::muted`](super::style::muted) has it.
    fn muted(&self, style: Style) -> Style {
        self.style(super::style::muted(self.theme, style))
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
pub(super) fn image_rows(placement: Placement) -> Vec<Line> {
    let rows = placement.size.rows;
    let placement = Rc::new(placement);
    (0..rows)
        .map(|row| Line::image(Rc::clone(&placement), row))
        .collect()
}

/// `content` with every sub- and superscript in it replaced by its own
/// content.
fn without_scripts(content: &[Inline]) -> Vec<Inline> {
    let mut out = Vec::new();
    for inline in content {
        out.push(match inline {
            Inline::Subscript(inner) | Inline::Superscript(inner) => {
                out.extend(without_scripts(inner));
                continue;
            }
            Inline::Emphasis(inner) => Inline::Emphasis(without_scripts(inner)),
            Inline::Strong(inner) => Inline::Strong(without_scripts(inner)),
            Inline::Strikethrough(inner) => Inline::Strikethrough(without_scripts(inner)),
            Inline::Kbd(inner) => Inline::Kbd(without_scripts(inner)),
            Inline::Mark(inner) => Inline::Mark(without_scripts(inner)),
            Inline::Link { url, content } => Inline::Link {
                url: url.clone(),
                content: without_scripts(content),
            },
            inline => inline.clone(),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse;
    use crate::render::style::text;

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
                match &line.band {
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
        inputs.images.wait(key.into(), placeholder);
        inputs
    }

    /// The pixels of an image of `width` × `height`.
    fn pixels(width: u32, height: u32) -> crate::images::Source {
        crate::images::Source::Raster(::image::RgbaImage::new(width, height))
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
        let key = ImageKey::block("mermaid", "flowchart TD\n");
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
        let (at, band) = (lines.iter().enumerate())
            .find_map(|(at, line)| Some((at, line.band.as_ref()?)))
            .expect("a band");
        (at, band.key.to_string(), band.above)
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
        let key = ImageKey::block("mermaid", "a\nb\n");
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
        let inputs = loading(ImageKey::block("mermaid", code), Placeholder::Rows(3));
        let lines = shown(&format!("```mermaid\n{code}```\n"), &inputs);
        assert_eq!(lines.len(), 3, "{lines:?}");
        assert!(lines[0].starts_with("Placeholder mermaid"), "{lines:?}");
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
            reading: Some(crate::render::reading::Reading {
                pictures: Default::default(),
                fonts: Rc::new(crate::reading::Fonts::fixture()),
                wide: crate::reading::page::Wide::Wrap,
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
    fn blocks_know_the_source_line_they_start_on() {
        let stream = lone(
            "Before\n\n<details>\n<summary>Outer</summary>\n\nIn outer\n\n</details>\n\n- item\n",
        );
        let mut seen = Vec::new();
        for line in stream.lines.iter().filter_map(|line| line.block) {
            if seen.last() != Some(&line) {
                seen.push(line);
            }
        }
        let lines: Vec<_> = seen
            .iter()
            .map(|&id| stream.blocks.get(id).line())
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
            stream.blocks.get(line.block.unwrap()).line()
        };
        assert_eq!(line_of("Two"), Some(3));
        assert_eq!(line_of("Three"), Some(3));
        // The header of a file is not in the source.
        let header = stream.lines[0].block.unwrap();
        assert_eq!(stream.blocks.get(header).line(), None);
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
        // Every block of the table has lines.
        assert_eq!(seen.len(), stream.blocks.len(), "{fixture}");
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
    fn loaded(key: impl Into<ImageKey>, image: crate::images::Source) -> Inputs {
        let mut inputs = Inputs::new(Capabilities {
            cell_pixels: Some((10, 20)),
            ..Capabilities::kitty(60)
        });
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
        let formula = loaded(ImageKey::block("math", "x\n"), pixels(40, 20));
        assert_eq!(
            rows("$$x$$\n", &formula),
            [format!("{}⟦4 cells⟧", " ".repeat(2 + 26))]
        );
        let diagram = loaded(ImageKey::block("mermaid", "x\n"), pixels(40, 20));
        assert_eq!(rows("```mermaid\nx\n```\n", &diagram), ["  ⟦4 cells⟧"]);
        let image = loaded("a.png", pixels(40, 20));
        assert_eq!(rows("![a](a.png)\n", &image), ["  ⟦4 cells⟧"]);
    }

    #[test]
    fn a_formula_is_centred_in_the_room_of_its_list_item_quote_or_alert() {
        let formula = || loaded(ImageKey::block("math", "x\n"), pixels(40, 20));
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
        let formula = loaded(ImageKey::block("math", "x\n"), pixels(1000, 20));
        assert_eq!(rows("$$x$$\n", &formula), ["  ⟦56 cells⟧"]);
    }

    #[test]
    fn a_formula_that_loads_is_centred_with_its_band() {
        let key = ImageKey::block("math", "x\n");
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
