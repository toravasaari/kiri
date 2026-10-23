//! Document model to styled lines of a fixed width.

mod code;
mod front_matter;
mod list;
mod quote;

use std::rc::Rc;

use super::block::{BlockId, Blocks};
use super::image::{self, Placement};
use super::style::{Line, Spaces, Span, Style, push};
use super::table::{self, Cells};
use super::wrap::wrap;
use super::{FrontMatterStyle, Inputs, RenderErrors, TableRows};
use crate::caps::Capabilities;
use crate::color::{Color, Underline};
use crate::images::{ImageError, ImageKey, Images};
use crate::model::{Block, Document, File, Image, Inline, Table};
use crate::size::TextSize;
use crate::theme::{BandColor, Theme};

/// How much of the text colour a striped table row mixes into the page.
const STRIPE: f32 = 0.06;

/// How much of the text colour the band of a file's header mixes into a
/// dark page and a light one: as much on a light page would be a heavy
/// grey.
const BAND_DARK: f32 = 0.3;
const BAND_LIGHT: f32 = 0.12;

pub fn layout(document: &Document, inputs: &Inputs) -> Vec<Line> {
    document_stream(document, inputs).lines
}

/// `document` laid out as a lone document, with the blocks its lines come
/// from.
fn document_stream(document: &Document, inputs: &Inputs) -> Stream {
    with_layout(inputs, 0, |layout, width, margin| {
        let mut stream = Stream::default();
        layout.top_level(
            &document.blocks,
            width,
            margin,
            &mut stream.lines,
            &mut stream.blocks,
            &mut Vec::new(),
        );
        stream
    })
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

/// Lines laid out, with the blocks they come from.
#[derive(Debug, Default)]
pub struct Stream {
    pub lines: Vec<Line>,
    /// The blocks that the lines' [`Line::block`] numbers.
    pub blocks: Blocks,
}

/// `files` laid out one after another, a blank line apart, each as
/// [`file`] lays it out. The blank line between two files belongs to the
/// first, as its header does.
pub fn files(files: &[File], inputs: &Inputs) -> Stream {
    let mut stream = Stream::default();
    let mut previous: Option<usize> = None;
    for (number, shown) in files.iter().enumerate() {
        if let Some(start) = previous {
            let mut blank = Line::blank();
            blank.block = stream.lines[start].block;
            stream.lines.push(blank);
        }
        previous = Some(stream.lines.len());
        let lines = file_into(shown, number, inputs, &mut stream.blocks);
        stream.lines.extend(lines);
    }
    stream
}

/// `file`, numbered `number` among the files shown together, laid out
/// under a header that names it, as [`Layout::file_header`] draws it, and
/// a blank line. An image file below it is fitted to the window with its
/// header, however many rows a long name takes.
pub fn file(file: &File, number: usize, inputs: &Inputs) -> Vec<Line> {
    file_into(file, number, inputs, &mut Blocks::default())
}

/// [`file`], with its blocks added to `blocks`. The header lines come from
/// a block of their own, which has no path.
fn file_into(file: &File, number: usize, inputs: &Inputs, blocks: &mut Blocks) -> Vec<Line> {
    with_layout(inputs, number, |layout, width, margin| {
        let mut lines = layout.file_header(&file.name, width, margin);
        stamp(&mut lines, blocks.add(number, Vec::new()));
        // The header and the blank line below it.
        let above = u16::try_from(lines.len() + 1).unwrap_or(u16::MAX);
        let layout = Layout {
            reserved: layout.reserved.saturating_add(above),
            ..*layout
        };
        let mut body = Vec::new();
        layout.top_level(
            &file.document.blocks,
            width,
            margin,
            &mut body,
            blocks,
            &mut Vec::new(),
        );
        append_block(&mut lines, body, true);
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
        table_rows,
        render_errors,
    } = inputs;
    // Without colours, the layout leaves out what only shows a colour, such
    // as heading fills and the padding around inline code.
    let colorless;
    let theme = if caps.color {
        theme
    } else {
        colorless = theme.clone().without_colors();
        &colorless
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
        table_rows: *table_rows,
        render_errors: *render_errors,
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
    table_rows: TableRows,
    render_errors: RenderErrors,
    /// The number of the document laid out among those shown together,
    /// which the keys of its images carry.
    document: usize,
    /// The rows of the window that an image file shown on its own leaves
    /// to others: the shell's prompt or the pager's status line below it,
    /// and the header above it.
    reserved: u16,
}

impl Layout<'_> {
    /// Lays out the document's own blocks: headings stay at the edge and
    /// thematic breaks span the terminal; everything else keeps `margin`
    /// free on both sides. The blocks inside a details block are laid out
    /// as the blocks around it are.
    ///
    /// Each block's lines are added to `lines` marked with the block, which
    /// is entered in `table` under `path`, the places of the blocks around
    /// it, with its own at the end; the summary of a details block is
    /// marked as that block. The blank line between blocks has no block.
    fn top_level(
        &self,
        blocks: &[Block],
        width: usize,
        margin: usize,
        lines: &mut Vec<Line>,
        table: &mut Blocks,
        path: &mut Vec<usize>,
    ) {
        for (index, block) in blocks.iter().enumerate() {
            path.push(index);
            let mut block_lines = match block {
                Block::ThematicBreak => vec![self.rule(usize::from(self.caps.width).max(width))],
                Block::Heading { .. } | Block::ImageFile(_) => {
                    self.block(block, width, Context::default())
                }
                Block::Details { summary, blocks } => {
                    let summary = self.summary(summary, width - 2 * margin);
                    let mut summary = with_margin(summary, margin);
                    stamp(&mut summary, table.add(self.document, path.clone()));
                    append_block(lines, summary, true);
                    self.top_level(blocks, width, margin, lines, table, path);
                    path.pop();
                    continue;
                }
                _ => with_margin(
                    self.block(block, width - 2 * margin, Context::default()),
                    margin,
                ),
            };
            if !block_lines.is_empty() {
                stamp(&mut block_lines, table.add(self.document, path.clone()));
            }
            append_block(lines, block_lines, true);
            path.pop();
        }
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
            return image_rows(Placement { key, size });
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
        let size = if self.caps.text_sizing && !context.nested {
            heading.size
        } else {
            TextSize::NORMAL
        };
        let scale = usize::from(size.scale());
        let style = self.style(Style {
            bold: heading.bold,
            italic: heading.italic,
            fg: heading.color,
            bg: heading.background,
            underline: heading.underline,
            ..Style::default()
        });
        // Indent and fill carry the background but not the underline.
        let background = self.style(Style {
            bg: heading.background,
            ..Style::default()
        });
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
        // In characters of the heading's size.
        let available = (width - indent) / scale;
        // An empty heading takes no room, but keeps its rule.
        let mut lines: Vec<Line> = if empty {
            Vec::new()
        } else {
            wrap(&spans, available)
                .into_iter()
                .map(|mut spans| {
                    let mut trail = Vec::new();
                    if background.bg.is_some() {
                        let used = super::wrap::width(&spans);
                        // A wide character can overflow a very narrow heading.
                        let fill = available.saturating_sub(used);
                        push(&mut spans, &" ".repeat(fill), &background);
                        // Scaled text covers a multiple of the scale; cells of
                        // normal size fill the rest of the width.
                        let scaled = (used + fill) * scale;
                        let rest = (width - indent).saturating_sub(scaled);
                        push(&mut trail, &" ".repeat(rest), &background);
                    }
                    Line {
                        lead: lead.clone(),
                        spans,
                        image: None,
                        trail,
                        size,
                        block: None,
                    }
                })
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
        wrap(&spans, width).into_iter().map(Line::new).collect()
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
        self.placed(key, wanted, width)
    }

    /// A code block drawn as a diagram, when it was.
    fn diagram(&self, language: Option<&str>, code: &str, width: usize) -> Option<Vec<Line>> {
        // Spares building a key for every code block of a document
        // without images.
        if self.images.is_empty() {
            return None;
        }
        let key = ImageKey::block(language?, code).in_document(self.document);
        self.placed(key, image::Wanted::default(), width)
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
        let text = format!("✗ {language} diagram could not be drawn: {reason}");
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
    /// when the terminal can draw it and it loaded.
    fn placed(&self, key: ImageKey, wanted: image::Wanted, width: usize) -> Option<Vec<Line>> {
        if !self.caps.draws_images() {
            return None;
        }
        let Some(Ok(source)) = self.images.get(&key) else {
            return None;
        };
        let max_cells = self.caps.max_image_cells;
        let max_columns = u16::try_from(width).unwrap_or(u16::MAX).min(max_cells);
        let size = image::size(
            source.size(),
            wanted,
            image::cell_size(self.caps),
            max_columns,
            max_cells,
        )?;
        Some(image_rows(Placement { key, size }))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse;
    use crate::render::style::text;

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

    #[test]
    fn a_block_marks_all_its_lines_and_the_blank_between_blocks_none() {
        let stream = lone("# Title\n\nOne\n\n```\na\n\nb\n```\n\nTwo\n");
        let lines = marked(&stream);
        let blocks: Vec<_> = lines
            .iter()
            .map(|(_, path)| path.as_ref().map(|path| path[0]))
            .collect();
        // The empty line inside the code block belongs to it.
        let code = lines
            .iter()
            .position(|(text, _)| text.contains('a'))
            .unwrap();
        assert_eq!(blocks[code + 1], Some(2), "{lines:?}");
        // Blank separators between blocks have no block.
        assert_eq!(blocks.iter().filter(|block| block.is_none()).count(), 3);
        assert_eq!(paths(&stream), [[0], [1], [2], [3]]);
        // The blank line after the heading is no block, and neither is
        // any other that no block holds.
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
    fn files_mark_their_headers_and_the_blank_between_them_with_the_file() {
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
        // The blank line before the second file is the first one's.
        assert!(stream.lines[start - 1].is_blank());
        assert_eq!(file_of_line(&stream.lines[start - 1]), Some(0));
        assert_eq!(
            table.path(stream.lines[start - 1].block.unwrap()),
            &[] as &[usize]
        );
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
        let blank = stream.lines.iter().filter(|line| line.is_blank());
        assert!(blank.count() > 0);
    }

    #[test]
    fn the_table_and_the_lines_agree_for_nested_blocks() {
        assert_agree("nested.md");
    }

    #[test]
    fn the_table_and_the_lines_agree_for_html_tags() {
        assert_agree("html-tags.md");
    }
}
