//! Which block and which text a row of a reading page shows, and what of the
//! document each text on the page shows.
//!
//! A page is one picture cut into rows, and each row is a line of the cell
//! layout. This file answers what the rest of the engine does not: of
//! which block of the document a row is a part, and which text of which
//! block each text on the page is. The blocks of a page are numbered in the
//! table of the layout (`Blocks`) before the page is laid out, and the page
//! knows its blocks by their paths from the block of the page they are in.

use std::ops::Range;
use std::rc::Rc;

use unicode_segmentation::UnicodeSegmentation;

use crate::typeset::block::BlockId;
use crate::typeset::images::size::Placement;
use crate::typeset::model::Block;
use crate::typeset::reading::engine::Parts;
use crate::typeset::reading::page::{Item, ItemRef, Placed, Shape};
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
            match &laid.shape {
                Shape::Text { paragraph, .. } => {
                    let texts = paragraph.from().iter().map(|origin| (Some(item), origin));
                    let pictures = (0..paragraph.pictures.len())
                        .filter_map(|number| paragraph.picture_from(number))
                        .map(|origin| (None, origin));
                    for (item, origin) in texts.chain(pictures) {
                        let origin = origin.clone();
                        seen.push(Seen {
                            block,
                            item,
                            origin,
                        });
                    }
                }
                Shape::Picture { .. } => {
                    if let Some(doc) = &at.picture {
                        seen.push(Seen {
                            block,
                            item: None,
                            origin: Origin::picture(at.text, doc.clone()),
                        });
                    }
                }
                _ => {}
            }
        }
        seen
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
        let lines = paragraph.lines();
        let height = f32::from(self.cell.1);
        let (mut from, mut to) = (f32::MAX, f32::MIN);
        let mut across: Option<Range<u32>> = None;
        let mut boxes = paragraph.boxes(bytes.clone());
        if boxes.is_empty() {
            // Bytes that end inside a character that is several, such as a
            // heart and its variation selector, cover none of it: use the
            // whole of what they are in.
            let text = paragraph.text();
            let starts: Vec<usize> = (text.grapheme_indices(true).map(|(at, _)| at))
                .chain([text.len()])
                .collect();
            let from = starts.iter().rev().find(|&&at| at <= bytes.start);
            let to = starts.iter().find(|&&at| at >= bytes.end);
            if let (Some(&from), Some(&to)) = (from, to) {
                boxes = paragraph.boxes(from..to);
            }
        }
        for part in boxes {
            let Some(line) = lines.get(part.line) else {
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
        let first = (from / height).floor().max(0.0) as usize;
        let rows = first..((to / height).ceil().max(0.0) as usize).max(first + 1);
        Some(Rows {
            rows,
            band: item.band.zip(across),
        })
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
    // A row belongs to the block its middle is in; the rows between
    // blocks belong to none. A block with no row's middle in it, a line of
    // small text between two middles, is on the row its own middle is in.
    // Its innermost block is the one inside that block whose pixels its
    // middle is in.
    let by_middle = |placed: &Placed| placed.rows.contains(&middle);
    let by_own_middle = |placed: &Placed| {
        let (first, end) = covered_rows(&placed.rows, cell_height);
        end <= first
            && ((placed.rows.start + placed.rows.end) / 2.0 / cell_height).floor() as u16 == row
    };
    let blocks = || parts.page.blocks.iter().zip(ids);
    let block = (blocks().find(|(placed, _)| by_middle(placed)))
        .or_else(|| blocks().find(|(placed, _)| by_own_middle(placed)));
    let address = Rc::as_ptr(parts) as usize;
    Line {
        lead: vec![lead.clone()],
        image: Some(ImageRow {
            placement,
            row: image_row,
            background: None,
            columns: 0..parts.columns,
            text: None,
            origin: None,
            page: Some(Rc::clone(parts)),
            overlay: None,
            links: parts.row_links(address, at.0, at.1),
        }),
        block: block.map(|(_, ids)| ids.id),
        inner: block.map(|(placed, ids)| ids.innermost(&placed.inner, middle)),
        ..Line::blank()
    }
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use super::*;
    use crate::render::{FrontMatterStyle, Inputs};
    use crate::typeset::caps::Capabilities;
    use crate::typeset::layout::{Content, Stream, content};
    use crate::typeset::model::{Block, Document};
    use crate::typeset::parse::parse;
    use crate::typeset::reading::engine::Reading;
    use crate::typeset::reading::page::{Shape, Wide};
    use crate::typeset::reading::text::Paragraph;
    use crate::typeset::text::DocumentText;

    /// Inputs for reading mode, in a window `width` columns wide, with cells
    /// of 10 × 20 pixels, the font the tests of reading mode use, and code
    /// and tables wider than the area as `wide` says.
    fn inputs(width: u16, wide: Wide, front_matter: FrontMatterStyle) -> Inputs {
        let caps = Capabilities {
            cell_pixels: Some((10, 20)),
            ..Capabilities::kitty(width)
        };
        Inputs {
            reading: Some(Reading {
                pictures: Default::default(),
                fonts: Rc::new(crate::typeset::reading::Fonts::fixture()),
                wide,
            }),
            front_matter,
            caps,
            ..Inputs::new(Capabilities::kitty(width))
        }
    }

    fn lay_out(source: &str, inputs: &Inputs) -> (Document, Stream) {
        let document = parse(source);
        let stream = content(Content::Document(&document), inputs);
        (document, stream)
    }

    /// The pages of `stream`, each once.
    fn pages(stream: &Stream) -> Vec<Rc<Parts>> {
        let mut pages: Vec<Rc<Parts>> = Vec::new();
        for parts in stream
            .lines
            .iter()
            .filter_map(|line| line.image.as_ref()?.page.as_ref())
        {
            if !pages.iter().any(|page| Rc::ptr_eq(page, parts)) {
                pages.push(Rc::clone(parts));
            }
        }
        pages
    }

    /// The block in the table that what `at` says is on the page is in.
    fn id_of(parts: &Parts, at: &ItemAt) -> BlockId {
        parts.block_of(at)
    }

    /// A text on a page: where it is, and its paragraph.
    struct Text<'a> {
        block: BlockId,
        number: u32,
        paragraph: &'a Paragraph,
    }

    /// The texts on `parts`, those on the page and those in its bands.
    fn texts(parts: &Parts) -> Vec<Text<'_>> {
        let items =
            (parts.page.items.iter()).chain(parts.page.bands.iter().flat_map(|band| &band.items));
        let mut found = Vec::new();
        for item in items {
            let (Shape::Text { paragraph, .. }, Some(at)) = (&item.shape, &item.at) else {
                continue;
            };
            found.push(Text {
                block: id_of(parts, at),
                number: at.text,
                paragraph,
            });
        }
        found
    }

    /// The fixtures and examples laid out in reading mode, at two widths,
    /// both ways with code and tables wider than the area, and with front
    /// matter shown in each way.
    fn each_page(mut check: impl FnMut(&str, &Document, &Stream, FrontMatterStyle)) {
        for path in crate::testing::documents() {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let source = std::fs::read_to_string(&path).unwrap();
            for width in [200, 60] {
                for wide in [Wide::Wrap, Wide::Scroll] {
                    for style in [
                        FrontMatterStyle::Table,
                        FrontMatterStyle::Hide,
                        FrontMatterStyle::Code,
                    ] {
                        let (document, stream) = lay_out(&source, &inputs(width, wide, style));
                        check(&format!("{name} at {width}"), &document, &stream, style);
                    }
                }
            }
        }
    }

    #[test]
    fn what_a_text_on_a_page_shows_is_what_the_text_it_names_says() {
        let (mut exact, mut others, mut paragraphs) = (0, 0, 0);
        each_page(|name, document, stream, _| {
            let text = DocumentText::new([document]);
            for parts in pages(stream) {
                for shown in texts(&parts) {
                    paragraphs += 1;
                    let path = stream.blocks.path(shown.block);
                    let pieces = text.texts(0, path);
                    let Some(piece) = pieces.get(shown.number as usize) else {
                        panic!("{name}: {path:?} has no text {}", shown.number);
                    };
                    let on_page = shown.paragraph.text();
                    for origin in shown.paragraph.from() {
                        let at = format!("{name}: {path:?} text {}: {origin:?}", shown.number);
                        assert!(origin.shown.end as usize <= on_page.len(), "{at}");
                        assert!(origin.doc.end as usize <= piece.text.len(), "{at}");
                        let shown =
                            &on_page[origin.shown.start as usize..origin.shown.end as usize];
                        let doc = &piece.text[origin.doc.start as usize..origin.doc.end as usize];
                        if origin.exact {
                            exact += 1;
                            assert_eq!(shown, doc, "{at}");
                        } else {
                            others += 1;
                        }
                    }
                }
            }
        });
        assert!(
            paragraphs > 1000 && exact > 1000,
            "{paragraphs} {exact} {others}"
        );
    }

    /// What a layout may leave out of the text of the document in reading
    /// mode, as the cell layout's list does (`layout/inlines.rs`).
    const LEFT_OUT: &[&str] = &[
        "the text of hidden front matter",
        "the keys and values of front matter shown as code",
    ];

    #[test]
    fn every_byte_of_a_text_on_a_page_is_shown_but_what_the_layout_names_it_leaves_out() {
        let mut left_out = [0usize; 2];
        let mut shown = 0;
        each_page(|name, document, stream, style| {
            let text = DocumentText::new([document]);
            let blocks = document.blocks();
            // For each text of each block: whether each byte is shown, and
            // whether the page shows it.
            let mut seen: Vec<(Vec<usize>, usize, Vec<bool>)> = Vec::new();
            for id in stream.blocks.ids() {
                let path = stream.blocks.path(id).to_vec();
                for (number, piece) in text.texts(0, &path).iter().enumerate() {
                    seen.push((path.clone(), number, vec![false; piece.text.len()]));
                }
            }
            let mut mark = |path: &[usize], number: usize, bytes: Range<u32>| {
                if let Some(entry) =
                    (seen.iter_mut()).find(|(at, n, ..)| at == path && *n == number)
                {
                    entry.2[bytes.start as usize..bytes.end as usize].fill(true);
                }
            };
            for parts in pages(stream) {
                for text in texts(&parts) {
                    let path = stream.blocks.path(text.block);
                    for origin in text.paragraph.from() {
                        mark(path, text.number as usize, origin.doc.clone());
                    }
                    for number in 0..text.paragraph.pictures.len() {
                        if let Some(origin) = text.paragraph.picture_from(number) {
                            mark(path, text.number as usize, origin.doc.clone());
                        }
                    }
                }
                for item in (parts.page.items.iter())
                    .chain(parts.page.bands.iter().flat_map(|band| &band.items))
                {
                    if let Some(at) = &item.at
                        && let Some(doc) = &at.picture
                    {
                        let id = id_of(&parts, at);
                        mark(stream.blocks.path(id), at.text as usize, doc.clone());
                    }
                }
            }
            // What the cell layout shows, in the blocks that reading mode
            // leaves to it.
            for line in &stream.lines {
                let Some(inner) = line.inner else { continue };
                let origins = (line.spans.iter()).flat_map(|span| span.origins.iter().cloned());
                let image = line.image.iter().filter_map(|row| row.origin.clone());
                for origin in origins.chain(image) {
                    mark(stream.blocks.path(inner), origin.text as usize, origin.doc);
                }
            }
            for (path, number, flags) in seen {
                let piece = &text.texts(0, &path)[number];
                let front_matter = blocks
                    .iter()
                    .any(|(at, block)| *at == path && matches!(block, Block::FrontMatter(_)));
                for (at, c) in piece.text.char_indices() {
                    if flags[at..at + c.len_utf8()]
                        .iter()
                        .all(|shown_byte| *shown_byte)
                    {
                        shown += 1;
                        continue;
                    }
                    let why = match (front_matter, style) {
                        (true, FrontMatterStyle::Hide) => 0,
                        (true, FrontMatterStyle::Code) => 1,
                        _ => panic!(
                            "{name}: {path:?} text {number}: {c:?} at {at} of {:?} is shown by nothing",
                            piece.text
                        ),
                    };
                    left_out[why] += 1;
                }
            }
        });
        for (count, what) in left_out.iter().zip(LEFT_OUT) {
            assert!(*count > 0, "nothing is left out as {what}");
        }
        assert!(shown > 20_000, "{shown}");
    }

    /// The paths of the innermost blocks of the rows of `source`, in order,
    /// each once, and the path of each of their blocks.
    fn inner_blocks(source: &str) -> Vec<Vec<usize>> {
        let (_, stream) = lay_out(source, &inputs(80, Wide::Wrap, FrontMatterStyle::Table));
        let mut found: Vec<Vec<usize>> = Vec::new();
        for line in &stream.lines {
            if let Some(inner) = line.inner {
                let path = stream.blocks.path(inner).to_vec();
                if !found.contains(&path) {
                    found.push(path);
                }
            }
        }
        found
    }

    #[test]
    fn a_row_knows_the_innermost_block_it_is_in() {
        // A paragraph in a list, and a rule in the same item, which has no
        // text; the gaps between them are the list's.
        let found = inner_blocks("- a paragraph\n\n  ***\n\n  and another\n");
        assert_eq!(
            found,
            [vec![0, 0, 0], vec![0], vec![0, 0, 1], vec![0, 0, 2]]
        );
        // A table in a quote.
        let found = inner_blocks("> | a | b |\n> |---|---|\n> | 1 | 2 |\n\nafter\n");
        assert_eq!(found, [vec![0, 0], vec![1]]);
        // A heading in an alert, whose title is the alert's own, and a quote
        // in a quote.
        let found = inner_blocks("> [!NOTE]\n> ## A heading\n\n> > deep\n");
        assert_eq!(found, [vec![0], vec![0, 0], vec![1, 0, 0]]);
    }

    #[test]
    fn the_table_has_the_blocks_of_a_page_in_the_documents_order_and_each_row_is_in_its_block() {
        each_page(|name, document, stream, _| {
            let known: Vec<Vec<usize>> = (document.blocks().into_iter())
                .map(|(path, _)| path)
                .collect();
            let table: Vec<&[usize]> = (stream.blocks.ids())
                .map(|id| stream.blocks.path(id))
                .collect();
            assert!(
                table.windows(2).all(|pair| pair[0] < pair[1]),
                "{name}: {table:?}"
            );
            assert!(
                table
                    .iter()
                    .all(|path| known.iter().any(|known| known == path)),
                "{name}"
            );
            for line in &stream.lines {
                match (line.block, line.inner) {
                    (Some(block), Some(inner)) => {
                        let (outer, inner) = (stream.blocks.path(block), stream.blocks.path(inner));
                        assert!(inner.starts_with(outer), "{name}: {outer:?} {inner:?}");
                    }
                    (None, None) => {}
                    other => panic!("{name}: a line with {other:?}"),
                }
            }
        });
    }

    /// The paragraphs of the page of `source`, as the texts of the block
    /// at `path`, by their number: what they say.
    fn said(source: &str, path: &[usize], inputs: &Inputs) -> Vec<(u32, String)> {
        let (_, stream) = lay_out(source, inputs);
        let mut found = Vec::new();
        for parts in pages(&stream) {
            for text in texts(&parts) {
                if stream.blocks.path(text.block) == path {
                    found.push((text.number, text.paragraph.text().to_owned()));
                }
            }
        }
        found.sort();
        found
    }

    fn plain() -> Inputs {
        inputs(80, Wide::Wrap, FrontMatterStyle::Table)
    }

    /// The origins of the paragraph of text `number` of the block at
    /// `path`, as (bytes shown, bytes of the document, exact).
    fn origins_of(
        source: &str,
        path: &[usize],
        inputs: &Inputs,
    ) -> Vec<(Range<u32>, Range<u32>, bool)> {
        let (_, stream) = lay_out(source, inputs);
        let parts = pages(&stream);
        let shown = parts
            .iter()
            .flat_map(|parts| texts(parts))
            .find(|text| stream.blocks.path(text.block) == path);
        let text = shown.expect("a text of the block");
        (text.paragraph.from().iter())
            .map(|origin| (origin.shown.clone(), origin.doc.clone(), origin.exact))
            .collect()
    }

    #[test]
    fn a_picture_in_a_paragraph_stands_in_the_place_of_its_alt_text() {
        let mut inputs = plain();
        let key = inputs.drawn("p.png");
        let image = crate::typeset::images::Source::Raster(::image::RgbaImage::new(20, 20));
        inputs.images.insert(key, Ok(image));
        let (_, stream) = lay_out("before ![the alt](p.png) after\n", &inputs);
        let parts = pages(&stream);
        let found = texts(&parts[0]);
        let [text] = &found[..] else {
            panic!("one text")
        };
        // The picture is a box where the alt text would be.
        assert_eq!(text.paragraph.text(), "before  after");
        let picture = text
            .paragraph
            .picture_from(0)
            .expect("the picture's origin");
        assert!(picture.picture);
        assert_eq!((picture.shown.clone(), picture.doc.clone()), (7..7, 7..14));
        let shown: Vec<_> = (text.paragraph.from().iter())
            .map(|origin| (origin.shown.clone(), origin.doc.clone()))
            .collect();
        assert_eq!(shown, [(0..7, 0..7), (7..13, 14..20)]);
        // Without the picture the alt text is shown, as the text it is.
        let alt = origins_of("before ![the alt](p.png) after\n", &[0], &plain());
        assert_eq!(alt, [(0..20, 0..20, true)]);
    }

    #[test]
    fn a_formula_written_out_shows_all_of_its_source_as_a_whole() {
        let origins = origins_of("a $x^2$ b\n", &[0], &plain());
        assert_eq!(origins.len(), 3, "{origins:?}");
        assert_eq!((origins[0].1.clone(), origins[0].2), (0..2, true));
        assert_eq!((origins[1].1.clone(), origins[1].2), (2..5, false));
        assert_eq!((origins[2].1.clone(), origins[2].2), (5..7, true));
    }

    #[test]
    fn the_cells_of_a_table_and_the_keys_and_values_of_front_matter_are_texts_by_number() {
        let table = said("| a | bb |\n|---|----|\n| c | d |\n", &[0], &plain());
        let words: Vec<(u32, &str)> = table.iter().map(|(n, text)| (*n, text.as_str())).collect();
        assert_eq!(words, [(0, "a"), (1, "bb"), (2, "c"), (3, "d")]);
        let front_matter = said(
            "---\nname: kiri\ntags: [a, b]\n---\n\nbody\n",
            &[0],
            &plain(),
        );
        let words: Vec<(u32, &str)> = front_matter
            .iter()
            .map(|(n, text)| (*n, text.as_str()))
            .collect();
        assert_eq!(words, [(0, "name"), (1, "kiri"), (2, "tags"), (3, "a, b")]);
    }

    #[test]
    fn a_summary_shows_its_text_after_a_triangle_that_is_the_layouts() {
        let source = "<details><summary>The summary</summary>\n\ninside\n\n</details>\n";
        let found = said(source, &[0], &plain());
        assert_eq!(found, [(0, "▼ The summary".to_owned())]);
        let origins = origins_of(source, &[0], &plain());
        assert_eq!(origins, [(4..15, 0..11, true)]);
    }

    #[test]
    fn code_shows_its_lines_and_its_tabs_as_wholes_in_a_band_and_wrapped() {
        let source = "```\nline one\n\tline two\n```\n";
        for wide in [Wide::Wrap, Wide::Scroll] {
            let origins = origins_of(source, &[0], &inputs(80, wide, FrontMatterStyle::Table));
            // Runs of the same bytes that go on from each other are one.
            let mut joined: Vec<(Range<u32>, Range<u32>, bool)> = Vec::new();
            for (shown, doc, exact) in origins {
                match joined.last_mut() {
                    Some((last_shown, last_doc, true))
                        if exact && last_shown.end == shown.start && last_doc.end == doc.start =>
                    {
                        last_shown.end = shown.end;
                        last_doc.end = doc.end;
                    }
                    _ => joined.push((shown, doc, exact)),
                }
            }
            // "line one" and its line break, a tab spread into four spaces,
            // and "line two".
            assert_eq!(
                joined,
                [
                    (0..9, 0..9, true),
                    (9..13, 9..10, false),
                    (13..21, 10..18, true)
                ],
                "{wide:?}"
            );
        }
    }

    /// Inputs with the image `key` loaded, `width` by `height` pixels.
    fn with_image(key: crate::typeset::images::ImageKey, (width, height): (u32, u32)) -> Inputs {
        let mut inputs = plain();
        let key = inputs.drawn(key);
        let image = crate::typeset::images::Source::Raster(::image::RgbaImage::new(width, height));
        inputs.images.insert(key, Ok(image));
        inputs
    }

    /// What the pictures on the page of `source` stand in the place of, as
    /// the block they are in, the text and the bytes.
    fn pictures_of(source: &str, inputs: &Inputs) -> Vec<(Vec<usize>, u32, Range<u32>)> {
        let (_, stream) = lay_out(source, inputs);
        let mut found = Vec::new();
        for parts in pages(&stream) {
            for item in &parts.page.items {
                let Some(at) = &item.at else { continue };
                let Some(doc) = &at.picture else { continue };
                let id = id_of(&parts, at);
                found.push((stream.blocks.path(id).to_vec(), at.text, doc.clone()));
            }
        }
        found
    }

    #[test]
    fn a_lone_image_a_linked_one_a_diagram_and_a_formula_stand_in_the_place_of_their_text() {
        use crate::typeset::images::ImageKey;
        let image = with_image(ImageKey::from("p.png"), (40, 40));
        assert_eq!(
            pictures_of("![the alt](p.png)\n", &image),
            [(vec![0], 0, 0..7)]
        );
        assert_eq!(
            pictures_of("[![the alt](p.png)](https://example.com)\n", &image),
            [(vec![0], 0, 0..7)]
        );
        // In a list item, the path is the item's.
        assert_eq!(
            pictures_of("- one\n\n  ![the alt](p.png)\n", &image),
            [(vec![0, 0, 1], 0, 0..7)]
        );
        let code = "flowchart LR\n  a --> b\n";
        let diagram = with_image(ImageKey::rendered("mermaid", code), (40, 40));
        assert_eq!(
            pictures_of(&format!("```mermaid\n{code}```\n"), &diagram),
            [(vec![0], 0, 0..22)]
        );
        let formula = with_image(ImageKey::rendered("math", "x\n"), (40, 20));
        assert_eq!(pictures_of("$$x$$\n", &formula), [(vec![0], 0, 0..1)]);
    }

    #[test]
    fn a_formula_drawn_in_a_line_stands_in_the_place_of_its_source() {
        use crate::typeset::diagrams::request::INLINE_FORMULA;
        use crate::typeset::images::ImageKey;
        let inputs = with_image(ImageKey::rendered(INLINE_FORMULA, "x^2\n"), (30, 20));
        let (_, stream) = lay_out("a $x^2$ b\n", &inputs);
        let parts = pages(&stream);
        let found = texts(&parts[0]);
        let [text] = &found[..] else {
            panic!("one text")
        };
        let picture = text
            .paragraph
            .picture_from(0)
            .expect("the picture's origin");
        assert!(picture.picture);
        assert_eq!(picture.doc, 2..5);
    }

    #[test]
    fn what_is_under_a_glyph_of_a_text_on_a_page_is_the_place_it_shows() {
        let (mut glyphs, mut wholes) = (0, 0);
        each_page(|name, _, stream, _| {
            for parts in pages(stream) {
                for (item, laid) in parts.all_items() {
                    let (
                        Some(at),
                        Shape::Text {
                            paragraph, x, y, ..
                        },
                    ) = (&laid.at, &laid.shape)
                    else {
                        continue;
                    };
                    let block = id_of(&parts, at);
                    let (band_x, top, visible) = match item.band {
                        None => (0.0, 0.0, f32::MAX),
                        Some(band) => {
                            let band = &parts.page.bands[band];
                            (band.x, band.top, band.visible)
                        }
                    };
                    let lines = paragraph.lines();
                    for origin in paragraph.from() {
                        let text = &paragraph.text()
                            [origin.shown.start as usize..origin.shown.end as usize];
                        for (offset, grapheme) in text.grapheme_indices(true) {
                            // A line break has no glyph to point at.
                            if grapheme.contains(['\n', '\r']) {
                                continue;
                            }
                            let byte = origin.shown.start as usize + offset;
                            let boxes = paragraph.boxes(byte..byte + grapheme.len());
                            let Some(part) = boxes.first() else { continue };
                            let line = &lines[part.line];
                            let middle = (
                                x + (part.left + part.right) / 2.0,
                                y + line.top + line.height / 2.0,
                            );
                            // A band shows what is within its width: scroll it to this.
                            let scrolled = (middle.0 - 5.0).max(0.0).floor();
                            let (px, scrolled) = match item.band {
                                None => (middle.0, 0),
                                Some(_) => (
                                    band_x + (middle.0 - scrolled).min(visible - 1.0),
                                    scrolled as u32,
                                ),
                            };
                            let expected = Found {
                                block,
                                text: Some((origin.text, origin.at(byte as u32).unwrap())),
                            };
                            assert_eq!(
                                parts.found_at(px, top + middle.1, scrolled),
                                Some(expected),
                                "{name}: {grapheme:?} at {byte} of {:?}",
                                paragraph.text()
                            );
                            match origin.exact {
                                true => glyphs += 1,
                                false => wholes += 1,
                            }
                        }
                    }
                }
            }
        });
        assert!(glyphs > 2000 && wholes > 5, "{glyphs} {wholes}");
    }
}
