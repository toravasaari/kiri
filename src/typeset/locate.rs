//! Where a stretch of the document is shown, and what is at a cell of the
//! screen.
//!
//! [`Map::locate`] says which lines show a range of the document, or why
//! none does. [`Map::hit`] says which place of the document is at a line and
//! a column. Both are answered from what the layout recorded: the origins of
//! the spans of the cell layout and the texts of the pages of reading mode.
//! [`Map::bands`] lists reading mode's bands with the blocks they show.
//! Columns and pixels stay in here, and what comes out is places and rows.
//!
//! The pager makes one map for each layout and keeps it while the lines
//! stay the same, as when only a band is shown anew.
//!
//! `Map::new` takes the lines of one layout and its table of blocks, and
//! `Map::extend` goes on with lines laid out after them, adding what the new
//! lines show, so that a layout in steps has one map. `laid_to` says where the
//! laid part ends. `locate`, `drawn` and `drawn_rows` answer `NotLaidOut` for a
//! range that reaches past it, and `not_laid_out` says whether a range does.
//!
//! `locate` gives the lines that show a range, by piece, with the band and the
//! pixels across it in reading mode, or why none does: hidden front matter,
//! front matter shown as its source, the text behind a picture. `drawn` is the
//! same where a picture that is drawn counts as shown, on its own lines, and
//! `picture` gives the key of the picture drawn in the place of a text. `hit`
//! gives the place at a line and a column, `start` the first line of a piece,
//! `first_on` the smallest place a line shows and `line_of` the first line that
//! shows a place or a later one. `bands` lists the bands of reading mode, each
//! with its lines and its block. `line_bytes` and `page_spots` say what of a
//! line's text or a page shows a range, which the painters need. Pixels and
//! columns stay here and in `reading/places.rs`.

use std::cell::OnceCell;
use std::collections::BTreeMap;
use std::ops::Range;
use std::rc::Rc;

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::typeset::block::{BlockInfo, Blocks, Header};
use crate::typeset::images::ImageKey;
use crate::typeset::position::Pos;
use crate::typeset::reading::animated;
use crate::typeset::reading::engine::{self, Parts};
use crate::typeset::reading::page::ItemRef;
use crate::typeset::reading::places::Centre;
use crate::typeset::style::{Line, Measured, Origin, Span};
use crate::typeset::wrap;

/// Where the places of the lines are, found once for one layout.
#[derive(Debug)]
pub struct Map {
    blocks: Blocks,
    /// What the lines show of the texts of the document, in the order of the
    /// document. Sorted by `lo` and then `hi`, and `hi` never goes down in
    /// that order: no stretch holds a later one that ends before it does.
    /// `locate`, `line_bytes` and `page_spots` search it with
    /// `partition_point` on `hi`, which is right only so; an origin that broke
    /// it would drop matches without a word, so `new` checks it.
    texts: Vec<Shown>,
    /// The lines that show no text, each at the place of its block, in the
    /// order of the document.
    points: Vec<(Pos, usize)>,
    /// The first line of each piece.
    starts: BTreeMap<BlockInfo, usize>,
    /// The blocks whose texts no line shows.
    hidden: Vec<BlockInfo>,
    pages: Vec<Page>,
    /// How many lines the map covers.
    rows: usize,
    /// The places that each line shows, for the lines that show any, in
    /// the order of the lines. A line that shows none has no entry: the
    /// lines of a large document that are only measured would each cost
    /// two places that are none.
    shown_on: Vec<LinePlaces>,
    /// The bands of reading mode, in the order of the lines.
    bands: Vec<Band>,
    /// The stretches that are only measured, in the order of the lines:
    /// they answer for the smallest and the greatest place of their lines.
    measured: Vec<Stretch>,
    /// Where what the layout has not laid out starts, when it has not laid
    /// out all: what is before it is shown by the lines, and no line shows
    /// what is after.
    laid_to: Option<Pos>,
    /// The first rows of what is after each stretch and each point, made
    /// when `line_of` first asks and dropped when the map changes.
    tails: OnceCell<Tails>,
}

/// For each index of `texts` and of `points`, the smallest first row of the
/// stretches or points from there on, `usize::MAX` after the last: a place
/// is no earlier than the row of the first stretch after it, and nothing
/// after a stretch that starts at or below the best row found can be above
/// it, whatever the order of the rows.
#[derive(Debug)]
struct Tails {
    texts: Vec<usize>,
    points: Vec<usize>,
}

/// The smallest of `rows` from each index on, with `usize::MAX` after the
/// last.
fn smallest_from(rows: impl DoubleEndedIterator<Item = usize>) -> Vec<usize> {
    let mut smallest = vec![usize::MAX];
    for row in rows.rev() {
        smallest.push(row.min(smallest[smallest.len() - 1]));
    }
    smallest.reverse();
    smallest
}

/// The smallest place that a line shows, and the place after the last
/// one, `first..last`.
#[derive(Debug)]
struct LinePlaces {
    line: usize,
    first: Pos,
    last: Pos,
}

/// The places that the lines `rows` show, from the `texts` and `points`
/// that were added for them, one entry for each line that shows any, in
/// the order of the lines. The lines of a measured stretch are answered
/// for by the stretch.
fn line_places(
    texts: &[Shown],
    points: &[(Pos, usize)],
    pages: &[Page],
    rows: &Range<usize>,
) -> Vec<LinePlaces> {
    let mut fresh: Vec<LinePlaces> = Vec::new();
    // A line that the last entry is of takes the new places in it, so
    // that the many texts of a line, one for each of its spans, do not
    // each cost an entry while the lines are added.
    let mut note = |line: usize, first: &Pos, last: &Pos| match fresh.last_mut() {
        Some(kept) if kept.line == line => {
            if *first < kept.first {
                kept.first = first.clone();
            }
            if *last > kept.last {
                kept.last = last.clone();
            }
        }
        _ => fresh.push(LinePlaces {
            line,
            first: first.clone(),
            last: last.clone(),
        }),
    };
    for shown in texts {
        if matches!(shown.at, At::Measured(_)) {
            continue;
        }
        let starts = row_starts(shown, pages);
        for line in shown.rows.start.max(rows.start)..shown.rows.end.min(rows.end) {
            let first = starts
                .binary_search_by_key(&line, |(row, _)| *row)
                .map_or(&shown.lo, |at| &starts[at].1);
            note(line, first, &shown.hi);
        }
    }
    for (at, line) in points {
        if rows.contains(line) {
            note(*line, at, at);
        }
    }
    // Entries of one line that were not next to each other are one now.
    // The caller puts them in the place of those the map had for `rows`.
    fresh.sort_by_key(|entry| entry.line);
    fresh.dedup_by(|entry, kept| {
        let same = entry.line == kept.line;
        if same {
            if entry.first < kept.first {
                std::mem::swap(&mut entry.first, &mut kept.first);
            }
            if entry.last > kept.last {
                std::mem::swap(&mut entry.last, &mut kept.last);
            }
        }
        same
    });
    fresh
}

/// The place that a text of reading mode begins with on each row of the
/// lines that show it, in the order of the rows; none for the others. A
/// paragraph is one item on a page, and the places of its rows are those of
/// its lines of text, asked of the page as the search asks where a range is
/// shown ([`Parts::rows_of`]). A row has the first text line that touches it.
fn row_starts(shown: &Shown, pages: &[Page]) -> Vec<(usize, Pos)> {
    let At::Item { page, item } = &shown.at else {
        return Vec::new();
    };
    let (Some(page), false) = (pages.get(*page), shown.origin.picture) else {
        return Vec::new();
    };
    let origin = &shown.origin;
    let bytes = origin.shown.start as usize..origin.shown.end as usize;
    let place = |byte: usize| {
        let offset = origin.at(u32::try_from(byte).ok()?)?;
        let (file, path) = (shown.lo.file, &shown.lo.path);
        Some(Pos::text(file, path, origin.text as usize, offset as usize))
    };
    (page.parts.row_starts(*item, &bytes).into_iter())
        .filter_map(|(row, byte)| Some((page.rows.start + row, place(byte)?)))
        .filter(|(row, _)| *row < page.rows.end)
        .collect()
}

/// A band of reading mode: a code block or a table on a page that scrolls
/// sideways, with the block it shows. How far it is scrolled and whether it
/// is selected are not here, as the state engine changes them on the lines
/// after the map is made; [`Band::scrolled`] reads them from the lines.
#[derive(Debug, Clone)]
pub struct Band {
    /// The lines that show it.
    pub lines: Range<usize>,
    block: Option<BlockInfo>,
    parts: Rc<Parts>,
    band: u32,
}

impl Band {
    /// The innermost block it shows, or, when its page does not say, the
    /// block of its first line. A method, so that the lines guard
    /// (`tests/it/guard_lines.rs`) can tell it from the field of a line.
    pub fn block(&self) -> Option<&BlockInfo> {
        self.block.as_ref()
    }

    /// How far it scrolls at most, in pixels.
    pub fn most(&self) -> u32 {
        self.parts.band_most(self.band)
    }

    /// The width it shows at a time, in pixels.
    pub fn visible(&self) -> u32 {
        self.parts.band_visible(self.band)
    }

    /// How far a key that steps by a little scrolls it, in pixels.
    pub fn step(&self) -> u32 {
        self.parts.step()
    }

    /// How far it is scrolled on `lines`, the lines the map was made of as
    /// they are now, and whether it is selected.
    pub fn scrolled(&self, lines: &[Line]) -> (u32, bool) {
        let rows = lines.get(self.lines.clone()).unwrap_or_default();
        (engine::bands(rows).first()).map_or((0, false), |band| (band.offset, band.selected))
    }
}

#[cfg(test)]
thread_local! {
    /// How many maps were made on this thread, which the tests of who makes
    /// them count.
    static MAPS_MADE: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// How many maps were made on this thread.
#[cfg(test)]
pub(crate) fn maps_made() -> usize {
    MAPS_MADE.with(std::cell::Cell::get)
}

/// The lines of a stretch that is only measured, and the places from the
/// first to the one after the last that they would show.
#[derive(Debug)]
struct Stretch {
    rows: Range<usize>,
    lo: Pos,
    hi: Pos,
}

/// A stretch of a text that something shows.
#[derive(Debug)]
struct Shown {
    lo: Pos,
    hi: Pos,
    origin: Origin,
    at: At,
    /// The lines that show it, none when none does.
    rows: Range<usize>,
}

/// Where a stretch of a text is shown.
#[derive(Debug)]
enum At {
    /// In a span of this line of the cell layout, which starts `at` bytes
    /// into the line's text.
    Line { line: usize, at: usize },
    /// On the image row that is this line of the cell layout.
    Row(usize),
    /// On lines of the cell layout that are only measured, of this
    /// stretch: the lines of `rows`, which hold nothing of the text.
    Measured(Rc<Measured>),
    /// In this text of this page of reading mode.
    Item { page: usize, item: ItemRef },
    /// In a picture of a page of reading mode, on these lines. `hole` is
    /// what its pixels carry when it is a hole for an animation: the id and
    /// the size, which tell which animation.
    Drawn {
        page: usize,
        rows: Range<usize>,
        hole: Option<(u64, (u32, u32))>,
        centre: Option<Centre>,
    },
}

/// A page of reading mode among the lines.
#[derive(Debug)]
struct Page {
    rows: Range<usize>,
    parts: Rc<Parts>,
    /// A number for each of its bands that no other band of the layout has,
    /// to tell the parts of one band from those of another.
    ids: Vec<usize>,
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

/// A band that shows part of a range: the pixels of the band's content it
/// covers from left to right. The band itself is found by the lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Across {
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
    /// The layout has not laid the range out yet, or not all of it: no line
    /// is known to show it or not to.
    NotLaidOut,
}

impl Map {
    /// The map of `lines`, which `blocks` is the table of.
    pub fn new(lines: &[Line], blocks: &Blocks) -> Self {
        #[cfg(test)]
        MAPS_MADE.with(|made| made.set(made.get() + 1));
        let mut map = Self {
            blocks: blocks.clone(),
            texts: Vec::new(),
            points: Vec::new(),
            starts: BTreeMap::new(),
            hidden: blocks.hidden().cloned().collect(),
            pages: Vec::new(),
            rows: 0,
            shown_on: Vec::new(),
            bands: Vec::new(),
            measured: Vec::new(),
            laid_to: None,
            tails: OnceCell::new(),
        };
        map.extend(lines, None);
        map
    }

    /// Takes the headers that `blocks`, the table the map was made of, has
    /// laid out since, into the map's own table, which then holds the one
    /// copy of the headers the map answers from. The layout lays a file's
    /// header out when it comes to it, which can be after the map was made.
    pub(crate) fn headers_of(&mut self, blocks: &Blocks) {
        if self.blocks.headers() == blocks.headers() {
            return;
        }
        for (&file, header) in blocks.headers() {
            self.blocks.set_header(file, header.clone());
        }
    }

    /// Where the header of file `file` is, as the layout laid it out: its
    /// lines, and the row that stands for it. `None` for a file without a
    /// header, as in a lone document, and for one whose header is not laid
    /// out, or was taken back.
    pub fn header(&self, file: usize) -> Option<(Range<usize>, &Header)> {
        let header = self.blocks.headers().get(&file)?;
        let start = self.start(&BlockInfo::new(file, Vec::new()))?;
        (start < self.rows).then(|| (start..start + header.rows, header))
    }

    /// Goes on with `lines`, which are the lines the map was made of and
    /// those laid out after them, in order: the new ones are added, and
    /// what the layout has not laid out is what is after the place `laid_to`,
    /// `None` when all is laid out. The new lines must start at a page of
    /// reading mode.
    pub fn extend(&mut self, lines: &[Line], laid_to: Option<Pos>) {
        let from = self.rows;
        self.laid_to = laid_to;
        let added = (self.texts.len(), self.points.len());
        for (index, line) in lines.iter().enumerate().skip(from) {
            if let Some(piece) = line.block {
                (self.starts.entry(self.blocks.get(piece).clone())).or_insert(index);
            }
        }
        self.add_pages(lines, from);
        let blocks = &self.blocks;
        let new_bands = engine::bands(&lines[from..]).into_iter().map(|band| {
            let inner = band.parts.band_block(band.band);
            let block = inner.or_else(|| lines.get(from + band.lines.start)?.block);
            Band {
                lines: from + band.lines.start..from + band.lines.end,
                block: block.map(|block| blocks.get(block).clone()),
                parts: band.parts,
                band: band.band,
            }
        });
        self.bands.extend(new_bands);
        self.rows = lines.len();
        self.add_lines(lines, from..lines.len(), added);
    }

    /// Lets go of what the lines from `rows` on show, as a layout of the cell
    /// layout taken back to the start of a piece there does: the map is that
    /// of the lines before it, and goes on with those laid out after them
    /// ([`Map::extend`]). No line before it shows a text that goes on past
    /// it, and none of them is of a page of reading mode.
    pub fn cut(&mut self, rows: usize) {
        self.tails.take();
        self.texts.retain(|shown| shown.rows.start < rows);
        self.points.retain(|(_, line)| *line < rows);
        self.starts.retain(|_, line| *line < rows);
        self.shown_on.retain(|entry| entry.line < rows);
        self.measured.retain(|stretch| stretch.rows.start < rows);
        self.bands.retain(|band| band.lines.start < rows);
        self.pages.retain(|page| page.rows.start < rows);
        self.rows = self.rows.min(rows);
    }

    /// Takes the lines `changed` of `lines` again, which were laid out in
    /// the place of those the map had there, as many: lines that were only
    /// measured and are laid out now, or the other way. They are not of a
    /// page of reading mode, and no block starts in them.
    pub fn relay(&mut self, lines: &[Line], changed: Range<usize>) {
        let inside = |rows: &Range<usize>| changed.start <= rows.start && rows.start < changed.end;
        self.texts.retain(|shown| !inside(&shown.rows));
        self.points.retain(|(_, line)| !changed.contains(line));
        self.measured.retain(|stretch| !inside(&stretch.rows));
        let added = (self.texts.len(), self.points.len());
        self.add_lines(lines, changed, added);
    }

    /// Adds what the lines `new` of `lines` show, which are of the cell
    /// layout or rows of pages that are added already: the texts and the
    /// points of the map from `texts_from` and `points_from` on are those of
    /// the pages.
    fn add_lines(
        &mut self,
        lines: &[Line],
        new: Range<usize>,
        (texts_from, points_from): (usize, usize),
    ) {
        let blocks = &self.blocks;
        for (index, line) in lines.iter().enumerate().take(new.end).skip(new.start) {
            let Some(inner) = line.inner else { continue };
            // The lines of a stretch that is only measured are added once,
            // at its first line.
            if let Some(measured) = &line.measured {
                let before = index.checked_sub(1).and_then(|before| lines.get(before));
                let goes_on = (before.and_then(|line| line.measured.as_ref()))
                    .is_some_and(|before| Rc::ptr_eq(before, measured));
                if let (false, Some((lo, hi))) = (goes_on, measured.bounds()) {
                    let info = blocks.get(inner);
                    let rows = index..index + measured.lines();
                    let at_place = |(text, byte): (u32, u32)| {
                        Pos::text(info.file(), info.path(), text as usize, byte as usize)
                    };
                    let shows = match lo.0 == hi.0 {
                        true => lo.1..hi.1,
                        false => lo.1..lo.1,
                    };
                    let origin = Origin {
                        exact: false,
                        ..Origin::exact(lo.0, shows)
                    };
                    self.texts.push(Shown {
                        lo: at_place(lo),
                        hi: at_place(hi),
                        origin,
                        at: At::Measured(Rc::clone(measured)),
                        rows: rows.clone(),
                    });
                    let stretch = Stretch {
                        rows,
                        lo: at_place(lo),
                        hi: at_place(hi),
                    };
                    let at = (self.measured)
                        .partition_point(|kept| kept.rows.start < stretch.rows.start);
                    self.measured.insert(at, stretch);
                }
                continue;
            }
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
                    let place = At::Line { line: index, at };
                    Self::add_text(&mut self.texts, info, origin, place, index..index + 1);
                    shows = shows || !origin.picture;
                }
                at += span.text.len();
            }
            let picture = line.image.as_ref().and_then(|image| image.origin.as_ref());
            if let Some(origin) = picture {
                Self::add_text(
                    &mut self.texts,
                    info,
                    origin,
                    At::Row(index),
                    index..index + 1,
                );
                shows = shows || !origin.picture;
            }
            if !shows {
                self.points
                    .push((Pos::block(info.file(), info.path()), index));
            }
        }
        // What the new lines show comes first, for the smallest and the
        // greatest place of each line. The lines of a measured stretch are
        // answered for by the stretch.
        let rows = new.start..new.end.min(self.rows);
        let fresh = line_places(
            &self.texts[texts_from..],
            &self.points[points_from..],
            &self.pages,
            &rows,
        );
        let from = self
            .shown_on
            .partition_point(|entry| entry.line < new.start);
        let until = self.shown_on.partition_point(|entry| entry.line < new.end);
        self.shown_on.splice(from..until, fresh);
        self.texts
            .sort_by(|a, b| a.lo.cmp(&b.lo).then(a.hi.cmp(&b.hi)));
        debug_assert!(
            self.texts.windows(2).all(|pair| pair[0].hi <= pair[1].hi),
            "a stretch of a text holds a later one that ends before it"
        );
        self.points.sort_by(|a, b| a.0.cmp(&b.0));
        self.tails.take();
    }

    /// Says where what the layout has not laid out starts: `None` when all
    /// is laid out. The map answers `NotLaidOut` for what reaches past it.
    pub fn laid_to(&mut self, laid_to: Option<Pos>) {
        self.laid_to = laid_to;
    }

    /// Adds a stretch of a text of `info` that `origin` says is shown at
    /// `at`, on the lines `rows`.
    fn add(&mut self, info: &BlockInfo, origin: &Origin, at: At, rows: Range<usize>) {
        Self::add_text(&mut self.texts, info, origin, at, rows);
    }

    /// [`Map::add`] to `texts`, which is the map's, for a caller that holds
    /// the table of blocks, which `info` is of, at the same time.
    fn add_text(
        texts: &mut Vec<Shown>,
        info: &BlockInfo,
        origin: &Origin,
        at: At,
        rows: Range<usize>,
    ) {
        let text = origin.text as usize;
        let at_byte = |byte: u32| Pos::text(info.file(), info.path(), text, byte as usize);
        texts.push(Shown {
            lo: at_byte(origin.doc.start),
            hi: at_byte(origin.doc.end),
            origin: origin.clone(),
            at,
            rows,
        });
    }

    /// The pages of reading mode among `lines` from the line `from` on: what
    /// they show of texts, and the rows that show no text.
    fn add_pages(&mut self, lines: &[Line], from: usize) {
        let mut bands: usize = self.pages.iter().map(|page| page.ids.len()).sum();
        for page in engine::pages(&lines[from..]) {
            let number = self.pages.len();
            let rows = from + page.lines.start..from + page.lines.end;
            let parts = page.parts;
            let ids = (0..parts.band_tops().len())
                .map(|_| {
                    bands += 1;
                    bands - 1
                })
                .collect();
            let mut with_text = std::collections::BTreeSet::new();
            for seen in parts.seen() {
                let info = self.blocks.get(seen.block).clone();
                let on_page = |found: Range<usize>| {
                    let start = rows.start;
                    start + found.start..(start + found.end).min(rows.end)
                };
                let (at, found) = match seen.item.filter(|_| !seen.origin.picture) {
                    Some(item) => {
                        let shown = seen.origin.shown.clone();
                        let bytes = shown.start as usize..shown.end as usize;
                        let found = parts.rows_of(item, bytes);
                        let found = found.map_or(0..0, |found| on_page(found.rows));
                        (At::Item { page: number, item }, found)
                    }
                    None => {
                        let found = seen.rows.clone().map_or(0..0, on_page);
                        let hole = seen.hole;
                        let centre = seen.centre;
                        let page = number;
                        let drawn = At::Drawn {
                            page,
                            rows: found.clone(),
                            hole,
                            centre,
                        };
                        (drawn, found)
                    }
                };
                self.add(&info, &seen.origin, at, found);
                if !seen.origin.picture {
                    with_text.insert(info);
                }
            }
            for index in rows.clone() {
                let Some(inner) = lines[index].inner else {
                    continue;
                };
                let info = self.blocks.get(inner);
                if !with_text.contains(info) {
                    let at = Pos::block(info.file(), info.path());
                    self.points.push((at, index));
                }
            }
            self.pages.push(Page { rows, parts, ids });
        }
    }

    /// The lines that show `range`, by piece, or why it is not shown. A range
    /// that is partly shown gives the parts that are; a range that has no
    /// lines and is not hidden, such as an empty code block, gives none.
    pub fn locate(&self, lines: &[Line], range: &Range<Pos>) -> Result<Vec<Part>, Hidden> {
        self.shown_on(lines, range, false)
    }

    /// Whether `range` reaches past what the layout has laid out: no line
    /// is known to show it or not to, which is not the same as hidden.
    pub fn not_laid_out(&self, range: &Range<Pos>) -> bool {
        range.start < range.end && self.laid_to.as_ref().is_some_and(|laid| range.end > *laid)
    }

    /// `locate`, and for `drawn` the lines of a picture are those of the
    /// text it stands in the place of.
    fn shown_on(
        &self,
        lines: &[Line],
        range: &Range<Pos>,
        drawn: bool,
    ) -> Result<Vec<Part>, Hidden> {
        if range.start >= range.end {
            return Ok(Vec::new());
        }
        if self.not_laid_out(range) {
            return Err(Hidden::NotLaidOut);
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
            let picture = shown.origin.picture;
            match &shown.at {
                At::Line { line, .. } | At::Row(line) if drawn || !picture => {
                    hits.push(Hit::line(*line))
                }
                At::Measured(measured) => hits.push(Hit {
                    rows: Self::measured_rows(shown, measured, range),
                    band: None,
                }),
                At::Drawn { rows, .. } if drawn => hits.push(Hit {
                    rows: rows.clone(),
                    band: None,
                }),
                At::Item { page, item } if !picture => {
                    let bytes = Self::shown_bytes(shown, range);
                    let page = &self.pages[*page];
                    let Some(found) = page.parts.rows_of(*item, bytes) else {
                        continue;
                    };
                    let rows = page.rows.start + found.rows.start
                        ..(page.rows.start + found.rows.end).min(page.rows.end);
                    let band = found.band.map(|(band, pixels)| (page.ids[band], pixels));
                    hits.push(Hit { rows, band });
                }
                _ => pictures = true,
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

    /// The lines of the stretch `measured`, which `shown` is, that would
    /// show the part of `range` in it.
    fn measured_rows(shown: &Shown, measured: &Measured, range: &Range<Pos>) -> Range<usize> {
        let Some((lo, hi)) = measured.bounds() else {
            return shown.rows.clone();
        };
        let place = |at: &Pos, default: (u32, u32)| match &at.within {
            crate::typeset::position::Within::Text { text, offset }
                if (at.file, &at.path) == (shown.lo.file, &shown.lo.path) =>
            {
                let text = u32::try_from(*text).unwrap_or(u32::MAX);
                let offset = u32::try_from(*offset).unwrap_or(u32::MAX);
                (text, offset).clamp(lo, hi)
            }
            _ => default,
        };
        let from = match range.start > shown.lo {
            true => place(&range.start, lo),
            false => lo,
        };
        let to = match range.end < shown.hi {
            true => place(&range.end, hi),
            false => hi,
        };
        let rows = measured.rows_of(from, to);
        shown.rows.start + rows.start..shown.rows.start + rows.end
    }

    /// The stretch that is only measured and holds the line `line`.
    fn measured_at(&self, line: usize) -> Option<&Stretch> {
        let at = self.measured.partition_point(|kept| kept.rows.end <= line);
        (self.measured.get(at)).filter(|kept| kept.rows.start <= line)
    }

    /// Whether the texts at `at` are in a block whose texts no line shows.
    fn is_hidden(&self, at: &Pos) -> bool {
        (self.hidden.iter()).any(|block| (block.file(), block.path()) == (at.file, &at.path[..]))
    }

    /// `hits`, which are on lines, as the parts of the pieces they are in.
    fn parts(&self, lines: &[Line], hits: Vec<Hit>) -> Vec<Part> {
        // Each part with the number of the band it is in, if it is in one.
        let mut parts: Vec<(Range<usize>, Part, Option<usize>)> = Vec::new();
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
            let known = parts
                .iter_mut()
                .find(|(_, part, kept)| part.piece == piece && *kept == band);
            match known {
                Some((rows, part, _)) => {
                    rows.start = rows.start.min(hit.rows.start);
                    rows.end = rows.end.max(hit.rows.end);
                    if let (Some(across), Some((_, pixels))) = (&mut part.band, &hit.band) {
                        across.pixels.start = across.pixels.start.min(pixels.start);
                        across.pixels.end = across.pixels.end.max(pixels.end);
                    }
                }
                None => {
                    let across = hit.band.map(|(_, pixels)| Across { pixels });
                    let part = Part {
                        piece,
                        rows: 0..0,
                        band: across,
                    };
                    parts.push((hit.rows, part, band));
                }
            }
        }
        parts.sort_by_key(|(rows, ..)| rows.start);
        (parts.into_iter())
            .map(|(rows, mut part, _)| {
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

    /// Like [`locate`](Map::locate), but a range whose picture is drawn is
    /// shown on the lines of the picture: the linked image and the formula
    /// shown as an image are shown where the reader sees them, though the
    /// text behind them is not. The range of text that is shown as text is
    /// where `locate` says.
    pub fn drawn(&self, lines: &[Line], range: &Range<Pos>) -> Result<Vec<Part>, Hidden> {
        self.shown_on(lines, range, true)
    }

    /// The picture drawn in the place of the text in `range`, when there is
    /// one: its key, which the lines of the picture carry. In reading mode a
    /// picture that is an animation is a hole in the page that carries a
    /// mark of its own, and is told among the `candidates`, which are the
    /// keys of the animations that may be there; any other picture there has
    /// no key to tell.
    pub fn picture(
        &self,
        lines: &[Line],
        range: &Range<Pos>,
        candidates: &[ImageKey],
    ) -> Option<ImageKey> {
        if range.start >= range.end {
            return None;
        }
        let first = self.texts.partition_point(|shown| shown.hi <= range.start);
        for shown in &self.texts[first..] {
            if shown.lo >= range.end {
                break;
            }
            if shown.hi <= range.start || !shown.origin.picture {
                continue;
            }
            let key = match &shown.at {
                At::Line { line, at } => {
                    let mut start = 0;
                    let spans = lines.get(*line).map(|line| line.spans.iter());
                    spans.into_iter().flatten().find_map(|span| {
                        let here = start;
                        start += span.text.len();
                        let embed = span.embed.as_ref().filter(|_| here == *at)?;
                        Some(embed.placement.key.clone())
                    })
                }
                At::Row(line) => {
                    let row = lines.get(*line)?.image.as_ref();
                    row.map(|row| row.placement.key.clone())
                }
                At::Drawn {
                    hole: Some((id, size)),
                    ..
                } => (candidates.iter())
                    .find(|key| animated::hole_id(key, *size) == *id)
                    .cloned(),
                At::Drawn { hole: None, .. } | At::Item { .. } | At::Measured(_) => None,
            };
            if key.is_some() {
                return key;
            }
        }
        // An image file has no text to stand in the place of: it is the
        // rows of the block itself, in the cell layout.
        let first = self.points.partition_point(|(at, _)| *at < range.start);
        let points = self.points[first..]
            .iter()
            .take_while(|(at, _)| *at < range.end);
        points.into_iter().find_map(|(_, line)| {
            let row = lines
                .get(*line)?
                .image
                .as_ref()
                .filter(|row| row.page.is_none());
            row.map(|row| row.placement.key.clone())
        })
    }

    /// The smallest place shown on line `line`; `None` for a line that
    /// shows no place of the document. In a page of reading mode it is the
    /// start of the first text line that touches the row, see `row_starts`.
    pub fn first_on(&self, line: usize) -> Option<&Pos> {
        if line >= self.rows {
            return None;
        }
        let first = self
            .shown_on
            .binary_search_by_key(&line, |entry| entry.line)
            .ok()
            .map(|at| &self.shown_on[at].first);
        // A line that is only measured is at the start of its stretch.
        first.or_else(|| self.measured_at(line).map(|stretch| &stretch.lo))
    }

    /// The bands of reading mode among the lines, in their order.
    pub fn bands(&self) -> &[Band] {
        &self.bands
    }

    /// The first line that shows the place `pos` or one after it, a picture
    /// counted as shown; `None` when no line does, as at the end of the
    /// document.
    ///
    /// In a page of reading mode the line is the row of the text line that
    /// has `pos`, a picture's the first row of it.
    pub fn line_of(&self, pos: &Pos) -> Option<usize> {
        let tails = self.tails.get_or_init(|| Tails {
            texts: smallest_from(self.texts.iter().map(|shown| shown.rows.start)),
            points: smallest_from(self.points.iter().map(|(_, line)| *line)),
        });
        let from = self.texts.partition_point(|shown| shown.hi < *pos);
        let from_point = self.points.partition_point(|(at, _)| at < pos);
        let mut best = Some(tails.points[from_point]).filter(|line| *line != usize::MAX);
        for (index, shown) in self.texts.iter().enumerate().skip(from) {
            // Nothing from here on starts above the best row so far, and a
            // place is never shown above the first row of its stretch.
            if best.is_some_and(|best| tails.texts[index] >= best) {
                break;
            }
            let candidate = (shown.hi > *pos || (shown.lo == shown.hi && shown.lo >= *pos))
                && !shown.rows.is_empty();
            if !candidate {
                continue;
            }
            // The row of the text line that has the place, as a search for
            // the rest of the text from there would take the reader, is
            // not above the first row of the text: it is asked for only
            // when it may be above the best so far.
            let line = match &shown.at {
                At::Measured(measured) => {
                    Self::measured_rows(shown, measured, &(pos.clone()..shown.hi.clone())).start
                }
                At::Item { .. } if best.is_some_and(|best| shown.rows.start >= best) => continue,
                At::Item { page, item } if !shown.origin.picture => {
                    let bytes = Self::shown_bytes(shown, &(pos.clone()..shown.hi.clone()));
                    let page = &self.pages[*page];
                    (page.parts.rows_of(*item, bytes)).map_or(shown.rows.start, |found| {
                        (page.rows.start + found.rows.start).clamp(shown.rows.start, page.rows.end)
                    })
                }
                _ => shown.rows.start,
            };
            best = Some(best.map_or(line, |best| best.min(line)));
        }
        best
    }
}

impl Map {
    /// The stretches of the map that overlap `range`, in order.
    fn overlapping<'a>(&'a self, range: &'a Range<Pos>) -> impl Iterator<Item = &'a Shown> {
        let from = match range.start < range.end {
            true => self.texts.partition_point(|shown| shown.hi <= range.start),
            false => self.texts.len(),
        };
        (self.texts[from..].iter())
            .take_while(move |shown| shown.lo < range.end)
            .filter(move |shown| shown.hi > range.start)
    }

    /// What `range` is of the text of each line of the cell layout that
    /// shows it: the line and the bytes of its text, which is the text of its
    /// spans one after another. A range that a line break splits is on both
    /// lines, and the space the break takes is no part of either. The bytes
    /// of an image's text are among them, whole.
    pub fn line_bytes(&self, range: &Range<Pos>) -> Vec<(usize, Range<usize>)> {
        let mut found: Vec<(usize, Range<usize>)> = Vec::new();
        for shown in self.overlapping(range) {
            let At::Line { line, at } = &shown.at else {
                continue;
            };
            let bytes = Self::shown_bytes(shown, range);
            let bytes = at + bytes.start..at + bytes.end;
            match found.last_mut() {
                Some((last, more)) if last == line && more.end == bytes.start => {
                    more.end = bytes.end
                }
                _ => found.push((*line, bytes)),
            }
        }
        found
    }

    /// Whether something that can carry a link shows `range`: text, or a
    /// picture in a span or on a page. The image rows of the cell layout, which
    /// draw the words of a heading or a block image, cannot: they have no
    /// link on them, as the terminal has no cells of them to make one of.
    pub fn holds_cells(&self, range: &Range<Pos>) -> bool {
        self.overlapping(range)
            .any(|shown| !matches!(shown.at, At::Row(_)))
    }

    /// What shows `range` on the page of `parts`, in order: the bytes of
    /// each text and each picture.
    pub fn page_spots(&self, parts: &Rc<Parts>, range: &Range<Pos>) -> Vec<Spot> {
        let mut found: Vec<Spot> = Vec::new();
        for shown in self.overlapping(range) {
            match &shown.at {
                At::Item { page, item } if Rc::ptr_eq(&self.pages[*page].parts, parts) => {
                    let bytes = Self::shown_bytes(shown, range);
                    let item = *item;
                    match found.last_mut() {
                        Some(Spot::Text {
                            item: last,
                            bytes: more,
                        }) if *last == item && more.end == bytes.start => more.end = bytes.end,
                        _ => found.push(Spot::Text { item, bytes }),
                    }
                }
                At::Drawn {
                    page,
                    centre: Some(centre),
                    ..
                } if Rc::ptr_eq(&self.pages[*page].parts, parts) => {
                    found.push(Spot::Picture(*centre))
                }
                _ => {}
            }
        }
        found
    }

    /// The places that the lines `window` show, from the smallest to the
    /// one after the last; `None` when they show none. A range that is
    /// outside it is not on the lines.
    pub fn span_of(&self, window: Range<usize>) -> Option<Range<Pos>> {
        let window = window.start.min(self.rows)..window.end.min(self.rows);
        // The stretches that are only measured answer for their lines.
        let from = self
            .measured
            .partition_point(|kept| kept.rows.end <= window.start);
        let until = window.end;
        let measured = self.measured[from..]
            .iter()
            .take_while(move |kept| kept.rows.start < until);
        let starts = measured.clone().map(|kept| &kept.lo);
        let ends = measured.map(|kept| &kept.hi);
        let from = self
            .shown_on
            .partition_point(|entry| entry.line < window.start);
        let until = self
            .shown_on
            .partition_point(|entry| entry.line < window.end);
        let lines = &self.shown_on[from..until];
        let start = (lines.iter().map(|entry| &entry.first))
            .chain(starts)
            .min()?;
        let end = (lines.iter().map(|entry| &entry.last)).chain(ends).max()?;
        Some(start.clone()..end.clone())
    }

    /// Whether `range` may be shown on lines that show the places `shown`, as
    /// `span_of` gives them. The end of `shown` is the place after the last
    /// one for text, but for a line that shows no text it is that line's own
    /// place, so a range that starts at it is near too: a range that is not
    /// near is not on the lines, and one that is may still not be.
    pub fn near(shown: &Range<Pos>, range: &Range<Pos>) -> bool {
        range.end > shown.start && range.start <= shown.end
    }

    /// The parts of the lines that show `range` as `drawn` has them, and the
    /// lines from the first to the last: for a range that has none, such as
    /// a block that is gone, the place between the lines where it would be,
    /// as an empty range, which at the end of the document is after the last
    /// line. Why not, when the range is not shown.
    pub fn drawn_rows(
        &self,
        lines: &[Line],
        range: &Range<Pos>,
    ) -> Result<(Vec<Part>, Range<usize>), Hidden> {
        let parts = self.drawn(lines, range)?;
        let rows = parts.iter().filter_map(|part| {
            let start = self.start(&part.piece)?;
            Some(start + part.rows.start..start + part.rows.end)
        });
        let rows = match (
            rows.clone().map(|rows| rows.start).min(),
            rows.map(|rows| rows.end).max(),
        ) {
            (Some(first), Some(last)) => first..last,
            _ => {
                // A range with no lines, such as a removal, that starts where
                // nothing is laid out is in a place the layout has not reached,
                // not at the end of the lines.
                if (self.laid_to.as_ref()).is_some_and(|laid| range.start >= *laid) {
                    return Err(Hidden::NotLaidOut);
                }
                let line = self.line_of(&range.start).unwrap_or(lines.len());
                line..line
            }
        };
        Ok((parts, rows))
    }
}

/// What shows a range on a page of reading mode.
#[derive(Debug, Clone, PartialEq)]
pub enum Spot {
    /// Bytes of a text on the page.
    Text { item: ItemRef, bytes: Range<usize> },
    /// A picture, at its centre.
    Picture(Centre),
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
mod tests;
