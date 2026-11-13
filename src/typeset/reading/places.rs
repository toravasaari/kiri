//! Which block and which text a row of a reading page shows.
//!
//! A page is one picture cut into rows, and each row is a line of the cell
//! layout. This file answers what the rest of the engine does not: of
//! which block of the document a row is a part. The map from the page's
//! texts to the places of the document is here too (`Item::origin`, once
//! it is there), so that the engine's driver stays the driver.

use std::rc::Rc;

use crate::typeset::block::BlockId;
use crate::typeset::images::size::Placement;
use crate::typeset::reading::engine::Parts;
use crate::typeset::style::{ImageRow, Line, Span};

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
    ids: &[BlockId],
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
    // blocks belong to none.
    let block =
        (parts.page.blocks.iter().zip(ids)).find(|(placed, _)| placed.rows.contains(&middle));
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
        block: block.map(|(_, id)| *id),
        ..Line::blank()
    }
}
