//! A run of a document's text blocks laid out as a page, as GitHub sets
//! them: headings, paragraphs, lists, quotes and thematic breaks, with
//! their margins collapsing between blocks as in CSS. The page is drawn a
//! part at a time, so that a pager draws only what it shows.

use std::ops::Range;

use image::RgbaImage;

use super::fonts::Fonts;
use super::paint::Canvas;
use super::text::{self, Brush, Paragraph, Run};
use crate::model::{Alignment, Block, Inline, List, Table};

/// The size of each heading level, `h1` first, in font sizes of the text,
/// and the line height of headings.
const HEADING_SIZES: [f32; 6] = [2.0, 1.5, 1.25, 1.0, 0.875, 0.85];
const HEADING_LINE_HEIGHT: f32 = 1.25;
/// The line height of text.
const LINE_HEIGHT: f32 = 1.5;
/// The weight of headings and of strong text.
const BOLD: f32 = 600.0;
/// How far list items are indented, and how far apart they are, in font
/// sizes; how far a marker ends before its item's text.
const LIST_INDENT: f32 = 2.0;
const ITEM_GAP: f32 = 0.5;
const MARKER_GAP: f32 = 0.5;
/// How far a quote's text is indented, and how wide its bar is.
const QUOTE_INDENT: f32 = 1.0;
const QUOTE_BAR: f32 = 0.25;
/// How far below a large heading its rule is, in its own font sizes.
const RULE_BELOW: f32 = 0.3;
/// A code block's padding and the radius of its corners, in font sizes;
/// its font size and line height, and its label's size.
const CODE_PADDING: f32 = 1.0;
const CODE_RADIUS: f32 = 0.375;
const CODE_LINE_HEIGHT: f32 = 1.45;
const LABEL_SIZE: f32 = 0.75;
/// The room below a band's content for its scroll bar, in font sizes;
/// how far its fades reach in from each edge; the scroll bar's thickness
/// in pixels, and the selected one's.
const BAR_ROOM: f32 = 0.6;
const FADE: f32 = 2.5;
const BAR: f32 = 4.0;
const BAR_SELECTED: f32 = 6.0;
/// How much wider than the room something may measure and still fit it,
/// in pixels: rounding in the widths of laid-out text.
const FITS: f32 = 0.5;
/// A width no line of code reaches, for code laid out unbroken.
const UNBOUNDED: f32 = 1.0e6;
/// How far a table cell's text is from its borders, across and down, in
/// font sizes.
const CELL_ACROSS: f32 = 13.0 / 16.0;
const CELL_DOWN: f32 = 6.0 / 16.0;

/// How the page is drawn: its sizes and colours.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Look {
    /// The size of the text's font in pixels.
    pub size: f32,
    pub text: Brush,
    /// The colour of each heading level, `h1` first.
    pub headings: [Brush; 6],
    pub link: Brush,
    /// The colour of inline code, and of its chip when it has one.
    pub code: Brush,
    pub chip: Option<Brush>,
    /// The colour of a quote's text.
    pub quote: Brush,
    /// The colour of rules, quote bars and the box of a task.
    pub border: Brush,
    /// The colour of list markers.
    pub marker: Brush,
    /// The background of code blocks, if they have one, and the colour of
    /// code that is not highlighted.
    pub code_block: Option<Brush>,
    pub code_text: Brush,
    /// The colour of a code block's language label; `None` for no label.
    pub label: Option<Brush>,
    /// The colour of a table's borders, and of every second body row;
    /// `None` for no shading.
    pub table_border: Brush,
    pub stripe: Option<Brush>,
}

/// The colours of code: each line of `code`, in `language` if known, as
/// runs of text in their colours. The tabs are spaces already.
pub trait Highlight {
    fn lines(&self, language: Option<&str>, code: &str) -> Vec<Vec<Run>>;
}

/// Code in the plain colour of code.
pub struct Plain;

impl Highlight for Plain {
    fn lines(&self, _: Option<&str>, code: &str) -> Vec<Vec<Run>> {
        code.lines().map(|line| vec![Run::new(line)]).collect()
    }
}

/// What becomes of a code block or table wider than the area.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Wide {
    /// Its lines and cells wrap to fit, breaking words in a table when
    /// they must.
    #[default]
    Wrap,
    /// It keeps its lines and words whole and becomes a band, which
    /// scrolls sideways.
    Scroll,
}

/// Where and how a page is laid out.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    /// The text's width, and the area's, which code and tables may
    /// reach, in pixels.
    pub width: f32,
    pub area: f32,
    /// The height of a row, which bands start on.
    pub row: f32,
    pub wide: Wide,
}

/// A code block or table too wide for the area, laid out on whole rows
/// of its own to scroll sideways, with a scroll bar below it.
pub struct Band {
    /// Its top, on a whole row, and its height in whole rows, from the
    /// top of the page.
    pub top: f32,
    pub height: f32,
    /// Its left edge, the width of its content, and the width it shows
    /// at a time, up to the area's edge.
    pub x: f32,
    pub width: f32,
    pub visible: f32,
    /// The height of its content, above the scroll bar.
    content: f32,
    /// What it draws, from its own top left.
    items: Vec<Item>,
}

impl Band {
    /// The furthest it scrolls, in pixels.
    pub fn most(&self) -> f32 {
        (self.width - self.visible).max(0.0)
    }
}

/// Where a block of the page is.
#[derive(Debug, Clone, PartialEq)]
pub struct Placed {
    /// The pixels it covers, from the top of the page.
    pub rows: Range<f32>,
    /// Its text, as a search finds it.
    pub text: String,
}

/// A run of blocks laid out.
pub struct Page {
    pub width: f32,
    pub height: f32,
    /// Each block laid out, in order.
    pub blocks: Vec<Placed>,
    /// The code blocks and tables that scroll, in order.
    pub bands: Vec<Band>,
    items: Vec<Item>,
    look: Look,
}

/// Something drawn on the page, with the pixels it may reach.
struct Item {
    rows: Range<f32>,
    shape: Shape,
}

impl Item {
    /// The same item moved by (`x`, `y`).
    fn shifted(mut self, x: f32, y: f32) -> Self {
        self.rows = self.rows.start + y..self.rows.end + y;
        match &mut self.shape {
            Shape::Text {
                x: left, y: top, ..
            }
            | Shape::Rect {
                x: left, y: top, ..
            }
            | Shape::RoundRect {
                x: left, y: top, ..
            }
            | Shape::Task {
                x: left, y: top, ..
            } => {
                *left += x;
                *top += y;
            }
        }
        self
    }
}

enum Shape {
    Text {
        paragraph: Box<Paragraph>,
        x: f32,
        y: f32,
        chip: Option<Brush>,
    },
    Rect {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        color: Brush,
    },
    /// A rectangle with rounded corners.
    RoundRect {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        radius: f32,
        color: Brush,
    },
    /// The box of a task list item, ticked or not.
    Task {
        x: f32,
        y: f32,
        size: f32,
        checked: bool,
        look: Look,
    },
}

/// Whether reading mode lays `block` out: text blocks, code blocks but
/// those `drawn` as images, given their language and code, and lists and
/// quotes of nothing else. Other blocks, such as images and diagrams, are
/// shown as they are without it.
pub fn lays_out(block: &Block, drawn: &dyn Fn(&str, &str) -> bool) -> bool {
    let all = |blocks: &[Block]| blocks.iter().all(|block| lays_out(block, drawn));
    match block {
        Block::Heading { .. } | Block::ThematicBreak | Block::Table(_) => true,
        Block::Paragraph(content) => Inline::lone_image(content).is_none(),
        Block::CodeBlock { language, code, .. } => !language
            .as_deref()
            .is_some_and(|language| drawn(language, code)),
        Block::BlockQuote(blocks) => all(blocks),
        Block::List(list) => list.items.iter().all(|item| all(&item.blocks)),
        _ => false,
    }
}

/// `blocks`, which reading mode lays out, laid out in `fonts` as `look`
/// says, in `frame`: the text at the left of the area, which tables and
/// code may reach across, and what becomes of those wider than it. Code
/// takes its colours from `highlight`.
pub fn lay_out(
    blocks: &[&Block],
    look: &Look,
    fonts: &Fonts,
    highlight: &dyn Highlight,
    frame: Frame,
) -> Page {
    let width = frame.width;
    let area = frame.area.max(width);
    let mut flow = Flow {
        fonts,
        look,
        highlight,
        area,
        row: frame.row.max(1.0),
        wide: frame.wide,
        items: Vec::new(),
        bands: Vec::new(),
        y: 0.0,
        margin: None,
    };
    let mut placed = Vec::new();
    for block in blocks {
        let top = flow.top(block_margins(block, look).0);
        flow.block(block, 0.0, width, 0);
        placed.push(Placed {
            rows: top..flow.y,
            text: plain(block),
        });
    }
    Page {
        width: area,
        height: flow.y,
        blocks: placed,
        bands: flow.bands,
        items: flow.items,
        look: *look,
    }
}

impl Page {
    /// How the page is drawn.
    pub fn look(&self) -> &Look {
        &self.look
    }

    /// The part of the page from `top`, `height` pixels tall and as wide as
    /// `width`, transparent where nothing is drawn. Bands are not drawn.
    pub fn draw(&self, top: u32, width: u32, height: u32) -> RgbaImage {
        let mut canvas = Canvas::new(width, height);
        draw_items(&mut canvas, &self.items, (0.0, top as f32));
        canvas.into_image()
    }

    /// The pixel rows `rows` of band `band`, counted from its top, as wide
    /// as the page: what the page has beside it, and the part of its
    /// content `offset` pixels from its left edge, faded on each edge with
    /// more beyond it, above its scroll bar, which is brighter and thicker
    /// when it is `selected`.
    pub fn draw_band(
        &self,
        band: usize,
        offset: f32,
        selected: bool,
        rows: Range<f32>,
    ) -> RgbaImage {
        let height = (rows.end - rows.start).ceil().max(1.0) as u32;
        let Some(band) = self.bands.get(band) else {
            return Canvas::new(self.width.ceil() as u32, height).into_image();
        };
        let size = self.look.size;
        let mut canvas = Canvas::new(self.width.ceil() as u32, height);
        draw_items(&mut canvas, &self.items, (0.0, band.top + rows.start));
        let offset = offset.clamp(0.0, band.most());
        // The content is above the scroll bar, which may be below this
        // part of the rows.
        let content = (band.content - rows.start).clamp(0.0, height as f32);
        if content > 0.0 {
            let mut shown = Canvas::new(band.visible.ceil() as u32, content.ceil() as u32);
            draw_items(&mut shown, &band.items, (offset, rows.start));
            let fade = (FADE * size).min(band.visible / 3.0);
            if offset < band.most() {
                shown.fade(band.visible - fade, fade, content, false);
            }
            if offset > 0.0 {
                shown.fade(0.0, fade, content, true);
            }
            canvas.overlay(&shown, band.x, 0.0);
        }
        let thickness = if selected { BAR_SELECTED } else { BAR };
        let bar = (band.content + 0.25 * size).round() - rows.start;
        let track = bar + (thickness - BAR) / 2.0;
        canvas.round_rect_alpha(
            band.x,
            track,
            band.visible,
            BAR,
            BAR / 2.0,
            self.look.border,
            153,
        );
        let thumb = band.visible * band.visible / band.width;
        let color = if selected {
            self.look.text
        } else {
            self.look.quote
        };
        let start = band.x + offset / band.width * band.visible;
        canvas.round_rect(start, bar, thumb, thickness, thickness / 2.0, color);
        canvas.into_image()
    }
}

/// Draws the `items` that reach the canvas, with the point (`x`, `y`) of
/// their coordinates at its top left.
fn draw_items(canvas: &mut Canvas, items: &[Item], (x, y): (f32, f32)) {
    let (from, to) = (y, y + canvas.height() as f32);
    for item in items {
        if item.rows.end < from || item.rows.start > to {
            continue;
        }
        let (dx, dy) = (-x, -y);
        match &item.shape {
            Shape::Text {
                paragraph,
                x,
                y,
                chip,
            } => canvas.text(paragraph, x + dx, y + dy, *chip),
            Shape::Rect {
                x,
                y,
                width,
                height,
                color,
            } => canvas.rect(x + dx, y + dy, *width, *height, *color),
            Shape::RoundRect {
                x,
                y,
                width,
                height,
                radius,
                color,
            } => canvas.round_rect(x + dx, y + dy, *width, *height, *radius, *color),
            Shape::Task {
                x,
                y,
                size,
                checked,
                look,
            } => task(canvas, x + dx, y + dy, *size, *checked, look),
        }
    }
}

/// Lays blocks out one below another, collapsing the margins between them.
struct Flow<'a> {
    fonts: &'a Fonts,
    look: &'a Look,
    highlight: &'a dyn Highlight,
    /// The right edge of the area, which tables and code may reach.
    area: f32,
    /// The height of a row, which bands start on, and what becomes of
    /// code and tables wider than the area.
    row: f32,
    wide: Wide,
    items: Vec<Item>,
    bands: Vec<Band>,
    /// The bottom of what is laid out so far.
    y: f32,
    /// The bottom margin of the block above, until the next block takes
    /// the larger of it and its own top margin; `None` at the top, where
    /// no margin is kept.
    margin: Option<f32>,
}

/// The margins above and below a block, as GitHub gives them.
fn block_margins(block: &Block, look: &Look) -> (f32, f32) {
    let size = look.size;
    match block {
        Block::Heading { .. } => (1.5 * size, size),
        Block::ThematicBreak => (1.5 * size, 1.5 * size),
        _ => (0.0, size),
    }
}

impl Flow<'_> {
    /// Moves below the block above by the larger of its bottom margin and
    /// `margin`, and returns where the next block starts.
    fn top(&mut self, margin: f32) -> f32 {
        if let Some(above) = self.margin.take() {
            self.y += above.max(margin);
        }
        self.y
    }

    fn push(&mut self, rows: Range<f32>, shape: Shape) {
        // Glyphs may reach past their line's box, so an item is drawn
        // into the parts a little above and below it too.
        let reach = self.look.size;
        self.items.push(Item {
            rows: rows.start - reach..rows.end + reach,
            shape,
        });
    }

    /// Lays `paragraph` out at (`x`, the current top), and returns its
    /// height.
    fn text(&mut self, paragraph: Paragraph, x: f32) -> f32 {
        let (y, height) = (self.y, paragraph.height());
        let chip = self.look.chip;
        self.push(
            y..y + height,
            Shape::Text {
                paragraph: Box::new(paragraph),
                x,
                y,
                chip,
            },
        );
        height
    }

    fn rect(&mut self, x: f32, y: f32, width: f32, height: f32, color: Brush) {
        self.push(
            y..y + height,
            Shape::Rect {
                x,
                y,
                width,
                height,
                color,
            },
        );
    }

    /// Lays `block` out at `x`, `width` pixels wide, `depth` lists deep;
    /// the caller has moved to its top.
    fn block(&mut self, block: &Block, x: f32, width: f32, depth: usize) {
        let look = *self.look;
        let size = look.size;
        match block {
            Block::Heading { level, content } => {
                let index = usize::from(level.clamp(&1, &6) - 1);
                let style = text::Look {
                    size: size * HEADING_SIZES[index],
                    line_height: HEADING_LINE_HEIGHT,
                    weight: BOLD,
                    color: look.headings[index],
                    ..text::Look::PLAIN
                };
                let paragraph = text::lay_out(self.fonts, &runs(content, &look), &style, width);
                self.y += self.text(paragraph, x);
                if *level <= 2 {
                    self.y += (RULE_BELOW * style.size).round();
                    self.rect(x, self.y, width, 1.0, look.border);
                    self.y += 1.0;
                }
            }
            Block::Paragraph(content) => {
                let paragraph = self.paragraph(content, look.text, width);
                self.y += self.text(paragraph, x);
            }
            Block::ThematicBreak => {
                let height = (QUOTE_BAR * size).round().max(1.0);
                self.rect(x, self.y, width, height, look.border);
                self.y += height;
            }
            Block::BlockQuote(blocks) => {
                let top = self.y;
                let quoted = Look {
                    text: look.quote,
                    ..look
                };
                let indent = QUOTE_INDENT * size;
                let mut inner = Flow {
                    fonts: self.fonts,
                    look: &quoted,
                    highlight: self.highlight,
                    area: self.area,
                    row: self.row,
                    wide: self.wide,
                    items: Vec::new(),
                    bands: Vec::new(),
                    y: top,
                    margin: None,
                };
                inner.blocks(blocks, x + indent, width - indent, depth);
                self.items.append(&mut inner.items);
                self.bands.append(&mut inner.bands);
                self.y = inner.y;
                let bar = (QUOTE_BAR * size).round().max(1.0);
                self.rect(x, top, bar, self.y - top, look.border);
            }
            Block::List(list) => self.list(list, x, width, depth),
            Block::CodeBlock { language, code, .. } => {
                self.code(language.as_deref(), code, x, width);
            }
            Block::Table(table) => self.table(table, x),
            // Reading mode lays out only the blocks `lays_out` takes.
            _ => {}
        }
        self.margin = Some(block_margins(block, &look).1);
    }

    /// Lays `blocks` out one below another from the current top.
    fn blocks(&mut self, blocks: &[Block], x: f32, width: f32, depth: usize) {
        for block in blocks {
            self.top(block_margins(block, self.look).0);
            self.block(block, x, width, depth);
        }
    }

    /// Lays a code block out at `x`: at least `width` pixels wide, as wide
    /// as its longest line up to the area's edge, and with lines longer
    /// than that wrapped at the edge.
    fn code(&mut self, language: Option<&str>, code: &str, x: f32, width: f32) {
        let look = *self.look;
        let size = look.size;
        let padding = CODE_PADDING * size;
        let style = text::Look {
            size: size * text::CODE_SIZE,
            line_height: CODE_LINE_HEIGHT,
            color: look.code_text,
            mono: true,
            wrap: text::Wrap::Anywhere,
            ..text::Look::PLAIN
        };
        let mut runs = Vec::new();
        for (index, line) in self.highlight.lines(language, code).into_iter().enumerate() {
            if index > 0 {
                push(&mut runs, "\n", &Run::default());
            }
            for run in line {
                push(&mut runs, &run.text.clone(), &run);
            }
        }
        let room = (self.area - x).max(width);
        // Scrolling, the lines stay whole; wrapping, they break at the
        // area's edge.
        let scroll = self.wide == Wide::Scroll;
        let line_width = if scroll {
            UNBOUNDED
        } else {
            room - 2.0 * padding
        };
        let paragraph = text::lay_out(self.fonts, &runs, &style, line_width);
        let (_, natural) = paragraph.content_widths();
        // A line that reaches the area's edge to within rounding fits.
        let banded = scroll && natural + 2.0 * padding > room + FITS;
        let block_width = match banded {
            true => natural + 2.0 * padding,
            false => (natural + 2.0 * padding).clamp(width, room),
        };
        let fonts = self.fonts;
        let draw = move |flow: &mut Self| {
            let height = paragraph.height() + 2.0 * padding;
            let top = flow.y;
            if let Some(color) = look.code_block {
                flow.push(
                    top..top + height,
                    Shape::RoundRect {
                        x,
                        y: top,
                        width: block_width,
                        height,
                        radius: CODE_RADIUS * size,
                        color,
                    },
                );
            }
            flow.y += padding;
            flow.text(paragraph, x + padding);
            if let (Some(language), Some(color)) =
                (language.filter(|name| !name.is_empty()), look.label)
            {
                // In the padding above the code, one line of its own
                // height, so that a long first line never runs under it.
                let style = text::Look {
                    size: size * LABEL_SIZE,
                    line_height: 1.0,
                    color,
                    ..text::Look::PLAIN
                };
                let label = text::lay_out(fonts, &[Run::new(language)], &style, block_width);
                let (label_x, label_y) = (
                    x + block_width - 0.6 * size - label.width(),
                    top + ((padding - label.height()) / 2.0).max(0.0),
                );
                let label_height = label.height();
                flow.push(
                    label_y..label_y + label_height,
                    Shape::Text {
                        paragraph: Box::new(label),
                        x: label_x,
                        y: label_y,
                        chip: None,
                    },
                );
            }
            flow.y = top + height;
        };
        match banded {
            true => self.band(x, block_width, draw),
            false => draw(self),
        }
    }

    /// Lays out what `lay_out` lays out from the next whole row, `width`
    /// pixels wide from `x`, as a band of its own, which scrolls sideways
    /// in the room the area leaves it, above a scroll bar.
    fn band(&mut self, x: f32, width: f32, lay_out: impl FnOnce(&mut Self)) {
        let row = self.row;
        let top = (self.y / row).ceil() * row;
        self.y = top;
        let start = self.items.len();
        lay_out(self);
        let content = self.y - top;
        let items = (self.items.drain(start..))
            .map(|item| item.shifted(-x, -top))
            .collect();
        let rows = ((content + BAR_ROOM * self.look.size) / row)
            .ceil()
            .max(1.0);
        self.bands.push(Band {
            top,
            height: rows * row,
            x,
            width,
            visible: (self.area - x).max(1.0),
            content,
            items,
        });
        self.y = top + rows * row;
    }

    /// Lays a table out at `x`: as wide as its content up to the area's
    /// edge, with its columns shared out as a browser shares them.
    fn table(&mut self, table: &Table, x: f32) {
        let look = *self.look;
        let size = look.size;
        let (across, down) = (CELL_ACROSS * size, CELL_DOWN * size);
        let grid: Vec<&Vec<Vec<Inline>>> =
            std::iter::once(&table.header).chain(&table.rows).collect();
        let columns = grid.iter().map(|row| row.len()).max().unwrap_or(0);
        if columns == 0 {
            return;
        }
        let fonts = self.fonts;
        let cell = |row: usize, column: usize, width: f32, wrap: text::Wrap| {
            let content = grid[row].get(column).map_or(&[][..], Vec::as_slice);
            let style = text::Look {
                size,
                weight: if row == 0 { BOLD } else { 400.0 },
                color: look.text,
                wrap,
                align: match table.alignments.get(column) {
                    Some(Alignment::Center) => text::Align::Centre,
                    Some(Alignment::Right) => text::Align::Right,
                    _ => text::Align::Left,
                },
                ..text::Look::PLAIN
            };
            text::lay_out(fonts, &runs(content, &look), &style, width)
        };
        let (mut least, mut most) = (vec![0.0f32; columns], vec![0.0f32; columns]);
        for row in 0..grid.len() {
            for column in 0..columns {
                let (min, max) = cell(row, column, f32::MAX, text::Wrap::Words).content_widths();
                least[column] = least[column].max(min);
                most[column] = most[column].max(max);
            }
        }
        // Each column takes its padding and the border at its left; the
        // last border closes the table.
        let extra = 2.0 * across + 1.0;
        let room = (self.area - x) - columns as f32 * extra - 1.0;
        let scroll = self.wide == Wide::Scroll;
        // Whether even the least widths are too wide for the area, which
        // `share` tells from the columns and not from a sum of widths that
        // may come out a hair over the room.
        let (widths, too_wide) = share(&least, &most, room);
        let (widths, forced) = match too_wide && scroll {
            // Scrolling, words stay whole: each column takes its least
            // width, and more up to half the area, and the table scrolls.
            true => {
                let half = (self.area - x) / 2.0;
                let widths = (least.iter().zip(&most))
                    .map(|(min, max)| min.max(max.min(half)))
                    .collect();
                (widths, false)
            }
            false => (widths, too_wide),
        };
        let wrap = if forced {
            text::Wrap::Forced
        } else {
            text::Wrap::Words
        };
        let table_width = widths.iter().sum::<f32>() + columns as f32 * extra + 1.0;
        let draw = |flow: &mut Self| {
            let top = flow.y;
            let mut y = top;
            let mut rows = Vec::new();
            for row in 0..grid.len() {
                let cells: Vec<Paragraph> = (0..columns)
                    .map(|column| cell(row, column, widths[column], wrap))
                    .collect();
                let height = cells.iter().map(Paragraph::height).fold(0.0, f32::max) + 2.0 * down;
                if row > 0
                    && row % 2 == 0
                    && let Some(stripe) = look.stripe
                {
                    flow.rect(x, y, table_width, height, stripe);
                }
                let mut left = x;
                for (column, paragraph) in cells.into_iter().enumerate() {
                    let paragraph_y = y + down;
                    let paragraph_height = paragraph.height();
                    flow.push(
                        paragraph_y..paragraph_y + paragraph_height,
                        Shape::Text {
                            paragraph: Box::new(paragraph),
                            x: left + 1.0 + across,
                            y: paragraph_y,
                            chip: look.chip,
                        },
                    );
                    left += widths[column] + extra;
                }
                rows.push(y);
                y += height;
            }
            rows.push(y);
            // The borders, over the shading.
            for &line in &rows {
                flow.rect(x, line.round(), table_width, 1.0, look.table_border);
            }
            let mut left = x;
            flow.rect(left.round(), top, 1.0, y - top + 1.0, look.table_border);
            for width in &widths {
                left += width + extra;
                flow.rect(left.round(), top, 1.0, y - top + 1.0, look.table_border);
            }
            flow.y = y + 1.0;
        };
        match scroll && too_wide {
            true => self.band(x, table_width, draw),
            false => draw(self),
        }
    }

    fn paragraph(&self, content: &[Inline], color: Brush, width: f32) -> Paragraph {
        let style = text::Look {
            size: self.look.size,
            line_height: LINE_HEIGHT,
            weight: 400.0,
            color,
            ..text::Look::PLAIN
        };
        text::lay_out(self.fonts, &runs(content, self.look), &style, width)
    }

    fn list(&mut self, list: &List, x: f32, width: f32, depth: usize) {
        let look = *self.look;
        let size = look.size;
        let indent = LIST_INDENT * size;
        let gap = if list.tight { ITEM_GAP * size } else { size };
        for (number, item) in list.items.iter().enumerate() {
            if number > 0 {
                self.y += gap;
            }
            let top = self.y;
            let mut inner = Flow {
                fonts: self.fonts,
                look: self.look,
                highlight: self.highlight,
                area: self.area,
                row: self.row,
                wide: self.wide,
                items: Vec::new(),
                bands: Vec::new(),
                y: top,
                margin: None,
            };
            for block in &item.blocks {
                // A tight item's blocks follow one another without
                // margins, as a nested list follows its item's text.
                if list.tight {
                    inner.margin = None;
                }
                inner.top(block_margins(block, &look).0);
                inner.block(block, x + indent, width - indent, depth + 1);
            }
            // The marker sits on the baseline of the item's first line.
            let baseline = (inner.items.iter())
                .find_map(|item| match &item.shape {
                    Shape::Text { paragraph, y, .. } => Some(y + paragraph.first_baseline()),
                    _ => None,
                })
                .unwrap_or(top + size);
            self.items.append(&mut inner.items);
            self.bands.append(&mut inner.bands);
            self.y = inner.y.max(top + LINE_HEIGHT * size);
            let marker_right = x + indent - MARKER_GAP * size;
            match item.checked {
                Some(checked) => {
                    let box_size = (0.8 * size).round();
                    let (box_x, box_y) =
                        (marker_right - box_size, baseline - box_size + 0.1 * size);
                    self.push(
                        box_y..box_y + box_size,
                        Shape::Task {
                            x: box_x,
                            y: box_y,
                            size: box_size,
                            checked,
                            look,
                        },
                    );
                }
                None => {
                    let marker = match list.start {
                        Some(start) => format!("{}.", start + number as u64),
                        None => ["•", "◦", "▪"][depth.min(2)].to_owned(),
                    };
                    let style = text::Look {
                        size,
                        line_height: LINE_HEIGHT,
                        weight: 400.0,
                        color: look.marker,
                        ..text::Look::PLAIN
                    };
                    let marker = text::lay_out(self.fonts, &[Run::new(&marker)], &style, width);
                    // Right-aligned, as GitHub sets markers.
                    let (marker_x, marker_y) = (
                        marker_right - marker.width(),
                        baseline - marker.first_baseline(),
                    );
                    let height = marker.height();
                    self.push(
                        marker_y..marker_y + height,
                        Shape::Text {
                            paragraph: Box::new(marker),
                            x: marker_x,
                            y: marker_y,
                            chip: None,
                        },
                    );
                }
            }
        }
    }
}

/// The widths of columns whose content is at least `least` and at most
/// `most` wide, sharing `room` as a browser's automatic table layout does:
/// as wide as their content when it fits; otherwise each at least its
/// least width, and the room left shared in proportion to how much each
/// could still grow. When even the least widths do not fit, each column
/// gets its share of the room in proportion to its least width, and the
/// second value says that words must then break inside to fit.
pub fn share(least: &[f32], most: &[f32], room: f32) -> (Vec<f32>, bool) {
    let (sum_least, sum_most) = (least.iter().sum::<f32>(), most.iter().sum::<f32>());
    if sum_most <= room {
        return (most.to_vec(), false);
    }
    if sum_least <= room {
        let flex: f32 = least.iter().zip(most).map(|(min, max)| max - min).sum();
        let spare = room - sum_least;
        let widths = (least.iter().zip(most))
            .map(|(min, max)| match flex > 0.0 {
                true => min + spare * (max - min) / flex,
                false => *min,
            })
            .collect();
        return (widths, false);
    }
    let room = room.max(0.0);
    let widths = (least.iter())
        .map(|min| match sum_least > 0.0 {
            true => min * room / sum_least,
            false => room / least.len() as f32,
        })
        .collect();
    (widths, true)
}

/// Draws the box of a task list item, `size` pixels square, with its top
/// left at (`x`, `y`): an outline, or filled in the link colour with a
/// tick when it is checked.
fn task(canvas: &mut Canvas, x: f32, y: f32, size: f32, checked: bool, look: &Look) {
    if checked {
        canvas.round_rect(x, y, size, size, (size * 0.2).round(), look.link);
        // A tick of two strokes, as squares along each, white on a dark
        // box and black on a light one.
        let stroke = (size / 8.0).max(1.0);
        let [red, green, blue] = look.link.map(f32::from);
        let light = 0.299 * red + 0.587 * green + 0.114 * blue > 160.0;
        let ink = if light { [0; 3] } else { [0xff; 3] };
        let tick = [(0.2, 0.5), (0.4, 0.72), (0.8, 0.28)];
        for pair in tick.windows(2) {
            let ((x0, y0), (x1, y1)) = (pair[0], pair[1]);
            for step in 0..=12 {
                let t = step as f32 / 12.0;
                let (px, py) = (x0 + (x1 - x0) * t, y0 + (y1 - y0) * t);
                canvas.rect(
                    x + px * size - stroke / 2.0,
                    y + py * size - stroke / 2.0,
                    stroke,
                    stroke,
                    ink,
                );
            }
        }
    } else {
        // An outline one pixel wide.
        canvas.rect(x, y, size, 1.0, look.border);
        canvas.rect(x, y + size - 1.0, size, 1.0, look.border);
        canvas.rect(x, y, 1.0, size, look.border);
        canvas.rect(x + size - 1.0, y, 1.0, size, look.border);
    }
}

/// `content` as runs of styled text, as `look` colours links and code.
fn runs(content: &[Inline], look: &Look) -> Vec<Run> {
    let mut runs = Vec::new();
    collect(content, &Run::default(), look, &mut runs);
    runs
}

fn collect(content: &[Inline], style: &Run, look: &Look, runs: &mut Vec<Run>) {
    for inline in content {
        match inline {
            Inline::Text(text) | Inline::Html(text) => push(runs, text, style),
            Inline::Code(code) => push(
                runs,
                code,
                &Run {
                    code: true,
                    color: Some(look.code),
                    ..style.clone()
                },
            ),
            Inline::SoftBreak => push(runs, " ", style),
            Inline::LineBreak => push(runs, "\n", style),
            Inline::Emphasis(inner) => {
                collect(
                    inner,
                    &Run {
                        italic: true,
                        ..style.clone()
                    },
                    look,
                    runs,
                );
            }
            Inline::Strong(inner) => {
                collect(
                    inner,
                    &Run {
                        bold: true,
                        ..style.clone()
                    },
                    look,
                    runs,
                );
            }
            Inline::Strikethrough(inner) => {
                collect(
                    inner,
                    &Run {
                        strike: true,
                        ..style.clone()
                    },
                    look,
                    runs,
                );
            }
            Inline::Kbd(inner) => {
                let key = Run {
                    code: true,
                    color: Some(look.code),
                    ..style.clone()
                };
                collect(inner, &key, look, runs);
            }
            Inline::Link { content, .. } => {
                let link = Run {
                    color: Some(look.link),
                    ..style.clone()
                };
                collect(content, &link, look, runs);
            }
            Inline::Image(image) => collect(&image.alt, style, look, runs),
            Inline::Mark(inner) | Inline::Subscript(inner) | Inline::Superscript(inner) => {
                collect(inner, style, look, runs);
            }
        }
    }
}

/// Adds `text` in `style` to `runs`, to the last run when it has that
/// style.
fn push(runs: &mut Vec<Run>, text: &str, style: &Run) {
    if text.is_empty() {
        return;
    }
    match runs.last_mut() {
        Some(last) if same_style(last, style) => last.text.push_str(text),
        _ => runs.push(Run {
            text: text.to_owned(),
            ..style.clone()
        }),
    }
}

fn same_style(a: &Run, b: &Run) -> bool {
    (a.bold, a.italic, a.code, a.strike, a.color) == (b.bold, b.italic, b.code, b.strike, b.color)
}

/// The text of `block`, as a search finds it.
fn plain(block: &Block) -> String {
    let mut text = String::new();
    plain_into(block, &mut text);
    text
}

fn plain_into(block: &Block, text: &mut String) {
    let inline = |content: &[Inline], text: &mut String| {
        for run in runs(content, &PLAIN) {
            text.push_str(&run.text);
        }
    };
    match block {
        Block::Heading { content, .. } | Block::Paragraph(content) => inline(content, text),
        Block::CodeBlock { code, .. } => text.push_str(code.trim_end()),
        Block::Table(table) => {
            for (index, row) in std::iter::once(&table.header)
                .chain(&table.rows)
                .enumerate()
            {
                if index > 0 {
                    text.push('\n');
                }
                for (column, cell) in row.iter().enumerate() {
                    if column > 0 {
                        text.push('\t');
                    }
                    inline(cell, text);
                }
            }
        }
        Block::BlockQuote(blocks) => {
            for (index, block) in blocks.iter().enumerate() {
                if index > 0 {
                    text.push('\n');
                }
                plain_into(block, text);
            }
        }
        Block::List(list) => {
            for (index, block) in list.items.iter().flat_map(|item| &item.blocks).enumerate() {
                if index > 0 {
                    text.push('\n');
                }
                plain_into(block, text);
            }
        }
        _ => {}
    }
}

/// A look for text whose colours do not matter.
const PLAIN: Look = Look {
    size: 16.0,
    text: [0; 3],
    headings: [[0; 3]; 6],
    link: [0; 3],
    code: [0; 3],
    chip: None,
    quote: [0; 3],
    border: [0; 3],
    marker: [0; 3],
    code_block: None,
    code_text: [0; 3],
    label: None,
    table_border: [0; 3],
    stripe: None,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse;

    const FRAME: Frame = Frame {
        width: 672.0,
        area: 1008.0,
        row: 20.0,
        wide: Wide::Wrap,
    };

    const LOOK: Look = Look {
        size: 16.0,
        text: [0xdd; 3],
        headings: [[0xee; 3]; 6],
        link: [0x40, 0x90, 0xf0],
        code: [0xcc; 3],
        chip: Some([0x30; 3]),
        quote: [0x90; 3],
        border: [0x3d, 0x44, 0x4d],
        marker: [0xaa; 3],
        code_block: Some([0x2b, 0x30, 0x3b]),
        code_text: [0xc0; 3],
        label: Some([0x80; 3]),
        table_border: [0x3d, 0x44, 0x4d],
        stripe: Some([0x15, 0x1b, 0x23]),
    };

    fn page(markdown: &str) -> Page {
        let document = parse(markdown);
        let blocks: Vec<&Block> = document.blocks.iter().collect();
        assert!(
            blocks.iter().all(|block| lays_out(block, &|_, _| false)),
            "{blocks:?}"
        );
        lay_out(&blocks, &LOOK, &Fonts::fixture(), &Plain, FRAME)
    }

    /// The texts laid out on the page, each with its left edge, its first
    /// baseline and its width.
    fn texts(page: &Page) -> Vec<(String, f32, f32, f32)> {
        (page.items.iter())
            .filter_map(|item| match &item.shape {
                Shape::Text {
                    paragraph, x, y, ..
                } => Some((
                    paragraph.text().to_owned(),
                    *x,
                    y + paragraph.first_baseline(),
                    paragraph.width(),
                )),
                _ => None,
            })
            .collect()
    }

    fn text<'a>(texts: &'a [(String, f32, f32, f32)], start: &str) -> &'a (String, f32, f32, f32) {
        (texts.iter())
            .find(|(text, ..)| text.starts_with(start))
            .unwrap_or_else(|| panic!("no text {start:?} in {texts:?}"))
    }

    #[test]
    fn paragraphs_are_a_font_size_apart() {
        let page = page("One.\n\nTwo.\n");
        let [one, two] = &page.blocks[..] else {
            panic!("two blocks: {:?}", page.blocks);
        };
        assert_eq!(one.rows, 0.0..24.0);
        assert_eq!(two.rows, 40.0..64.0);
        assert_eq!(page.height, 64.0);
        assert_eq!(two.text, "Two.");
    }

    #[test]
    fn margins_collapse_to_the_larger() {
        // A paragraph's 16 px below and a heading's 24 px above make 24.
        let page = page("One.\n\n### Three\n\nFour.\n");
        let [one, three, four] = &page.blocks[..] else {
            panic!("three blocks: {:?}", page.blocks);
        };
        assert_eq!(three.rows.start - one.rows.end, 24.0);
        assert_eq!(four.rows.start - three.rows.end, 16.0);
    }

    #[test]
    fn the_first_block_has_no_margin_above() {
        let page = page("# Title\n");
        assert_eq!(page.blocks[0].rows.start, 0.0);
    }

    #[test]
    fn large_headings_have_a_rule_below() {
        let page = page("## Two\n\n### Three\n");
        let rules: Vec<f32> = (page.items.iter())
            .filter_map(|item| match item.shape {
                Shape::Rect { y, height: 1.0, .. } => Some(y),
                _ => None,
            })
            .collect();
        assert_eq!(rules.len(), 1, "{rules:?}");
        assert!(
            rules[0] > 30.0 && rules[0] < page.blocks[0].rows.end,
            "{rules:?}"
        );
    }

    #[test]
    fn list_markers_sit_on_their_items_first_baseline_right_aligned() {
        let markdown = "8. eight\n9. nine\n10. ten and `code`\n";
        let texts = texts(&page(markdown));
        for (marker, item) in [("8.", "eight"), ("9.", "nine"), ("10.", "ten")] {
            let marker = text(&texts, marker);
            let item = text(&texts, item);
            assert_eq!(marker.2, item.2, "{marker:?} {item:?}");
            assert_eq!(marker.1 + marker.3, item.1 - 8.0, "{marker:?} {item:?}");
        }
        let (eight, ten) = (text(&texts, "8."), text(&texts, "10."));
        assert!(ten.1 < eight.1, "10. reaches further left than 8.");
    }

    #[test]
    fn tight_list_items_are_half_a_line_apart() {
        let texts = texts(&page("- one\n- two\n"));
        let (one, two) = (text(&texts, "one"), text(&texts, "two"));
        assert_eq!(two.2 - one.2, 24.0 + 8.0);
        assert_eq!(one.1, 32.0);
    }

    #[test]
    fn nested_lists_are_indented_with_their_own_bullets() {
        let texts = texts(&page("- one\n  - two\n"));
        assert_eq!(text(&texts, "two").1, 64.0);
        let markers: Vec<&str> = (texts.iter())
            .map(|(text, ..)| text.as_str())
            .filter(|text| ["•", "◦"].contains(text))
            .collect();
        assert_eq!(markers, ["◦", "•"]);
    }

    #[test]
    fn a_quote_is_indented_with_a_bar_beside_it() {
        let page = page("> quoted\n> text\n");
        let texts = texts(&page);
        assert_eq!(text(&texts, "quoted").1, 16.0);
        let bar = (page.items.iter()).find_map(|item| match item.shape {
            Shape::Rect {
                x,
                y,
                width,
                height,
                ..
            } => Some((x, y, width, height)),
            _ => None,
        });
        assert_eq!(bar, Some((0.0, 0.0, 4.0, 24.0)));
    }

    #[test]
    fn task_items_get_a_box() {
        let page = page("- [x] done\n- [ ] to do\n");
        let tasks: Vec<bool> = (page.items.iter())
            .filter_map(|item| match item.shape {
                Shape::Task { checked, .. } => Some(checked),
                _ => None,
            })
            .collect();
        assert_eq!(tasks, [true, false]);
    }

    #[test]
    fn columns_are_as_wide_as_their_content_when_it_fits() {
        assert_eq!(
            share(&[10.0, 20.0], &[30.0, 40.0], 100.0),
            (vec![30.0, 40.0], false)
        );
    }

    #[test]
    fn the_spare_room_goes_to_the_columns_that_can_grow() {
        // 30 to share: the first column could grow by 20, the second by 40.
        let (widths, forced) = share(&[10.0, 20.0], &[30.0, 60.0], 60.0);
        assert_eq!(widths, [20.0, 40.0]);
        assert!(!forced);
    }

    #[test]
    fn columns_too_wide_even_wrapped_share_the_room_and_break_words() {
        let (widths, forced) = share(&[100.0, 300.0], &[200.0, 600.0], 200.0);
        assert_eq!(widths, [50.0, 150.0]);
        assert!(forced);
    }

    /// The left edge and width of each table's top border.
    fn tables(page: &Page) -> Vec<(f32, f32)> {
        let mut tops: Vec<(f32, f32, f32)> = (page.items.iter())
            .filter_map(|item| match item.shape {
                Shape::Rect {
                    x,
                    y,
                    width,
                    height: 1.0,
                    color,
                    ..
                } if color == LOOK.table_border && width > 1.0 => Some((y, x, width)),
                _ => None,
            })
            .collect();
        tops.sort_by(|a, b| a.0.total_cmp(&b.0));
        tops.first()
            .map(|&(_, x, width)| (x, width))
            .into_iter()
            .collect()
    }

    #[test]
    fn a_small_table_is_as_wide_as_its_content() {
        let page = page("| a | b |\n|---|---|\n| c | d |\n");
        let [(x, width)] = tables(&page)[..] else {
            panic!("one table");
        };
        assert_eq!(x, 0.0);
        assert!(width < 150.0, "{width}");
        let texts = texts(&page);
        let (a, b) = (text(&texts, "a"), text(&texts, "b"));
        assert_eq!(a.1, 1.0 + 13.0);
        assert!(b.1 > a.1 + a.3 + 26.0, "{a:?} {b:?}");
        assert_eq!(page.blocks[0].text, "a\tb\nc\td");
    }

    #[test]
    fn a_wide_table_reaches_into_the_area_and_wraps_its_cells() {
        let words = "word ".repeat(60);
        let page = page(&format!("| a | b |\n|---|---|\n| {words} | {words} |\n"));
        let [(_, width)] = tables(&page)[..] else {
            panic!("one table");
        };
        assert!((width - 1008.0).abs() < 1.0, "{width}");
        let texts = texts(&page);
        assert!(text(&texts, "word").3 < 500.0);
    }

    #[test]
    fn a_table_too_wide_even_wrapped_breaks_its_words_to_fit_the_area() {
        let long = "a".repeat(150);
        let page = page(&format!("| x | y |\n|---|---|\n| {long} | {long} |\n"));
        let [(_, width)] = tables(&page)[..] else {
            panic!("one table");
        };
        assert!(width <= 1008.0 + 0.5, "{width}");
        let texts = texts(&page);
        assert!(text(&texts, "aaa").3 < 500.0, "{texts:?}");
    }

    #[test]
    fn every_second_body_row_is_shaded() {
        let page = page("| a |\n|---|\n| 1 |\n| 2 |\n| 3 |\n");
        let shaded = (page.items.iter())
            .filter(|item| matches!(item.shape, Shape::Rect { color, .. } if Some(color) == LOOK.stripe))
            .count();
        assert_eq!(shaded, 1);
    }

    #[test]
    fn a_column_is_aligned_as_the_table_says() {
        let page = page("| left | right |\n|:---|---:|\n| a | b |\n");
        let paragraphs: Vec<(String, f32)> = (page.items.iter())
            .filter_map(|item| match &item.shape {
                Shape::Text { paragraph, .. } => {
                    let line = paragraph.layout.lines().next()?;
                    Some((paragraph.text().to_owned(), line.metrics().offset))
                }
                _ => None,
            })
            .collect();
        let offset = |text: &str| paragraphs.iter().find(|(t, _)| t == text).unwrap().1;
        assert_eq!(offset("a"), 0.0);
        assert!(offset("b") > 0.0, "{paragraphs:?}");
    }

    fn scrolling(markdown: &str) -> Page {
        let document = parse(markdown);
        let blocks: Vec<&Block> = document.blocks.iter().collect();
        let frame = Frame {
            wide: Wide::Scroll,
            ..FRAME
        };
        lay_out(&blocks, &LOOK, &Fonts::fixture(), &Plain, frame)
    }

    #[test]
    fn a_code_line_wider_than_the_area_scrolls_in_a_band_of_whole_rows() {
        let long = "a".repeat(200);
        let page = scrolling(&format!("Text.\n\n```\n{long}\n```\n\nMore.\n"));
        let [band] = &page.bands[..] else {
            panic!("one band");
        };
        assert_eq!(band.top % 20.0, 0.0);
        assert_eq!(band.height % 20.0, 0.0);
        assert!(band.top >= page.blocks[0].rows.end + 16.0, "{}", band.top);
        assert_eq!((band.x, band.visible), (0.0, 1008.0));
        assert!(band.width > 1008.0 + 500.0, "{}", band.width);
        assert!(band.most() > 500.0);
        // Its code stays one line, in the band and not on the page.
        assert!(code_blocks(&page).is_empty());
        let line = (band.items.iter()).find_map(|item| match &item.shape {
            Shape::Text { paragraph, .. } if paragraph.text() == long => {
                Some(paragraph.lines().len())
            }
            _ => None,
        });
        assert_eq!(line, Some(1));
        assert!(page.blocks[2].rows.start >= band.top + band.height);
        // Wrapping, the same code has no band.
        assert!(page_wrapped(&long).bands.is_empty());
    }

    fn page_wrapped(long: &str) -> Page {
        page(&format!("```\n{long}\n```\n"))
    }

    #[test]
    fn a_scrolling_table_keeps_its_words_whole() {
        let long = "a".repeat(150);
        let page = scrolling(&format!("| x | y |\n|---|---|\n| {long} | {long} |\n"));
        let [band] = &page.bands[..] else {
            panic!("one band");
        };
        let widest = (band.items.iter())
            .filter_map(|item| match &item.shape {
                Shape::Text { paragraph, .. } if paragraph.text() == long => {
                    Some((paragraph.lines().len(), paragraph.width()))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(widest.len(), 2);
        assert!(widest.iter().all(|&(lines, _)| lines == 1), "{widest:?}");
        assert!(band.width > 1008.0);
    }

    /// `markdown` laid out to scroll in an area `area` pixels wide.
    fn scrolling_in(markdown: &str, area: f32) -> Page {
        let document = parse(markdown);
        let blocks: Vec<&Block> = document.blocks.iter().collect();
        let frame = Frame {
            wide: Wide::Scroll,
            area,
            ..FRAME
        };
        lay_out(&blocks, &LOOK, &Fonts::fixture(), &Plain, frame)
    }

    #[test]
    fn a_table_that_shares_the_room_exactly_is_no_band() {
        // Tables whose cells wrap to fill the room: the widths share the
        // room out, and their sum may come out a hair over it.
        for area in [700.0, 800.0, 864.0, 1008.0, 1100.0] {
            for words in 20..90 {
                let (left, right) = ("word ".repeat(words), "other text ".repeat(100 - words));
                let table =
                    format!("| a | b | c |\n|---|---|---|\n| {left} | {right} | {left} |\n");
                let page = scrolling_in(&table, area);
                for band in &page.bands {
                    // A band scrolls by more than float rounding.
                    assert!(
                        band.most() > 2.0,
                        "{words} words in {area} px: {}",
                        band.most()
                    );
                }
            }
        }
    }

    #[test]
    fn a_code_line_that_reaches_the_edge_to_within_rounding_is_no_band() {
        let line = "a".repeat(120);
        let width = |area: f32| {
            let page = scrolling_in(&format!("```\n{line}\n```\n"), area);
            (page.bands.len(), page.items.len())
        };
        // The line and the block's padding, as wide as a very wide area
        // lets the block be.
        let natural = (scrolling_in(&format!("```\n{line}\n```\n"), 5000.0)
            .items
            .iter())
        .find_map(|item| match &item.shape {
            Shape::RoundRect { width, .. } => Some(*width),
            _ => None,
        })
        .expect("the block's background");
        assert_eq!(width(natural).0, 0);
        assert_eq!(width(natural - 0.25).0, 0);
        assert_eq!(width(natural - 4.0).0, 1);
    }

    #[test]
    fn a_table_that_fits_does_not_scroll() {
        let page = scrolling("| a | b |\n|---|---|\n| c | d |\n");
        assert!(page.bands.is_empty());
    }

    /// All the pixel rows of the first band of `page`.
    fn whole(page: &Page) -> Range<f32> {
        0.0..page.bands[0].height
    }

    #[test]
    fn a_slice_of_a_band_is_the_same_pixels_as_those_rows_of_the_whole() {
        let code: String = (0..40)
            .map(|n| format!("{n} {}\n", "a".repeat(200)))
            .collect();
        let page = scrolling(&format!("```\n{code}```\n"));
        let band = &page.bands[0];
        let (offset, rows) = (50.0, whole(&page));
        let all = page.draw_band(0, offset, true, rows.clone());
        // Rows 600 to the end, with the code's end and the scroll bar,
        // against the same rows cut from the whole.
        let part = page.draw_band(0, offset, true, 600.0..rows.end);
        assert_eq!(part.height(), (rows.end - 600.0) as u32);
        let cut = image::imageops::crop_imm(&all, 0, 600, all.width(), part.height()).to_image();
        // Below the first row, where a glyph cut by the slice's edge may
        // antialias a level or two differently, the pixels are the same.
        let rest = |image: &RgbaImage| {
            image::imageops::crop_imm(image, 0, 1, image.width(), image.height() - 1).to_image()
        };
        assert_eq!(rest(&part), rest(&cut));
        let bar = (band.content + 4.0).round() as u32;
        assert!(bar > 600, "the scroll bar is in the last slice");
        // Inside the thumb, which starts 50 pixels in, scaled.
        let thumb = (50.0 / band.width * band.visible) as u32 + 5;
        assert_eq!(&part.get_pixel(thumb, bar - 600 + 1).0[..3], &LOOK.text);
    }

    /// The alpha of the band's pixel at (`x`, `y`).
    fn alpha(image: &RgbaImage, x: u32, y: u32) -> u8 {
        image.get_pixel(x, y).0[3]
    }

    #[test]
    fn a_band_fades_on_each_edge_with_more_beyond_it() {
        let page = scrolling(&format!("```\n{}\n```\n", "a".repeat(200)));
        let band = &page.bands[0];
        let start = page.draw_band(0, 0.0, false, whole(&page));
        let end = page.draw_band(0, band.most(), false, whole(&page));
        let middle = page.draw_band(0, band.most() / 2.0, false, whole(&page));
        // The code's background, opaque in the middle, nearly gone at an
        // edge that fades.
        let (left, right, inner) = (1, 1006, 500);
        assert_eq!(alpha(&start, inner, 10), 255);
        assert!(
            alpha(&start, right, 10) < 60,
            "{}",
            alpha(&start, right, 10)
        );
        assert_eq!(alpha(&start, left + 4, 10), 255);
        assert!(alpha(&end, left, 10) < 60);
        assert_eq!(alpha(&end, right - 4, 10), 255);
        assert!(alpha(&middle, left, 10) < 60 && alpha(&middle, right, 10) < 60);
    }

    #[test]
    fn the_selected_band_s_thumb_is_brighter_and_thicker() {
        let page = scrolling(&format!("```\n{}\n```\n", "a".repeat(200)));
        let band = &page.bands[0];
        let bar = (band.content + 0.25 * 16.0).round() as u32;
        let (plain, selected) = (
            page.draw_band(0, 0.0, false, whole(&page)),
            page.draw_band(0, 0.0, true, whole(&page)),
        );
        let at = |image: &RgbaImage, y| image.get_pixel(20, y).0;
        assert_eq!(&at(&plain, bar + 1)[..3], &LOOK.quote);
        assert_eq!(&at(&selected, bar + 1)[..3], &LOOK.text);
        // Six pixels thick when selected, four otherwise.
        assert_eq!(at(&selected, bar + 5)[3], 255);
        assert_ne!(&at(&plain, bar + 5)[..3], &LOOK.quote);
        // The thumb is as long as the visible share: past it, the track.
        let thumb = (band.visible * band.visible / band.width) as u32;
        assert_ne!(&plain.get_pixel(thumb + 10, bar + 1).0[..3], &LOOK.quote);
    }

    #[test]
    fn a_tick_shows_on_a_light_box() {
        let light = Look {
            link: [0xee; 3],
            ..LOOK
        };
        let document = parse("- [x] done\n");
        let blocks: Vec<&Block> = document.blocks.iter().collect();
        let page = lay_out(&blocks, &light, &Fonts::fixture(), &Plain, FRAME);
        let image = page.draw(0, 40, 24);
        // The box is 13 pixels square, its right edge 8 pixels before the
        // text at 32: the tick is as dark as the box is light.
        let dark = (19..32)
            .flat_map(|x| (0..24).map(move |y| (x, y)))
            .filter(|&(x, y)| {
                let pixel = image.get_pixel(x, y).0;
                pixel[3] == 255 && pixel[0] < 0x40
            })
            .count();
        assert!(dark > 5, "{dark} dark pixels of the tick");
    }

    #[test]
    fn drawn_diagrams_and_lone_images_are_left_out() {
        let drawn = |language: &str, _: &str| language == "mermaid";
        let left_out = "```mermaid\ngraph\n```\n\n![x](x.png)\n\n\
            - a\n\n  ```mermaid\n  graph\n  ```\n";
        let document = parse(left_out);
        assert!(document.blocks.iter().all(|block| !lays_out(block, &drawn)));
        let laid_out = parse(
            "```\ncode\n```\n\n```mermaid\ngraph\n```\n\n| a |\n|---|\n\n- a\n\n      code\n",
        );
        assert!(
            laid_out
                .blocks
                .iter()
                .all(|block| lays_out(block, &|_, _| false))
        );
    }

    /// The left edge, width and height of each code block's background.
    fn code_blocks(page: &Page) -> Vec<(f32, f32, f32)> {
        (page.items.iter())
            .filter_map(|item| match item.shape {
                Shape::RoundRect {
                    x, width, height, ..
                } => Some((x, width, height)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_code_block_is_at_least_as_wide_as_the_text() {
        let page = page("```\nx\n```\n");
        let [(x, width, height)] = code_blocks(&page)[..] else {
            panic!("one code block");
        };
        assert_eq!((x, width), (0.0, 672.0));
        assert_eq!(height, page.blocks[0].rows.end);
        assert!(height > 32.0 + 19.0, "{height}");
    }

    #[test]
    fn a_long_code_line_reaches_into_the_area() {
        let page = page(&format!("```\n{}\n```\n", "a".repeat(100)));
        let [(_, width, _)] = code_blocks(&page)[..] else {
            panic!("one code block");
        };
        assert!(width > 672.0 && width < 1008.0, "{width}");
    }

    #[test]
    fn a_code_line_longer_than_the_area_wraps_at_its_edge() {
        let long = "a".repeat(200);
        let page = page(&format!("```\n{long}\n```\n"));
        let [(_, width, _)] = code_blocks(&page)[..] else {
            panic!("one code block");
        };
        assert_eq!(width, 1008.0);
        let texts = texts(&page);
        let code = text(&texts, "aaa");
        assert!(code.3 <= 1008.0 - 32.0, "{code:?}");
        let lines = (page.items.iter()).find_map(|item| match &item.shape {
            Shape::Text { paragraph, .. } if paragraph.text() == long => {
                Some(paragraph.lines().len())
            }
            _ => None,
        });
        assert_eq!(lines, Some(2));
    }

    #[test]
    fn a_code_block_has_its_language_in_the_top_right_corner() {
        let page = page("```rust\nfn main() {}\n```\n");
        let texts = texts(&page);
        let label = text(&texts, "rust");
        assert_eq!(label.1 + label.3, 672.0 - 0.6 * 16.0);
        assert!(label.2 < 20.0, "{label:?}");
        assert_eq!(page.blocks[0].text, "fn main() {}");
        // In the padding above the code, so that a long first line never
        // runs under it.
        let bottom = (page.items.iter()).find_map(|item| match &item.shape {
            Shape::Text { paragraph, y, .. } if paragraph.text() == "rust" => {
                Some(y + paragraph.height())
            }
            _ => None,
        });
        assert!(bottom.is_some_and(|bottom| bottom <= 16.0), "{bottom:?}");
    }

    #[test]
    fn a_part_of_the_page_draws_what_reaches_into_it() {
        let page = page("---\n\nText.\n");
        let top = page.draw(0, 100, 4);
        assert_eq!(top.get_pixel(50, 0).0, [0x3d, 0x44, 0x4d, 255]);
        let below = page.draw(4, 100, 4);
        assert_eq!(below.get_pixel(50, 0).0, [0; 4]);
        assert_eq!((below.width(), below.height()), (100, 4));
    }
}
