//! Reading mode's pen for the driver, which holds the pieces that reading mode
//! lays out as one stream of pixels. It holds them until the stream is cut, and
//! gives the rest to the cell layout.
//!
//! The pieces are the blocks that [`page::lays_out`] says reading mode
//! lays out. A block it does not (an image file, an error), the header of a
//! file and front matter that is hidden cut the stream, and are laid out in
//! cells as the text layout does, so that the page before it is a page of
//! its own.
//!
//! The pen also owns `Area`, where reading mode sets its stream in a window,
//! and `Setting`, what a page needs besides its blocks ([`Setting`]).

use std::borrow::Cow;
use std::collections::VecDeque;

use crate::typeset::caps::Capabilities;
use crate::typeset::driver::{Pen, Piece, Saved, Shape, Stack};
use crate::typeset::images::size::cell_size;
use crate::typeset::layout::{Cells, PageInputs};
use crate::typeset::model::Block;
use crate::typeset::reading::engine::{self, Reading};
use crate::typeset::reading::page::{self, Frame, FrontMatterShown};
use crate::typeset::reading::places::Ids;
use crate::typeset::reading::{font_size, page_look};
use crate::typeset::theme::Look as Resolved;
use crate::typeset::walk::{Place, Walk};

/// The widest the text is, in font sizes.
const TEXT_WIDTH: f32 = 42.0;
/// How much wider than the text the area is that tables and code may
/// take: the area is centred in the window, and text starts at its left
/// edge.
const AREA: f32 = 1.5;
/// The cells kept free on each side of the area.
const SIDE: u16 = 2;
/// The rows after which a page ends, at the end of the block that passes
/// them.
const PAGE_ROWS: u32 = 3000;

/// Where reading mode sets its text in a window: the area that tables
/// and code may take, centred, and the text at its left edge.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Area {
    /// The text's size in pixels: a fraction of the cell height, at the
    /// zoom.
    pub size: f32,
    /// The cells before the area, and its width in cells.
    pub left: u16,
    pub area: u16,
    /// The text's width in pixels, and in whole cells: the column that
    /// the blocks laid out as text take.
    pub text: f32,
    pub column: u16,
}

impl Area {
    /// The area in a window `width` cells wide on a terminal with `caps`,
    /// for text at `zoom` percent.
    pub fn of(one: &Resolved, caps: &Capabilities, width: usize, zoom: u16) -> Self {
        let (cell_width, cell_height) = cell_size(caps);
        let size = font_size(one, cell_height, zoom);
        let cell_width = f32::from(cell_width);
        let window = u16::try_from(width).unwrap_or(u16::MAX);
        let room = window.saturating_sub(2 * SIDE).max(1);
        let text = (TEXT_WIDTH * size).min(f32::from(room) * cell_width);
        let area = ((AREA * text / cell_width).ceil() as u16).min(room);
        Self {
            size,
            left: (window - area) / 2,
            area,
            text,
            column: ((text / cell_width) as u16).clamp(1, area),
        }
    }
}

/// What a page needs besides its blocks, settled for each cut: the
/// size of a cell, how the page looks, where it stands in the window and
/// how front matter shows.
pub struct Setting<'a> {
    pub(super) inputs: &'a PageInputs<'a>,
    pub(super) cell: (u16, u16),
    pub(super) look: page::Look,
    pub(super) area: Area,
    /// The columns the page covers, as tables and code may reach across
    /// the area.
    pub(super) columns: u16,
    pub(super) front_matter: FrontMatterShown,
    /// The rows after which a page ends, at the end of the block that
    /// passes them; `None` for one page whatever its height.
    pub(super) budget: Option<u32>,
    /// What the page before ends with: its last block's bottom margin.
    pub(super) margin: Option<f32>,
}

impl<'a> Setting<'a> {
    /// The setting of pages laid out with `inputs` in `area`.
    pub(crate) fn new(inputs: &'a PageInputs<'a>, area: Area) -> Self {
        let caps = inputs.caps;
        let look = page_look(inputs.look, &inputs.code, inputs.stripes, caps, area.size);
        Self {
            inputs,
            cell: cell_size(caps),
            look,
            area,
            columns: area.area.clamp(1, caps.max_image_cells()),
            front_matter: match inputs.front_matter_as_code {
                true => FrontMatterShown::Code,
                false => FrontMatterShown::Table,
            },
            budget: Some(PAGE_ROWS),
            margin: None,
        }
    }
}

impl Frame {
    /// The frame of a page of `setting`.
    pub(super) fn of(setting: &Setting, wide: page::Wide, snap: bool) -> Self {
        let (cell_width, cell_height) = (f32::from(setting.cell.0), f32::from(setting.cell.1));
        Self {
            width: setting.area.text,
            area: f32::from(setting.columns) * cell_width,
            row: cell_height,
            wide,
            snap,
            front_matter: setting.front_matter,
            budget: (setting.budget).map_or(f32::INFINITY, |rows| rows as f32 * cell_height),
            margin: setting.margin,
        }
    }
}

/// How many blocks of a held run are given to the layout of a page at first;
/// when they are all taken the page may go on, and it is laid out again with
/// twice as many.
const WINDOW: usize = 64;

#[cfg(test)]
thread_local! {
    /// How many blocks were handed to the layout of pages on this thread,
    /// which the tests of the cost of a run count.
    static HANDED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// How many blocks were handed to the layout of pages on this thread.
#[cfg(test)]
pub(crate) fn handed() -> usize {
    HANDED.with(std::cell::Cell::get)
}

/// The pen of reading mode, with the cell layout's pen for what it does not
/// lay out.
pub(crate) struct Pixels<'a, 'd> {
    cells: Cells<'a>,
    reading: &'a Reading,
    /// Where the text stands in the window, which the blocks laid out as
    /// cells and the pages both follow.
    area: Area,
    /// The first pieces of the run held, as far as they are needed: the
    /// block, which for a `<details>` is the block without its blocks, and
    /// its place in the document.
    held: VecDeque<Held<'d>>,
    /// How many more pieces of the run follow those in `held`, and where the
    /// walk gives the first of them. They are found again when a page needs
    /// them, so a pen made again for each step does not read the whole run.
    unseen: usize,
    after: Option<Place>,
    /// How many blocks the next page is laid out of at first.
    window: usize,
    /// The file the run is in, which gives the pieces held but not seen.
    walk: Option<Walk<'d>>,
    /// What the page of the run that was laid out last ends with: its last
    /// block's bottom margin, which the next page goes on with.
    margin: Option<f32>,
    /// Whether a page of the run held was laid out, so that the next joins
    /// it.
    joined: bool,
}

/// A piece held for a page: its place, its block, and whether the centred
/// wrapper it is in centres it.
struct Held<'d> {
    path: Vec<usize>,
    block: Cow<'d, Block>,
    /// The block in a wrapper of its own when the centred wrapper it is in
    /// centres it, as the wrapper's flow does, made once when it is held.
    wrapped: Option<Block>,
}

impl Held<'_> {
    /// The block as the page lays it out.
    fn laid(&self) -> &Block {
        self.wrapped.as_ref().unwrap_or(&self.block)
    }
}

impl<'a, 'd> Pixels<'a, 'd> {
    /// The pen of `reading`, which gives the blocks it does not lay out to
    /// `cells`, in the column of the text.
    pub(crate) fn new(mut cells: Cells<'a>, reading: &'a Reading) -> Self {
        // The blocks laid out as cells start where the area does and are
        // as wide as the text's column, so that they line up with the text,
        // a centred formula in the middle of the column and a wide image
        // shrunk to it.
        let inputs = cells.page_inputs();
        let area = Area::of(inputs.look, inputs.caps, cells.window(), reading.zoom);
        cells.set_column(usize::from(area.left), usize::from(area.column));
        Self {
            cells,
            reading,
            area,
            held: VecDeque::new(),
            window: WINDOW,
            unseen: 0,
            after: None,
            walk: None,
            margin: None,
            joined: false,
        }
    }

    /// What is held of `piece`, if the pen holds it: a `<details>` is held as
    /// its summary alone. A centred wrapper is held as nothing, since its
    /// blocks are pieces of their own and it has no lines.
    fn entry(piece: Piece<'d>) -> Option<Held<'d>> {
        let (block, centred) = match piece.shape {
            Shape::Summary { summary, .. } => {
                let summary = Block::Details {
                    summary: summary.to_vec(),
                    blocks: Vec::new(),
                };
                (Cow::Owned(summary), false)
            }
            Shape::Block(block) => (Cow::Borrowed(block), false),
            Shape::Centred(block) => (Cow::Borrowed(block), true),
            Shape::Wrapper => return None,
            _ => unreachable!("only blocks and summaries are held"),
        };
        let wrapped = centred.then(|| Block::Centered(vec![block.as_ref().clone()]));
        Some(Held {
            path: piece.path,
            block,
            wrapped,
        })
    }

    /// Whether the walk goes into `block`: a centred wrapper is laid out as
    /// the blocks in it, as the cell layout lays it out.
    fn goes_into(block: &Block) -> bool {
        matches!(block, Block::Centered(_))
    }

    /// Makes the next piece that is held but not seen held.
    fn see_one(&mut self) {
        let (Some(walk), Some(mut after)) = (self.walk, self.after.take()) else {
            self.unseen = 0;
            return;
        };
        while let Some((piece, next)) = walk.next(&after, Self::goes_into) {
            after = next;
            if let Some(held) = Self::entry(piece) {
                self.held.push_back(held);
                self.after = Some(after);
                self.unseen -= 1;
                return;
            }
        }
        self.unseen = 0;
    }

    /// Sees pieces until `count` are held, or all are.
    fn see(&mut self, count: usize) {
        while self.held.len() < count && self.unseen > 0 {
            self.see_one();
        }
    }
}

impl<'d> Pen<'d> for Pixels<'_, 'd> {
    fn piece(&mut self, piece: Piece<'d>, stack: &mut Stack) {
        if self.holds(&piece) {
            // A `<details>` is its summary and then its blocks, each with a
            // place of its own, as the cell layout has them. The pieces come
            // after those held but not seen.
            self.see(usize::MAX);
            self.held.extend(Self::entry(piece));
        } else {
            self.cut(stack, usize::MAX);
            self.cells.piece(piece, stack);
        }
    }

    fn holds(&self, piece: &Piece<'d>) -> bool {
        match piece.shape {
            Shape::Summary { blocks, .. } => blocks.iter().all(page::lays_out),
            Shape::Block(block) => page::lays_out(block) && !self.cells.hides(block),
            Shape::Centred(block) => page::lays_out(block),
            Shape::Wrapper => true,
            _ => false,
        }
    }

    fn cuts(&self, block: &Block) -> bool {
        Self::goes_into(block)
    }

    fn pending(&self) -> bool {
        !self.held.is_empty() || self.unseen > 0
    }

    /// Lays the pieces held out as pages: a page ends at the end of the
    /// block that passes the page's budget of rows, and the next starts
    /// with the rest, which depends on the blocks alone. A page goes on
    /// from the one before it with the margin it ended with, and the stack
    /// joins it without the blank row it puts between other pieces, so
    /// that the gap at the cut is what the gap between the blocks is, and
    /// the rounding of the page above to rows. It stops after the page that
    /// brings the stack to `until` lines, and the blocks that are left stay
    /// held, with the margin and whether a page is laid, to go on. A page
    /// is laid out of as many blocks as it takes, not of the whole run.
    fn cut(&mut self, stack: &mut Stack, until: usize) -> bool {
        let mut window = self.window;
        while stack.len() < until && (!self.held.is_empty() || self.unseen > 0) {
            self.see(window);
            let given = self.held.len().min(window);
            let page = {
                // Each block and the blocks inside it have their numbers in
                // the table before the page is laid out.
                let ids: Vec<Ids> = (self.held.iter().take(given))
                    .map(|held| {
                        let ids = Ids::new(&held.block, |inside| {
                            self.cells.number(&[&held.path[..], inside].concat())
                        });
                        match held.wrapped {
                            Some(_) => ids.in_wrapper(),
                            None => ids,
                        }
                    })
                    .collect();
                let blocks: Vec<&Block> = (self.held.iter().take(given)).map(Held::laid).collect();
                // Reading mode sets its own width: the theme's widest text is
                // for text in the terminal's font.
                let inputs = self.cells.page_inputs();
                let mut setting = Setting::new(&inputs, self.area);
                setting.margin = self.margin;
                #[cfg(test)]
                HANDED.with(|handed| handed.set(handed.get() + blocks.len()));
                engine::lines(&blocks, &ids, self.reading, &setting)
            };
            // A page that takes all it was given may take more.
            if page.taken >= given && (self.held.len() > given || self.unseen > 0) {
                window = window.saturating_mul(2);
                continue;
            }
            // The next page is about as long as this one.
            window = WINDOW.max(2 * page.taken);
            self.window = window;
            match self.joined {
                false => stack.push(page.lines),
                true => stack.join(page.lines),
            }
            self.joined = true;
            self.margin = page.margin;
            self.held.drain(..page.taken);
        }
        let done = self.held.is_empty() && self.unseen == 0;
        if done {
            (self.margin, self.joined) = (None, false);
        }
        done
    }

    fn save(&self) -> Saved {
        let first = match self.held.front() {
            Some(held) => Some(Place::at_block(held.path.clone())),
            None => self.after.clone().filter(|_| self.unseen > 0),
        };
        Saved {
            cells: self.cells.save(),
            held: first.map(|first| (first, self.held.len() + self.unseen)),
            margin: self.margin,
            joined: self.joined,
        }
    }

    fn restore(&mut self, saved: &Saved, walk: &Walk<'d>) {
        self.cells.restore(&saved.cells, walk);
        self.walk = Some(*walk);
        self.held.clear();
        (self.after, self.unseen) = match &saved.held {
            Some((first, count)) => (Some(first.clone()), *count),
            None => (None, 0),
        };
        (self.margin, self.joined) = (saved.margin, saved.joined);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::typeset::model::Inline;

    #[test]
    fn a_held_block_that_a_wrapper_centres_is_put_in_a_wrapper_once() {
        let block = Block::Paragraph(vec![Inline::Text("x".into())]);
        let piece = Piece {
            shape: Shape::Centred(&block),
            path: vec![0, 0],
        };
        let held = Pixels::entry(piece).expect("held");
        let (first, again) = (held.laid(), held.laid());
        assert_eq!(*first, Block::Centered(vec![block.clone()]));
        assert!(std::ptr::eq(first, again), "made again");
    }
}
