//! A run of a document's text blocks laid out as a page, as GitHub sets them,
//! with their margins collapsing between blocks as in CSS. The blocks are
//! headings, paragraphs, lists, quotes and thematic breaks.
//!
//! This is the reading engine's layout: the flow of blocks, their margins
//! and the pixel boxes of a page. The layout of each block kind is in its
//! own file under `page/`. It takes its fonts and pictures as values and
//! does no I/O.
//!
//! A new block kind does not go in here as more code in this file, nor
//! does text shaping, which is `text.rs`, painting, which is `paint.rs`, or
//! anything about terminals, files or the pager's state.
//!
//! A block kind of its own gets a file in `page/`, the typesetting engine's
//! reading module (`src/typeset/reading/`). Showing the pixels in a terminal
//! is the work of the terminal layer (`src/present/`), and what the pager
//! does with a page is the state engine's (`src/session/`).
//!
//! The page is drawn a part at a time, so that a pager draws only what it
//! shows.

mod centred;
mod code;
mod details;
#[cfg(test)]
mod finds;
mod front_matter;
mod heading;
mod html;
mod list;
mod origins;
mod picture;
mod quote;
mod reach;
pub(crate) mod table;

use std::cell::Cell as Shared;
use std::hash::Hash;
use std::ops::Range;

use std::rc::Rc;

use image::RgbaImage;

use crate::typeset::grid;
use crate::typeset::model::{AlertKind, Block, Image, Inline};
use crate::typeset::reading::fonts::Fonts;
use crate::typeset::reading::glyph_cache::GlyphCache;
use crate::typeset::reading::paint::{Canvas, TextMark};
use crate::typeset::reading::pixels::Pixels;
use crate::typeset::reading::places::ItemAt;
use crate::typeset::reading::text::{self, Brush, Paragraph, Run};
use crate::typeset::text::{Cursor, Part, number as number_of};
use crate::typeset::theme::PixelTable;

use self::list::task;
use super::inlines::{Ink, push, push_with, runs, runs_into};

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
    /// How tables are drawn.
    pub table_style: TableStyle,
    pub stripe: Option<Brush>,
    /// The page's colour, which a table in the cell of a shaded row paints
    /// its own unshaded rows in; nothing is painted over an unshaded row.
    pub page: Brush,
    /// Whether the page is drawn on an opaque background in `page`, which
    /// is so when the colour is known and not a guess.
    pub opaque: bool,
    /// The colour of what could not be drawn, such as a diagram's error.
    pub error: Brush,
    /// The colours of marked text, `<mark>`.
    pub marked: Marked,
    /// The spacing, GitHub's, by which blocks are laid out.
    pub spacing: PixelTable,
    /// How alerts look, by kind: note, tip, important, warning, caution.
    pub alerts: [AlertLook; 5],
}

/// The colours of marked text: its own, and the box behind it; `None`
/// leaves what the text has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Marked {
    pub foreground: Option<Brush>,
    pub background: Option<Brush>,
}

/// How a table is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableStyle {
    /// A rounded frame, lines between the columns and under the header
    /// row, none between the rows.
    Frame,
    /// A line round every cell, square corners.
    Grid,
}

impl Look {
    /// How alerts of `kind` look.
    pub fn alert(&self, kind: AlertKind) -> AlertLook {
        self.alerts[kind as usize]
    }
}

/// The colours of one kind of alert.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AlertLook {
    /// Its icon and its title.
    pub color: Brush,
    /// Its bar.
    pub bar: Brush,
    /// Its background; `None` for the page's.
    pub tint: Option<Brush>,
    /// The chip behind inline code in it, if inline code has one.
    pub chip: Option<Brush>,
}

/// What a table's cell holds in reading mode: text.
pub type Cell = grid::Cell<Leaf>;

/// Rows of cells, as a table or a front matter's map has them; a header's
/// cells show in bold.
pub type Grid = grid::Grid<Leaf>;

/// A leaf of reading mode's grid: the inlines of a cell, and the number of
/// the text of the block that it is, when it is one.
#[derive(Debug, Clone, PartialEq)]
pub struct Leaf {
    pub text: Option<usize>,
    pub content: Vec<Inline>,
}

/// The colours of code: each line of `code`, in `language` if known, as
/// runs of text in their colours. The tabs are spaces already.
pub trait Highlight {
    fn lines(&self, language: Option<&str>, code: &str) -> Vec<Vec<Run>>;
}

/// What a picture, an image or a diagram, is on the page.
pub enum Picture {
    /// Not one: the document's alt text shows in its place, or for a
    /// diagram the code.
    None,
    /// Its pixels, at the size they are shown at, and an id of what they
    /// are, so that the same page of other pixels is another page. Set
    /// when what shows is a stand-in while the real one loads.
    Ready {
        pixels: Rc<Pixels>,
        id: u64,
        loading: Option<usize>,
        /// How many of its pixels are below the baseline it stands on in
        /// a line of text; none unless it is a formula in one.
        depth: f32,
    },
    /// Not here yet: room for it, and a tag for the pictures to find it
    /// by.
    Loading { width: u32, height: u32, tag: usize },
    /// A diagram that could not be drawn: the message to show below its
    /// code, if any.
    Failed(Option<String>),
}

/// The pictures of a document, as the page asks for them.
pub trait Pictures {
    /// The image `image` shows, in a room at most `room.0` pixels wide and
    /// `room.1` high.
    fn image(&self, image: &Image, room: (f32, f32)) -> Picture;
    /// The code block `code` in `language`, when it is drawn as a diagram
    /// or formula.
    fn diagram(&self, language: &str, code: &str, room: (f32, f32)) -> Picture;
    /// The formula `source` in the middle of text, as the text around it
    /// is: in text style, at the size of the text. Its `depth` is how far
    /// it reaches below its baseline.
    fn formula(&self, source: &str, room: (f32, f32)) -> Picture;
}

/// Code in the plain colour of code.
pub struct Plain;

impl Pictures for Plain {
    fn image(&self, _: &Image, _: (f32, f32)) -> Picture {
        Picture::None
    }

    fn diagram(&self, _: &str, _: &str, _: (f32, f32)) -> Picture {
        Picture::None
    }

    fn formula(&self, _: &str, _: (f32, f32)) -> Picture {
        Picture::None
    }
}

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
    /// Whether headings start on whole rows (`snap_to_rows`).
    pub snap: bool,
    /// How front matter shows.
    pub front_matter: FrontMatterShown,
    /// The height in pixels after which the page ends, at the end of the
    /// block that passes it; infinite for one page whatever its height.
    pub budget: f32,
    /// The bottom margin of the block before the page, when the page goes
    /// on from one that was cut.
    pub margin: Option<f32>,
}

/// How front matter shows, when it shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrontMatterShown {
    /// As GitHub shows it: a table of keys and values, with tables in it
    /// for nested values.
    Table,
    /// As GitLab shows it: a code block in its language.
    Code,
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
    /// A code block's language, which stays at the band's top right corner
    /// as the code scrolls under it.
    label: Option<Label>,
    /// What it draws, from its own top left.
    pub(super) items: Vec<Item>,
}

/// A band's label, and its top from the band's.
struct Label {
    paragraph: Box<Paragraph>,
    y: f32,
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
    /// The blocks inside it, and itself, by their path from it, with the
    /// pixels each covers, from the top of the page.
    pub(super) inner: Vec<(Vec<usize>, Range<f32>)>,
}

/// A run of blocks laid out.
pub struct Page {
    pub width: f32,
    pub height: f32,
    /// Each block laid out, in order.
    pub blocks: Vec<Placed>,
    /// The code blocks and tables that scroll, in order.
    pub bands: Vec<Band>,
    /// The pictures not loaded yet, or standing in for ones loading.
    pub loading: Vec<Loading>,
    /// The ids of the pictures on the page, in order, which are part of
    /// what the page shows.
    pub pictures: Vec<u64>,
    pub(super) items: Vec<Item>,
    look: Look,
    /// Its links, from the top down.
    links: Vec<LinkPlace>,
    /// The bottom margin of its last block, which the page after it starts
    /// with when the run of blocks goes on.
    pub margin: Option<f32>,
}

/// The part of a line that a link or a match covers, in pixels from the
/// top left of the page, or of the band it is in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Piece {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// A link on the page: where it goes and where it is.
#[derive(Debug, Clone, PartialEq)]
pub struct LinkPlace {
    pub url: Rc<str>,
    /// One piece for each line it covers.
    pub pieces: Vec<Piece>,
    /// The band it is in, whose pixels the pieces are in, if it is in one.
    pub band: Option<usize>,
}

/// The colours of a search's marks: foreground and background of a match,
/// and of the current one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarkColors {
    pub found: (Option<Brush>, Option<Brush>),
    pub current: (Option<Brush>, Option<Brush>),
}

/// What is drawn over a page: the places of a search, one of them the
/// current, and the selected link, as its number among the page's links.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Overlay {
    pub colors: MarkColors,
    pub marks: Vec<(ItemRef, Range<usize>, bool)>,
    pub link: Option<usize>,
    /// The pixel rows from the top of the page that each mark covers, and
    /// the selected link.
    pub rows: Vec<Range<u32>>,
    pub link_rows: Option<Range<u32>>,
}

impl Overlay {
    /// A hash of what the overlay draws over the pixel rows `rows` of the
    /// page, and 0 when it draws nothing there: a part with no mark in it
    /// is the part it was.
    pub fn hash_over(&self, rows: Range<u32>) -> u64 {
        use std::hash::{Hash, Hasher};
        let over = |covered: &Range<u32>| covered.start < rows.end && rows.start < covered.end;
        let marks: Vec<_> = (self.marks.iter().zip(&self.rows))
            .filter(|(_, covered)| over(covered))
            .map(|(mark, _)| mark)
            .collect();
        let link = self
            .link
            .filter(|_| self.link_rows.as_ref().is_some_and(over));
        if marks.is_empty() && link.is_none() {
            return 0;
        }
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        format!("{:?} {marks:?} {link:?}", self.colors).hash(&mut hasher);
        hasher.finish().max(1)
    }

    fn marks_in(&self, at: ItemRef) -> Vec<TextMark> {
        (self.marks.iter())
            .filter(|(item, ..)| *item == at)
            .map(|(_, range, current)| {
                let (foreground, background) = match current {
                    true => self.colors.current,
                    false => self.colors.found,
                };
                TextMark {
                    range: range.clone(),
                    foreground,
                    background,
                }
            })
            .collect()
    }
}

/// Text and pictures of one link: where the last ends, where it links to,
/// and each member's bytes and, for a picture, its number.
type Joined = (usize, Rc<str>, Vec<(Range<usize>, Option<usize>)>);

/// Where an item is: its number among the items of the page, or of a
/// band, which `band` says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ItemRef {
    pub band: Option<usize>,
    pub index: usize,
}

impl Page {
    /// The links on the page, from the top down.
    pub fn links(&self) -> &[LinkPlace] {
        &self.links
    }

    /// The pixel rows from the top of the page that the bytes `range` of
    /// the text `item` cover.
    pub fn mark_rows(&self, item: ItemRef, range: Range<usize>) -> Range<u32> {
        let (items, top) = match item.band {
            None => (&self.items, 0.0),
            Some(band) => match self.bands.get(band) {
                Some(band) => (&band.items, band.top),
                None => return 0..0,
            },
        };
        let Some(Shape::Text { paragraph, y, .. }) = items.get(item.index).map(|item| &item.shape)
        else {
            return 0..0;
        };
        let (mut from, mut to) = (f32::MAX, 0.0f32);
        for part in paragraph.boxes(range) {
            if let Some(line) = paragraph.line(part.line) {
                from = from.min(top + y + line.top);
                to = to.max(top + y + line.top + line.height);
            }
        }
        if from > to {
            return 0..0;
        }
        from.floor() as u32..to.ceil() as u32
    }

    /// The pixel rows from the top of the page that link `number` covers.
    pub fn link_rows(&self, number: usize) -> Range<u32> {
        let Some(link) = self.links.get(number) else {
            return 0..0;
        };
        let top = link
            .band
            .and_then(|band| self.bands.get(band))
            .map_or(0.0, |band| band.top);
        let from = (link.pieces.iter())
            .map(|piece| piece.y)
            .fold(f32::MAX, f32::min);
        let to = (link.pieces.iter())
            .map(|piece| piece.y + piece.height)
            .fold(0.0, f32::max);
        (top + from).floor() as u32..(top + to).ceil() as u32
    }

    /// Finds the links among the items, in no order: `lay_out` sorts them.
    fn place_links(&self) -> Vec<LinkPlace> {
        let mut found: Vec<LinkPlace> = Vec::new();
        let on_page = self.items.iter().map(|item| (None, item));
        let in_bands = (self.bands.iter().enumerate())
            .flat_map(|(number, band)| (band.items.iter()).map(move |item| (Some(number), item)));
        for (band, item) in on_page.chain(in_bands) {
            let Shape::Text {
                paragraph, x, y, ..
            } = &item.shape
            else {
                continue;
            };
            let lines = paragraph.lines();
            // The text and the pictures of a link, in order; those of one
            // link that touch are one link, whatever their styles.
            let mut parts: Vec<(usize, usize, Rc<str>, Option<usize>)> = Vec::new();
            for (at, number, url) in paragraph.linked_pictures() {
                parts.push((*at, *at, Rc::clone(url), Some(*number)));
            }
            for (range, url) in paragraph.links() {
                parts.push((range.start, range.end, Rc::clone(url), None));
            }
            parts.sort_by_key(|(start, _, _, picture)| (*start, picture.is_none()));
            let mut joined: Vec<Joined> = Vec::new();
            for (start, end, url, picture) in parts {
                match joined.last_mut() {
                    Some((last, before, members)) if *last == start && *before == url => {
                        *last = end;
                        members.push((start..end, picture));
                    }
                    _ => joined.push((end, url, vec![(start..end, picture)])),
                }
            }
            for (_, url, members) in joined {
                let mut boxes = Vec::new();
                for (range, picture) in members {
                    match picture {
                        Some(number) => boxes.extend(paragraph.picture_box(number)),
                        None => boxes.extend(paragraph.boxes(range)),
                    }
                }
                let mut pieces: Vec<Piece> = Vec::new();
                for part in boxes {
                    let Some(line) = lines.get(part.line) else {
                        continue;
                    };
                    let piece = Piece {
                        x: x + part.left,
                        y: y + line.top,
                        width: part.right - part.left,
                        height: line.height,
                    };
                    // Pieces side by side on a line are one.
                    match pieces.last_mut() {
                        Some(last) if last.y == piece.y && piece.x <= last.x + last.width + 0.5 => {
                            let right = (last.x + last.width).max(piece.x + piece.width);
                            last.width = right - last.x;
                        }
                        _ => pieces.push(piece),
                    }
                }
                if !pieces.is_empty() {
                    found.push(LinkPlace { url, pieces, band });
                }
            }
        }
        found
    }
}

/// A picture that is loading: where it is and the tag its `Pictures` gave.
#[derive(Debug, Clone, PartialEq)]
pub struct Loading {
    /// The pixels it covers, from the top of the page.
    pub rows: Range<f32>,
    pub x: f32,
    pub width: f32,
    pub tag: usize,
}

/// Something drawn on the page, with the pixels it may reach.
pub(super) struct Item {
    rows: Range<f32>,
    pub(super) shape: Shape,
    /// What of the document it shows, if it is a text or a picture that
    /// shows any.
    pub(super) at: Option<ItemAt>,
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
            | Shape::Frame {
                x: left, y: top, ..
            }
            | Shape::Stripe {
                x: left, y: top, ..
            }
            | Shape::Picture {
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

pub(super) enum Shape {
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
    /// The outline of a rectangle with rounded corners, one pixel thick.
    Frame {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        radius: f32,
        color: Brush,
    },
    /// A rectangle with its corners rounded to `radii`: top left, top
    /// right, bottom right, bottom left; the fill of the first or the last
    /// row of a table in a frame.
    Stripe {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        radii: [f32; 4],
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
    /// A picture at its own size.
    Picture { x: f32, y: f32, pixels: Rc<Pixels> },
    /// The box of a task list item, ticked or not.
    Task {
        x: f32,
        y: f32,
        size: f32,
        checked: bool,
        look: Box<Look>,
    },
}

/// Whether reading mode lays `block` out: every block of a document is
/// laid out, with what is inside it. What is left is no part of a
/// document: a message of kiri's own about a file that could not be
/// shown, and an image file shown on its own.
pub fn lays_out(block: &Block) -> bool {
    match block {
        Block::FrontMatter(_)
        | Block::Heading { .. }
        | Block::ThematicBreak
        | Block::Table(_)
        | Block::Paragraph(_)
        | Block::CodeBlock { .. }
        | Block::Html(_)
        | Block::Note(_) => true,
        Block::Details { .. }
        | Block::BlockQuote(_)
        | Block::Alert { .. }
        | Block::Centered(_)
        | Block::List(_) => block.children().all(|(_, child)| lays_out(child)),
        Block::Error(_) | Block::ImageFile(_) => false,
    }
}

/// `blocks`, which reading mode lays out, laid out in `fonts` as `look`
/// says, in `frame`: the text at the left of the area, which tables and
/// code may reach across, and what becomes of those wider than it. Code
/// takes its colours from `highlight`, and pictures come from `pictures`.
pub fn lay_out(
    blocks: &[&Block],
    look: &Look,
    fonts: &Fonts,
    (highlight, pictures): (&dyn Highlight, &dyn Pictures),
    frame: Frame,
) -> Page {
    let width = frame.width;
    let area = frame.area.max(width);
    let mut flow = Flow {
        fonts,
        look,
        highlight,
        pictures,
        area,
        row: frame.row.max(1.0),
        wide: frame.wide,
        front_matter: frame.front_matter,
        items: Vec::new(),
        bands: Vec::new(),
        loading: Vec::new(),
        linked: Vec::new(),
        ids: Vec::new(),
        path: Vec::new(),
        extents: Vec::new(),
        cursor: Shared::new(Cursor::off()),
        y: 0.0,
        margin: frame.margin,
        shaded: false,
        centre: false,
    };
    let mut placed = Vec::new();
    for (number, block) in blocks.iter().enumerate() {
        let above = flow.y;
        let mut top = flow.top(block_margins(block, look).0);
        if frame.snap && heading::snaps(block) {
            top = flow.snap(above);
        }
        let (items, bands) = (flow.items.len(), flow.bands.len());
        flow.block(block, 0.0, width, 0);
        let inner = flow.closed(number, (items, bands));
        placed.push(Placed {
            rows: top..flow.y,
            inner,
        });
        // The block that passes the budget is the last of the page.
        if flow.y >= frame.budget {
            break;
        }
    }
    let mut page = Page {
        width: area,
        height: flow.y,
        blocks: placed,
        bands: flow.bands,
        loading: flow.loading,
        pictures: flow.ids,
        items: flow.items,
        look: *look,
        links: Vec::new(),
        margin: flow.margin,
    };
    page.links = page.place_links();
    page.links.extend(flow.linked);
    page.links.sort_by(|a, b| {
        let key = |link: &LinkPlace| {
            let top = link
                .band
                .and_then(|band| page.bands.get(band))
                .map_or(0.0, |band| band.top);
            (top + link.pieces[0].y, link.pieces[0].x)
        };
        key(a)
            .partial_cmp(&key(b))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    page
}

impl Page {
    /// How the page is drawn.
    pub fn look(&self) -> &Look {
        &self.look
    }

    /// The colour the page is drawn on, where it is opaque.
    fn background(&self) -> Option<Brush> {
        self.look.opaque.then_some(self.look.page)
    }

    /// The part of the page from `top`, `height` pixels tall and as wide as
    /// `width`, transparent where nothing is drawn unless the page is
    /// opaque, with `overlay`'s marks and selected link on it. Bands are
    /// not drawn.
    pub fn draw_with(
        &self,
        glyphs: &mut GlyphCache,
        top: u32,
        width: u32,
        height: u32,
        overlay: Option<&Overlay>,
    ) -> RgbaImage {
        let mut canvas = Canvas::on(width, height, self.background(), glyphs);
        let at = (0.0, top as f32);
        draw_items(
            &mut canvas,
            &self.items,
            at,
            overlay.map(|overlay| (None, overlay)),
        );
        if let Some(overlay) = overlay {
            self.draw_link(&mut canvas, None, at, overlay);
        }
        canvas.into_image()
    }

    /// The box round the selected link of `overlay`, where it is in the
    /// page (`band` is `None`) or in a band, with the point `at` of their
    /// coordinates at the canvas's top left.
    fn draw_link(
        &self,
        canvas: &mut Canvas,
        band: Option<usize>,
        at: (f32, f32),
        overlay: &Overlay,
    ) {
        let Some(link) = overlay.link.and_then(|number| self.links.get(number)) else {
            return;
        };
        if link.band != band {
            return;
        }
        let size = self.look.size;
        for piece in &link.pieces {
            let (x, y) = (piece.x - at.0 - 2.0, piece.y - at.1 - 1.0);
            let (width, height) = (piece.width + 4.0, piece.height + 2.0);
            canvas.round_rect_alpha(x, y, width, height, 0.25 * size, self.look.link, 64);
            canvas.frame(x, y, width, height, 2.0, self.look.link);
        }
    }

    /// The pixel rows `rows` of band `band`, counted from its top, as wide
    /// as the page: what the page has beside it, and the part of its
    /// content `offset` pixels from its left edge, faded on each edge with
    /// more beyond it, above its scroll bar, which is brighter and thicker
    /// when it is `selected`, with `overlay`'s marks and selected link on
    /// it.
    pub fn draw_band_with(
        &self,
        glyphs: &mut GlyphCache,
        number: usize,
        offset: f32,
        selected: bool,
        rows: Range<f32>,
        overlay: Option<&Overlay>,
    ) -> RgbaImage {
        let height = (rows.end - rows.start).ceil().max(1.0) as u32;
        let width = self.width.ceil() as u32;
        let Some(band) = self.bands.get(number) else {
            return Canvas::on(width, height, self.background(), glyphs).into_image();
        };
        let size = self.look.size;
        let mut canvas = Canvas::on(width, height, self.background(), glyphs);
        draw_items(
            &mut canvas,
            &self.items,
            (0.0, band.top + rows.start),
            overlay.map(|overlay| (None, overlay)),
        );
        let offset = offset.clamp(0.0, band.most());
        // The content is above the scroll bar, which may be below this
        // part of the rows.
        let content = (band.content - rows.start).clamp(0.0, height as f32);
        if content > 0.0 {
            let at = (offset, rows.start);
            let shown = (band.visible.ceil() as u32, content.ceil() as u32);
            canvas.layer(shown, (band.x, 0.0), |shown| {
                draw_items(shown, &band.items, at, overlay.map(|o| (Some(number), o)));
                if let Some(overlay) = overlay {
                    self.draw_link(shown, Some(number), at, overlay);
                }
                let fade = (FADE * size).min(band.visible / 3.0);
                if offset < band.most() {
                    shown.fade(band.visible - fade, fade, content, false);
                }
                if offset > 0.0 {
                    shown.fade(0.0, fade, content, true);
                }
            });
        }
        if let Some(label) = &band.label {
            let left = band.x + band.visible - 0.6 * size - label.paragraph.width();
            canvas.text(&label.paragraph, left, label.y - rows.start, None);
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
fn draw_items(
    canvas: &mut Canvas,
    items: &[Item],
    (x, y): (f32, f32),
    overlay: Option<(Option<usize>, &Overlay)>,
) {
    let (from, to) = (y, y + canvas.height() as f32);
    for (number, item) in items.iter().enumerate() {
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
            } => {
                let marks = overlay.map_or_else(Vec::new, |(band, overlay)| {
                    overlay.marks_in(ItemRef {
                        band,
                        index: number,
                    })
                });
                canvas.text_marked(paragraph, x + dx, y + dy, *chip, &marks);
            }
            Shape::Rect {
                x,
                y,
                width,
                height,
                color,
            } => canvas.rect(x + dx, y + dy, *width, *height, *color),
            Shape::Frame {
                x,
                y,
                width,
                height,
                radius,
                color,
            } => canvas.round_frame(x + dx, y + dy, *width, *height, *radius, *color),
            Shape::Stripe {
                x,
                y,
                width,
                height,
                radii,
                color,
            } => canvas.corners_rect(x + dx, y + dy, *width, *height, *radii, *color),
            Shape::RoundRect {
                x,
                y,
                width,
                height,
                radius,
                color,
            } => canvas.round_rect(x + dx, y + dy, *width, *height, *radius, *color),
            Shape::Picture { x, y, pixels } => canvas.picture(pixels, x + dx, y + dy),
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
    pictures: &'a dyn Pictures,
    /// The right edge of the area, which tables and code may reach.
    area: f32,
    /// The height of a row, which bands start on, and what becomes of
    /// code and tables wider than the area.
    row: f32,
    wide: Wide,
    front_matter: FrontMatterShown,
    items: Vec<Item>,
    bands: Vec<Band>,
    loading: Vec<Loading>,
    /// The links that are pictures on their own, in the page's pixels.
    linked: Vec<LinkPlace>,
    /// The ids of the pictures laid out.
    ids: Vec<u64>,
    /// Where the block being laid out is in the block of the page that it is
    /// inside, and what the blocks laid out so far cover; see `origins.rs`.
    path: Vec<usize>,
    extents: Vec<(Vec<usize>, Range<f32>)>,
    /// Where the inlines being laid out are in the text of their block.
    cursor: Shared<Cursor>,
    /// The bottom of what is laid out so far.
    y: f32,
    /// The bottom margin of the block above, until the next block takes
    /// the larger of it and its own top margin; `None` at the top, where
    /// no margin is kept.
    margin: Option<f32>,
    /// Whether it is laid out on a shaded row of a table, which a table in
    /// it paints its own unshaded rows over, in the page's colour.
    shaded: bool,
    /// Whether a paragraph, heading or picture is centred in its room.
    centre: bool,
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

impl<'a> Flow<'a> {
    /// A flow for what is inside a block, in `look`, from the row `top`.
    fn child<'b>(&self, look: &'b Look, top: f32) -> Flow<'b>
    where
        'a: 'b,
    {
        Flow {
            fonts: self.fonts,
            look,
            highlight: self.highlight,
            pictures: self.pictures,
            area: self.area,
            row: self.row,
            wide: self.wide,
            front_matter: self.front_matter,
            items: Vec::new(),
            bands: Vec::new(),
            loading: Vec::new(),
            linked: Vec::new(),
            ids: Vec::new(),
            path: self.path.clone(),
            extents: Vec::new(),
            cursor: Shared::new(Cursor::off()),
            y: top,
            margin: None,
            shaded: false,
            centre: false,
        }
    }

    /// The pictures in text `room` pixels wide.
    fn ink(&self, room: f32) -> Ink<'_> {
        Ink {
            pictures: self.pictures,
            room,
            cursor: &self.cursor,
        }
    }

    /// Takes what `inner` laid out, which is where it put it.
    fn take(&mut self, inner: &mut Flow) {
        self.items.append(&mut inner.items);
        self.bands.append(&mut inner.bands);
        self.loading.append(&mut inner.loading);
        self.linked.append(&mut inner.linked);
        self.ids.append(&mut inner.ids);
        self.extents.append(&mut inner.extents);
    }

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
        if let Shape::Text { paragraph, .. } = &shape {
            self.ids.extend(paragraph.pictures.iter().map(|p| p.id));
        }
        self.items.push(Item {
            rows: rows.start - reach..rows.end + reach,
            shape,
            at: None,
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
        self.items.last_mut().expect("the item just pushed").at = self.item_at();
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
        let top = self.y;
        self.text_is(None);
        match block {
            Block::Heading { level, content, .. } => {
                self.text_is(number_of(block, Part::Content));
                self.heading(level, content, x, width);
            }
            Block::Paragraph(content) => {
                self.text_is(number_of(block, Part::Content));
                match Inline::lone_linked_image(content) {
                    Some((image, link)) => self.image(image, link, x, width),
                    None => {
                        let paragraph = self.paragraph(content, look.text, width);
                        self.y += self.text(paragraph, x);
                    }
                }
            }
            Block::ThematicBreak => {
                let height = (look.spacing.rule_height * size).round().max(1.0);
                self.rect(x, self.y, width, height, look.border);
                self.y += height;
            }
            Block::BlockQuote(blocks) => self.quote(blocks, x, width, depth),
            Block::List(list) => self.list(list, x, width, depth),
            Block::CodeBlock { language, code, .. } => {
                self.text_is(number_of(block, Part::Source));
                self.code_or_diagram(language.as_deref(), code, x, width);
            }
            Block::Table(table) => self.table(table, x),
            Block::Html(html) => {
                self.text_is(number_of(block, Part::Source));
                self.html(html, x, width);
            }
            Block::Note(note) => {
                let content = [Inline::Text(note.clone())];
                let paragraph = self.paragraph(&content, look.quote, width);
                self.y += self.text(paragraph, x);
            }
            Block::Details { summary, blocks } => {
                let number = number_of(block, Part::Summary);
                self.details(summary, number, blocks, x, width, depth);
            }
            Block::FrontMatter(front_matter) => self.front_matter(block, front_matter, x, width),
            Block::Alert { .. } => self.alert(block, x, width, depth),
            Block::Centered(blocks) => self.centred(blocks, x, width, depth),
            // Not part of a document, which `lays_out` leaves to the text
            // layout.
            Block::Error(_) | Block::ImageFile(_) => {}
        }
        self.margin = Some(block_margins(block, &look).1);
        self.covers(top);
    }

    /// Lays `blocks` out one below another from the current top.
    fn blocks(&mut self, blocks: &[Block], x: f32, width: f32, depth: usize) {
        for (index, block) in blocks.iter().enumerate() {
            self.top(block_margins(block, self.look).0);
            self.step(index, |flow| flow.block(block, x, width, depth));
        }
    }

    fn paragraph(&self, content: &[Inline], color: Brush, width: f32) -> Paragraph {
        let style = text::Look {
            size: self.look.size,
            line_height: self.look.spacing.line_height,
            weight: 400.0,
            color,
            align: self.align(),
            ..text::Look::PLAIN
        };
        let ink = self.ink(width);
        text::lay_out(self.fonts, &runs(content, self.look, ink), &style, width)
    }
}

#[cfg(test)]
pub(crate) mod summary;

#[cfg(test)]
mod tests;
