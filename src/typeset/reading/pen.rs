//! Reading mode's pen for the driver: it holds the pieces that reading
//! mode lays out as one stream of pixels until the stream is cut, and gives
//! the others to the cell layout.
//!
//! The pieces are the blocks that [`page::lays_out`] says reading mode
//! lays out. A block it does not (an image file, an error), the header of a
//! file and front matter that is hidden cut the stream, and are laid out in
//! cells as the text layout does, so that the page before it is a page of
//! its own.
//!
//! The pen also owns where reading mode sets its stream in a window and
//! what a page needs besides its blocks ([`Setting`]).

use std::borrow::Cow;

use crate::typeset::caps::Capabilities;
use crate::typeset::driver::{Pen, Piece, Shape, Stack};
use crate::typeset::images::size::cell_size;
use crate::typeset::layout::{Cells, PageInputs};
use crate::typeset::model::Block;
use crate::typeset::reading::engine::{self, Reading};
use crate::typeset::reading::page::{self, Frame, FrontMatterShown};
use crate::typeset::reading::page_look;
use crate::typeset::reading::places::Ids;
use crate::typeset::theme::Look as Resolved;

/// The widest the text is, in font sizes.
const TEXT_WIDTH: f32 = 42.0;
/// How much wider than the text the area is that tables and code may
/// take: the area is centred in the window, and text starts at its left
/// edge.
const AREA: f32 = 1.5;
/// The cells kept free on each side of the area.
const SIDE: u16 = 2;

/// Where reading mode sets its text in a window: the area that tables
/// and code may take, centred, and the text at its left edge.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Area {
    /// The cells before the area, and its width in cells.
    pub left: u16,
    pub area: u16,
    /// The text's width in pixels, and in whole cells: the column that
    /// the blocks laid out as text take.
    pub text: f32,
    pub column: u16,
}

impl Area {
    /// The area in a window `width` cells wide on a terminal with `caps`.
    pub fn of(one: &Resolved, caps: &Capabilities, width: usize) -> Self {
        let (cell_width, cell_height) = cell_size(caps);
        let (cell_width, size) = (
            f32::from(cell_width),
            (one.pixels.font_size * f32::from(cell_height)).round(),
        );
        let window = u16::try_from(width).unwrap_or(u16::MAX);
        let room = window.saturating_sub(2 * SIDE).max(1);
        let text = (TEXT_WIDTH * size).min(f32::from(room) * cell_width);
        let area = ((AREA * text / cell_width).ceil() as u16).min(room);
        Self {
            left: (window - area) / 2,
            area,
            text,
            column: ((text / cell_width) as u16).clamp(1, area),
        }
    }
}

/// What a page needs besides its blocks, settled once for a file: the
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
}

impl<'a> Setting<'a> {
    /// The setting of pages laid out with `inputs` in a window `window`
    /// cells wide.
    pub(crate) fn new(inputs: &'a PageInputs<'a>, window: usize) -> Self {
        let caps = inputs.caps;
        let look = page_look(inputs.look, &inputs.code, inputs.stripes, caps);
        let area = Area::of(inputs.look, caps, window);
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
        }
    }
}

impl Frame {
    /// The frame of a page of `setting`.
    pub(super) fn of(setting: &Setting, wide: page::Wide) -> Self {
        let (cell_width, cell_height) = (f32::from(setting.cell.0), f32::from(setting.cell.1));
        Self {
            width: setting.area.text,
            area: f32::from(setting.columns) * cell_width,
            row: cell_height,
            wide,
            front_matter: setting.front_matter,
        }
    }
}

/// The pen of reading mode, with the cell layout's pen for what it does not
/// lay out.
pub(crate) struct Pixels<'a, 'd> {
    cells: Cells<'a>,
    reading: &'a Reading,
    /// The pieces of the stream: the block, which for a `<details>` is the
    /// block without its blocks, and its place in the document.
    held: Vec<(Vec<usize>, Cow<'d, Block>)>,
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
        let area = Area::of(inputs.look, inputs.caps, cells.window());
        cells.set_column(usize::from(area.left), usize::from(area.column));
        Self {
            cells,
            reading,
            held: Vec::new(),
        }
    }
}

impl<'d> Pen<'d> for Pixels<'_, 'd> {
    fn piece(&mut self, piece: Piece<'d>, stack: &mut Stack) {
        match piece.shape {
            // A `<details>` is its summary and then its blocks, each with a
            // place of its own, as the cell layout has them.
            Shape::Summary(Block::Details { summary, blocks })
                if blocks.iter().all(page::lays_out) =>
            {
                let summary = Block::Details {
                    summary: summary.clone(),
                    blocks: Vec::new(),
                };
                self.held.push((piece.path, Cow::Owned(summary)));
            }
            Shape::Block(block) if page::lays_out(block) && !self.cells.hides(block) => {
                self.held.push((piece.path, Cow::Borrowed(block)));
            }
            _ => {
                self.cut(stack);
                self.cells.piece(piece, stack);
            }
        }
    }

    /// Lays the pieces held out as a page.
    fn cut(&mut self, stack: &mut Stack) {
        if self.held.is_empty() {
            return;
        }
        let held = std::mem::take(&mut self.held);
        // Each block and the blocks inside it are in the table, in the order
        // of the document, before the page is laid out.
        let ids: Vec<Ids> = (held.iter())
            .map(|(path, block)| {
                let ids = Ids::new(block, |inside| {
                    self.cells.number(&[&path[..], inside].concat())
                });
                self.cells.hide(block, ids.id);
                ids
            })
            .collect();
        let blocks: Vec<&Block> = held.iter().map(|(_, block)| block.as_ref()).collect();
        // Reading mode sets its own width: the theme's widest text is for
        // text in the terminal's font.
        let inputs = self.cells.page_inputs();
        let setting = Setting::new(&inputs, self.cells.window());
        stack.push(engine::lines(&blocks, &ids, self.reading, &setting));
    }
}
