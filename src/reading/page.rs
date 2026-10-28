//! A run of a document's text blocks laid out as a page, as GitHub sets
//! them: headings, paragraphs, lists, quotes and thematic breaks, with
//! their margins collapsing between blocks as in CSS. The page is drawn a
//! part at a time, so that a pager draws only what it shows.

use std::ops::Range;

use image::RgbaImage;

use super::fonts::Fonts;
use super::paint::Canvas;
use super::text::{self, Brush, Paragraph, Run};
use crate::model::{Block, Inline, List};

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
    items: Vec<Item>,
}

/// Something drawn on the page, with the pixels it may reach.
struct Item {
    rows: Range<f32>,
    shape: Shape,
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
        Block::Heading { .. } | Block::ThematicBreak => true,
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
/// says: the text `width` pixels wide, at the left of an area `area`
/// pixels wide, which tables and code may reach across. Code takes its
/// colours from `highlight`.
pub fn lay_out(
    blocks: &[&Block],
    look: &Look,
    fonts: &Fonts,
    highlight: &dyn Highlight,
    (width, area): (f32, f32),
) -> Page {
    let area = area.max(width);
    let mut flow = Flow {
        fonts,
        look,
        highlight,
        area,
        items: Vec::new(),
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
        items: flow.items,
    }
}

impl Page {
    /// The part of the page from `top`, `height` pixels tall and as wide as
    /// `width`, transparent where nothing is drawn.
    pub fn draw(&self, top: u32, width: u32, height: u32) -> RgbaImage {
        let mut canvas = Canvas::new(width, height);
        let (from, to) = (top as f32, (top + height) as f32);
        for item in &self.items {
            if item.rows.end < from || item.rows.start > to {
                continue;
            }
            let shift = -from;
            match &item.shape {
                Shape::Text {
                    paragraph,
                    x,
                    y,
                    chip,
                } => canvas.text(paragraph, *x, y + shift, *chip),
                Shape::Rect {
                    x,
                    y,
                    width,
                    height,
                    color,
                } => canvas.rect(*x, y + shift, *width, *height, *color),
                Shape::RoundRect {
                    x,
                    y,
                    width,
                    height,
                    radius,
                    color,
                } => canvas.round_rect(*x, y + shift, *width, *height, *radius, *color),
                Shape::Task {
                    x,
                    y,
                    size,
                    checked,
                    look,
                } => task(&mut canvas, *x, y + shift, *size, *checked, look),
            }
        }
        canvas.into_image()
    }
}

/// Lays blocks out one below another, collapsing the margins between them.
struct Flow<'a> {
    fonts: &'a Fonts,
    look: &'a Look,
    highlight: &'a dyn Highlight,
    /// The right edge of the area, which tables and code may reach.
    area: f32,
    items: Vec<Item>,
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
                    items: Vec::new(),
                    y: top,
                    margin: None,
                };
                inner.blocks(blocks, x + indent, width - indent, depth);
                self.items.append(&mut inner.items);
                self.y = inner.y;
                let bar = (QUOTE_BAR * size).round().max(1.0);
                self.rect(x, top, bar, self.y - top, look.border);
            }
            Block::List(list) => self.list(list, x, width, depth),
            Block::CodeBlock { language, code, .. } => {
                self.code(language.as_deref(), code, x, width);
            }
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
        let paragraph = text::lay_out(self.fonts, &runs, &style, room - 2.0 * padding);
        let (_, natural) = paragraph.content_widths();
        let block_width = (natural + 2.0 * padding).clamp(width, room);
        let height = paragraph.height() + 2.0 * padding;
        let top = self.y;
        if let Some(color) = look.code_block {
            self.push(
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
        self.y += padding;
        self.text(paragraph, x + padding);
        if let (Some(language), Some(color)) =
            (language.filter(|name| !name.is_empty()), look.label)
        {
            let style = text::Look {
                size: size * LABEL_SIZE,
                color,
                ..text::Look::PLAIN
            };
            let label = text::lay_out(self.fonts, &[Run::new(language)], &style, block_width);
            let (label_x, label_y) = (
                x + block_width - 0.6 * size - label.width(),
                top + 0.3 * size,
            );
            let label_height = label.height();
            self.push(
                label_y..label_y + label_height,
                Shape::Text {
                    paragraph: Box::new(label),
                    x: label_x,
                    y: label_y,
                    chip: None,
                },
            );
        }
        self.y = top + height;
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
                items: Vec::new(),
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
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse;

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
    };

    fn page(markdown: &str) -> Page {
        let document = parse(markdown);
        let blocks: Vec<&Block> = document.blocks.iter().collect();
        assert!(
            blocks.iter().all(|block| lays_out(block, &|_, _| false)),
            "{blocks:?}"
        );
        lay_out(&blocks, &LOOK, &Fonts::fixture(), &Plain, (672.0, 1008.0))
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
    fn a_tick_shows_on_a_light_box() {
        let light = Look {
            link: [0xee; 3],
            ..LOOK
        };
        let document = parse("- [x] done\n");
        let blocks: Vec<&Block> = document.blocks.iter().collect();
        let page = lay_out(&blocks, &light, &Fonts::fixture(), &Plain, (672.0, 1008.0));
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
    fn drawn_diagrams_tables_and_lone_images_are_left_out() {
        let drawn = |language: &str, _: &str| language == "mermaid";
        let left_out = "```mermaid\ngraph\n```\n\n| a |\n|---|\n\n![x](x.png)\n\n\
            - a\n\n  ```mermaid\n  graph\n  ```\n";
        let document = parse(left_out);
        assert!(document.blocks.iter().all(|block| !lays_out(block, &drawn)));
        let laid_out = parse("```\ncode\n```\n\n```mermaid\ngraph\n```\n\n- a\n\n      code\n");
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
