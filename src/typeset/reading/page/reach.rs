//! How far right a page draws, part by part: the measure that lets a part
//! of a page be only as wide as what it draws.

use super::{Item, Page, Shape};
use crate::typeset::color::Color;
use crate::typeset::reading::engine::PART_ROWS;
use crate::typeset::reading::text::Brush;

/// The room kept beyond the right edge of what a part draws, in font sizes:
/// the box round a selected link and the chip of inline code are drawn
/// later, or a little outside their text, and must not widen the part.
const MARGIN: f32 = 1.0;

impl Page {
    /// The columns each part of the page is drawn at, on `rows` rows of
    /// `columns` columns of `cell` pixels: as wide as what the part draws,
    /// up to the page's. `writer` is the background the terminal layer
    /// paints beside a part; where the page is drawn opaque, only a writer
    /// of that colour exactly lets a part be narrower, or a seam would
    /// show, and the page's width stays. A transparent page shows the
    /// terminal's own background on both sides.
    pub(in crate::typeset::reading) fn part_columns(
        &self,
        (cell, columns, rows): ((u16, u16), u16, u16),
        writer: Option<Color>,
        palette: &[Option<Color>; 16],
    ) -> Vec<u16> {
        let count = usize::from(rows).div_ceil(usize::from(PART_ROWS));
        if !self.narrows(writer, palette) {
            return vec![columns; count];
        }
        let height = f32::from(PART_ROWS) * f32::from(cell.1);
        (self.reaches(height, count).into_iter())
            .map(|right| ((right / f32::from(cell.0)).ceil() as u16).clamp(1, columns))
            .collect()
    }

    /// Whether parts may be narrower than the page, as `part_columns` says.
    fn narrows(&self, writer: Option<Color>, palette: &[Option<Color>; 16]) -> bool {
        let writer: Option<Brush> = match writer.and_then(|color| color.rgb(palette)) {
            Some(Color::Rgb(red, green, blue)) => Some([red, green, blue]),
            _ => None,
        };
        !self.look.opaque || writer == Some(self.look.page)
    }

    /// For each of `parts` parts `height` pixels tall, from the top of the
    /// page, how many pixels from its left edge the page draws in it, plus
    /// the margin, or 0 for a part with nothing. The items of bands are in
    /// the bands' own coordinates and are left out, as bands are as wide as
    /// the page; the pictures still loading have no item and count by their
    /// own width. Items of any shape count, so that a table's frame bounds
    /// the cells that are aligned inside it.
    pub(in crate::typeset::reading) fn reaches(&self, height: f32, parts: usize) -> Vec<f32> {
        let mut reaches = vec![0.0f32; parts];
        let mut note = |rows: &std::ops::Range<f32>, right: f32| {
            for part in touched(rows, height, parts) {
                reaches[part] = reaches[part].max(right);
            }
        };
        for item in &self.items {
            note(&item.rows, right_of(item));
        }
        for loading in &self.loading {
            note(&loading.rows, loading.x + loading.width);
        }
        let margin = MARGIN * self.look.size;
        reaches
            .into_iter()
            .map(|right| if right > 0.0 { right + margin } else { 0.0 })
            .collect()
    }
}

/// The parts whose pixel rows `rows` touch, edges included as drawing
/// includes them.
fn touched(rows: &std::ops::Range<f32>, height: f32, parts: usize) -> std::ops::Range<usize> {
    if parts == 0 || height <= 0.0 || rows.end < 0.0 {
        return 0..0;
    }
    let first = ((rows.start / height).ceil() - 1.0).max(0.0) as usize;
    let last = ((rows.end / height).floor().max(0.0) as usize).min(parts - 1);
    first..(last + 1).max(first)
}

/// The right edge of what `item` draws, in the page's pixels.
fn right_of(item: &Item) -> f32 {
    match &item.shape {
        Shape::Text {
            paragraph, x, chip, ..
        } => {
            let pad = if chip.is_some() {
                0.12 * paragraph.look.size
            } else {
                0.0
            };
            x + paragraph.right() + pad
        }
        Shape::Rect { x, width, .. }
        | Shape::Frame { x, width, .. }
        | Shape::Stripe { x, width, .. }
        | Shape::RoundRect { x, width, .. } => x + width,
        Shape::Picture { x, pixels, .. } => x.round() + pixels.width() as f32,
        Shape::Task { x, size, .. } => x + size,
    }
}
