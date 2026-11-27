//! Which block and which text a row of a reading page shows, and what of the
//! document each text on the page shows.
//!
//! A page is one picture cut into rows, and each row is a line of the cell
//! layout. This file answers what the rest of the engine does not: of
//! which block of the document a row is a part, and which text of which
//! block each text on the page is. The blocks of a page are numbered in the
//! table of the layout (`Blocks`) before the page is laid out, and the page
//! knows its blocks by their paths from the block of the page they are in.
//!
//! The answers are `Ids`, `ItemAt` and the line that a page row makes, for the
//! row's block and text. What the page says of the document is `seen`, what
//! each of its texts shows, `rows_of`, the rows a text is on, and `found_at`,
//! what is at a point.

use std::ops::Range;
use std::rc::Rc;

use unicode_segmentation::GraphemeCursor;

use crate::typeset::block::BlockId;
use crate::typeset::images::size::Placement;
use crate::typeset::model::Block;
use crate::typeset::reading::engine::Parts;
use crate::typeset::reading::page::{Item, ItemRef, Page, Shape};
use crate::typeset::style::{ImageRow, Line, Origin, Span};

/// What of the document a text or a picture on a page shows: the text of
/// the block at `path` from the block `block` of the page, numbered
/// `text`; a picture stands in the place of the bytes `picture` of it.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct ItemAt {
    pub block: usize,
    pub path: Vec<usize>,
    pub text: u32,
    pub picture: Option<Range<u32>>,
}

/// The numbers in the table of a block of a page, and of the blocks inside
/// it by their path from it, in the order of the document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ids {
    pub id: BlockId,
    pub inside: Vec<(Vec<usize>, BlockId)>,
}

impl Ids {
    /// The numbers of `block` and of the blocks inside it, which `add` gives:
    /// it is called first for the block, with an empty path, and then for
    /// each block inside it with its path from it, each before the blocks
    /// inside that.
    pub fn new(block: &Block, mut add: impl FnMut(&[usize]) -> BlockId) -> Self {
        fn inside(
            block: &Block,
            path: &mut Vec<usize>,
            add: &mut impl FnMut(&[usize]) -> BlockId,
            found: &mut Vec<(Vec<usize>, BlockId)>,
        ) {
            for (steps, child) in block.children() {
                let at = path.len();
                match steps {
                    crate::typeset::model::Steps::One(a) => path.push(a),
                    crate::typeset::model::Steps::Two(a, b) => path.extend([a, b]),
                }
                found.push((path.clone(), add(path)));
                inside(child, path, add, found);
                path.truncate(at);
            }
        }
        let id = add(&[]);
        let mut found = Vec::new();
        inside(block, &mut Vec::new(), &mut add, &mut found);
        Self { id, inside: found }
    }

    /// The numbers of a block that the page lays out inside a centred
    /// wrapper of its own, so that the page's block is the wrapper and the
    /// block is its first: the same block, one step deeper, and the blocks
    /// inside it with the step in front of their paths.
    pub fn in_wrapper(self) -> Self {
        let inside = (self.inside.into_iter())
            .map(|(mut path, id)| {
                path.insert(0, 0);
                (path, id)
            })
            .collect();
        let mut found = vec![(vec![0], self.id)];
        found.extend::<Vec<_>>(inside);
        Self {
            id: self.id,
            inside: found,
        }
    }

    /// The innermost block of the page's block that covers `middle`, from
    /// the top of the page: the block itself when no block inside it does.
    fn innermost(&self, rows: &[(Vec<usize>, Range<f32>)], middle: f32) -> BlockId {
        let deepest = (rows.iter())
            .filter(|(_, covered)| covered.contains(&middle))
            .max_by_key(|(path, _)| path.len());
        deepest
            .and_then(|(path, _)| self.inside.iter().find(|(inside, _)| inside == path))
            .map_or(self.id, |(_, id)| *id)
    }
}

/// Some bytes of a text on a page, or a picture, that show some of the
/// document: `origin` has the bytes in the text of the item, and says
/// which text of the block they show.
#[derive(Debug, Clone)]
pub struct Seen {
    pub block: BlockId,
    /// The text on the page, or none for a picture.
    pub item: Option<ItemRef>,
    pub origin: Origin,
    /// For a picture, the rows of the page it is on, counted from the top
    /// of the page.
    pub rows: Option<Range<usize>>,
    /// For a picture that is a hole for an animation to play in, the id its
    /// pixels carry and how large it is: what tells which picture it is.
    pub hole: Option<(u64, (u32, u32))>,
    /// For a picture, its centre, which tells the link it is in.
    pub centre: Option<Centre>,
}

/// A point of a page in the pixels its links are in: from the top left of
/// the page, or of the band, which `band` says.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Centre {
    pub band: Option<usize>,
    pub x: f32,
    pub y: f32,
}

/// The rows of a page that some bytes of a text are on, counted from the top
/// of the page, and the band they are in with the pixels of its content they
/// cover.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rows {
    pub rows: Range<usize>,
    pub band: Option<(usize, Range<u32>)>,
}

/// What is at a point of a page: the block, and the text of it and the byte
/// of that text when the point is on a text or a picture of one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub block: BlockId,
    pub text: Option<(u32, u32)>,
}

impl Parts {
    /// The block in the table that what `at` says is on the page is in.
    fn block_of(&self, at: &ItemAt) -> BlockId {
        let ids = &self.ids[at.block];
        match at.path.is_empty() {
            true => ids.id,
            false => {
                let inside = ids.inside.iter().find(|(path, _)| *path == at.path);
                inside.map_or(ids.id, |(_, id)| *id)
            }
        }
    }

    /// The block that band `band` shows: the one of its first text, which is
    /// the code or the table, inside any quote or list it is in.
    pub fn band_block(&self, band: u32) -> Option<BlockId> {
        let laid = self.page.bands.get(band as usize)?;
        let at = laid.items.iter().find_map(|item| item.at.as_ref())?;
        Some(self.block_of(at))
    }

    /// Every item of the page, those on the page and those in its bands,
    /// with where it is.
    fn all_items(&self) -> impl Iterator<Item = (ItemRef, &Item)> {
        let on_page = (self.page.items.iter().enumerate())
            .map(|(index, item)| (ItemRef { band: None, index }, item));
        let in_bands = (self.page.bands.iter().enumerate()).flat_map(|(band, laid)| {
            (laid.items.iter().enumerate()).map(move |(index, item)| {
                let band = Some(band);
                (ItemRef { band, index }, item)
            })
        });
        on_page.chain(in_bands)
    }

    /// What the page shows of the document: the parts of each text, and
    /// each picture, with the block it is of.
    pub fn seen(&self) -> Vec<Seen> {
        let mut seen = Vec::new();
        for (item, laid) in self.all_items() {
            let Some(at) = &laid.at else { continue };
            let block = self.block_of(at);
            let top = item.band.map_or(0.0, |band| self.page.bands[band].top);
            let text = |origin: &Origin| Seen {
                block,
                item: Some(item),
                origin: origin.clone(),
                rows: None,
                hole: None,
                centre: None,
            };
            match &laid.shape {
                Shape::Text {
                    paragraph, x, y, ..
                } => {
                    let y_in = *y;
                    seen.extend(paragraph.from().iter().map(text));
                    for number in 0..paragraph.pictures.len() {
                        let Some(origin) = paragraph.picture_from(number) else {
                            continue;
                        };
                        let pixels = &paragraph.pictures[number].pixels;
                        let part = paragraph.picture_box(number);
                        let line = part.as_ref().and_then(|part| paragraph.line(part.line));
                        let centre = part.zip(line.as_ref()).map(|(part, line)| Centre {
                            band: item.band,
                            x: *x + (part.left + part.right) / 2.0,
                            y: y_in + line.top + line.height / 2.0,
                        });
                        seen.push(Seen {
                            block,
                            item: None,
                            origin: origin.clone(),
                            centre,
                            rows: line.map(|line| {
                                self.rows_between(
                                    top + y + line.top,
                                    top + y + line.top + line.height,
                                )
                            }),
                            hole: pixels.hole().map(|id| (id, pixels.dimensions())),
                        });
                    }
                }
                Shape::Picture { x, y, pixels } => {
                    if let Some(doc) = &at.picture {
                        let (width, height) = pixels.dimensions();
                        let height = height as f32;
                        seen.push(Seen {
                            block,
                            item: None,
                            origin: Origin::picture(at.text, doc.clone()),
                            centre: Some(Centre {
                                band: item.band,
                                x: x + width as f32 / 2.0,
                                y: y + height / 2.0,
                            }),
                            rows: Some(self.rows_between(top + y, top + y + height)),
                            hole: pixels.hole().map(|id| (id, pixels.dimensions())),
                        });
                    }
                }
                _ => {}
            }
        }
        seen
    }

    /// The rows of the page that the pixels from `from` down to `to` are
    /// on, counted from the top of the page: every row they touch, and at
    /// least one.
    fn rows_between(&self, from: f32, to: f32) -> Range<usize> {
        let height = f32::from(self.cell.1);
        let first = (from / height).floor().max(0.0) as usize;
        first..((to / height).ceil().max(0.0) as usize).max(first + 1)
    }

    /// The tops of the page's bands, from the top of the page, in order.
    pub fn band_tops(&self) -> Vec<f32> {
        self.page.bands.iter().map(|band| band.top).collect()
    }

    /// The rows of the page that the bytes `bytes` of the text `item` are on,
    /// and for a text in a band, the pixels of the band's content across
    /// them; none when they are on no line.
    pub fn rows_of(&self, item: ItemRef, bytes: Range<usize>) -> Option<Rows> {
        let (laid, top) = match item.band {
            None => (self.page.items.get(item.index)?, 0.0),
            Some(band) => {
                let band = self.page.bands.get(band)?;
                (band.items.get(item.index)?, band.top)
            }
        };
        let Shape::Text {
            paragraph, x, y, ..
        } = &laid.shape
        else {
            return None;
        };
        let (mut from, mut to) = (f32::MAX, f32::MIN);
        let mut across: Option<Range<u32>> = None;
        let mut boxes = paragraph.boxes(bytes.clone());
        if boxes.is_empty() {
            // Bytes that end inside a character that is several, such as a
            // heart and its variation selector, cover none of it: use the
            // whole of what they are in.
            if let Some(whole) = whole_graphemes(paragraph.text(), &bytes) {
                boxes = paragraph.boxes(whole);
            }
        }
        for part in boxes {
            let Some(line) = paragraph.line(part.line) else {
                continue;
            };
            from = from.min(top + y + line.top);
            to = to.max(top + y + line.top + line.height);
            let (left, right) = (x + part.left, x + part.right);
            across.get_or_insert(left.floor().max(0.0) as u32..right.ceil().max(0.0) as u32);
        }
        if from > to {
            return None;
        }
        // Every row the text touches, which is where a search takes the
        // reader to.
        Some(Rows {
            rows: self.rows_between(from, to),
            band: item.band.zip(across),
        })
    }

    /// Where the text of `item` that `bytes` are goes on each row of the
    /// page: for each row that a line of it touches, in the order of the
    /// rows, the first byte of `bytes` that the first of those lines shows.
    /// A row that holds the end of one line and the start of the next is
    /// of the first.
    pub fn row_starts(&self, item: ItemRef, bytes: &Range<usize>) -> Vec<(usize, usize)> {
        let (laid, top) = match item.band {
            None => (self.page.items.get(item.index), 0.0),
            Some(band) => match self.page.bands.get(band) {
                Some(band) => (band.items.get(item.index), band.top),
                None => (None, 0.0),
            },
        };
        let Some(Shape::Text { paragraph, y, .. }) = laid.map(|laid| &laid.shape) else {
            return Vec::new();
        };
        let mut starts: Vec<(usize, usize)> = Vec::new();
        for (line, text) in paragraph.lines_in(bytes) {
            let (from, to) = (text.start.max(bytes.start), text.end.min(bytes.end));
            let Some(laid) = paragraph.line(line).filter(|_| from < to) else {
                continue;
            };
            let pixels = top + y + laid.top;
            for row in self.rows_between(pixels, pixels + laid.height) {
                if starts.last().is_none_or(|(last, _)| *last < row) {
                    starts.push((row, from));
                }
            }
        }
        starts
    }

    /// The centre of the first box of the bytes `bytes` of the text `item`.
    pub fn centre_of(&self, item: ItemRef, bytes: Range<usize>) -> Option<Centre> {
        let laid = match item.band {
            None => self.page.items.get(item.index)?,
            Some(band) => self.page.bands.get(band)?.items.get(item.index)?,
        };
        let Shape::Text {
            paragraph, x, y, ..
        } = &laid.shape
        else {
            return None;
        };
        let part = paragraph.boxes(bytes).into_iter().next()?;
        let line = paragraph.line(part.line)?;
        Some(Centre {
            band: item.band,
            x: x + (part.left + part.right) / 2.0,
            y: y + line.top + line.height / 2.0,
        })
    }

    /// The number of the page's link that covers `centre`, if one does.
    pub fn link_at(&self, centre: &Centre) -> Option<usize> {
        let covers = |piece: &crate::typeset::reading::page::Piece| {
            (piece.x - 0.5..=piece.x + piece.width + 0.5).contains(&centre.x)
                && (piece.y..piece.y + piece.height).contains(&centre.y)
        };
        (self.page.links().iter())
            .position(|link| link.band == centre.band && link.pieces.iter().any(covers))
    }

    /// What is at the cell `column` of row `row` of the page, in the link
    /// whose pieces cover the larger part of the cell, the leftmost when
    /// they cover as much: a link narrower than a cell is there wherever
    /// in the cell it is. For a cell in a band, `offset` is how far the
    /// band is scrolled. The place is the middle of what the link covers
    /// of the cell. `None` when no link is over the cell.
    pub fn link_in_cell(&self, column: u16, row: u16, offset: u32) -> Option<Found> {
        let (width, height) = (f32::from(self.cell.0), f32::from(self.cell.1));
        let (left, top) = (f32::from(column) * width, f32::from(row) * height);
        // A cell is in a band by its middle, as `found_at` has it.
        let (middle_x, middle_y) = (left + width / 2.0, top + height / 2.0);
        let in_band = (self.page.bands.iter().enumerate()).find(|(_, band)| {
            (band.top..band.top + band.height).contains(&middle_y)
                && (band.x..band.x + band.visible).contains(&middle_x)
        });
        // The cell in the pixels the pieces are in.
        let (number, items, (left, top)) = match in_band {
            Some((number, band)) => (
                Some(number),
                &band.items,
                (left - band.x + offset as f32, top - band.top),
            ),
            None => (None, &self.page.items, (left, top)),
        };
        let (right, bottom) = (left + width, top + height);
        let mut best: Option<(f32, f32, (f32, f32))> = None;
        for link in self.page.links().iter().filter(|link| link.band == number) {
            let mut cover = 0.0;
            let mut leftmost = f32::MAX;
            let mut biggest = (0.0, (0.0, 0.0));
            for piece in &link.pieces {
                let (from_x, to_x) = (piece.x.max(left), (piece.x + piece.width).min(right));
                let (from_y, to_y) = (piece.y.max(top), (piece.y + piece.height).min(bottom));
                if from_x >= to_x || from_y >= to_y {
                    continue;
                }
                let area = (to_x - from_x) * (to_y - from_y);
                cover += area;
                leftmost = leftmost.min(piece.x);
                if area > biggest.0 {
                    biggest = (area, ((from_x + to_x) / 2.0, (from_y + to_y) / 2.0));
                }
            }
            let better = best
                .is_none_or(|(most, first, _)| cover > most || (cover == most && leftmost < first));
            if cover > 0.0 && better {
                best = Some((cover, leftmost, biggest.1));
            }
        }
        let (_, _, (x, y)) = best?;
        self.found_in(items, x, y)
    }

    /// What is at the point (`x`, `y`) of the page, in pixels from its top
    /// left. For a point in a band, `offset` is how far the band is scrolled.
    pub fn found_at(&self, x: f32, y: f32, offset: u32) -> Option<Found> {
        let in_band = (self.page.bands.iter()).find(|band| {
            (band.top..band.top + band.height).contains(&y)
                && (band.x..band.x + band.visible).contains(&x)
        });
        let (items, x, y) = match in_band {
            Some(band) => (&band.items, x - band.x + offset as f32, y - band.top),
            None => (&self.page.items, x, y),
        };
        self.found_in(items, x, y)
    }

    /// What is at the point (`x`, `y`) among `items`, in the pixels of the
    /// page or, for a band's items, of the band's content.
    fn found_in(&self, items: &[Item], x: f32, y: f32) -> Option<Found> {
        items.iter().find_map(|laid| {
            let at = laid.at.as_ref()?;
            match &laid.shape {
                Shape::Text {
                    paragraph,
                    x: left,
                    y: top,
                    ..
                } => {
                    let (x, y) = (x - left, y - top);
                    if x < 0.0 || y < 0.0 || y >= paragraph.height() {
                        return None;
                    }
                    let block = self.block_of(at);
                    if let Some(number) = paragraph.picture_at(x, y) {
                        let origin = paragraph.picture_from(number)?;
                        return Some(Found {
                            block,
                            text: Some((origin.text, origin.doc.start)),
                        });
                    }
                    let byte = u32::try_from(paragraph.index_at(x, y)?).ok()?;
                    let origin =
                        (paragraph.from().iter()).find(|origin| origin.shown.contains(&byte))?;
                    Some(Found {
                        block,
                        text: Some((origin.text, origin.at(byte)?)),
                    })
                }
                Shape::Picture {
                    x: left,
                    y: top,
                    pixels,
                } if (*left..*left + pixels.width() as f32).contains(&x)
                    && (*top..*top + pixels.height() as f32).contains(&y) =>
                {
                    Some(Found {
                        block: self.block_of(at),
                        text: Some((at.text, at.picture.as_ref()?.start)),
                    })
                }
                _ => None,
            }
        })
    }
}

/// The rows whose middles the pixels `pixels` of a page cover, as the first
/// and the end of the rows, when the rows are `height` pixels high: none
/// when `pixels` are between two middles.
fn covered_rows(pixels: &Range<f32>, height: f32) -> (usize, usize) {
    let row = |at: f32| (at / height - 0.5).ceil().max(0.0) as usize;
    (row(pixels.start), row(pixels.end))
}

/// The pieces of a page's links by the rows they may be over, so that a row
/// finds its links without going through all of them: one list of rows for
/// the page's own links and one for each band's. A row's entry holds, in the
/// order of the links and of their pieces, the link and piece numbers of
/// every piece whose line may have the row's middle in it: a piece is put
/// in the rows round the ones its pixels cover, as `row_blocks` does, and
/// `Parts::row_links` makes the exact test.
pub(super) struct LinkRows(Vec<Vec<Vec<(usize, usize)>>>);

impl LinkRows {
    pub(super) fn new(page: &Page, rows: u16, cell_height: f32) -> Self {
        let mut slots: Vec<Vec<Vec<(usize, usize)>>> = vec![Vec::new(); page.bands.len() + 1];
        for (number, link) in page.links().iter().enumerate() {
            let Some(slot) = slots.get_mut(link.band.map_or(0, |band| band + 1)) else {
                continue;
            };
            for (at, piece) in link.pieces.iter().enumerate() {
                let (first, end) = covered_rows(&(piece.y..piece.y + piece.height), cell_height);
                let to = end.saturating_add(1).min(usize::from(rows));
                if slot.len() < to {
                    slot.resize_with(to, Vec::new);
                }
                for bucket in slot.iter_mut().take(to).skip(first.saturating_sub(1)) {
                    bucket.push((number, at));
                }
            }
        }
        Self(slots)
    }
}

impl Parts {
    /// The links' pieces that may be over row `row` of the page, or of band
    /// `band`'s rows: the number of the link and of its piece.
    pub(super) fn link_candidates(&self, band: Option<usize>, row: u16) -> &[(usize, usize)] {
        let slot = self.link_rows.0.get(band.map_or(0, |band| band + 1));
        (slot.and_then(|rows| rows.get(usize::from(row)))).map_or(&[], Vec::as_slice)
    }
}

/// The block each row of the page `parts` belongs to, by its index among
/// the page's blocks: a row belongs to the block its middle is in (the
/// first in order); the rows between blocks belong to none. A block with
/// no row's middle in it, a line of small text between two middles, is on
/// the row its own middle is in; a block with no height, such as empty
/// front matter or an empty block quote, is on none. Linear in rows and blocks.
pub(super) fn row_blocks(parts: &Parts, ids: &[Ids], rows: u16) -> Vec<Option<usize>> {
    let cell_height = f32::from(parts.cell.1);
    let middle = |row: u16| {
        let (top, bottom) = (
            f32::from(row) * cell_height,
            f32::from(row + 1) * cell_height,
        );
        (top + bottom) / 2.0
    };
    let mut found: Vec<Option<usize>> = vec![None; usize::from(rows)];
    let mut own: Vec<Option<usize>> = vec![None; usize::from(rows)];
    // Only blocks that have ids, as the scan's zip with them had it.
    for (index, placed) in parts.page.blocks.iter().take(ids.len()).enumerate() {
        let (first, end) = covered_rows(&placed.rows, cell_height);
        // Exact test on the rows around the estimate, as the scan had it.
        let from = first.saturating_sub(1);
        let to = end.saturating_add(1).min(usize::from(rows));
        for (row, slot) in found.iter_mut().enumerate().take(to).skip(from) {
            if slot.is_none() && placed.rows.contains(&middle(row as u16)) {
                *slot = Some(index);
            }
        }
        // A block that takes no room, such as empty front matter, is on no
        // row: nothing of it is drawn there, and the row is another's.
        if end <= first && placed.rows.start < placed.rows.end {
            let row = ((placed.rows.start + placed.rows.end) / 2.0 / cell_height).floor() as u16;
            if let Some(slot) = own.get_mut(usize::from(row)) {
                slot.get_or_insert(index);
            }
        }
    }
    found
        .iter()
        .zip(own)
        .map(|(by_middle, by_own)| by_middle.or(by_own))
        .collect()
}

/// What a row of the page shows of a picture: its placement, the row in that
/// picture, and where the row is, as a band's number and offset if it is in
/// a band, and its row of the page.
pub(super) struct Shown {
    pub placement: Rc<Placement>,
    pub row: u16,
    pub at: (Option<(u32, u32)>, u16),
}

/// The line that shows row `row` of the page `parts`, with `lead` before
/// it. `ids` are the blocks of the page, in its order.
pub(super) fn line(
    parts: &Rc<Parts>,
    ids: &[Ids],
    row: u16,
    row_blocks: &[Option<usize>],
    lead: &Span,
    Shown {
        placement,
        row: image_row,
        at,
    }: Shown,
) -> Line {
    let cell_height = f32::from(parts.cell.1);
    let (top, bottom) = (
        f32::from(row) * cell_height,
        f32::from(row + 1) * cell_height,
    );
    let middle = (top + bottom) / 2.0;
    let block = row_blocks[usize::from(row)].map(|index| (&parts.page.blocks[index], &ids[index]));
    let address = Rc::as_ptr(parts) as usize;
    let placement_columns = placement.size.columns;
    Line {
        lead: vec![lead.clone()],
        image: Some(Box::new(ImageRow {
            placement,
            row: image_row,
            background: None,
            columns: 0..placement_columns,
            text: None,
            origin: None,
            page: Some(Rc::clone(parts)),
            overlay: None,
            links: parts.row_links(address, at.0, at.1),
        })),
        block: block.map(|(_, ids)| ids.id),
        inner: block.map(|(placed, ids)| ids.innermost(&placed.inner, middle)),
        ..Line::blank()
    }
}

/// The range of whole characters (graphemes) that covers `bytes` of `text`;
/// none when `bytes` ends past the text.
fn whole_graphemes(text: &str, bytes: &Range<usize>) -> Option<Range<usize>> {
    // The boundaries are looked for from the range, not from the start of
    // the text: a range at a cut of a text of 100 000 lines is not a reason
    // to walk the lines before it.
    let len = text.len();
    if bytes.end > len {
        return None;
    }
    let start = text.floor_char_boundary(bytes.start.min(len));
    let end = text.ceil_char_boundary(bytes.end.min(len));
    let mut at = GraphemeCursor::new(start, len, true);
    let from = match at.is_boundary(text, 0).ok()? {
        true => start,
        false => at.prev_boundary(text, 0).ok()??,
    };
    let mut at = GraphemeCursor::new(end, len, true);
    let to = match at.is_boundary(text, 0).ok()? {
        true => end,
        false => at.next_boundary(text, 0).ok()?.unwrap_or(len),
    };
    Some(from..to)
}

#[cfg(test)]
mod tests;
