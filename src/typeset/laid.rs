//! `Laid`, a layout and the questions that are asked of it.
//!
//! The driver lays a document out and gives a [`Stream`]: the lines and the
//! table of the blocks they come from. [`Map`] finds places in them. `Laid`
//! holds both and answers for them, so that whoever needs the layout asks
//! one thing: which lines are in a range ([`Laid::rows`]), how tall it is
//! ([`Laid::height`]), where a range of the document is shown
//! ([`Laid::locate`]), what is at a line and a column ([`Laid::hit`]) and
//! the other questions of the map. What the document wants of the world
//! before it is laid out, its images, is [`Laid::needs`].
//!
//! It can be filled in steps. [`Laid::begin`] numbers the blocks and lays
//! nothing out, [`Laid::extend`] lays out the next pieces from where the
//! layout stopped, and a layout that is laid out to its end in one step, as
//! [`driver::laid`] does, is the one that the steps give. Until it is whole,
//! what a question is about may not be laid out: the lines are the first of
//! the whole layout, and [`Laid::locate`] says [`Hidden::NotLaidOut`] for a
//! range that reaches past them. The table of the blocks is whole from the
//! start, so a block has the same number whenever it is laid out.
//!
//! The map is made when it is first asked, once for the lines, and goes on
//! with the lines as they are laid out; it stays good while the lines stay
//! the same, as when only a band is shown anew. A caller that changes lines,
//! such as the pager scrolling a band, does it through [`Laid::lines_mut`]
//! or [`Laid::parts_mut`], and the lines it changes are the ones the map was
//! made of or a size that the map does not read.
//!
//! A layout that is filled in steps is also asked [`Laid::restart`] and
//! [`Laid::is_whole`]. [`driver::laid`] and `driver::laid_from` make one whole
//! in one step, which is what every caller does for now. Until it is whole,
//! `height` counts the laid lines only. The session still asks the `Map` itself
//! with the lines (`session/search.rs`, `session/state.rs`), through
//! [`Laid::map`], so the map queries of `Laid` are used by tests only.
//!
//! `Laid::needs` is the images a document wants, with `images::plan` under it.
//! It is asked of the documents before anything is laid out, as the sizes of
//! the images feed the layout.
//!
//! Nothing outside `src/typeset/` names `Stream`, calls `Map::new` or lays out
//! with `driver::content`, `content_from` or `files`; `tests/it/guard_laid.rs`
//! checks it. The pager, the paging decision and the image plan use `height`,
//! `lines`, `map` and `needs`.

use std::cell::OnceCell;
use std::ops::Range;
use std::rc::Rc;

use crate::typeset::block::Blocks;
use crate::typeset::caps::Capabilities;
use crate::typeset::diagrams::renderer::Renderer;
use crate::typeset::driver::{Content, Progress, Stream};
use crate::typeset::images::ImageKey;
use crate::typeset::images::options::{Looks, Part as ImagePart};
use crate::typeset::images::plan::{self, Outcome, Wanted};
use crate::typeset::images::store::Images;
use crate::typeset::locate::{Band, Hidden, Map, Part, Spot};
use crate::typeset::position::Pos;
use crate::typeset::reading::engine::Parts;
use crate::typeset::style::Line;

/// What a document wants before it is laid out: the images, each once with
/// the index of its part, and beside each what the store held of it, `None`
/// for one to load.
pub(crate) type Needs<'p> = (Vec<(usize, Wanted<'p>)>, Vec<Option<Outcome>>);

/// A document laid out, with the answers to the questions about it.
#[derive(Debug, Default)]
pub struct Laid {
    lines: Vec<Line>,
    /// The blocks that the lines' [`Line::block`] numbers.
    blocks: Blocks,
    map: OnceCell<Map>,
    /// What the layout needs to be laid out further; `None` for lines that
    /// were laid out whole elsewhere.
    progress: Option<Progress>,
}

impl From<Laid> for Stream {
    fn from(laid: Laid) -> Self {
        Self {
            lines: laid.lines,
            blocks: laid.blocks,
        }
    }
}

impl From<Stream> for Laid {
    fn from(stream: Stream) -> Self {
        Self::new(stream.lines, stream.blocks)
    }
}

impl Laid {
    /// `lines` that are laid out, with `blocks`, the table of the blocks they
    /// come from.
    pub fn new(lines: Vec<Line>, blocks: Blocks) -> Self {
        Self {
            lines,
            blocks,
            map: OnceCell::new(),
            progress: None,
        }
    }

    /// A layout with `blocks`, the table of all of them, and nothing laid
    /// out, that `progress` lays out.
    pub(crate) fn stepped(blocks: Blocks, progress: Progress) -> Self {
        Self {
            lines: Vec::new(),
            blocks,
            map: OnceCell::new(),
            progress: Some(progress),
        }
    }

    /// The lines, the table of blocks and the progress of a layout in steps,
    /// for the driver to lay out more with; `None` for lines that were laid
    /// out whole elsewhere.
    pub(crate) fn stepping(&mut self) -> Option<(&mut Vec<Line>, &mut Blocks, &mut Progress)> {
        let progress = self.progress.as_mut()?;
        Some((&mut self.lines, &mut self.blocks, progress))
    }

    /// Notes that the driver laid out more: the map, if it was made, goes on
    /// with the new lines.
    pub(crate) fn went_on(&mut self) {
        let frontier = self.progress.as_ref().and_then(Progress::frontier);
        if let Some(map) = self.map.get_mut() {
            map.headers_of(&self.blocks);
            map.extend(&self.lines, frontier.cloned());
        }
    }

    /// What the layout in steps keeps, when it is one.
    pub(crate) fn progress(&self) -> Option<&Progress> {
        self.progress.as_ref()
    }

    /// Notes that the lines `changed` were laid out in the place of others,
    /// as many: the map, if it was made, takes the new ones.
    pub(crate) fn changed(&mut self, changed: Range<usize>) {
        if let Some(map) = self.map.get_mut() {
            map.relay(&self.lines, changed);
        }
    }

    /// Whether the layout is whole: nothing is left to lay out.
    pub fn is_whole(&self) -> bool {
        self.progress.as_ref().is_none_or(Progress::is_done)
    }

    /// Takes the layout of `content`, which it was begun with, back to the
    /// last checkpoint at or before line `line`, so that [`Laid::extend`]
    /// lays out again from there, as it must when what the layout read, such
    /// as an image, has changed. The checkpoints are the start of every
    /// stretch of a code block, the start of every page of a run of reading
    /// mode, and about every `stretch::LINES` lines between, so what comes
    /// before the stretch or page that `line` is in is not laid out again.
    pub fn restart(&mut self, content: Content, line: usize) {
        let Some(progress) = &mut self.progress else {
            return;
        };
        progress.restart(&mut self.lines, content, line);
        // The map is of the lines that were, and is made again when asked.
        self.map = OnceCell::new();
    }

    /// [`Laid::restart`] for a layout of the cell layout taken back to the
    /// start of a piece: the map, if it was made, lets go of the lines after
    /// the checkpoint and goes on with those laid out after it, instead of
    /// being made again of all of them.
    pub(crate) fn go_back(&mut self, content: Content, line: usize) {
        let Some(progress) = &mut self.progress else {
            return;
        };
        progress.restart(&mut self.lines, content, line);
        let frontier = progress.frontier().cloned();
        if let Some(map) = self.map.get_mut() {
            map.cut(self.lines.len());
            map.laid_to(frontier);
        }
    }

    /// The images `parts` want, whether or not anything is laid out: each
    /// once with the index of its part, in the order of the documents, and
    /// beside each what `kept` holds of it, taken out of `kept`; `None` for
    /// one to load. The layout of what is wanted comes after it and takes the
    /// sizes of the images from it, so it is a question of the documents and
    /// not of a layout. It looks at the whole of each document.
    pub(crate) fn needs<'p>(
        parts: &[ImagePart<'p>],
        renderer: &dyn Renderer,
        looks: &Looks,
        kept: Option<&mut Images>,
    ) -> Needs<'p> {
        plan::plan(parts, renderer, looks, kept)
    }

    /// The keys of the images `parts` want, each once, in the order of the
    /// documents: what [`Laid::needs`] says, without what was kept.
    pub(crate) fn needs_keys(
        parts: &[ImagePart<'_>],
        renderer: &dyn Renderer,
        looks: &Looks,
    ) -> Vec<ImageKey> {
        plan::keys(parts, renderer, looks)
    }

    /// All the lines.
    pub fn lines(&self) -> &[Line] {
        &self.lines
    }

    /// All the lines, to change. The map is not made again, so what changes
    /// must be what it does not read, such as a band's scroll.
    pub fn lines_mut(&mut self) -> &mut [Line] {
        &mut self.lines
    }

    /// The table of the blocks the lines come from.
    pub fn blocks(&self) -> &Blocks {
        &self.blocks
    }

    /// How many lines there are.
    pub fn len(&self) -> usize {
        self.lines.len()
    }

    /// Whether nothing is laid out.
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// The lines in `range`; those of it that are past the last line are
    /// left out.
    pub fn rows(&self, range: Range<usize>) -> &[Line] {
        let end = range.end.min(self.lines.len());
        &self.lines[range.start.min(end)..end]
    }

    /// The rows the lines take on a terminal with `caps`: as many as the
    /// largest scale of each line's text, unless the terminal gets plain
    /// text.
    pub fn height(&self, caps: &Capabilities) -> usize {
        (self.lines.iter())
            .map(|line| usize::from(line.rows_on(caps)))
            .sum()
    }

    /// The map of the lines, made when first asked.
    pub fn map(&self) -> &Map {
        self.map.get_or_init(|| {
            let mut map = Map::new(&self.lines, &self.blocks);
            map.laid_to(self.progress.as_ref().and_then(|p| p.frontier()).cloned());
            map
        })
    }

    /// The lines to change and the map of the lines as they are, for a
    /// change that is made by what the map knows, as a band's scroll is.
    pub fn parts_mut(&mut self) -> (&mut [Line], &Map) {
        self.map();
        (&mut self.lines, self.map.get().expect("the map was made"))
    }

    /// The lines that show `range`, by piece, or why it is not shown; see
    /// [`Map::locate`].
    pub fn locate(&self, range: &Range<Pos>) -> Result<Vec<Part>, Hidden> {
        self.map().locate(&self.lines, range)
    }

    /// Like [`Laid::locate`], but a picture is shown where it is drawn; see
    /// [`Map::drawn`].
    pub fn drawn(&self, range: &Range<Pos>) -> Result<Vec<Part>, Hidden> {
        self.map().drawn(&self.lines, range)
    }

    /// The parts that show `range` as [`Laid::drawn`] has them, and the lines
    /// from the first to the last; see [`Map::drawn_rows`].
    pub fn drawn_rows(&self, range: &Range<Pos>) -> Result<(Vec<Part>, Range<usize>), Hidden> {
        self.map().drawn_rows(&self.lines, range)
    }

    /// The place at column `column` of line `line`; see [`Map::hit`].
    pub fn hit(&self, line: usize, column: usize) -> Option<Pos> {
        self.map().hit(&self.lines, line, column)
    }

    /// The first line of `piece`; see [`Map::start`].
    pub fn start(&self, piece: &crate::typeset::block::BlockInfo) -> Option<usize> {
        self.map().start(piece)
    }

    /// The picture drawn in the place of the text in `range`; see
    /// [`Map::picture`].
    pub fn picture(&self, range: &Range<Pos>, candidates: &[ImageKey]) -> Option<ImageKey> {
        self.map().picture(&self.lines, range, candidates)
    }

    /// The smallest place shown on line `line`; see [`Map::first_on`].
    pub fn first_on(&self, line: usize) -> Option<&Pos> {
        self.map().first_on(line)
    }

    /// The bands of reading mode among the lines; see [`Map::bands`].
    pub fn bands(&self) -> &[Band] {
        self.map().bands()
    }

    /// The first line that shows the place `pos` or one after it; see
    /// [`Map::line_of`].
    pub fn line_of(&self, pos: &Pos) -> Option<usize> {
        self.map().line_of(pos)
    }

    /// What `range` is of the text of each line of the cell layout that
    /// shows it; see [`Map::line_bytes`].
    pub fn line_bytes(&self, range: &Range<Pos>) -> Vec<(usize, Range<usize>)> {
        self.map().line_bytes(range)
    }

    /// Whether something that can carry a link shows `range`; see
    /// [`Map::holds_cells`].
    pub fn holds_cells(&self, range: &Range<Pos>) -> bool {
        self.map().holds_cells(range)
    }

    /// What shows `range` on the page of `parts`; see [`Map::page_spots`].
    pub fn page_spots(&self, parts: &Rc<Parts>, range: &Range<Pos>) -> Vec<Spot> {
        self.map().page_spots(parts, range)
    }

    /// The places that the lines `window` show; see [`Map::span_of`].
    pub fn span_of(&self, window: Range<usize>) -> Option<Range<Pos>> {
        self.map().span_of(window)
    }
}

#[cfg(test)]
mod tests;
