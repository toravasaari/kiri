//! Where a stretch of the document is shown, and what is at a cell of the
//! screen.
//!
//! [`Map::locate`] says which lines show a range of the document, or why
//! none does. [`Map::hit`] says which place of the document is at a line and
//! a column. Both are answered from what the layout recorded: the origins of
//! the spans of the cell layout and the texts of the pages of reading mode.
//! Columns and pixels stay in here, and what comes out is places and rows.

pub mod bridge;

use std::collections::BTreeMap;
use std::ops::Range;
use std::rc::Rc;

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::typeset::block::{BlockInfo, Blocks};
use crate::typeset::images::ImageKey;
use crate::typeset::position::Pos;
use crate::typeset::reading::engine::{self, Parts};
use crate::typeset::reading::page::ItemRef;
use crate::typeset::style::{Line, Origin, Span};
use crate::typeset::wrap;

/// Where the places of the lines are, found once for one layout.
pub struct Map {
    blocks: Blocks,
    /// What the lines show of the texts of the document, in the order of the
    /// document. Sorted by `lo` and then `hi`, and `hi` never goes down in
    /// that order: no stretch holds a later one that ends before it does.
    /// `locate` and the bridge search it with `partition_point` on `hi`, which
    /// is right only so; an origin that broke it would drop matches without a
    /// word, so `new` checks it.
    texts: Vec<Shown>,
    /// The lines that show no text, each at the place of its block, in the
    /// order of the document.
    points: Vec<(Pos, usize)>,
    /// The first line of each piece.
    starts: BTreeMap<BlockInfo, usize>,
    /// The blocks whose texts no line shows.
    hidden: Vec<BlockInfo>,
    pages: Vec<Page>,
}

/// A stretch of a text that something shows.
struct Shown {
    lo: Pos,
    hi: Pos,
    origin: Origin,
    at: At,
}

/// Where a stretch of a text is shown.
enum At {
    /// In a span of this line of the cell layout, which starts `at` bytes
    /// into the line's text.
    Line { line: usize, at: usize },
    /// On the image row that is this line of the cell layout.
    Row(usize),
    /// In this text of this page of reading mode.
    Item { page: usize, item: ItemRef },
    /// In a picture, which has no rows to tell.
    Picture,
}

/// A page of reading mode among the lines.
struct Page {
    rows: Range<usize>,
    parts: Rc<Parts>,
    /// The number of each of its bands among the bands of the piece it is
    /// in.
    nth: Vec<usize>,
}

/// A part of the answer to [`Map::locate`]: the lines of one block that the
/// range is shown on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Part {
    /// The top-level block or `<details>` the lines are in.
    pub piece: BlockInfo,
    /// The lines, counted from the first line of the piece (see
    /// [`Map::start`]).
    pub rows: Range<usize>,
    /// Where it is in a band of reading mode, if it is in one.
    pub band: Option<Across>,
}

/// A band that shows part of a range, and the pixels of the band's content it
/// covers from left to right.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Across {
    /// The number of the band among the bands of its piece, as the state
    /// engine tells them apart.
    pub band: usize,
    pub pixels: Range<u32>,
}

/// Why a range is not shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hidden {
    /// It is in front matter that is not shown, or is shown as its source.
    FrontMatter,
    /// It is the text behind a picture: the alt text of an image or the
    /// source of a formula or a diagram that is drawn.
    Picture,
    /// Reserved: a cell that is cut short does not show it. Neither layout
    /// cuts cells.
    CutShort,
    /// Reserved: the layout has not laid the range out yet.
    NotLaidOut,
}

impl Map {
    /// The map of `lines`, which `blocks` is the table of.
    pub fn new(lines: &[Line], blocks: &Blocks) -> Self {
        let mut map = Self {
            blocks: blocks.clone(),
            texts: Vec::new(),
            points: Vec::new(),
            starts: BTreeMap::new(),
            hidden: blocks.hidden().cloned().collect(),
            pages: Vec::new(),
        };
        for (index, line) in lines.iter().enumerate() {
            if let Some(piece) = line.block {
                map.starts.entry(blocks.get(piece).clone()).or_insert(index);
            }
        }
        map.add_pages(lines);
        for (index, line) in lines.iter().enumerate() {
            let Some(inner) = line.inner else { continue };
            if line
                .image
                .as_ref()
                .is_some_and(|image| image.page.is_some())
            {
                continue;
            }
            let info = blocks.get(inner);
            let mut shows = false;
            let mut at = 0;
            for span in &line.spans {
                for origin in &span.origins {
                    map.add(info, origin, At::Line { line: index, at });
                    shows = shows || !origin.picture;
                }
                at += span.text.len();
            }
            let picture = line.image.as_ref().and_then(|image| image.origin.as_ref());
            if let Some(origin) = picture {
                map.add(info, origin, At::Row(index));
                shows = shows || !origin.picture;
            }
            if !shows {
                map.points
                    .push((Pos::block(info.file(), info.path()), index));
            }
        }
        map.texts
            .sort_by(|a, b| a.lo.cmp(&b.lo).then(a.hi.cmp(&b.hi)));
        debug_assert!(
            map.texts.windows(2).all(|pair| pair[0].hi <= pair[1].hi),
            "a stretch of a text holds a later one that ends before it"
        );
        map.points.sort_by(|a, b| a.0.cmp(&b.0));
        map
    }

    fn add(&mut self, info: &BlockInfo, origin: &Origin, at: At) {
        let text = origin.text as usize;
        let at_byte = |byte: u32| Pos::text(info.file(), info.path(), text, byte as usize);
        self.texts.push(Shown {
            lo: at_byte(origin.doc.start),
            hi: at_byte(origin.doc.end),
            origin: origin.clone(),
            at,
        });
    }

    /// The pages of reading mode among `lines`: what they show of texts, and
    /// the rows that show no text.
    fn add_pages(&mut self, lines: &[Line]) {
        let mut counted: BTreeMap<BlockInfo, usize> = BTreeMap::new();
        for (number, page) in engine::pages(lines).into_iter().enumerate() {
            let parts = page.parts;
            let height = usize::from(parts.cells().1.1).max(1);
            let nth = (parts.band_tops().into_iter())
                .map(|top| {
                    let line = page.lines.start + (top.max(0.0) as usize) / height;
                    let piece = lines.get(line).and_then(|line| line.block);
                    let piece = piece.map(|piece| self.blocks.get(piece).clone());
                    let count = piece.map(|piece| counted.entry(piece).or_insert(0));
                    count.map_or(0, |count| {
                        *count += 1;
                        *count - 1
                    })
                })
                .collect();
            let mut with_text = Vec::new();
            for seen in parts.seen() {
                let info = self.blocks.get(seen.block).clone();
                let at = match seen.item.filter(|_| !seen.origin.picture) {
                    Some(item) => At::Item { page: number, item },
                    None => At::Picture,
                };
                self.add(&info, &seen.origin, at);
                if !seen.origin.picture {
                    with_text.push(info);
                }
            }
            for index in page.lines.clone() {
                let Some(inner) = lines[index].inner else {
                    continue;
                };
                let info = self.blocks.get(inner);
                if !with_text.contains(info) {
                    let at = Pos::block(info.file(), info.path());
                    self.points.push((at, index));
                }
            }
            self.pages.push(Page {
                rows: page.lines,
                parts,
                nth,
            });
        }
    }

    /// The lines that show `range`, by piece, or why it is not shown. A range
    /// that is partly shown gives the parts that are; a range that has no
    /// lines and is not hidden, such as an empty code block, gives none.
    pub fn locate(&self, lines: &[Line], range: &Range<Pos>) -> Result<Vec<Part>, Hidden> {
        if range.start >= range.end {
            return Ok(Vec::new());
        }
        let mut hits: Vec<Hit> = Vec::new();
        let mut pictures = false;
        let first = self.texts.partition_point(|shown| shown.hi <= range.start);
        for shown in &self.texts[first..] {
            if shown.lo >= range.end {
                break;
            }
            if shown.hi <= range.start {
                continue;
            }
            match &shown.at {
                At::Picture => pictures = true,
                _ if shown.origin.picture => pictures = true,
                At::Line { line, .. } | At::Row(line) => hits.push(Hit::line(*line)),
                At::Item { page, item } => {
                    let bytes = Self::shown_bytes(shown, range);
                    let page = &self.pages[*page];
                    let Some(found) = page.parts.rows_of(*item, bytes) else {
                        continue;
                    };
                    let rows = page.rows.start + found.rows.start
                        ..(page.rows.start + found.rows.end).min(page.rows.end);
                    let band = found.band.map(|(band, pixels)| (page.nth[band], pixels));
                    hits.push(Hit { rows, band });
                }
            }
        }
        let first = self.points.partition_point(|(at, _)| *at < range.start);
        for (at, line) in &self.points[first..] {
            if *at >= range.end {
                break;
            }
            hits.push(Hit::line(*line));
        }
        if hits.is_empty() {
            return match (pictures, self.is_hidden(&range.start)) {
                (true, _) => Err(Hidden::Picture),
                (false, true) => Err(Hidden::FrontMatter),
                (false, false) => Ok(Vec::new()),
            };
        }
        Ok(self.parts(lines, hits))
    }

    /// The bytes of the text of `shown` that the part of `range` in it is
    /// shown as.
    fn shown_bytes(shown: &Shown, range: &Range<Pos>) -> Range<usize> {
        let origin = &shown.origin;
        let offset = |at: &Pos, edge: &Pos, default: u32| match (&at.within, at > edge) {
            (crate::typeset::position::Within::Text { offset, .. }, true)
                if (at.file, &at.path) == (shown.lo.file, &shown.lo.path) =>
            {
                u32::try_from(*offset)
                    .unwrap_or(default)
                    .clamp(origin.doc.start, origin.doc.end)
            }
            _ => default,
        };
        let from = offset(&range.start, &shown.lo, origin.doc.start);
        let to = match range.end < shown.hi {
            true => offset(&range.end, &shown.lo, origin.doc.end),
            false => origin.doc.end,
        };
        let (from, to) = match origin.exact {
            true => (
                origin.shown.start + (from - origin.doc.start),
                origin.shown.start + (to.max(from) - origin.doc.start),
            ),
            false => (origin.shown.start, origin.shown.end),
        };
        from as usize..to as usize
    }

    /// Whether the texts at `at` are in a block whose texts no line shows.
    fn is_hidden(&self, at: &Pos) -> bool {
        (self.hidden.iter()).any(|block| (block.file(), block.path()) == (at.file, &at.path[..]))
    }

    /// `hits`, which are on lines, as the parts of the pieces they are in.
    fn parts(&self, lines: &[Line], hits: Vec<Hit>) -> Vec<Part> {
        let mut parts: Vec<(Range<usize>, Part)> = Vec::new();
        for mut hit in hits {
            // The rows of a tall line may start in the space above its block,
            // which is no block's.
            let in_block = |row: &usize| lines.get(*row).is_some_and(|line| line.block.is_some());
            let Some(first) = hit.rows.clone().find(in_block) else {
                continue;
            };
            hit.rows.start = first;
            let piece = lines[first].block.expect("a row in a block");
            let piece = self.blocks.get(piece).clone();
            let band = hit.band.as_ref().map(|(band, _)| *band);
            let known = parts.iter_mut().find(|(_, part)| {
                part.piece == piece && part.band.as_ref().map(|a| a.band) == band
            });
            match known {
                Some((rows, part)) => {
                    rows.start = rows.start.min(hit.rows.start);
                    rows.end = rows.end.max(hit.rows.end);
                    if let (Some(across), Some((_, pixels))) = (&mut part.band, &hit.band) {
                        across.pixels.start = across.pixels.start.min(pixels.start);
                        across.pixels.end = across.pixels.end.max(pixels.end);
                    }
                }
                None => {
                    let band = hit.band.map(|(band, pixels)| Across { band, pixels });
                    let part = Part {
                        piece,
                        rows: 0..0,
                        band,
                    };
                    parts.push((hit.rows, part));
                }
            }
        }
        parts.sort_by_key(|(rows, _)| rows.start);
        (parts.into_iter())
            .map(|(rows, mut part)| {
                let start = self.starts.get(&part.piece).copied().unwrap_or(rows.start);
                part.rows = rows.start - start..rows.end - start;
                part
            })
            .collect()
    }

    /// The place at column `column` of line `line`, if it is a place of the
    /// document and not what the layout made up.
    pub fn hit(&self, lines: &[Line], line: usize, column: usize) -> Option<Pos> {
        let info = self.blocks.get(lines.get(line)?.inner?);
        // The lead is what the layout made up, on every row.
        column.checked_sub(wrap::width(&lines.get(line)?.lead))?;
        // Front matter shown as its source has no text of its own on its
        // rows: a cell there is at the block.
        self.text_at(lines, line, column).or_else(|| {
            let hidden = self.hidden.contains(info);
            hidden.then(|| Pos::block(info.file(), info.path()))
        })
    }

    /// `hit`, for a place in a text or an image, or the block of a row of
    /// an image that shows no text.
    fn text_at(&self, lines: &[Line], line: usize, column: usize) -> Option<Pos> {
        let laid = lines.get(line)?;
        let info = self.blocks.get(laid.inner?);
        let place = |text: u32, byte: u32| {
            Pos::text(info.file(), info.path(), text as usize, byte as usize)
        };
        let columns = column.checked_sub(wrap::width(&laid.lead))?;
        if let Some(image) = &laid.image {
            if let Some(parts) = &image.page {
                let page = self.pages.iter().find(|page| page.rows.contains(&line))?;
                let (width, height) = parts.cells().1;
                let x = (columns as f32 + 0.5) * f32::from(width);
                let y = ((line - page.rows.start) as f32 + 0.5) * f32::from(height);
                let offset = match image.placement.key {
                    ImageKey::Band { offset, .. } => offset,
                    _ => 0,
                };
                let found = parts.found_at(x, y, offset)?;
                let info = self.blocks.get(found.block);
                return Some(match found.text {
                    Some((text, byte)) => {
                        Pos::text(info.file(), info.path(), text as usize, byte as usize)
                    }
                    None => Pos::block(info.file(), info.path()),
                });
            }
            return Some(match &image.origin {
                Some(origin) => place(origin.text, origin.doc.start),
                None => Pos::block(info.file(), info.path()),
            });
        }
        let mut left = columns;
        for span in &laid.spans {
            let width = wrap::width(std::slice::from_ref(span));
            if left >= width {
                left -= width;
                continue;
            }
            let byte = byte_at(span, left)?;
            let origin = span.origins.of(byte)?;
            return Some(place(origin.text, origin.at(u32::try_from(byte).ok()?)?));
        }
        None
    }

    /// The first line of `piece`.
    pub fn start(&self, piece: &BlockInfo) -> Option<usize> {
        self.starts.get(piece).copied()
    }
}

/// The lines a range is on, and where in a band.
struct Hit {
    rows: Range<usize>,
    band: Option<(usize, Range<u32>)>,
}

impl Hit {
    fn line(line: usize) -> Self {
        Self {
            rows: line..line + 1,
            band: None,
        }
    }
}

/// The byte of the text of `span` that is drawn `cells` cells into it: where
/// the grapheme there starts, or where the group of text packed at a fraction
/// starts, whichever the span draws.
fn byte_at(span: &Span, cells: usize) -> Option<usize> {
    if span.embed.is_some() {
        return Some(0);
    }
    let size = span.style.size;
    let scale = usize::from(size.scale());
    let mut left = cells;
    if size.fraction().is_some() {
        for group in size.groups(&span.text) {
            let width = scale * usize::from(group.cells);
            if left < width {
                return Some(group.at);
            }
            left -= width;
        }
        return None;
    }
    for (at, grapheme) in span.text.grapheme_indices(true) {
        let width = scale * grapheme.width();
        if left < width {
            return Some(at);
        }
        left -= width;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::{FrontMatterStyle, Inputs};
    use crate::typeset::caps::Capabilities;
    use crate::typeset::layout::{Content, Stream, content};
    use crate::typeset::model::Document;
    use crate::typeset::parse::parse;
    use crate::typeset::position::Within;
    use crate::typeset::reading::engine::Reading;
    use crate::typeset::reading::page::Wide;
    use crate::typeset::style::{Origin, Span};
    use crate::typeset::text::DocumentText;
    use std::rc::Rc;

    /// Which engine and how wide a window.
    #[derive(Debug, Clone, Copy)]
    enum Engine {
        Text(u16),
        Reading(u16, Wide),
    }

    fn inputs(engine: Engine, front_matter: FrontMatterStyle) -> Inputs {
        match engine {
            Engine::Text(width) => Inputs {
                front_matter,
                ..Inputs::new(Capabilities::kitty(width))
            },
            Engine::Reading(width, wide) => Inputs {
                reading: Some(Reading {
                    pictures: Default::default(),
                    fonts: Rc::new(crate::typeset::reading::Fonts::fixture()),
                    wide,
                }),
                front_matter,
                ..Inputs::new(Capabilities {
                    cell_pixels: Some((10, 20)),
                    ..Capabilities::kitty(width)
                })
            },
        }
    }

    const ENGINES: [Engine; 5] = [
        Engine::Text(200),
        Engine::Text(40),
        Engine::Reading(200, Wide::Scroll),
        Engine::Reading(60, Wide::Scroll),
        Engine::Reading(60, Wide::Wrap),
    ];

    const STYLES: [FrontMatterStyle; 3] = [
        FrontMatterStyle::Table,
        FrontMatterStyle::Hide,
        FrontMatterStyle::Code,
    ];

    /// Every fixture and example in every engine and with each way of
    /// showing front matter: the name, the document, its layout and map.
    fn each_layout(mut check: impl FnMut(&str, &Document, &Stream, &Map, Engine)) {
        for path in crate::testing::documents() {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let source = std::fs::read_to_string(&path).unwrap();
            let document = parse(&source);
            for engine in ENGINES {
                for style in STYLES {
                    let stream = content(Content::Document(&document), &inputs(engine, style));
                    let map = Map::new(&stream.lines, &stream.blocks);
                    check(
                        &format!("{name} {engine:?} {style:?}"),
                        &document,
                        &stream,
                        &map,
                        engine,
                    );
                }
            }
        }
    }

    /// The first line of each piece is where [`Map::start`] says.
    #[test]
    fn a_piece_starts_at_its_first_line() {
        each_layout(|name, _, stream, map, _| {
            let places = stream.blocks.places(&stream.lines);
            let mut checked = 0;
            for (line, place) in places.iter().enumerate() {
                let Some(piece) = place else { continue };
                let first = places
                    .iter()
                    .position(|other| other.as_ref() == Some(piece));
                assert_eq!(map.start(piece), first, "{name}: line {line}");
                checked += 1;
            }
            assert!(checked > 0 || stream.lines.is_empty(), "{name}");
        });
    }

    #[test]
    fn a_range_of_a_text_is_shown_on_the_lines_that_show_it() {
        let document = parse("First paragraph.\n\nSecond paragraph.\n");
        let text = DocumentText::new([&document]);
        for engine in [Engine::Text(80), Engine::Reading(80, Wide::Scroll)] {
            let stream = content(
                Content::Document(&document),
                &inputs(engine, FrontMatterStyle::Table),
            );
            let map = Map::new(&stream.lines, &stream.blocks);
            let found = text.find("Second", false);
            assert_eq!(found.len(), 1);
            let parts = map.locate(&stream.lines, &found[0]).unwrap();
            assert_eq!(parts.len(), 1, "{engine:?}: {parts:?}");
            assert_eq!(parts[0].piece.path(), [1], "{engine:?}");
            let line = map.start(&parts[0].piece).unwrap() + parts[0].rows.start;
            assert!(stream.lines[line].inner.is_some(), "{engine:?}");
        }
    }

    /// The place of the byte `byte` of the text that `origin` is of, in the
    /// block of line `line`.
    fn place(stream: &Stream, line: &Line, origin: &Origin, byte: usize) -> Pos {
        let info = stream.blocks.get(line.inner.expect("a line with texts"));
        let at = origin
            .at(u32::try_from(byte).unwrap())
            .expect("a byte of it");
        Pos::text(info.file(), info.path(), origin.text as usize, at as usize)
    }

    /// The columns of the graphemes of a span of the cell layout, as the
    /// span's size draws them: where each starts, from the span's start.
    fn graphemes(span: &Span) -> Vec<(usize, usize)> {
        let size = span.style.size;
        if span.embed.is_some() {
            return vec![(0, 0)];
        }
        if size.fraction().is_some() {
            let mut column = 0;
            return (size.groups(&span.text).into_iter())
                .map(|group| {
                    let found = (group.at, column);
                    column += usize::from(size.scale()) * usize::from(group.cells);
                    found
                })
                .collect();
        }
        (span.text.grapheme_indices(true))
            .map(|(at, _)| (at, size.width(&span.text[..at])))
            .collect()
    }

    /// The characters of the document text that the stretch of the text of
    /// `pos` that starts a grapheme shows, as a range: one character on, or
    /// the whole of a whole piece.
    fn stretch(texts: &DocumentText, origin: &Origin, pos: &Pos) -> Range<Pos> {
        let Within::Text { text, offset } = pos.within else {
            panic!("a place in a text")
        };
        let doc = &texts.texts(pos.file, &pos.path)[text].text;
        let end = match origin.exact {
            true => doc[offset..]
                .chars()
                .next()
                .map_or(offset, |c| offset + c.len_utf8()),
            false => origin.doc.end as usize,
        };
        pos.clone()..Pos::text(pos.file, &pos.path, text, end.max(offset))
    }

    /// Whether the parts have line `line`.
    fn has(map: &Map, lines: &[Line], parts: &[Part], line: usize) -> bool {
        let _ = lines;
        parts.iter().any(|part| {
            let start = map.start(&part.piece).unwrap();
            (start + part.rows.start..start + part.rows.end).contains(&line)
        })
    }

    #[test]
    fn what_is_at_the_start_of_each_grapheme_of_a_line_is_its_place_and_locate_finds_the_line() {
        let (mut exact, mut whole) = (0, 0);
        for path in crate::testing::documents() {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let document = parse(&std::fs::read_to_string(&path).unwrap());
            let texts = DocumentText::new([&document]);
            for width in [200, 40] {
                for style in STYLES {
                    let stream = content(
                        Content::Document(&document),
                        &inputs(Engine::Text(width), style),
                    );
                    let map = Map::new(&stream.lines, &stream.blocks);
                    for (index, line) in stream.lines.iter().enumerate() {
                        let mut column = wrap::width(&line.lead);
                        for span in &line.spans {
                            for (at, cells) in graphemes(span) {
                                let Some(origin) = span.origins.of(at) else {
                                    continue;
                                };
                                let at_place = place(&stream, line, origin, at);
                                let found = map.hit(&stream.lines, index, column + cells);
                                let label = format!(
                                    "{name} {width} {style:?}: line {index}, {:?} at {at}",
                                    span.text
                                );
                                assert_eq!(found, Some(at_place.clone()), "{label}");
                                let range = stretch(&texts, origin, &at_place);
                                if origin.picture || range.start == range.end {
                                    continue;
                                }
                                let parts = map.locate(&stream.lines, &range);
                                assert!(
                                    has(&map, &stream.lines, &parts.expect(&label), index),
                                    "{label}: {range:?}"
                                );
                                match origin.exact {
                                    true => exact += 1,
                                    false => whole += 1,
                                }
                            }
                            column += wrap::width(std::slice::from_ref(span));
                        }
                    }
                }
            }
        }
        assert!(exact > 5000 && whole > 20, "{exact} {whole}");
    }

    /// The pieces of the parts of a result, or why there are none.
    fn pieces(found: Result<Vec<Part>, Hidden>) -> Result<Vec<Vec<usize>>, Hidden> {
        found.map(|parts| {
            let mut pieces: Vec<Vec<usize>> = parts
                .iter()
                .map(|part| part.piece.path().to_vec())
                .collect();
            pieces.dedup();
            pieces
        })
    }

    #[test]
    fn an_image_file_is_a_block_without_text_whose_place_is_the_block() {
        use crate::typeset::images::{ImageKey, Source};
        use crate::typeset::model::Block;
        let document = Document::new(vec![
            Block::ImageFile("a.png".to_owned()),
            Block::Paragraph(vec![crate::typeset::model::Inline::Text(
                "after".to_owned(),
            )]),
        ]);
        let mut inputs = inputs(Engine::Text(60), FrontMatterStyle::Table);
        inputs.caps.cell_pixels = Some((10, 20));
        let key = inputs.drawn(ImageKey::from("a.png"));
        inputs
            .images
            .insert(key, Ok(Source::Raster(::image::RgbaImage::new(40, 40))));
        let stream = content(Content::Document(&document), &inputs);
        let map = Map::new(&stream.lines, &stream.blocks);
        let rows: Vec<usize> = (0..stream.lines.len())
            .filter(|&line| stream.lines[line].image.is_some())
            .collect();
        assert!(!rows.is_empty(), "the image is drawn");
        let block = Pos::block(0, &[0]);
        for &row in &rows {
            assert_eq!(map.hit(&stream.lines, row, 0), Some(block.clone()));
        }
        let parts = map
            .locate(&stream.lines, &(block..Pos::end(0, &[0])))
            .unwrap();
        let start = map.start(&parts[0].piece).unwrap();
        assert_eq!(parts[0].piece.path(), [0]);
        assert_eq!(
            start + parts[0].rows.start..start + parts[0].rows.end,
            rows[0]..rows[0] + rows.len()
        );
    }

    #[test]
    fn a_picture_hides_its_text_and_is_found_at_the_start_of_it() {
        use crate::typeset::images::{ImageKey, Source};
        let document = parse("before ![the alt](p.png) after\n\n![lone alt](p.png)\n");
        let texts = DocumentText::new([&document]);
        for engine in [Engine::Text(60), Engine::Reading(60, Wide::Wrap)] {
            let mut with = inputs(engine, FrontMatterStyle::Table);
            let key = with.drawn(ImageKey::from("p.png"));
            with.images
                .insert(key, Ok(Source::Raster(::image::RgbaImage::new(20, 20))));
            let stream = content(Content::Document(&document), &with);
            let map = Map::new(&stream.lines, &stream.blocks);
            // The alt text of an image that is drawn is not found.
            for term in ["the alt", "lone alt"] {
                let found = texts.find(term, false);
                assert_eq!(found.len(), 1);
                assert_eq!(
                    map.locate(&stream.lines, &found[0]).map(|_| ()),
                    Err(Hidden::Picture),
                    "{engine:?} {term}"
                );
            }
            // The text around it is.
            let found = texts.find("after", false);
            assert!(
                map.locate(&stream.lines, &found[0])
                    .is_ok_and(|p| !p.is_empty())
            );
            // A range of both has the visible part.
            let both = Pos::text(0, &[0], 0, 0)..Pos::text(0, &[0], 0, 30);
            assert_eq!(pieces(map.locate(&stream.lines, &both)), Ok(vec![vec![0]]));
        }
        // Where the same image is not drawn, its text is.
        for engine in [Engine::Text(60), Engine::Reading(60, Wide::Wrap)] {
            let stream = content(
                Content::Document(&document),
                &inputs(engine, FrontMatterStyle::Table),
            );
            let map = Map::new(&stream.lines, &stream.blocks);
            for term in ["the alt", "lone alt"] {
                let found = texts.find(term, false);
                assert!(
                    map.locate(&stream.lines, &found[0])
                        .is_ok_and(|p| !p.is_empty()),
                    "{engine:?} {term}"
                );
            }
        }
        // A picture is at the start of its text.
        let mut with = inputs(Engine::Text(60), FrontMatterStyle::Table);
        let key = with.drawn(ImageKey::from("p.png"));
        with.images
            .insert(key, Ok(Source::Raster(::image::RgbaImage::new(20, 20))));
        let stream = content(Content::Document(&document), &with);
        let map = Map::new(&stream.lines, &stream.blocks);
        let (line, span) = (stream.lines.iter().enumerate())
            .find_map(|(line, laid)| {
                let span = laid.spans.iter().position(|span| span.embed.is_some())?;
                Some((line, span))
            })
            .expect("an embedded image");
        let laid = &stream.lines[line];
        let before = wrap::width(&laid.lead) + wrap::width(&laid.spans[..span]);
        assert_eq!(
            map.hit(&stream.lines, line, before + 1),
            Some(Pos::text(0, &[0], 0, 7))
        );
    }

    const FRONT_MATTER: &str = "---\ntitle: Secret\nauthor: Someone\n---\n\nBody text.\n";

    #[test]
    fn front_matter_that_is_hidden_or_shown_as_its_source_is_not_shown() {
        let document = parse(FRONT_MATTER);
        let texts = DocumentText::new([&document]);
        let key = texts.find("Secret", false);
        let body = texts.find("Body", false);
        assert_eq!((key.len(), body.len()), (1, 1));
        for engine in ENGINES {
            for style in STYLES {
                let stream = content(Content::Document(&document), &inputs(engine, style));
                let map = Map::new(&stream.lines, &stream.blocks);
                let label = format!("{engine:?} {style:?}");
                let found = map.locate(&stream.lines, &key[0]);
                match style {
                    FrontMatterStyle::Table => {
                        assert!(
                            found.as_ref().is_ok_and(|p| !p.is_empty()),
                            "{label}: {found:?}"
                        )
                    }
                    _ => assert_eq!(found.map(|_| ()), Err(Hidden::FrontMatter), "{label}"),
                }
                let shown = map.locate(&stream.lines, &body[0]);
                assert!(
                    shown.as_ref().is_ok_and(|p| !p.is_empty()),
                    "{label}: {shown:?} {:?}",
                    body[0]
                );
                // A range across the hidden and the visible has the visible.
                let across = key[0].start.clone()..body[0].end.clone();
                let pieces = pieces(map.locate(&stream.lines, &across));
                let last = vec![1];
                assert_eq!(pieces.map(|p| p.last().cloned()), Ok(Some(last)), "{label}");
            }
        }
    }

    /// Three ranges in each text of the document: its first character, the
    /// one in the middle and the last.
    fn samples(texts: &DocumentText) -> Vec<Range<Pos>> {
        let mut found = Vec::new();
        for pos in texts.positions() {
            let Within::Text { text, offset: 0 } = pos.within else {
                continue;
            };
            let doc = &texts.texts(pos.file, &pos.path)[text].text;
            let chars: Vec<usize> = doc.char_indices().map(|(at, _)| at).collect();
            for number in [0, chars.len() / 2, chars.len().saturating_sub(1)] {
                let Some(&from) = chars.get(number) else {
                    continue;
                };
                let to = from + doc[from..].chars().next().map_or(0, char::len_utf8);
                let at = |offset| Pos::text(pos.file, &pos.path, text, offset);
                found.push(at(from)..at(to));
            }
        }
        found
    }

    /// Whether a difference between the engines is one that is known: a space
    /// or a line break where the cell layout starts a new line is shown by no
    /// cell, as the break between two lines of code is, and reading mode shows
    /// it as a character.
    fn known(
        said: &str,
        text: &Result<Vec<Vec<usize>>, Hidden>,
        page: &Result<Vec<Vec<usize>>, Hidden>,
    ) -> bool {
        said.trim().is_empty()
            && text == &Ok(Vec::new())
            && page.as_ref().is_ok_and(|pieces| !pieces.is_empty())
    }

    #[test]
    fn the_same_range_is_in_the_same_pieces_in_both_engines() {
        let (mut compared, mut breaks, mut differing): (usize, usize, Vec<String>) =
            (0, 0, Vec::new());
        for path in crate::testing::documents() {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let document = parse(&std::fs::read_to_string(&path).unwrap());
            let texts = DocumentText::new([&document]);
            for style in STYLES {
                let layout = |engine| {
                    let stream = content(Content::Document(&document), &inputs(engine, style));
                    let map = Map::new(&stream.lines, &stream.blocks);
                    (stream, map)
                };
                let (text, text_map) = layout(Engine::Text(200));
                let (page, page_map) = layout(Engine::Reading(200, Wide::Scroll));
                for range in samples(&texts) {
                    let one = pieces(text_map.locate(&text.lines, &range));
                    let other = pieces(page_map.locate(&page.lines, &range));
                    compared += 1;
                    let said = texts.text(&range);
                    if one != other && known(&said, &one, &other) {
                        breaks += 1;
                    } else if one != other {
                        differing.push(format!(
                            "{name} {style:?} {:?} {said:?}: {one:?} and {other:?}",
                            range.start
                        ));
                    }
                }
            }
        }
        assert!(compared > 5000, "{compared}");
        assert!(
            differing.is_empty(),
            "{} differ:\n{}",
            differing.len(),
            differing.join("\n")
        );
        // A known difference that no text shows any more is not known.
        assert!(breaks > 0, "the spaces at line breaks differ no more");
    }

    #[test]
    fn what_is_at_a_cell_of_a_page_is_a_place_that_is_shown_on_that_row() {
        let (mut cells, mut files) = (0, 0);
        for path in crate::testing::documents() {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let document = parse(&std::fs::read_to_string(&path).unwrap());
            let texts = DocumentText::new([&document]);
            for engine in [
                Engine::Reading(200, Wide::Scroll),
                Engine::Reading(60, Wide::Wrap),
            ] {
                for style in STYLES {
                    let stream = content(Content::Document(&document), &inputs(engine, style));
                    let map = Map::new(&stream.lines, &stream.blocks);
                    for (index, line) in stream.lines.iter().enumerate() {
                        if line.image.as_ref().is_none_or(|image| image.page.is_none()) {
                            continue;
                        }
                        for column in 0..usize::from(Capabilities::kitty(200).width) {
                            let Some(pos) = map.hit(&stream.lines, index, column) else {
                                continue;
                            };
                            let Within::Text { text, offset } = pos.within else {
                                files += 1;
                                continue;
                            };
                            let doc = &texts.texts(pos.file, &pos.path)[text].text;
                            let end = doc[offset..]
                                .chars()
                                .next()
                                .map_or(offset, |c| offset + c.len_utf8());
                            let range = pos.clone()..Pos::text(pos.file, &pos.path, text, end);
                            let found = map.locate(&stream.lines, &range);
                            let label = format!(
                                "{name} {engine:?} {style:?}: line {index} column {column} {range:?}"
                            );
                            match found {
                                Ok(parts) => {
                                    assert!(has(&map, &stream.lines, &parts, index), "{label}")
                                }
                                Err(why) => assert_eq!(why, Hidden::Picture, "{label}"),
                            }
                            cells += 1;
                        }
                    }
                }
            }
        }
        assert!(cells > 5000, "{cells} {files}");
    }

    /// The number of each band among those of its piece, as the state engine
    /// tells them apart.
    fn band_numbers(stream: &Stream) -> Vec<(Range<usize>, usize)> {
        let places = stream.blocks.places(&stream.lines);
        let mut counted: Vec<(Option<BlockInfo>, usize)> = Vec::new();
        (engine::bands(&stream.lines).into_iter())
            .map(|band| {
                let place = places[band.lines.start].clone();
                let nth = match counted.iter_mut().find(|(kept, _)| *kept == place) {
                    Some((_, count)) => {
                        *count += 1;
                        *count - 1
                    }
                    None => {
                        counted.push((place, 1));
                        0
                    }
                };
                (band.lines, nth)
            })
            .collect()
    }

    #[test]
    fn a_range_in_a_band_tells_the_band_and_the_pixels_across_it() {
        let long = "a".repeat(200);
        let source = format!(
            "> ```\n> {long}\n> ```\n>\n> text\n>\n> ```\n> {long} needle\n> ```\n\n```\n{long} other needle\n```\n"
        );
        let document = parse(&source);
        let texts = DocumentText::new([&document]);
        let stream = content(
            Content::Document(&document),
            &inputs(Engine::Reading(80, Wide::Scroll), FrontMatterStyle::Table),
        );
        let map = Map::new(&stream.lines, &stream.blocks);
        let numbers = band_numbers(&stream);
        assert_eq!(numbers.len(), 3);
        let found = texts.find("needle", false);
        assert_eq!(found.len(), 2);
        for (found, (lines, nth)) in found.iter().zip(&numbers[1..]) {
            let parts = map.locate(&stream.lines, found).unwrap();
            let [part] = &parts[..] else {
                panic!("{parts:?}")
            };
            let across = part.band.as_ref().expect("in a band");
            assert_eq!(across.band, *nth, "{part:?}");
            assert!(
                across.pixels.start > 1000 && across.pixels.end > across.pixels.start,
                "{across:?}"
            );
            let start = map.start(&part.piece).unwrap();
            assert!(
                lines.contains(&(start + part.rows.start)),
                "{lines:?} {part:?}"
            );
        }
        // The two blocks of the quote are bands 0 and 1 of it; the last is
        // the first of its own block.
        assert_eq!(
            numbers.iter().map(|(_, nth)| *nth).collect::<Vec<_>>(),
            [0, 1, 0]
        );
        // Text that is in no band says none.
        let text = texts.find("text", false);
        let parts = map.locate(&stream.lines, &text[0]).unwrap();
        assert!(parts.iter().all(|part| part.band.is_none()));
    }

    #[test]
    fn what_the_layout_makes_up_is_no_place() {
        let document = parse("- item\n\n1. one\n\n> quoted\n\n```rust\nlet a = 1;\n```\n");
        let stream = content(
            Content::Document(&document),
            &inputs(Engine::Text(60), FrontMatterStyle::Table),
        );
        let map = Map::new(&stream.lines, &stream.blocks);
        let line_of = |text: &str| {
            stream
                .lines
                .iter()
                .position(|line| crate::typeset::style::text(&line.spans).contains(text))
                .unwrap()
        };
        // The marker and the number are not in the document.
        let item = line_of("item");
        let marker = wrap::width(&stream.lines[item].lead);
        assert_eq!(map.hit(&stream.lines, item, marker), None);
        assert!(
            map.hit(&stream.lines, item, marker + 3).is_some()
                || map.hit(&stream.lines, item, marker + 2).is_some()
        );
        let one = line_of("one");
        assert_eq!(map.hit(&stream.lines, one, 0), None);
        // Nor is the name of the language of a code block.
        let language = line_of("rust");
        let found = (0..60).filter_map(|column| map.hit(&stream.lines, language, column));
        assert_eq!(found.count(), 0);
    }

    #[test]
    fn a_drawn_formula_and_diagram_are_found_at_the_start_of_their_source() {
        use crate::typeset::images::{ImageKey, Source};
        let code = "flowchart LR\n  a --> b\n";
        let document = parse(&format!("$$x$$\n\n```mermaid\n{code}```\n"));
        for engine in [Engine::Text(60), Engine::Reading(60, Wide::Wrap)] {
            let mut with = inputs(engine, FrontMatterStyle::Table);
            for key in [
                ImageKey::rendered("math", "x\n"),
                ImageKey::rendered("mermaid", code),
            ] {
                let key = with.drawn(key);
                with.images
                    .insert(key, Ok(Source::Raster(::image::RgbaImage::new(40, 40))));
            }
            let stream = content(Content::Document(&document), &with);
            let map = Map::new(&stream.lines, &stream.blocks);
            for (path, source) in [([0], 0..1), ([1], 0..22)] {
                let block = Pos::block(0, &path);
                let whole = block..Pos::end(0, &path);
                let rows: Vec<usize> = (0..stream.lines.len())
                    .filter(|&line| {
                        let row = stream.lines[line]
                            .inner
                            .map(|id| stream.blocks.get(id).path());
                        row == Some(&path[..]) && stream.lines[line].image.is_some()
                    })
                    .collect();
                assert!(!rows.is_empty(), "{engine:?} {path:?} is drawn");
                let middle = rows[rows.len() / 2];
                let shown = (0..40).find_map(|column| map.hit(&stream.lines, middle, column));
                let start = Pos::text(0, &path, 0, source.start);
                assert_eq!(shown, Some(start), "{engine:?} {path:?}");
                // The text behind the picture is not shown, and the block is.
                let text = Pos::text(0, &path, 0, 0)..Pos::text(0, &path, 0, source.end);
                assert_eq!(
                    map.locate(&stream.lines, &text).map(|_| ()),
                    Err(Hidden::Picture)
                );
                assert!(
                    map.locate(&stream.lines, &whole)
                        .is_ok_and(|parts| !parts.is_empty())
                );
            }
        }
    }

    #[test]
    fn a_row_of_front_matter_shown_as_its_source_is_at_the_block() {
        let document = parse(FRONT_MATTER);
        for engine in ENGINES {
            let stream = content(
                Content::Document(&document),
                &inputs(engine, FrontMatterStyle::Code),
            );
            let map = Map::new(&stream.lines, &stream.blocks);
            let block = Pos::block(0, &[0]);
            let rows: Vec<usize> = (0..stream.lines.len())
                .filter(|&line| {
                    let inner = stream.lines[line].inner;
                    inner.is_some_and(|id| stream.blocks.get(id).path() == [0])
                })
                .collect();
            assert!(!rows.is_empty(), "{engine:?}: the source is shown");
            let columns = usize::from(Capabilities::kitty(200).width);
            for &row in &rows {
                let found: Vec<Pos> = (0..columns)
                    .filter_map(|column| map.hit(&stream.lines, row, column))
                    .collect();
                let lead = wrap::width(&stream.lines[row].lead);
                for column in 0..lead {
                    assert_eq!(
                        map.hit(&stream.lines, row, column),
                        None,
                        "{engine:?}: column {column} of row {row} is in the lead"
                    );
                }
                assert!(!found.is_empty(), "{engine:?}: row {row} is at no place");
                assert!(
                    found.iter().all(|pos| *pos == block),
                    "{engine:?}: {found:?}"
                );
            }
            // The body below it is still at its text.
            let body = stream.lines.iter().position(|line| {
                line.inner
                    .is_some_and(|id| stream.blocks.get(id).path() == [1])
            });
            let body = body.expect("the body is shown");
            let found = (0..columns).find_map(|column| map.hit(&stream.lines, body, column));
            assert!(
                matches!(
                    found,
                    Some(Pos {
                        within: Within::Text { .. },
                        ..
                    })
                ),
                "{engine:?}"
            );
        }
    }
}
